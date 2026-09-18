//! The `text` mode: a literal substring scan over the walked tree
//! (`SPEC.md` §8.2, §4.6).
//!
//! Sourced by scanning the files — never the index — because the graph holds
//! no bodies. Semantics are `nanus` `grep`, exactly: literal substring, one
//! positive `include` glob, per-line matches in walk order, capped with a
//! notice, binary files skipped.

use crate::Result;
use crate::config::WalkPolicy;
use crate::error::Error;
use crate::walk::{resolve_search_root, walk};
use globset::{Glob, GlobMatcher};
use graph_search_types::TextQuery;
use graph_search_types::limits::MAX_MATCH_LINE;
use graph_search_types::result::{Stats, TextHit, TextResult, Truncation, TruncationKind};
use std::path::Path;

/// How many leading bytes are sniffed for a NUL when classifying a file as
/// binary (the same sniff `nanus` makes).
const BINARY_SNIFF: usize = 8 * 1024;

/// Checks that an `include` filter is one positive glob, with the same
/// guidance `nanus` gives (`SPEC.md` §8.2).
///
/// # Errors
///
/// [`Error::InvalidInclude`] with the message explaining what to pass instead.
pub fn validate_include(include: &str) -> Result<()> {
    let trimmed = include.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInclude(String::from(
            "is empty; omit it to search every file",
        )));
    }
    if trimmed.contains(',') {
        return Err(Error::InvalidInclude(format!(
            "takes one glob, but {trimmed:?} lists several; search once per pattern"
        )));
    }
    if trimmed.starts_with('!') {
        return Err(Error::InvalidInclude(format!(
            "is a positive filter, but {trimmed:?} negates; name what to search instead"
        )));
    }
    Ok(())
}

/// Compiles the include filter. The pattern is matched against the relative
/// path *and* the file name, so `*.rs` works whether or not a directory was
/// prefixed (`SPEC.md` §8.2).
#[must_use]
pub fn compile_include(include: &str) -> Option<GlobMatcher> {
    Glob::new(include.trim()).ok().map(|g| g.compile_matcher())
}

/// Shortens a matched line so one pathological line cannot dominate the
/// result. The same truncation `nanus` renders.
#[must_use]
pub fn truncate_line(line: &str, max: usize) -> String {
    if line.len() <= max {
        return line.to_owned();
    }
    let mut end = max;
    while end > 0 && !line.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}…", line.get(..end).unwrap_or_default())
}

/// Reads a file for content search, returning `None` for binary or invalid
/// files.
fn text_for_search(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let sniff = bytes.get(..BINARY_SNIFF).unwrap_or(&bytes);
    if sniff.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Runs the `text` query over the tree.
///
/// The `include` glob is pushed down to the file level, so the cap counts
/// included matches; with no `include` the behaviour is byte-for-byte `nanus`
/// `grep` semantics.
///
/// # Errors
///
/// When the root is missing or the include filter is rejected.
#[allow(clippy::too_many_lines)] // the two matching paths share one loop shape
pub fn search_text(root: &Path, query: &TextQuery, policy: &WalkPolicy) -> Result<TextResult> {
    let started = std::time::Instant::now();
    if query.pattern.is_empty() {
        return Err(Error::InvalidPattern {
            pattern: query.pattern.clone(),
            reason: String::from("the pattern is empty"),
        });
    }
    if let Some(include) = query.include.as_deref() {
        validate_include(include)?;
    }
    let include = query.include.as_deref().and_then(compile_include);

    let search_root = resolve_search_root(root, query.path.as_deref())?;
    let entries = walk(&search_root, policy)?;
    let needle_owned;
    let needle: &str = if query.ignore_case {
        needle_owned = query.pattern.to_lowercase();
        &needle_owned
    } else {
        &query.pattern
    };

    let mut items: Vec<TextHit> = Vec::new();
    let mut files_scanned: u64 = 0;
    let mut truncated = false;
    let cap = usize::try_from(query.limit).unwrap_or(usize::MAX);

    'files: for entry in &entries {
        if let Some(matcher) = &include {
            let rel = crate::walk::rel_to_root(&search_root, &entry.path);
            let name = entry
                .path
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
            // The pattern is matched against the relative path text and the
            // file name, as `nanus` filters its hits.
            if !matcher.is_match(&rel) && !matcher.is_match(&name) {
                continue;
            }
        }
        let Some(text) = text_for_search(&entry.path) else {
            continue;
        };
        files_scanned = files_scanned.saturating_add(1);
        let haystack_owned;
        let haystack: &str = if query.ignore_case {
            haystack_owned = text.to_lowercase();
            &haystack_owned
        } else {
            &text
        };

        if query.ignore_case {
            // Line-wise, as `nanus` folds case per line.
            for (index, line) in text.lines().enumerate() {
                if !line.to_lowercase().contains(needle) {
                    continue;
                }
                if items.len() >= cap {
                    truncated = true;
                    break 'files;
                }
                items.push(TextHit {
                    path: entry.rel.clone(),
                    line: u64::try_from(index).map_or(1, |n| n.saturating_add(1)),
                    text: truncate_line(line, MAX_MATCH_LINE),
                });
            }
        } else {
            // SIMD substring scan over the whole text, then one hit per line.
            let finder = memchr::memmem::Finder::new(needle);
            let mut matched_lines: Vec<usize> = Vec::new();
            let mut cursor = 0usize;
            while let Some(at) = finder.find(&haystack.as_bytes()[cursor..]) {
                let absolute = cursor.saturating_add(at);
                let line_index = haystack[..absolute].bytes().filter(|b| *b == b'\n').count();
                if matched_lines.last() != Some(&line_index) {
                    matched_lines.push(line_index);
                }
                cursor = absolute.saturating_add(needle.len());
                if cursor >= haystack.len() {
                    break;
                }
            }
            for line_index in matched_lines {
                if items.len() >= cap {
                    truncated = true;
                    break 'files;
                }
                let line_text = haystack.lines().nth(line_index).unwrap_or_default();
                items.push(TextHit {
                    path: entry.rel.clone(),
                    line: u64::try_from(line_index).map_or(1, |n| n.saturating_add(1)),
                    text: truncate_line(line_text, MAX_MATCH_LINE),
                });
            }
        }
    }

    let mut truncations = Vec::new();
    if truncated {
        truncations.push(Truncation::new(
            TruncationKind::Results,
            u64::from(query.limit),
            format!(
                "(stopped at {} matches; narrow the pattern or the include filter)",
                query.limit
            ),
        ));
    }
    let match_count = items.len() as u64;
    Ok(TextResult {
        items,
        truncations,
        stats: Stats {
            files_scanned,
            matches: match_count,
            candidates: 0,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        },
    })
}
