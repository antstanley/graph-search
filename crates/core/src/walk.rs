//! The filesystem walk every mode shares (`SPEC.md` §4.5, §6.1).
//!
//! The exclusion policy is [`WalkPolicy`], not scattered flags: ignore files
//! are honoured by default (the deliberate divergence from `nanus`), hidden
//! entries and always-skip directories are not, and files above the size cap
//! are skipped.

use crate::Result;
use crate::config::{VCS_DIRS, WalkPolicy};
use crate::error::Error;
use graph_search_types::coverage::Coverage;
use graph_search_types::kind::Language;
use graph_search_types::limits::{MAX_FILES, MAX_WALK_ENTRIES};
use graph_search_types::result::{Truncation, TruncationKind};
use std::path::{Path, PathBuf};

/// One walked file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WalkEntry {
    /// The absolute path.
    pub path: PathBuf,
    /// The workspace-relative path, `/`-separated.
    pub rel: String,
    /// The language claimed by extension, when any.
    pub language: Option<Language>,
    /// Size in bytes.
    pub size: u64,
    /// Modification time in nanoseconds since the epoch, when readable.
    pub mtime_ns: u64,
}

impl WalkEntry {
    /// Whether the file should be read and parsed (claimed extension, enabled
    /// language, size within the cap).
    #[must_use]
    pub fn is_parseable(&self, policy: &WalkPolicy) -> bool {
        self.language
            .is_none_or(|language| policy.is_enabled(language))
            && self.size <= policy.max_file_bytes
    }
}

/// Walks `search_root` (the workspace root, or the `--path` sub-directory
/// under it) and returns the files, sorted by path as `nanus` walks them.
///
/// # Errors
///
/// When the search root does not exist or the walk cannot start.
pub fn walk(search_root: &Path, policy: &WalkPolicy) -> Result<Vec<WalkEntry>> {
    let report = walk_report(search_root, policy)?;
    report.require_complete()?;
    Ok(report.entries)
}

/// A possibly partial enumeration with explicit coverage.
#[derive(Clone, Debug)]
pub struct WalkReport {
    /// Observed package-manifest paths, including files over the source-size cap.
    pub package_boundaries: std::collections::BTreeSet<String>,
    /// Admitted files in deterministic path order.
    pub entries: Vec<WalkEntry>,
    /// Coverage under the configured inclusion policy.
    pub coverage: Coverage,
}

impl WalkReport {
    /// Requires complete enumeration before a mutation may infer removals.
    ///
    /// # Errors
    /// Reports the observed failures or limits without modifying the index.
    pub fn require_complete(&self) -> Result<()> {
        if self.coverage.enumeration_complete != Some(true) {
            return Err(Error::IncompleteWalk(format!(
                "{} admitted files, {} entry errors, {} exhausted limits",
                self.entries.len(),
                self.coverage.unreadable_entries,
                self.coverage.truncations.len()
            )));
        }
        Ok(())
    }
}

/// Enumerates the tree without disguising limits or unreadable entries.
/// Search consumers may use partial results; mutation consumers use `walk`.
///
/// # Errors
/// When the root is missing or is not a directory.
pub fn walk_report(search_root: &Path, policy: &WalkPolicy) -> Result<WalkReport> {
    walk_report_checked(search_root, policy, MAX_WALK_ENTRIES, || Ok(()))
}

/// Enumerates source under shared query entry limits and cancellation checkpoints.
/// Counts processed visible entries; one lookahead entry can detect truncation.
/// # Errors
/// On cancellation, deadline, or an invalid root.
pub fn walk_report_with_work(
    search_root: &Path,
    policy: &WalkPolicy,
    work: &mut crate::work::WorkBudget,
) -> Result<WalkReport> {
    let remaining = work.remaining_walk_entries();
    let report = walk_report_checked(search_root, policy, remaining, || work.check())?;
    let exhausted = report.coverage.entries_visited >= remaining as u64
        && report
            .coverage
            .truncations
            .iter()
            .any(|item| item.kind == TruncationKind::WalkEntries);
    work.charge_walk_entries(report.coverage.entries_visited, exhausted);
    Ok(report)
}

