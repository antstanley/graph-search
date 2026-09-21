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
use crate::source::read_checked;
use crate::walk::resolve_search_root;
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
    crate::work::validate_query_bytes("include", include)?;
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
fn text_for_search(
    path: &Path,
    coverage: &mut graph_search_types::coverage::Coverage,
    max_bytes: u64,
    work: &mut crate::work::WorkBudget,
) -> Result<Option<String>> {
    work.check()?;
    let read = match std::fs::File::open(path) {
        Ok(file) => read_checked(file, max_bytes.saturating_add(1), work)?,
        Err(_) => None,
    };
    let Some(bytes) = read else {
        coverage.source_read_errors = coverage.source_read_errors.saturating_add(1);
        return Ok(None);
    };
    if work.source_over_budget() {
        coverage.source_budget_exceeded_files =
            coverage.source_budget_exceeded_files.saturating_add(1);
        return Ok(None);
    }
    if bytes.len() as u64 > max_bytes {
        coverage.oversized_files = coverage.oversized_files.saturating_add(1);
        return Ok(None);
    }
    let sniff = bytes.get(..BINARY_SNIFF).unwrap_or(&bytes);
    if sniff.contains(&0) {
        coverage.binary_files = coverage.binary_files.saturating_add(1);
        return Ok(None);
    }
    if let Ok(text) = String::from_utf8(bytes) {
        Ok(Some(text))
    } else {
        coverage.invalid_utf8_files = coverage.invalid_utf8_files.saturating_add(1);
        Ok(None)
    }
}

