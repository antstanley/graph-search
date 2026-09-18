//! The `files` mode: a glob over the walked tree (`SPEC.md` §8.1).
//!
//! Semantics are `nanus` `glob`, exactly: the pattern is anchored to the
//! search root, `*` does not cross `/`, results are capped, and a hit cap
//! says so.

use crate::Result;
use crate::config::WalkPolicy;
use crate::error::Error;
use crate::walk::{resolve_search_root, walk};
use globset::{GlobBuilder, GlobSet};
use graph_search_types::result::{FileHit, FilesResult, Stats, Truncation};
use std::path::Path;

/// Compiles an anchored glob: `*.rs` is top-level only, `**/*.rs` is any
/// depth (`SPEC.md` §8.1).
///
/// # Errors
///
/// [`Error::InvalidPattern`] when the pattern is empty or does not compile.
pub fn compile_anchored_glob(pattern: &str) -> Result<GlobSet> {
    let trimmed = pattern.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidPattern {
            pattern: pattern.to_owned(),
            reason: String::from("the pattern is empty"),
        });
    }
    let glob = GlobBuilder::new(trimmed)
        .literal_separator(true)
        .build()
        .map_err(|error| Error::InvalidPattern {
            pattern: pattern.to_owned(),
            reason: error.to_string(),
        })?;
    let mut builder = globset::GlobSetBuilder::new();
    builder.add(glob);
    builder.build().map_err(|error| Error::InvalidPattern {
        pattern: pattern.to_owned(),
        reason: error.to_string(),
    })
}

/// Runs the `files` query over the tree.
///
/// # Errors
///
/// When the root is missing or the pattern does not compile.
pub fn search_files(
    root: &Path,
    query: &graph_search_types::FilesQuery,
    policy: &WalkPolicy,
) -> Result<FilesResult> {
    let started = std::time::Instant::now();
    let search_root = resolve_search_root(root, query.path.as_deref())?;
    let set = compile_anchored_glob(&query.pattern)?;

    let entries = walk(&search_root, policy)?;
    let mut items: Vec<FileHit> = Vec::new();
    let mut truncated = false;
    for entry in &entries {
        // Matched against the path relative to the search root, so the
        // pattern anchors where the caller asked it to (`SPEC.md` §8.1).
        let rel = crate::walk::rel_to_root(&search_root, &entry.path);
        if !set.is_match(rel) {
            continue;
        }
        if items.len() >= usize::try_from(query.limit).unwrap_or(usize::MAX) {
            truncated = true;
            break;
        }
        items.push(FileHit {
            path: entry.rel.clone(),
            language: entry
                .language
                .unwrap_or(graph_search_types::Language::Unknown),
        });
    }

    let mut truncations = Vec::new();
    if truncated {
        truncations.push(Truncation::new(
            graph_search_types::result::TruncationKind::Files,
            u64::from(query.limit),
            format!(
                "(more than {} matches; narrow the pattern to see the rest)",
                query.limit
            ),
        ));
    }
    Ok(FilesResult {
        items,
        truncations,
        stats: Stats {
            files_scanned: entries.len() as u64,
            matches: 0,
            candidates: 0,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup() -> TempDir {
        let tmp = TempDir::new().unwrap_or_else(|e| panic!("tmp: {e}"));
        let root = tmp.path();
        for rel in [
            "src/main.rs",
            "src/lib.rs",
            "src/deep/mod.rs",
            "README.md",
            "site.css",
        ] {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap_or_else(|| Path::new(".")))
                .unwrap_or_else(|e| panic!("mkdir {rel}: {e}"));
            std::fs::write(
                path, "x
",
            )
            .unwrap_or_else(|e| panic!("write {rel}: {e}"));
        }
        tmp
    }

    fn run(root: &Path, pattern: &str) -> FilesResult {
        let query = graph_search_types::FilesQuery::new(pattern);
        search_files(root, &query, &WalkPolicy::default()).unwrap_or_else(|e| panic!("search: {e}"))
    }

    #[test]
    fn a_bare_pattern_is_top_level_only() {
        let tmp = setup();
        let found = run(tmp.path(), "*.rs");
        assert!(found.items.is_empty(), "{:?}", found.items);
        let found = run(tmp.path(), "*.md");
        assert_eq!(
            found.items.first().map(|h| h.path.as_str()),
            Some("README.md")
        );
    }

    #[test]
    fn double_star_reaches_every_depth() {
        let tmp = setup();
        let found = run(tmp.path(), "**/*.rs");
        let paths: Vec<&str> = found.items.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["src/deep/mod.rs", "src/lib.rs", "src/main.rs"],
            "{paths:?}"
        );
    }

    #[test]
    fn the_cap_says_so() {
        let tmp = setup();
        let query = graph_search_types::FilesQuery::new("**/*.rs").with_limit(2);
        let found = search_files(tmp.path(), &query, &WalkPolicy::default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(found.items.len(), 2);
        assert_eq!(found.truncations.len(), 1);
        assert_eq!(
            found.truncations[0].message,
            "(more than 2 matches; narrow the pattern to see the rest)"
        );
    }

    #[test]
    fn an_empty_pattern_is_refused() {
        let tmp = setup();
        let query = graph_search_types::FilesQuery::new("   ");
        let error = search_files(tmp.path(), &query, &WalkPolicy::default())
            .err()
            .unwrap_or_else(|| panic!("an empty pattern must be refused"));
        assert!(
            error.to_string().contains("the pattern is empty"),
            "{error}"
        );
    }
}