#[allow(clippy::too_many_lines)]
fn walk_report_checked(
    search_root: &Path,
    policy: &WalkPolicy,
    query_entry_cap: usize,
    mut check: impl FnMut() -> Result<()>,
) -> Result<WalkReport> {
    check()?;
    if !search_root.is_dir() {
        return Err(Error::RootMissing {
            root: search_root.to_path_buf(),
        });
    }
    let mut coverage = Coverage {
        policy: Some(graph_search_types::coverage::SourcePolicy {
            include_hidden: policy.include_hidden,
            respect_ignore: policy.respect_ignore,
            excludes: policy.excludes.clone(),
            max_file_bytes: policy.max_file_bytes,
            fingerprint: policy.fingerprint(),
        }),
        enumeration_complete: Some(true),
        ..Coverage::default()
    };
    let file_cap = policy.max_files.min(MAX_FILES);
    let entry_cap = policy
        .max_walk_entries
        .min(MAX_WALK_ENTRIES)
        .min(query_entry_cap);
    let mut builder = ignore::WalkBuilder::new(search_root);
    builder.hidden(!policy.include_hidden).require_git(false);
    builder.sort_by_file_path(std::cmp::Ord::cmp);
    if policy.respect_ignore {
        builder
            .ignore(true)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .parents(true);
    } else {
        builder
            .ignore(false)
            .git_ignore(false)
            .git_global(false)
            .git_exclude(false)
            .parents(false);
    }
    let excludes = policy.excludes.clone();
    // Query scopes may name the same subtree through a symlink or `..`.
    // Descendant directory symlinks are not followed by WalkBuilder. Translate
    // exclusions once, avoiding a canonicalization or path allocation per entry.
    let actual_root = search_root
        .canonicalize()
        .map_err(|source| Error::io(search_root, source))?;
    let exclude_root = policy
        .excluded_paths
        .iter()
        .any(|path| actual_root.starts_with(path));
    let excluded_paths: Vec<_> = policy
        .excluded_paths
        .iter()
        .filter_map(|path| path.strip_prefix(&actual_root).ok())
        .map(|relative| search_root.join(relative))
        .collect();
    builder.filter_entry(move |entry| {
        let kind = entry.file_type();
        !exclude_root
            && !excluded_paths
                .iter()
                .any(|path| entry.path().starts_with(path))
            && !is_skipped_dir(entry.path(), kind, &excludes)
    });

    let mut entries: Vec<WalkEntry> = Vec::new();
    let mut package_boundaries = std::collections::BTreeSet::new();
    let mut walker = builder.build();
    loop {
        check()?;
        let Some(entry) = walker.next() else { break };
        check()?;
        if coverage.entries_visited >= entry_cap as u64 {
            coverage.enumeration_complete = Some(false);
            coverage.truncations.push(Truncation::new(
                TruncationKind::WalkEntries,
                entry_cap as u64,
                "filesystem entry enumeration reached its cap",
            ));
            break;
        }
        coverage.entries_visited = coverage.entries_visited.saturating_add(1);
        let Ok(entry) = entry else {
            coverage.unreadable_entries = coverage.unreadable_entries.saturating_add(1);
            coverage.enumeration_complete = Some(false);
            continue;
        };
        if entry.error().is_some() {
            coverage.unreadable_entries = coverage.unreadable_entries.saturating_add(1);
            coverage.enumeration_complete = Some(false);
        }
        if entry.depth() == 0 {
            continue; // the search root itself
        }
        let is_file = entry.file_type().is_some_and(|kind| kind.is_file());
        if !is_file {
            continue;
        }
        let path = entry.into_path();
        let Ok(metadata) = std::fs::metadata(&path) else {
            coverage.unreadable_entries = coverage.unreadable_entries.saturating_add(1);
            coverage.enumeration_complete = Some(false);
            continue;
        };
        let rel = rel_to_root(search_root, &path);
        if crate::packages::manifest_family(&rel).is_some() {
            package_boundaries.insert(rel.clone());
        }
        let size = metadata.len();
        if size > policy.max_file_bytes {
            coverage.oversized_files = coverage.oversized_files.saturating_add(1);
            continue;
        }
        let language = policy.language_for(Path::new(&rel));
        let modified = if let Ok(modified) = metadata.modified() {
            Some(modified)
        } else {
            coverage.unreadable_entries = coverage.unreadable_entries.saturating_add(1);
            coverage.enumeration_complete = Some(false);
            None
        };
        let mtime_ns = modified
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        if entries.len() >= file_cap {
            coverage.enumeration_complete = Some(false);
            coverage.truncations.push(Truncation::new(
                TruncationKind::Files,
                file_cap as u64,
                "admitted file enumeration reached its cap",
            ));
            break;
        }
        if language.is_none() {
            coverage.unsupported_files = coverage.unsupported_files.saturating_add(1);
        }
        if language.is_some_and(|lang| !policy.is_enabled(lang)) {
            coverage.disabled_language_files = coverage.disabled_language_files.saturating_add(1);
        }
        entries.push(WalkEntry {
            path,
            rel,
            language,
            size,
            mtime_ns,
        });
    }
    check()?;
    entries.sort_by(|a, b| a.rel.cmp(&b.rel).then_with(|| a.path.cmp(&b.path)));
    check()?;
    // OKF bundle membership depends on the whole admitted set: an unclaimed
    // `.md` file under an `index.md` is an OKF document (`SPEC.md` §7.6). With
    // `okf` disabled membership is never decided, so the language cannot drift
    // between a sync (which then skips the bundle-change reindex) and a rebuild.
    let okf_dirs = if policy.is_enabled(Language::Okf) {
        crate::okf::index_dirs(entries.iter().map(|entry| entry.rel.as_str()))
    } else {
        std::collections::BTreeSet::new()
    };
    for entry in &mut entries {
        if entry.language.is_none() && crate::okf::is_member(&entry.rel, &okf_dirs) {
            entry.language = Some(Language::Okf);
            coverage.unsupported_files = coverage.unsupported_files.saturating_sub(1);
        }
    }
    coverage.admitted_files = entries.len() as u64;
    Ok(WalkReport {
        package_boundaries,
        entries,
        coverage,
    })
}