fn include_matcher(include: Option<&str>) -> Result<Option<GlobMatcher>> {
    include
        .map(|pattern| {
            Glob::new(pattern.trim())
                .map(|g| g.compile_matcher())
                .map_err(|e| Error::InvalidPattern {
                    pattern: pattern.to_owned(),
                    reason: e.to_string(),
                })
        })
        .transpose()
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
pub fn search_text(root: &Path, query: &TextQuery, policy: &WalkPolicy) -> Result<TextResult> {
    search_text_with_work(
        root,
        query,
        policy,
        &mut crate::work::WorkBudget::new(crate::work::WorkLimits::default()),
    )
}

/// Runs a literal scan with cooperative cancellation and deadline checks.
/// # Errors
/// On cancellation, deadline, invalid pattern/filter or missing root.
#[allow(clippy::too_many_lines)]
pub fn search_text_with_work(
    root: &Path,
    query: &TextQuery,
    policy: &WalkPolicy,
    work: &mut crate::work::WorkBudget,
) -> Result<TextResult> {
    work.check()?;
    let started = std::time::Instant::now();
    crate::work::validate_query_bytes("text pattern", &query.pattern)?;
    if let Some(path) = query.path.as_deref() {
        crate::work::validate_query_bytes("search path", path)?;
    }
    if query.pattern.is_empty() {
        return Err(Error::InvalidPattern {
            pattern: query.pattern.clone(),
            reason: String::from("the pattern is empty"),
        });
    }
    if query.pattern.contains(['\n', '\r']) {
        return Err(Error::InvalidPattern {
            pattern: query.pattern.clone(),
            reason: String::from("text search accepts a single-line literal"),
        });
    }
    if let Some(include) = query.include.as_deref() {
        validate_include(include)?;
    }
    let include = include_matcher(query.include.as_deref())?;

    let search_root = resolve_search_root(root, query.path.as_deref())?;
    let report = crate::walk::walk_report_with_work(&search_root, policy, work)?;
    let entries = report.entries;
    let needle = if query.ignore_case {
        query.pattern.to_lowercase()
    } else {
        query.pattern.clone()
    };

    let finder = memchr::memmem::Finder::new(&needle);
    let mut context = graph_search_types::context::ResultContext::live(report.coverage);
    let mut items: Vec<TextHit> = Vec::new();
    let mut files_scanned: u64 = 0;
    let mut truncated = false;
    let mut bytes = crate::payload::ScanBudget::default();
    let mut byte_cap_hit = false;
    let mut match_line_cut = false;
    let limit = query
        .limit
        .min(graph_search_types::limits::TEXT_LIMIT_CEILING);
    let cap = usize::try_from(limit).unwrap_or(usize::MAX);

    'files: for entry in &entries {
        work.check()?;
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
        if !work.source_file()? {
            context.coverage.source_budget_exceeded_files = context
                .coverage
                .source_budget_exceeded_files
                .saturating_add(1);
            break;
        }
        let Some(text) = text_for_search(
            &entry.path,
            &mut context.coverage,
            policy.max_file_bytes,
            work,
        )?
        else {
            if work.source_over_budget() {
                break;
            }
            continue;
        };
        files_scanned = files_scanned.saturating_add(1);
        for (index, line) in text.lines().enumerate() {
            work.check()?;
            let folded;
            let haystack = if query.ignore_case {
                // Unicode lowercase, not full Unicode case folding. Keep the
                // original line for evidence even when lowercase expands it.
                folded = line.to_lowercase();
                folded.as_bytes()
            } else {
                line.as_bytes()
            };
            if finder.find(haystack).is_none() {
                continue;
            }
            if items.len() >= cap {
                truncated = true;
                break 'files;
            }
            let hit = TextHit {
                path: entry.rel.clone(),
                line: u64::try_from(index).map_or(1, |n| n.saturating_add(1)),
                text: truncate_line(line, MAX_MATCH_LINE),
            };
            if !bytes.admit(&hit)? {
                byte_cap_hit = true;
                break 'files;
            }
            match_line_cut |= line.len() > MAX_MATCH_LINE;
            context.sources.entry(entry.rel.clone()).or_insert_with(|| {
                graph_search_types::context::SourceIdentity {
                    indexed_hash: None,
                    observed_hash: Some(crate::hash::content_hash(text.as_bytes())),
                    verification: graph_search_types::context::SourceVerification::Live,
                }
            });
            items.push(hit);
        }
    }

    let (_, _, work_truncations) = work.report();
    for limit in work_truncations {
        if !context
            .coverage
            .truncations
            .iter()
            .any(|prior| prior.kind == limit.kind)
        {
            context.coverage.truncations.push(limit);
        }
    }
    let mut truncations = context.coverage.truncations.clone();
    if byte_cap_hit {
        crate::payload::scan_notice(&mut truncations);
    }
    if match_line_cut {
        truncations.push(Truncation::new(
            TruncationKind::MatchLine,
            MAX_MATCH_LINE as u64,
            "matching source lines exceeded the per-line byte cap",
        ));
    }
    if truncated {
        truncations.push(Truncation::new(
            TruncationKind::Results,
            u64::from(limit),
            format!("(stopped at {limit} matches; narrow the pattern or the include filter)"),
        ));
    }
    let match_count = items.len() as u64;
    work.check()?;
    let mut result = TextResult {
        context,
        items,
        truncations,
        stats: Stats {
            source_files_attempted: work.source_report().0,
            source_bytes_read: work.source_report().1,
            files_scanned,
            matches: match_count,
            candidates: 0,
            elapsed_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            ..Stats::default()
        },
    };
    crate::payload::fit_text(&mut result)?;
    work.check()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    #[test]
    fn regex_syntax_remains_literal_even_when_it_would_be_invalid_regex() {
        let source = "axb\na.*b\nx\n(?=x)\nabc\n[a-z]+\nvalue\n^value$\n123\n\\d+\n";
        for ignore_case in [false, true] {
            for (pattern, line) in [
                ("a.*b", 2),
                ("(?=x)", 4),
                ("[a-z]+", 6),
                ("^value$", 8),
                ("\\d+", 10),
                ("[", 6),
            ] {
                let result = run(source, pattern, 20, ignore_case).unwrap();
                assert_eq!(result.items.len(), 1, "{pattern}");
                assert_eq!(result.items[0].line, line, "{pattern}");
                assert!(result.truncations.is_empty());
            }
        }
    }
    use super::*;

    fn run(source: &str, pattern: &str, limit: u32, ignore_case: bool) -> Result<TextResult> {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("sample.txt"), source).unwrap();
        let mut query = TextQuery::new(pattern);
        query.limit = limit;
        query.ignore_case = ignore_case;
        search_text(dir.path(), &query, &WalkPolicy::default())
    }

    #[test]
    fn line_search_preserves_source_and_deduplicates_occurrences() {
        for ignore_case in [false, true] {
            let result = run("prefix\r\n\r\nhit hit\r\nlast hit", "hit", 10, ignore_case).unwrap();
            assert_eq!(
                result
                    .items
                    .iter()
                    .map(|h| (h.line, h.text.as_str()))
                    .collect::<Vec<_>>(),
                vec![(3, "hit hit"), (4, "last hit")]
            );
            assert!(result.truncations.is_empty());
        }
    }

    #[test]
    fn cap_requires_an_omitted_matching_line() {
        assert!(
            run("hit hit", "hit", 1, false)
                .unwrap()
                .truncations
                .is_empty()
        );
        let result = run(&"hit\n".repeat(32_000), "hit", 1, false).unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.truncations.len(), 1);
        assert!(run("miss", "hit", 0, false).unwrap().truncations.is_empty());
        assert_eq!(run("hit", "hit", 0, false).unwrap().truncations.len(), 1);
    }

    #[test]
    fn lowercase_expansion_does_not_change_evidence() {
        let result = run("İSTANBUL\nother", "i\u{307}stanbul", 10, true).unwrap();
        assert_eq!(result.items[0].text, "İSTANBUL");
        assert_eq!(result.items[0].line, 1);
        assert!(run("Straße", "STRASSE", 10, true).unwrap().items.is_empty());
    }

    #[test]
    fn multiline_literals_are_rejected_consistently() {
        for ignore_case in [false, true] {
            for pattern in ["a\nb", "a\rb", ""] {
                assert!(matches!(
                    run("a\nb", pattern, 10, ignore_case),
                    Err(Error::InvalidPattern { .. })
                ));
            }
        }
    }
    #[test]
    fn cancellation_between_read_chunks_stops_before_the_next_read() {
        struct CancellingRead {
            token: crate::work::CancellationToken,
            calls: usize,
        }
        impl std::io::Read for CancellingRead {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.calls = self.calls.saturating_add(1);
                buffer[0] = b'x';
                self.token.cancel();
                Ok(1)
            }
        }
        let token = crate::work::CancellationToken::default();
        let mut work = crate::work::WorkBudget::new(crate::work::WorkLimits {
            cancellation: Some(token.clone()),
            ..crate::work::WorkLimits::default()
        });
        let mut reader = CancellingRead { token, calls: 0 };
        assert!(matches!(
            read_checked(&mut reader, 1024, &mut work),
            Err(Error::QueryCancelled)
        ));
        assert_eq!(reader.calls, 1);
        let mut expired = crate::work::WorkBudget::new(crate::work::WorkLimits {
            deadline: Some(std::time::Instant::now()),
            ..crate::work::WorkLimits::default()
        });
        assert!(matches!(
            read_checked(&mut reader, 1024, &mut expired),
            Err(Error::QueryDeadline)
        ));
        assert_eq!(
            reader.calls, 1,
            "expired work must not initiate another read"
        );
    }
    #[test]
    fn checked_reads_retry_interruption_and_respect_the_byte_ceiling() {
        struct InterruptedOnce {
            interrupted: bool,
            source: std::io::Cursor<&'static [u8]>,
        }
        impl std::io::Read for InterruptedOnce {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if !self.interrupted {
                    self.interrupted = true;
                    return Err(std::io::ErrorKind::Interrupted.into());
                }
                std::io::Read::read(&mut self.source, buffer)
            }
        }
        let mut reader = InterruptedOnce {
            interrupted: false,
            source: std::io::Cursor::new(b"abcdef"),
        };
        let mut work = crate::work::WorkBudget::new(crate::work::WorkLimits::default());
        assert_eq!(
            read_checked(&mut reader, 4, &mut work).unwrap().unwrap(),
            b"abcd"
        );
        assert_eq!(reader.source.position(), 4);
    }
    #[test]
    fn failed_reads_account_for_the_prefix_they_consumed() {
        struct FailsAfterPrefix {
            calls: usize,
        }
        impl std::io::Read for FailsAfterPrefix {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                self.calls = self.calls.saturating_add(1);
                if self.calls == 1 {
                    buffer[..3].copy_from_slice(b"abc");
                    Ok(3)
                } else {
                    Err(std::io::ErrorKind::Other.into())
                }
            }
        }
        let mut work = crate::work::WorkBudget::new(crate::work::WorkLimits::default());
        assert!(work.source_file().unwrap());
        assert!(
            read_checked(FailsAfterPrefix { calls: 0 }, 100, &mut work)
                .unwrap()
                .is_none()
        );
        assert_eq!(work.source_report(), (1, 3));
    }
}
