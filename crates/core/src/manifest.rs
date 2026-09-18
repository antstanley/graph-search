//! Reconcile: classifying the tree against the manifest (`SPEC.md` §6.3).
//!
//! Every path lands in exactly one class: `added`, `modified`, `removed`,
//! `renamed`, or `unchanged`. A parser or schema bump re-parses everything
//! (`unchanged-global`). Quarantine classification happens during extraction;
//! a quarantined file keeps its manifest entry with the reason.

use crate::Result;
use crate::config::WalkPolicy;
use crate::walk::{WalkEntry, walk};
use graph_search_types::limits::{PARSER_VERSION, SCHEMA_VERSION};
use graph_search_types::manifest::{FileEntry, Manifest, Rename};
use std::collections::BTreeMap;
use std::path::Path;

/// The outcome of the diff, before extraction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Diff {
    /// Walked, not in the manifest (or a version bump): parse and insert.
    pub added: Vec<WalkEntry>,
    /// In the manifest, content hash differs: re-parse and replace.
    pub modified: Vec<WalkEntry>,
    /// In the manifest, gone from the tree: delete nodes and incident edges.
    pub removed: Vec<String>,
    /// Same content hash as a removed entry: a move, not a delete + add.
    pub renamed: Vec<Rename>,
    /// Skipped by the O(1) no-op path.
    pub unchanged: Vec<String>,
    /// Whether a parser/schema bump forced the full re-parse.
    pub reindexed_all: bool,
}

impl Diff {
    /// Whether nothing changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.modified.is_empty()
            && self.removed.is_empty()
            && self.renamed.is_empty()
    }
}

/// Cheaply compares the tree with the manifest: no hashing on the `unchanged`
/// path — `(size, mtime)` equality with matching versions is the no-op.
///
/// # Errors
///
/// When the walk fails.
pub fn diff_against(search_root: &Path, manifest: &Manifest, policy: &WalkPolicy) -> Result<Diff> {
    let entries = walk(search_root, policy)?;
    Ok(classify(&entries, manifest))
}

/// The pure classification over walked entries; also the unit-test seam.
#[must_use]
pub fn classify(entries: &[WalkEntry], manifest: &Manifest) -> Diff {
    let mut diff = Diff::default();
    let version_bump =
        manifest.parser_version != PARSER_VERSION || manifest.schema_version != SCHEMA_VERSION;
    diff.reindexed_all = version_bump && !manifest.is_empty();

    // Hash-based rename detection needs content; it happens after the cheap
    // pass, over the `removed` and unmanifested `added` sets only.
    let mut hash_pending: BTreeMap<String, String> = BTreeMap::new(); // path -> hash (lazily)
    let mut candidate_removed: BTreeMap<String, &FileEntry> = BTreeMap::new();
    let walked: std::collections::BTreeSet<&str> = entries.iter().map(|e| e.rel.as_str()).collect();

    for (path, entry) in &manifest.entries {
        if !walked.contains(path.as_str()) {
            candidate_removed.insert(path.clone(), entry);
        }
    }

    for walked_entry in entries {
        let stored = manifest.get(&walked_entry.rel);
        match stored {
            Some(entry)
                if !version_bump
                    && entry.size == walked_entry.size
                    && entry.mtime_ns == walked_entry.mtime_ns
                    && entry.matches_versions(PARSER_VERSION, SCHEMA_VERSION) =>
            {
                diff.unchanged.push(walked_entry.rel.clone());
            }
            Some(entry)
                if !version_bump
                    && entry.matches_versions(PARSER_VERSION, SCHEMA_VERSION)
                    && entry.quarantine.is_none() =>
            {
                // Size or mtime moved: hash before deciding modified.
                hash_pending.insert(walked_entry.rel.clone(), entry.content_hash.clone());
            }
            Some(entry) if !version_bump && entry.quarantine.is_some() => {
                // A quarantined file is re-attempted only when its bytes change.
                if entry.size == walked_entry.size && entry.mtime_ns == walked_entry.mtime_ns {
                    diff.unchanged.push(walked_entry.rel.clone());
                } else {
                    diff.modified.push(walked_entry.clone());
                }
            }
            _ => {
                diff.added.push(walked_entry.clone());
            }
        }
    }

    // Hash the pending set and split into renamed vs modified.
    for (path, old_hash) in &hash_pending {
        let Some(entry) = entries.iter().find(|e| &e.rel == path) else {
            continue;
        };
        let hash = crate::hash::content_hash(&std::fs::read(&entry.path).unwrap_or_default());
        if *old_hash == hash {
            diff.unchanged.push(path.clone());
        } else if let Some(_removed) = candidate_removed.get(path) {
            // Impossible in v1: a path cannot be both walked and removed.
            diff.modified.push(entry.clone());
        } else {
            diff.modified.push(entry.clone());
        }
    }

    // Renames: an added path whose hash equals a removed entry's hash. Deterministic:
    // both sides are walked in sorted order, so equal hashes pair first-first.
    let mut removed_by_hash: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    let mut removable: Vec<String> = Vec::new();
    for (path, entry) in &candidate_removed {
        removed_by_hash
            .entry(entry.content_hash.as_str())
            .or_default()
            .push(path);
        removable.push(path.clone());
    }
    let mut consumed: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut still_added: Vec<WalkEntry> = Vec::new();
    for added in &diff.added {
        let hash = crate::hash::content_hash(&std::fs::read(&added.path).unwrap_or_default());
        if let Some(matches) = removed_by_hash.get_mut(hash.as_str())
            && let Some(from) = matches.iter().find(|p| !consumed.contains(**p))
        {
            let from = (*from).clone();
            consumed.insert(from.clone());
            diff.renamed.push(Rename {
                from,
                to: added.rel.clone(),
            });
            continue;
        }
        still_added.push(added.clone());
    }
    diff.added = still_added;
    diff.removed = removable
        .into_iter()
        .filter(|p| !consumed.contains(p))
        .collect();

    // When a version bump forced a full re-parse, every manifest entry the
    // tree still holds is `modified`, not `added`: their nodes are replaced.
    if diff.reindexed_all {
        let mut replaced = std::mem::take(&mut diff.added);
        for entry in &mut replaced {
            if manifest.get(&entry.rel).is_some() {
                diff.modified.push(entry.clone());
            } else {
                diff.added.push(entry.clone());
            }
        }
    }

    diff
}

/// Whether `entry` is stale compared with what a fresh walk would record —
/// the cheap `(size, mtime)` test, no hashing.
#[must_use]
pub fn entry_differs(entry: &FileEntry, walked: &WalkEntry) -> bool {
    entry.size != walked.size || entry.mtime_ns != walked.mtime_ns
}

/// Builds a manifest entry for a walked file that was hashed `content_hash`.
#[must_use]
pub fn entry_for(walked: &WalkEntry, content_hash: &str, quarantine: Option<String>) -> FileEntry {
    FileEntry {
        size: walked.size,
        mtime_ns: walked.mtime_ns,
        content_hash: content_hash.to_owned(),
        parser_version: PARSER_VERSION,
        schema_version: SCHEMA_VERSION,
        quarantine,
    }
}
