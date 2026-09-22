//! Native framework single-file-component script regions.
//!
//! This adapter recognizes the *declared* script regions of Svelte, Vue and
//! Astro components and reuses the ordinary JS/TS extractors through the
//! coordinate-preserving [`crate::embedded::script`] bridge. It is deliberately
//! not a template engine: markup outside declared script regions is indexed as
//! body text (the file stays searchable), but template expressions, component
//! tags and framework events are not turned into symbols or resolved edges.
//!
//! Recognized but unmodeled regions are recorded as facts so the coverage
//! report can show the parser gap instead of silently losing authored code.

use crate::embedded;
use graph_search_core::extraction::Extraction;
use graph_search_core::ports::{LanguageExtractor, ParseError, SourceFile};
use graph_search_types::extraction::EmbeddedRegionFact;
use graph_search_types::{Language, limits, node::Span};
use std::ops::Range;
use std::path::Path;

/// Maximum attributes inspected on one recognized script start tag.
const MAX_ATTRIBUTES: usize = 64;
/// Maximum candidate start tags inspected before the scan reports omission.
const MAX_CANDIDATES: usize = 256;

/// The Svelte single-file-component adapter.
pub struct SvelteExtractor;
/// The Vue single-file-component adapter.
pub struct VueExtractor;
/// The Astro component adapter.
pub struct AstroExtractor;

impl LanguageExtractor for SvelteExtractor {
    fn language(&self) -> Language {
        Language::Svelte
    }
    fn supports(&self, path: &Path) -> bool {
        extension(path) == Some("svelte")
    }
    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        framework(file, Dialect::Svelte)
    }
}

impl LanguageExtractor for VueExtractor {
    fn language(&self) -> Language {
        Language::Vue
    }
    fn supports(&self, path: &Path) -> bool {
        extension(path) == Some("vue")
    }
    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        framework(file, Dialect::Vue)
    }
}

