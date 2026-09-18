//! The filesystem walk every mode shares (`SPEC.md` §4.5, §6.1).
//!
//! The exclusion policy is [`WalkPolicy`], not scattered flags: ignore files
//! are honoured by default (the deliberate divergence from `nanus`), hidden
//! entries and always-skip directories are not, and files above the size cap
//! are skipped.

use crate::Result;
use crate::config::{VCS_DIRS, WalkPolicy};
use crate::error::Error;
use graph_search_types::kind::Language;
use graph_search_types::limits::MAX_FILES;
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
    let mut builder = ignore::WalkBuilder::new(search_root);
    builder.hidden(!policy.include_hidden).require_git(false);
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
    builder.filter_entry(move |entry| {
        let kind = entry.file_type();
        !is_skipped_dir(entry.path(), kind, &excludes)
    });

    let mut entries: Vec<WalkEntry> = Vec::new();
    for entry in builder.build() {
        // `flatten` semantics: an unreadable corner of the tree is skipped,
        // not fatal to the search (the same posture as `nanus`).
        let Ok(entry) = entry else { continue };
        if entry.depth() == 0 {
            continue; // the search root itself
        }
        let is_file = entry.file_type().is_some_and(|kind| kind.is_file());
        if !is_file {
            continue;
        }
        let path = entry.into_path();
        let metadata = std::fs::metadata(&path).ok();
        let size = metadata.as_ref().map_or(0, std::fs::Metadata::len);
        if size > policy.max_file_bytes {
            continue;
        }
        let rel = rel_to_root(search_root, &path);
        let language = policy.language_for(Path::new(&rel));
        let mtime_ns = metadata
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        entries.push(WalkEntry {
            path,
            rel,
            language,
            size,
            mtime_ns,
        });
        if entries.len() >= MAX_FILES {
            break; // a pathological tree degrades by reporting, not hanging
        }
    }
    entries.sort_by(|a, b| a.rel.cmp(&b.rel).then_with(|| a.path.cmp(&b.path)));
    Ok(entries)
}

/// Resolves the `--path` sub-directory against the root, defaulting to the
/// root itself. Refuses a missing root.
///
/// # Errors
///
/// [`Error::RootMissing`] when the resolved directory does not exist.
pub fn resolve_search_root(root: &Path, subdir: Option<&str>) -> Result<PathBuf> {
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
