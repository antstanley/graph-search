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

/// Classifies walked entries, reading changed content and rename candidates.
#[must_use]
pub fn classify(entries: &[WalkEntry], manifest: &Manifest) -> Diff {
    let result = classify_with_hash(entries, manifest, |entry| {
        Ok::<_, std::convert::Infallible>(
            std::fs::read(&entry.path)
                .ok()
                .map(|bytes| crate::hash::content_hash(&bytes)),
        )
    });
    match result {
        Ok(diff) => diff,
        Err(never) => match never {},
    }
}

/// A fallible hash seam so maintenance can charge reads to its request budget.
/// Unreadable content is never equivalent to an empty file.
pub(crate) fn classify_with_hash<E>(
    entries: &[WalkEntry],
    manifest: &Manifest,
    mut hash: impl FnMut(&WalkEntry) -> std::result::Result<Option<String>, E>,
) -> std::result::Result<Diff, E> {
    let mut diff = Diff::default();
    let version_bump =
        manifest.parser_version != PARSER_VERSION || manifest.schema_version != SCHEMA_VERSION;
    diff.reindexed_all = version_bump && !manifest.is_empty();

    // Hash-based rename detection needs content; it happens after the cheap
    // pass, over the `removed` and unmanifested `added` sets only.
    let mut hash_pending = Vec::new();
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
                hash_pending.push((walked_entry, &entry.content_hash));
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
    for (entry, old_hash) in hash_pending {
        if hash(entry)?.as_ref() == Some(old_hash) {
            diff.unchanged.push(entry.rel.clone());
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
        // With no unpaired removals, a read cannot discover another rename.
        let added_hash = if consumed.len() < candidate_removed.len() {
            hash(added)?
        } else {
            None
        };
        if let Some(matches) = added_hash
            .as_deref()
            .and_then(|value| removed_by_hash.get_mut(value))
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

    Ok(diff)
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
        extraction: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> WalkEntry {
        WalkEntry {
            path: path.into(),
            rel: path.into(),
            language: None,
            size: 0,
            mtime_ns: 1,
        }
    }

    #[test]
    fn additions_without_removals_do_not_read_content() {
        let entries = [entry("new.rs")];
        let manifest = Manifest::new(PARSER_VERSION, SCHEMA_VERSION);
        let diff = classify_with_hash::<crate::Error>(&entries, &manifest, |_| {
            panic!("there is no possible rename")
        })
        .unwrap();
        assert_eq!(diff.added, entries);
    }

    #[test]
    fn unreadable_changed_empty_file_is_not_unchanged() {
        let current = entry("empty.rs");
        let mut stored = entry_for(&current, &crate::hash::content_hash(b""), None);
        stored.mtime_ns = 0;
        let mut manifest = Manifest::new(PARSER_VERSION, SCHEMA_VERSION);
        manifest.entries.insert(current.rel.clone(), stored);
        let diff =
            classify_with_hash::<crate::Error>(std::slice::from_ref(&current), &manifest, |_| {
                Ok(None)
            })
            .unwrap();
        assert_eq!(diff.modified, [current]);
        assert!(diff.unchanged.is_empty());
    }

    #[test]
    fn rename_hash_failure_propagates_and_unreadable_is_not_empty() {
        let old = entry("old.rs");
        let new = entry("new.rs");
        let mut manifest = Manifest::new(PARSER_VERSION, SCHEMA_VERSION);
        manifest.entries.insert(
            old.rel.clone(),
            entry_for(&old, &crate::hash::content_hash(b""), None),
        );
        let result = classify_with_hash(std::slice::from_ref(&new), &manifest, |_| {
            Err(crate::Error::QueryCancelled)
        });
        assert!(matches!(result, Err(crate::Error::QueryCancelled)));
        let diff =
            classify_with_hash::<crate::Error>(std::slice::from_ref(&new), &manifest, |_| Ok(None))
                .unwrap();
        assert!(diff.renamed.is_empty());
        assert_eq!(diff.added, [new]);
        assert_eq!(diff.removed, [old.rel]);
    }
}