impl LanguageExtractor for AstroExtractor {
    fn language(&self) -> Language {
        Language::Astro
    }
    fn supports(&self, path: &Path) -> bool {
        extension(path) == Some("astro")
    }
    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError> {
        framework(file, Dialect::Astro)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialect {
    Svelte,
    Vue,
    Astro,
}

/// One recognized authored region, before extraction.
struct Candidate {
    span: Span,
    kind: &'static str,
    domain: Option<&'static str>,
    language: Language,
    script: Option<Range<usize>>,
    reason: Option<&'static str>,
}

fn extension(path: &Path) -> Option<&str> {
    path.extension().and_then(std::ffi::OsStr::to_str)
}

fn framework(file: &SourceFile<'_>, dialect: Dialect) -> Result<Extraction, ParseError> {
    let text = file.text;
    if text.len() > limits::MAX_EMBEDDED_REGION_BYTES {
        return Err(ParseError::new(
            "framework source exceeds the bounded region scan",
        ));
    }
    let offsets = line_offsets(text);
    let (candidates, truncated) = scan(text, &offsets, dialect);
    let mut extraction = Extraction {
        embedded_truncated: truncated,
        ..Extraction::default()
    };
    for candidate in candidates {
        let mut fact = EmbeddedRegionFact {
            span: candidate.span,
            kind: candidate.kind.into(),
            domain: candidate.domain.map(str::to_owned),
            extracted: false,
            reason: candidate.reason.map(str::to_owned),
        };
        if let (Some(domain), Some(range)) = (candidate.domain, candidate.script) {
            match embedded::script(file, range, candidate.language, domain) {
                Ok(region) => {
                    extraction.merge(region);
                    fact.extracted = true;
                    fact.reason = None;
                }
                Err(_) => fact.reason = Some("script_parse_error".into()),
            }
        }
        extraction.embedded.push(fact);
    }
    extraction.embedded.sort_by(|a, b| {
        (a.span.start_byte, a.span.end_byte, &a.kind).cmp(&(
            b.span.start_byte,
            b.span.end_byte,
            &b.kind,
        ))
    });
    Ok(extraction)
}

/// Finds declared script regions in source order. The second value reports
/// whether the candidate bound or a malformed start tag stopped the scan.
fn scan(text: &str, offsets: &[usize], dialect: Dialect) -> (Vec<Candidate>, bool) {
    let mut found = Vec::new();
    let mut cursor = 0;
    let mut truncated = false;
    if dialect == Dialect::Astro
        && let Some((candidate, next)) = frontmatter(text, offsets)
    {
        found.push(candidate);
        cursor = next;
    }
    let mut candidates = 0;
    while cursor < text.len() {
        if candidates >= MAX_CANDIDATES || found.len() >= limits::MAX_EMBEDDED_REGIONS_PER_FILE {
            truncated = true;
            break;
        }
        let Some(start) = find_ignoring_case(text.as_bytes(), b"<script", cursor) else {
            break;
        };
        candidates = candidates.saturating_add(1);
        // Authored HTML comments hide markup; they are not script regions.
        if let Some(comment) = html_comment(text, cursor)
            && comment.start < start
        {
            cursor = comment.end;
            continue;
        }
        let after = start.saturating_add("<script".len());
        let boundary = text.as_bytes().get(after).copied();
        if !boundary.is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'>' | b'/')) {
            cursor = after;
            continue;
        }
        let Some(close) = text.as_bytes()[after..]
            .iter()
            .position(|&byte| byte == b'>')
            .map(|offset| offset.saturating_add(after))
        else {
            truncated = true;
            break;
        };
        let Some(attributes) = attributes(text[after..close].trim_start_matches('/')) else {
            found.push(Candidate {
                span: span_for(offsets, start, close.saturating_add(1)),
                kind: "script",
                domain: None,
                language: Language::JavaScript,
                script: None,
                reason: Some("attribute_limit"),
            });
            cursor = close.saturating_add(1);
            continue;
        };
        let body = close.saturating_add(1);
        let (region_end, body_end, closed) =
            match find_ignoring_case(text.as_bytes(), b"</script", body) {
                Some(closer) => {
                    let end = text.as_bytes()[closer..]
                        .iter()
                        .position(|&byte| byte == b'>')
                        .map_or(closer, |offset| {
                            closer.saturating_add(offset).saturating_add(1)
                        });
                    (end, closer, true)
                }
                None => (text.len(), text.len(), false),
            };
        let span = span_for(offsets, start, region_end);
        let candidate = match decide(dialect, &attributes) {
            Ok((kind, domain, language)) if closed => Candidate {
                span,
                kind,
                domain: Some(domain),
                language,
                script: Some(body..body_end),
                reason: None,
            },
            Ok((kind, domain, _)) => Candidate {
                span,
                kind,
                domain: Some(domain),
                language: Language::JavaScript,
                script: None,
                reason: Some("unclosed_script"),
            },
            Err(reason) => Candidate {
                span,
                kind: "script",
                domain: None,
                language: Language::JavaScript,
                script: None,
                reason: Some(reason),
            },
        };
        found.push(candidate);
        cursor = region_end.saturating_add(1).max(body);
    }
    (found, truncated)
}

/// Interprets the supported declared attributes for one script tag.
fn decide(
    dialect: Dialect,
    attributes: &[(String, String)],
) -> Result<(&'static str, &'static str, Language), &'static str> {
    let value = |name: &str| {
        attributes
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    let language = match value("lang").map(str::to_ascii_lowercase) {
        None => default_language(dialect),
        Some(lang) if matches!(lang.as_str(), "js" | "javascript") => Language::JavaScript,
        Some(lang) if matches!(lang.as_str(), "ts" | "typescript") => Language::TypeScript,
        Some(_) => return Err("unsupported_lang"),
    };
    match dialect {
        Dialect::Svelte => match value("context").map(str::to_ascii_lowercase) {
            None => Ok(("script", "instance", language)),
            Some(context) if context == "module" => Ok(("module", "module", language)),
            Some(_) => Err("unsupported_context"),
        },
        Dialect::Vue => {
            let setup = attributes.iter().any(|(key, _)| key == "setup");
            Ok(if setup {
                ("setup", "setup", language)
            } else {
                ("script", "default", language)
            })
        }
        Dialect::Astro => Ok(("script", "script", language)),
    }
}

fn default_language(dialect: Dialect) -> Language {
    let _ = dialect;
    Language::JavaScript
}

