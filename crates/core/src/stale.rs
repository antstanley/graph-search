//! The cheap staleness scan behind lazy reconcile (`SPEC.md` §6.5).
//!
//! Metadata mode marks a file *suspect* when its `(size, mtime)` differs from the
//! manifest, or it appears or disappears. A suspect set means the index is
//! stale; the command then reconciles unless told not to, and reports either
//! way. Content verification additionally compares source hashes.

use crate::Result;
use crate::config::WalkPolicy;
use crate::walk::{resolve_search_root, walk_report};
use graph_search_types::manifest::Manifest;
use graph_search_types::result::Staleness;
use std::path::Path;

/// Scans the tree against the manifest without reading file contents.
///
/// # Errors
///
/// When the walk fails.
pub fn check(search_root: &Path, manifest: &Manifest, policy: &WalkPolicy) -> Result<Staleness> {
    Ok(inspect(search_root, manifest, policy, false)?.0)
}

/// Returns freshness observations together with enumeration coverage.
/// Missing paths imply drift only after a complete enumeration.
///
/// # Errors
/// When the root cannot be enumerated.
pub fn inspect(
    search_root: &Path,
    manifest: &Manifest,
    policy: &WalkPolicy,
    content: bool,
) -> Result<(Staleness, graph_search_types::coverage::Coverage)> {
    inspect_inner(search_root, manifest, policy, content, None)
}

/// Verifies freshness under a shared request budget. Partial checks are errors,
/// never successful observations that could be mistaken for current source.
/// # Errors
/// On incomplete enumeration, source-budget exhaustion, cancellation or deadline.
pub fn inspect_with_work(
    search_root: &Path,
    manifest: &Manifest,
    policy: &WalkPolicy,
    content: bool,
    work: &mut crate::work::WorkBudget,
) -> Result<(Staleness, graph_search_types::coverage::Coverage)> {
    inspect_inner(search_root, manifest, policy, content, Some(work))
}

fn inspect_inner(
    search_root: &Path,
    manifest: &Manifest,
    policy: &WalkPolicy,
    content: bool,
    mut work: Option<&mut crate::work::WorkBudget>,
) -> Result<(Staleness, graph_search_types::coverage::Coverage)> {
    let report = if let Some(work) = work.as_deref_mut() {
        let report = crate::walk::walk_report_with_work(search_root, policy, work)?;
        report.require_complete()?;
        report
    } else {
        walk_report(search_root, policy)?
    };
    let entries = report.entries;
    let mut coverage = report.coverage;
    coverage.quarantined_files = manifest
        .entries
        .values()
        .filter(|entry| entry.quarantine.is_some())
        .count() as u64;

    let mut changed: Vec<String> = report
        .package_boundaries
        .difference(&manifest.package_boundaries)
        .cloned()
        .collect();
    if coverage.enumeration_complete == Some(true) {
        changed.extend(
            manifest
                .package_boundaries
                .difference(&report.package_boundaries)
                .cloned(),
        );
    }

    let walked: std::collections::BTreeSet<&str> = entries.iter().map(|e| e.rel.as_str()).collect();
    for path in manifest.entries.keys() {
        if let Some(work) = work.as_deref() {
            work.check()?;
        }
        if coverage.enumeration_complete == Some(true) && !walked.contains(path.as_str()) {
            changed.push(path.clone());
        }
    }
    let policy_changed =
        manifest.policy_fingerprint.as_deref() != Some(policy.fingerprint().as_str());
    for walked_entry in &entries {
        if let Some(work) = work.as_deref() {
            work.check()?;
        }
        match manifest.get(&walked_entry.rel) {
            None => changed.push(walked_entry.rel.clone()),
            Some(entry) => {
                let version_mismatch = manifest.parser_version
                    != graph_search_types::PARSER_VERSION
                    || manifest.schema_version != graph_search_types::SCHEMA_VERSION
                    || !manifest.versions().retrieval_is_current()
                    || manifest.occurrence_version
                        != graph_search_types::limits::OCCURRENCE_VERSION
                    || !entry.matches_versions(
                        graph_search_types::PARSER_VERSION,
                        graph_search_types::SCHEMA_VERSION,
                    );
                if crate::manifest::entry_differs(entry, walked_entry)
                    || version_mismatch
                    || policy_changed
                {
                    changed.push(walked_entry.rel.clone());
                }
            }
        }
    }
    changed.sort();
    changed.dedup();
    let mut staleness = Staleness {
        changed: changed.len() as u64,
        changed_paths: changed,
    };
    if content {
        if let Some(work) = work.as_deref_mut() {
            verify_content_with_work(
                search_root,
                manifest,
                policy,
                &mut staleness,
                &mut coverage,
                work,
            )?;
        } else {
            verify_content(search_root, manifest, policy, &mut staleness, &mut coverage);
        }
    }
    if let Some(work) = work {
        work.check()?;
    }
    Ok((staleness, coverage))
}