/// Resolves the `--path` sub-directory against the root, defaulting to the
/// root itself. Refuses a missing root.
///
/// # Errors
///
/// [`Error::RootMissing`] when the resolved directory does not exist.
pub fn resolve_search_root(root: &Path, subdir: Option<&str>) -> Result<PathBuf> {
    if let Some(subdir) = subdir {
        crate::work::validate_query_bytes("search path", subdir)?;
    }
    let resolved = match subdir {
        None | Some("") => root.to_path_buf(),
        Some(sub) => {
            let cleaned = sub.trim_start_matches("./");
            let cleaned = cleaned.strip_suffix('/').unwrap_or(cleaned);
            if cleaned.is_empty() {
                root.to_path_buf()
            } else {
                root.join(cleaned)
            }
        }
    };
    if !resolved.is_dir() {
        return Err(Error::RootMissing { root: resolved });
    }
    Ok(resolved)
}

/// The workspace-relative form of `path` (which sits under `search_root`,
/// itself under the workspace root or equal to it).
#[must_use]
pub fn rel_to_root(search_root: &Path, path: &Path) -> String {
    path.strip_prefix(search_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_skipped_dir(path: &Path, kind: Option<std::fs::FileType>, excludes: &[String]) -> bool {
    if kind.is_some_and(|k| k.is_dir()) {
        let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else {
            return false;
        };
        if VCS_DIRS.contains(&name) || name == ".graph-search" {
            return true;
        }
        return excludes.iter().any(|ex| ex == name);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &[u8]) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))
            .unwrap_or_else(|_| panic!("create parent for {rel}"));
        std::fs::write(path, contents).unwrap_or_else(|_| panic!("write {rel}"));
    }

    #[test]
    fn the_walker_honours_ignore_files_and_always_skip_dirs() {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        write(
            root,
            "src/a.rs",
            b"fn a() {}
",
        );
        write(
            root,
            "target/x.rs",
            b"generated
",
        );
        write(
            root,
            "node_modules/y.js",
            b"var y;
",
        );
        write(
            root,
            "ignored/z.rs",
            b"fn z() {}
",
        );
        write(
            root,
            ".gitignore",
            b"ignored/
",
        );

        let policy = WalkPolicy::default();
        let found = walk(root, &policy).unwrap_or_else(|e| panic!("walk: {e}"));
        let rels: Vec<&str> = found.iter().map(|e| e.rel.as_str()).collect();
        // `.gitignore` is hidden, so the default walk does not list it.
        assert_eq!(rels, vec!["src/a.rs"], "{rels:?}");
    }

    #[test]
    fn no_ignore_descends_where_ignore_was_honoured() {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        write(
            root,
            "src/a.rs",
            b"fn a() {}
",
        );
        write(
            root,
            "ignored/z.rs",
            b"fn z() {}
",
        );
        write(
            root,
            ".gitignore",
            b"ignored/
",
        );

        let policy = WalkPolicy {
            respect_ignore: false,
            include_hidden: true,
            ..WalkPolicy::default()
        };
        let found = walk(root, &policy).unwrap_or_else(|e| panic!("walk: {e}"));
        let rels: Vec<&str> = found.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(
            rels,
            vec![".gitignore", "ignored/z.rs", "src/a.rs"],
            "{rels:?}"
        );
    }

    #[test]
    fn hidden_files_need_the_flag() {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        write(
            root,
            "src/a.rs",
            b"fn a() {}
",
        );
        write(
            root,
            ".hidden.rs",
            b"fn h() {}
",
        );

        let policy = WalkPolicy::default();
        let found = walk(root, &policy).unwrap_or_else(|e| panic!("walk: {e}"));
        assert_eq!(found.len(), 1);

        let policy = WalkPolicy {
            include_hidden: true,
            ..WalkPolicy::default()
        };
        let found = walk(root, &policy).unwrap_or_else(|e| panic!("walk: {e}"));
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn oversized_files_are_skipped() {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        write(
            root,
            "src/small.rs",
            b"fn a() {}
",
        );
        write(root, "src/big.rs", &vec![b'x'; 4096]);

        let policy = WalkPolicy {
            max_file_bytes: 1024,
            ..WalkPolicy::default()
        };
        let found = walk(root, &policy).unwrap_or_else(|e| panic!("walk: {e}"));
        let rels: Vec<&str> = found.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, vec!["src/small.rs"], "{rels:?}");
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    #[test]
    fn file_cap_is_deterministic_and_requires_an_omitted_file() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("z.rs"), "z").unwrap();
        std::fs::write(root.path().join("a.rs"), "a").unwrap();
        let policy = WalkPolicy {
            max_files: 1,
            ..WalkPolicy::default()
        };
        let report = walk_report(root.path(), &policy).unwrap();
        assert_eq!(report.entries[0].rel, "a.rs");
        assert_eq!(report.coverage.enumeration_complete, Some(false));
        assert_eq!(report.coverage.admitted_files, 1);
        assert!(matches!(
            walk(root.path(), &policy),
            Err(Error::IncompleteWalk(_))
        ));
        std::fs::remove_file(root.path().join("z.rs")).unwrap();
        let report = walk_report(root.path(), &policy).unwrap();
        assert_eq!(report.coverage.enumeration_complete, Some(true));
        assert!(report.coverage.truncations.is_empty());
    }

    #[test]
    fn directory_budget_works_without_admitted_files() {
        let root = tempfile::tempdir().unwrap();
        for name in ["a", "b", "c"] {
            std::fs::create_dir(root.path().join(name)).unwrap();
        }
        let policy = WalkPolicy {
            max_walk_entries: 2,
            ..WalkPolicy::default()
        };
        let report = walk_report(root.path(), &policy).unwrap();
        assert!(report.entries.is_empty());
        assert_eq!(report.coverage.entries_visited, 2);
        assert_eq!(
            report.coverage.truncations[0].kind,
            TruncationKind::WalkEntries
        );
        assert_eq!(report.coverage.enumeration_complete, Some(false));
    }

    #[test]
    fn exclusions_are_distinct_from_enumeration_failures() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("large.rs"), "too large").unwrap();
        std::fs::write(root.path().join("small.txt"), "a").unwrap();
        std::fs::write(root.path().join("code.rs"), "b").unwrap();
        let policy = WalkPolicy {
            max_file_bytes: 2,
            languages: vec![],
            ..WalkPolicy::default()
        };
        let report = walk_report(root.path(), &policy).unwrap();
        assert_eq!(report.coverage.enumeration_complete, Some(true));
        assert_eq!(report.coverage.oversized_files, 1);
        assert_eq!(report.coverage.unsupported_files, 1);
        assert_eq!(report.coverage.disabled_language_files, 1);
        assert_eq!(
            report.coverage.policy.unwrap().fingerprint,
            policy.fingerprint()
        );
    }

    #[test]
    fn invalid_ignore_files_are_reported_and_cannot_authorize_deletion() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(".ignore"), "[z-a]\n").unwrap();
        std::fs::write(root.path().join("a.rs"), "fn a() {}").unwrap();
        let report = walk_report(root.path(), &WalkPolicy::default()).unwrap();
        assert!(report.coverage.unreadable_entries > 0);
        assert_eq!(report.coverage.enumeration_complete, Some(false));
        assert!(report.require_complete().is_err());
    }
    #[test]
    fn cancellation_during_enumeration_does_not_return_a_partial_success() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.rs"), "fn a() {}\n").unwrap();
        let token = crate::work::CancellationToken::default();
        let work = crate::work::WorkBudget::new(crate::work::WorkLimits {
            cancellation: Some(token.clone()),
            ..crate::work::WorkLimits::default()
        });
        let mut checkpoints = 0usize;
        let result = walk_report_checked(
            root.path(),
            &WalkPolicy::default(),
            MAX_WALK_ENTRIES,
            || {
                checkpoints = checkpoints.saturating_add(1);
                if checkpoints == 4 {
                    token.cancel();
                }
                work.check()
            },
        );
        assert!(matches!(result, Err(Error::QueryCancelled)));
        assert_eq!(checkpoints, 4);
    }
}