/// Astro frontmatter: an unindented `---` first line closed by `---`.
fn frontmatter(text: &str, offsets: &[usize]) -> Option<(Candidate, usize)> {
    let first_line_end = text.find('\n').unwrap_or(text.len());
    if text[..first_line_end].trim_end_matches('\r') != "---" {
        return None;
    }
    let body = first_line_end.saturating_add(1).min(text.len());
    let mut cursor = body;
    while cursor <= text.len() {
        let line_end = text[cursor..]
            .find('\n')
            .map_or(text.len(), |index| index.saturating_add(cursor));
        if text[cursor..line_end].trim_end_matches('\r') == "---" {
            let span = span_for(offsets, 0, line_end);
            let next = line_end.saturating_add(1).min(text.len());
            return Some((
                Candidate {
                    span,
                    kind: "frontmatter",
                    domain: Some("frontmatter"),
                    language: Language::TypeScript,
                    script: Some(body..cursor),
                    reason: None,
                },
                next,
            ));
        }
        if line_end >= text.len() {
            break;
        }
        cursor = line_end.saturating_add(1);
    }
    Some((
        Candidate {
            span: span_for(offsets, 0, text.len()),
            kind: "frontmatter",
            domain: Some("frontmatter"),
            language: Language::TypeScript,
            script: None,
            reason: Some("unclosed_frontmatter"),
        },
        text.len(),
    ))
}

/// Span of the authored byte interval, with 1-based inclusive lines.
fn span_for(offsets: &[usize], start: usize, end: usize) -> Span {
    let line = |byte: usize| {
        u32::try_from(offsets.partition_point(|&offset| offset <= byte)).unwrap_or(u32::MAX)
    };
    let last = end.saturating_sub(1).max(start);
    Span::new(
        line(start),
        line(last),
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
    )
}

fn line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    offsets.extend(
        text.bytes()
            .enumerate()
            .filter(|(_, byte)| *byte == b'\n')
            .map(|(index, _)| index.saturating_add(1)),
    );
    offsets
}

fn html_comment(text: &str, from: usize) -> Option<Range<usize>> {
    let start = text.get(from..)?.find("<!--")?.saturating_add(from);
    let end = text.get(start..)?.find("-->").map_or(text.len(), |index| {
        index.saturating_add(start).saturating_add(3)
    });
    Some(start..end)
}

fn find_ignoring_case(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from > haystack.len() {
        return None;
    }
    haystack
        .iter()
        .enumerate()
        .skip(from)
        .find(|(index, byte)| {
            byte.eq_ignore_ascii_case(&needle[0])
                && haystack
                    .get(*index..index.saturating_add(needle.len()))
                    .is_some_and(|window| window.eq_ignore_ascii_case(needle))
        })
        .map(|(index, _)| index)
}