fn verify_content_with_work(
    root: &Path,
    manifest: &Manifest,
    policy: &WalkPolicy,
    result: &mut Staleness,
    coverage: &mut graph_search_types::coverage::Coverage,
    work: &mut crate::work::WorkBudget,
) -> Result<()> {
    for (path, entry) in &manifest.entries {
        let changed = match crate::source_capture::read(
            &root.join(path),
            policy.max_file_bytes.saturating_add(1),
            work,
        )? {
            crate::source_capture::ReadOutcome::BudgetExceeded => {
                return Err(crate::Error::IncompleteVerification(
                    "source-read allowance exhausted".into(),
                ));
            }
            crate::source_capture::ReadOutcome::Unavailable => {
                coverage.source_read_errors = coverage.source_read_errors.saturating_add(1);
                true
            }
            crate::source_capture::ReadOutcome::Bytes(bytes) => {
                bytes.len() as u64 > policy.max_file_bytes
                    || crate::hash::content_hash(&bytes) != entry.content_hash
            }
        };
        if changed {
            result.changed_paths.push(path.clone());
        }
    }
    work.check()?;
    result.changed_paths.sort();
    result.changed_paths.dedup();
    result.changed = result.changed_paths.len() as u64;
    Ok(())
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

/// Checks content fingerprints as well as metadata. Each file is observed
/// independently; this does not claim an atomic snapshot of the live tree.
///
/// # Errors
/// Propagates enumeration errors. Unreadable files are reported as changed.
pub fn check_content(root: &Path, manifest: &Manifest, policy: &WalkPolicy) -> Result<Staleness> {
    Ok(inspect(root, manifest, policy, true)?.0)
}

fn verify_content(
    root: &Path,
    manifest: &Manifest,
    policy: &WalkPolicy,
    result: &mut Staleness,
    coverage: &mut graph_search_types::coverage::Coverage,
) {
    use std::io::Read;
    for (path, entry) in &manifest.entries {
        let mut bytes = Vec::new();
        let read = std::fs::File::open(root.join(path)).and_then(|file| {
            file.take(policy.max_file_bytes.saturating_add(1))
                .read_to_end(&mut bytes)
        });
        if read.is_err() {
            coverage.source_read_errors = coverage.source_read_errors.saturating_add(1);
        }
        if read.is_err()
            || bytes.len() as u64 > policy.max_file_bytes
            || crate::hash::content_hash(&bytes) != entry.content_hash
        {
            result.changed_paths.push(path.clone());
        }
    }
    result.changed_paths.sort();
    result.changed_paths.dedup();
    result.changed = result.changed_paths.len() as u64;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::work::{WorkBudget, WorkLimits};

    fn fixture() -> (tempfile::TempDir, Manifest, WalkPolicy) {
        let root = tempfile::tempdir().unwrap();
        for name in ["a.txt", "b.txt"] {
            std::fs::write(root.path().join(name), "same").unwrap();
        }
        let policy = WalkPolicy::default();
        let mut manifest = Manifest::new(
            graph_search_types::PARSER_VERSION,
            graph_search_types::SCHEMA_VERSION,
        );
        manifest.policy_fingerprint = Some(policy.fingerprint());
        for entry in walk_report(root.path(), &policy).unwrap().entries {
            manifest.entries.insert(
                entry.rel,
                graph_search_types::manifest::FileEntry {
                    size: entry.size,
                    mtime_ns: entry.mtime_ns,
                    content_hash: crate::hash::content_hash(b"same"),
                    parser_version: graph_search_types::PARSER_VERSION,
                    schema_version: graph_search_types::SCHEMA_VERSION,
                    quarantine: None,
                    extraction: None,
                },
            );
        }
        (root, manifest, policy)
    }

    #[test]
    fn bounded_freshness_requires_complete_work_and_allows_exact_limits() {
        let (root, manifest, policy) = fixture();
        let mut exact = WorkBudget::new(WorkLimits {
            walk_entries: 3,
            source_files: 2,
            source_bytes: 8,
            ..WorkLimits::default()
        });
        let (fresh, coverage) =
            inspect_with_work(root.path(), &manifest, &policy, true, &mut exact).unwrap();
        assert_eq!(fresh.changed, 0);
        assert_eq!(coverage.enumeration_complete, Some(true));
        assert_eq!(exact.source_report(), (2, 8));
        assert_eq!(exact.remaining_walk_entries(), 0);
        assert!(exact.report().2.is_empty());
        for limits in [
            WorkLimits {
                walk_entries: 2,
                ..WorkLimits::default()
            },
            WorkLimits {
                source_files: 1,
                ..WorkLimits::default()
            },
            WorkLimits {
                source_bytes: 7,
                ..WorkLimits::default()
            },
        ] {
            let mut work = WorkBudget::new(limits);
            assert!(matches!(
                inspect_with_work(root.path(), &manifest, &policy, true, &mut work),
                Err(crate::Error::IncompleteWalk(_) | crate::Error::IncompleteVerification(_))
            ));
        }
    }

    #[test]
    fn metadata_reads_no_source_and_content_detects_same_metadata_edits() {
        let (root, manifest, policy) = fixture();
        let path = root.path().join("a.txt");
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        std::fs::write(&path, "edit").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        let mut metadata = WorkBudget::new(WorkLimits {
            source_files: 0,
            source_bytes: 0,
            ..WorkLimits::default()
        });
        let result =
            inspect_with_work(root.path(), &manifest, &policy, false, &mut metadata).unwrap();
        assert_eq!(result.0.changed, 0);
        assert_eq!(metadata.source_report(), (0, 0));
        let mut content = WorkBudget::new(WorkLimits::default());
        let result =
            inspect_with_work(root.path(), &manifest, &policy, true, &mut content).unwrap();
        assert_eq!(result.0.changed_paths, ["a.txt"]);
        assert_eq!(content.source_report(), (2, 8));
    }
}
