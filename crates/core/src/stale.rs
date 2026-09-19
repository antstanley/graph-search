//! The cheap staleness scan behind lazy reconcile (`SPEC.md` §6.5).
//!
//! No hashing: a file is *suspect* when its `(size, mtime)` differs from the
//! manifest, or it appears or disappears. A suspect set means the index is
//! stale; the command then reconciles unless told not to, and reports either
//! way.

use crate::Result;
use crate::config::WalkPolicy;
use crate::walk::{resolve_search_root, walk};
use graph_search_types::manifest::Manifest;
use graph_search_types::result::Staleness;
use std::path::Path;

/// Scans the tree against the manifest without reading file contents.
///
/// # Errors
///
/// When the walk fails.
pub fn check(search_root: &Path, manifest: &Manifest, policy: &WalkPolicy) -> Result<Staleness> {
    let entries = walk(search_root, policy)?;
    let mut changed: Vec<String> = Vec::new();

    let walked: std::collections::BTreeSet<&str> = entries.iter().map(|e| e.rel.as_str()).collect();
    for path in manifest.entries.keys() {
        if !walked.contains(path.as_str()) {
            changed.push(path.clone());
        }
    }
    for walked_entry in &entries {
        match manifest.get(&walked_entry.rel) {
            None => changed.push(walked_entry.rel.clone()),
            Some(entry) => {
                let version_mismatch = manifest.parser_version
                    != graph_search_types::PARSER_VERSION
                    || manifest.schema_version != graph_search_types::SCHEMA_VERSION
                    || !entry.matches_versions(
                        graph_search_types::PARSER_VERSION,
                        graph_search_types::SCHEMA_VERSION,
                    );
                if crate::manifest::entry_differs(entry, walked_entry) || version_mismatch {
                    changed.push(walked_entry.rel.clone());
                }
            }
        }
    }
    changed.sort();
    changed.dedup();
    Ok(Staleness {
        changed: changed.len() as u64,
        changed_paths: changed,
    })
}

/// Convenience: resolve the search root, then check.
///
/// # Errors
/// When the root is missing or the walk fails.
pub fn check_from_root(
    root: &Path,
    subdir: Option<&str>,
    manifest: &Manifest,
    policy: &WalkPolicy,
) -> Result<Staleness> {
    check(&resolve_search_root(root, subdir)?, manifest, policy)
}