/// Bounded attribute parsing: names lower-cased, values preserved verbatim.
fn attributes(inner: &str) -> Option<Vec<(String, String)>> {
    let bytes = inner.as_bytes();
    let mut attributes = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while bytes
            .get(cursor)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'/')
        {
            cursor = cursor.saturating_add(1);
        }
        if cursor >= bytes.len() {
            break;
        }
        let name_start = cursor;
        while bytes
            .get(cursor)
            .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(*byte, b'=' | b'/' | b'>'))
        {
            cursor = cursor.saturating_add(1);
        }
        let name = inner.get(name_start..cursor)?.to_ascii_lowercase();
        if name.is_empty() {
            cursor = cursor.saturating_add(1);
            continue;
        }
        let mut value = String::new();
        let mut probe = cursor;
        while bytes.get(probe).is_some_and(u8::is_ascii_whitespace) {
            probe = probe.saturating_add(1);
        }
        if bytes.get(probe) == Some(&b'=') {
            cursor = probe.saturating_add(1);
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor = cursor.saturating_add(1);
            }
            match bytes.get(cursor).copied() {
                Some(quote @ (b'"' | b'\'')) => {
                    let value_start = cursor.saturating_add(1);
                    let value_end = bytes
                        .get(value_start..)?
                        .iter()
                        .position(|byte| *byte == quote)
                        .map_or(bytes.len(), |offset| offset.saturating_add(value_start));
                    inner.get(value_start..value_end)?.clone_into(&mut value);
                    cursor = value_end.saturating_add(1);
                }
                Some(byte) if !byte.is_ascii_whitespace() => {
                    let value_start = cursor;
                    while bytes.get(cursor).is_some_and(|byte| {
                        !byte.is_ascii_whitespace() && !matches!(*byte, b'/' | b'>')
                    }) {
                        cursor = cursor.saturating_add(1);
                    }
                    inner.get(value_start..cursor)?.clone_into(&mut value);
                }
                _ => {}
            }
        }
        attributes.push((name, value));
        if attributes.len() > MAX_ATTRIBUTES {
            return None;
        }
    }
    Some(attributes)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn facts(text: &str, dialect: Dialect) -> Extraction {
        let file = SourceFile {
            path: Path::new("component"),
            text,
        };
        framework(&file, dialect).unwrap()
    }

    #[test]
    fn svelte_instance_and_module_regions_are_namespaced() {
        let text = "<script context=\"module\">\nexport const shared = 1;\n</script>\n\
                    <p>{shared}</p>\n\
                    <script lang=\"ts\">\nconst local: number = shared;\n</script>\n";
        let extraction = facts(text, Dialect::Svelte);
        assert_eq!(extraction.embedded.len(), 2);
        assert!(extraction.embedded.iter().all(|fact| fact.extracted));
        assert_eq!(
            extraction
                .embedded
                .iter()
                .filter_map(|fact| fact.domain.as_deref())
                .collect::<Vec<_>>(),
            vec!["module", "instance"]
        );
        assert!(
            extraction
                .symbols
                .iter()
                .any(|symbol| symbol.key.contains("shared"))
        );
        assert!(
            extraction
                .symbols
                .iter()
                .any(|symbol| symbol.name == "local")
        );
    }

    #[test]
    fn unsupported_language_stays_visible() {
        let text = "<script lang=\"coffee\">x = 1</script>\n<script>let a = 1;</script>\n";
        let extraction = facts(text, Dialect::Svelte);
        assert_eq!(extraction.embedded.len(), 2);
        assert!(!extraction.embedded[0].extracted);
        assert_eq!(
            extraction.embedded[0].reason.as_deref(),
            Some("unsupported_lang")
        );
        assert!(extraction.embedded[1].extracted);
        assert!(extraction.symbols.iter().any(|symbol| symbol.name == "a"));
    }

    #[test]
    fn comments_and_case_folding_do_not_invent_regions() {
        let text =
            "<!-- <SCRIPT>let hidden = 1;</SCRIPT> -->\n<ScRiPt>\nlet shown = 2;\n</sCrIpT>\n";
        let extraction = facts(text, Dialect::Vue);
        assert_eq!(extraction.embedded.len(), 1);
        assert!(
            extraction
                .symbols
                .iter()
                .any(|symbol| symbol.name == "shown")
        );
        assert!(
            !extraction
                .symbols
                .iter()
                .any(|symbol| symbol.name == "hidden")
        );
    }

    #[test]
    fn astro_frontmatter_and_script_regions_are_extracted() {
        let text = "---\nconst title: string = \"hi\";\n---\n<h1>{title}</h1>\n\
                    <script>\nconsole.log(title);\n</script>\n";
        let extraction = facts(text, Dialect::Astro);
        assert_eq!(extraction.embedded.len(), 2);
        assert_eq!(extraction.embedded[0].kind, "frontmatter");
        assert_eq!(
            extraction.embedded[0].domain.as_deref(),
            Some("frontmatter")
        );
        assert_eq!(extraction.embedded[1].domain.as_deref(), Some("script"));
        assert!(
            extraction
                .symbols
                .iter()
                .any(|symbol| symbol.name == "title")
        );
    }

    #[test]
    fn unclosed_regions_are_reported_without_quarantine() {
        let text = "---\nconst a = 1;\n<p>no close</p>\n";
        let extraction = facts(text, Dialect::Astro);
        assert_eq!(extraction.embedded.len(), 1);
        assert!(!extraction.embedded[0].extracted);
        assert_eq!(
            extraction.embedded[0].reason.as_deref(),
            Some("unclosed_frontmatter")
        );
        assert!(extraction.symbols.is_empty());
    }

    #[test]
    fn quoted_angle_brackets_do_not_end_a_start_tag() {
        let text = "<script data-x=\">\">\nlet ok = 1;\n</script>\n";
        let extraction = facts(text, Dialect::Svelte);
        assert_eq!(extraction.embedded.len(), 1);
        assert!(extraction.embedded[0].extracted);
        assert!(extraction.symbols.iter().any(|symbol| symbol.name == "ok"));
    }
}
