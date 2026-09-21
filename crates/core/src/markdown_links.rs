//! Bounded authored-link coordinates for a declared native Markdown subset.
use graph_search_types::source::{MarkdownLink, MarkdownLinkKind};
use std::ops::Range;

pub(crate) fn uri_autolink(line: &str) -> bool {
    line.strip_prefix('<')
        .and_then(|tail| tail.split_once('>'))
        .is_some_and(|(uri, _)| valid_uri(uri.as_bytes()))
}

fn valid_uri(text: &[u8]) -> bool {
    let Some(colon) = text.iter().position(|byte| *byte == b':') else {
        return false;
    };
    (2..=32).contains(&colon)
        && text[0].is_ascii_alphabetic()
        && text[1..colon]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
        && !text.iter().any(|byte| {
            byte.is_ascii_whitespace() || byte.is_ascii_control() || matches!(byte, b'<' | b'\\')
        })
}

pub(super) fn region(
    text: &str,
    region: &crate::markdown::Region,
    offsets: &[usize],
    allowance: &mut usize,
) -> (Vec<MarkdownLink>, bool) {
    if region.fenced || region.frontmatter || region.block.is_some_and(|block| block.unsupported) {
        (Vec::new(), false)
    } else {
        let found = scan(
            &text[region.start..region.end],
            region.start,
            offsets,
            (*allowance).min(graph_search_types::limits::MAX_MARKDOWN_LINKS),
        );
        *allowance = allowance.saturating_sub(found.0.len());
        found
    }
}

pub(super) fn within(links: &[MarkdownLink], start: usize, end: usize) -> Vec<MarkdownLink> {
    links
        .iter()
        .filter(|link| link.span.start_byte as usize >= start && link.span.end_byte as usize <= end)
        .copied()
        .collect()
}

pub(super) fn valid(unit: &graph_search_types::source::SourceUnit) -> bool {
    use graph_search_types::source::SourceUnitKind;
    if unit.links.len() > graph_search_types::limits::MAX_MARKDOWN_LINKS {
        return false;
    }
    if (!unit.links.is_empty() || unit.links_truncated)
        && (!matches!(
            unit.kind,
            SourceUnitKind::Markdown
                | SourceUnitKind::MarkdownParagraph
                | SourceUnitKind::MarkdownListItem
                | SourceUnitKind::MarkdownTable
        ) || unit.block.is_some_and(|block| block.contains_unsupported))
    {
        return false;
    }
    let mut end = unit.span.start_byte;
    unit.links.iter().all(|link| {
        let valid = super::contains_span(unit.span, link.span)
            && link.span.start_byte >= end
            && link.span.start_byte < link.span.end_byte
            && link.span.start_line == link.span.end_line
            && super::contains_span(link.span, link.label_span)
            && super::contains_span(link.span, link.destination_span)
            && link.label_span.start_byte > link.span.start_byte
            && link.label_span.end_byte < link.span.end_byte
            && link.destination_span.end_byte < link.span.end_byte
            && link.title_span.is_none_or(|title| {
                super::contains_span(link.span, title)
                    && title.start_byte >= link.destination_span.end_byte
            })
            && match link.kind {
                MarkdownLinkKind::Autolink => {
                    link.label_span == link.destination_span && link.title_span.is_none()
                }
                _ => link.label_span.end_byte < link.destination_span.start_byte,
            };
        end = link.span.end_byte;
        valid
    })
}

pub(super) fn valid_file(source: &graph_search_types::source::SourceFileUnits) -> bool {
    let mut spans = std::collections::BTreeMap::new();
    source
        .units
        .iter()
        .flat_map(|unit| &unit.links)
        .all(|link| {
            let prior = spans.insert((link.span.start_byte, link.span.end_byte), link);
            prior.is_none_or(|prior| prior == link)
                && spans.len() <= graph_search_types::limits::MAX_MARKDOWN_LINKS_PER_FILE
        })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    work: usize,
    exhausted: bool,
}

impl Cursor<'_> {
    fn reserve(&mut self, bytes: usize) -> Option<()> {
        if bytes > self.work {
            self.work = 0;
            self.exhausted = true;
            None
        } else {
            self.work = self.work.saturating_sub(bytes);
            Some(())
        }
    }
    fn escaped(&mut self, position: usize) -> Option<usize> {
        match self.byte(position.saturating_add(1))? {
            b'\r' | b'\n' => None,
            _ => Some(position.saturating_add(2)),
        }
    }
    fn byte(&mut self, position: usize) -> Option<u8> {
        if self.work == 0 {
            self.exhausted = true;
            return None;
        }
        self.work = self.work.saturating_sub(1);
        self.bytes.get(position).copied()
    }

    fn spaces(&mut self, mut position: usize) -> usize {
        while matches!(self.byte(position), Some(b' ' | b'\t')) {
            position = position.saturating_add(1);
        }
        position
    }

    fn quoted(&mut self, start: usize, close: u8) -> Option<usize> {
        let mut at = start;
        while let Some(byte) = self.byte(at) {
            match byte {
                b'\r' | b'\n' => return None,
                b'\\' => at = self.escaped(at)?,
                byte if byte == close => return Some(at),
                _ => at = at.saturating_add(1),
            }
        }
        None
    }

    fn destination(&mut self, start: usize) -> Option<(Range<usize>, usize)> {
        if self.byte(start)? == b'<' {
            let content = start.saturating_add(1);
            let mut at = content;
            while let Some(byte) = self.byte(at) {
                match byte {
                    b'>' => return Some((content..at, at.saturating_add(1))),
                    b'<' | b'\r' | b'\n' => return None,
                    b'\\' => at = self.escaped(at)?,
                    _ => at = at.saturating_add(1),
                }
            }
            return None;
        }
        let mut depth = 0usize;
        let mut at = start;
        while let Some(byte) = self.byte(at) {
            match byte {
                b'\\' => at = self.escaped(at)?,
                b'(' => {
                    depth = depth.saturating_add(1);
                    if depth > 16 {
                        return None;
                    }
                    at = at.saturating_add(1);
                }
                b')' if depth > 0 => {
                    depth = depth.saturating_sub(1);
                    at = at.saturating_add(1);
                }
                b')' | b' ' | b'\t' if depth == 0 => return Some((start..at, at)),
                b'<' | b'>' | b'\r' | b'\n' | 0..=31 => return None,
                _ => at = at.saturating_add(1),
            }
        }
        None
    }

    fn inline(&mut self, start: usize, bracket: usize) -> Option<RawLink> {
        let label_start = bracket.saturating_add(1);
        let mut at = label_start;
        let label_end = loop {
            match self.byte(at)? {
                b']' => break at,
                b'[' | b'`' | b'\r' | b'\n' => return None,
                b'\\' => at = self.escaped(at)?,
                _ => at = at.saturating_add(1),
            }
        };
        if self.byte(label_end.saturating_add(1))? != b'(' {
            return None;
        }
        let destination_start = self.spaces(label_end.saturating_add(2));
        let (destination, end) = self.destination(destination_start)?;
        let mut close = self.spaces(end);
        let mut title = None;
        if close > end && matches!(self.byte(close), Some(b'\'' | b'"')) {
            let quote = self.byte(close)?;
            let title_start = close.saturating_add(1);
            let title_end = self.quoted(title_start, quote)?;
            title = Some(title_start..title_end);
            close = self.spaces(title_end.saturating_add(1));
        }
        if self.byte(close)? != b')' {
            return None;
        }
        Some(RawLink {
            span: start..close.saturating_add(1),
            label: label_start..label_end,
            destination,
            title,
            kind: if start == bracket {
                MarkdownLinkKind::Inline
            } else {
                MarkdownLinkKind::Image
            },
        })
    }

    fn angle(&mut self, start: usize) -> Option<RawLink> {
        let content = start.saturating_add(1);
        let end = self.quoted(content, b'>')?;
        let text = self.bytes.get(content..end)?;
        // Scheme lookup, scheme validation and URI validation each inspect at
        // most this many bytes. Reserve them as well as delimiter scanning.
        self.reserve(text.len().saturating_mul(3))?;
        if !valid_uri(text) {
            return None;
        }
        Some(RawLink {
            span: start..end.saturating_add(1),
            label: content..end,
            destination: content..end,
            title: None,
            kind: MarkdownLinkKind::Autolink,
        })
    }
}

struct RawLink {
    span: Range<usize>,
    label: Range<usize>,
    destination: Range<usize>,
    title: Option<Range<usize>>,
    kind: MarkdownLinkKind,
}

pub(super) fn scan(
    text: &str,
    offset: usize,
    offsets: &[usize],
    limit: usize,
) -> (Vec<MarkdownLink>, bool) {
    let mut cursor = Cursor {
        bytes: text.as_bytes(),
        work: text
            .len()
            .saturating_mul(8)
            .saturating_add(32)
            .min(graph_search_types::limits::MAX_MARKDOWN_LINK_WORK),
        exhausted: false,
    };
    let mut links = Vec::new();
    let mut at = 0usize;
    let mut code = None;
    while at < text.len() && !cursor.exhausted {
        let Some(byte) = cursor.byte(at) else {
            break;
        };
        if byte == b'`' {
            let start = at;
            while cursor.byte(at) == Some(b'`') {
                at = at.saturating_add(1);
            }
            let run = at.saturating_sub(start);
            if code == Some(run) {
                code = None;
            } else if code.is_none() {
                code = Some(run);
            }
            continue;
        }
        if code.is_some() {
            at = at.saturating_add(1);
            continue;
        }
        if byte == b'\\' {
            at = at.saturating_add(2);
            continue;
        }
        let candidate = match byte {
            b'[' => cursor.inline(at, at),
            b'!' if cursor.byte(at.saturating_add(1)) == Some(b'[') => {
                cursor.inline(at, at.saturating_add(1))
            }
            b'<' => cursor.angle(at),
            _ => None,
        };
        if let Some(link) = candidate {
            if links.len() == limit {
                return (links, true);
            }
            at = link.span.end;
            let span = |range: Range<usize>| {
                super::source_span(
                    offset.saturating_add(range.start),
                    offset.saturating_add(range.end),
                    offsets,
                )
            };
            links.push(MarkdownLink {
                span: span(link.span),
                label_span: span(link.label),
                destination_span: span(link.destination),
                title_span: link.title.map(span),
                kind: link.kind,
            });
        } else if byte == b'<' {
            // Raw HTML attributes are not Markdown. Quotes may contain `>`.
            at = at.saturating_add(1);
            let mut quote = None;
            while let Some(byte) = cursor.byte(at) {
                if quote == Some(byte) {
                    quote = None;
                } else if quote.is_none() && matches!(byte, b'\'' | b'"') {
                    quote = Some(byte);
                } else if quote.is_none() && byte == b'>' {
                    at = at.saturating_add(1);
                    break;
                }
                at = at.saturating_add(1);
            }
        } else {
            at = at.saturating_add(1);
        }
    }
    (links, cursor.exhausted)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use graph_search_types::{Language, Node, NodeKind, source::SourceFileUnits};

    fn facts(text: &str) -> SourceFileUnits {
        super::super::extract("guide.md", text, "hash", Language::Unknown, &[])
    }

    fn links(text: &str) -> Vec<MarkdownLink> {
        let facts = facts(text);
        let file = Node {
            kind: NodeKind::File,
            path: "guide.md".into(),
            content_hash: Some("hash".into()),
            bytes: Some(text.len() as u64),
            ..Node::default()
        };
        assert!(super::super::validate(&file, &facts, |_| None).is_ok());
        assert!(facts.units.iter().all(|unit| !unit.links_truncated));
        facts
            .units
            .into_iter()
            .flat_map(|unit| unit.links)
            .collect()
    }

    #[test]
    fn authored_fields_keep_utf8_crlf_escapes_balanced_paths_and_empty_values() {
        for newline in ["\n", "\r\n"] {
            let text = format!(
                "# Guide{newline}[élève](docs/a(b).md \"café title\") ![image](<img/a b.svg> 'alt title'){newline}[escaped\\]](a\\)b) [empty](){newline}<https://example.test/café?q=x>{newline}"
            );
            let found = links(&text);
            assert_eq!(found.len(), 5);
            for (link, label, destination, title, kind) in [
                (
                    &found[0],
                    "élève",
                    "docs/a(b).md",
                    Some("café title"),
                    MarkdownLinkKind::Inline,
                ),
                (
                    &found[1],
                    "image",
                    "img/a b.svg",
                    Some("alt title"),
                    MarkdownLinkKind::Image,
                ),
                (
                    &found[2],
                    "escaped\\]",
                    "a\\)b",
                    None,
                    MarkdownLinkKind::Inline,
                ),
                (&found[3], "empty", "", None, MarkdownLinkKind::Inline),
                (
                    &found[4],
                    "https://example.test/café?q=x",
                    "https://example.test/café?q=x",
                    None,
                    MarkdownLinkKind::Autolink,
                ),
            ] {
                let slice = |span: graph_search_types::node::Span| {
                    &text[span.start_byte as usize..span.end_byte as usize]
                };
                assert_eq!(slice(link.label_span), label);
                assert_eq!(slice(link.destination_span), destination);
                assert_eq!(link.title_span.map(slice), title);
                assert_eq!(link.kind, kind);
                assert_eq!(link.span.start_line, link.span.end_line);
            }
        }
    }

    #[test]
    fn code_html_opaque_blocks_and_escaped_openers_cannot_invent_links() {
        for text in [
            "```\n[label](url)\n```\n",
            "---\nx: '[label](url)'\n---\n",
            "<!--\n[label](url)\n-->\n",
            "> [label](url)\n",
            "    [label](url)\n",
            "text `[label](url)` end\n",
            "text ``[label](url)` ignored`` end\n",
            "text \\[label](url)\n",
            "text <a title='[label](url) >'>hello</a>\n",
            "text <a title='\n[label](url) >\n'>hello</a>\n",
            "text <a title='\n[label](url)\n",
            "- ## Nested [label](url)\n",
            "[reference][id]\n\n[id]: url\n",
            "[split\nlabel](url)\n",
            "[split\\\nlabel](url)\n",
            "[label](split\\\ndestination)\n",
            "[label](url \"split\\\ntitle\")\n",
        ] {
            assert!(links(text).is_empty(), "{text}");
        }
        assert_eq!(links("- [label](url)\n").len(), 1);
        assert_eq!(links("<https://example.test>\n").len(), 1);
        assert_eq!(links("text `[hidden](url)` [shown](url)\n").len(), 1);
        for text in [
            "<a:b>\n",
            "<1abc:def>\n",
            "<mail@example.test>\n",
            "[label](unbalanced(foo)\n",
            "[label](url 'open)\n",
        ] {
            assert!(links(text).is_empty(), "{text}");
        }
    }

    #[test]
    fn record_and_work_caps_preserve_searchable_source_and_report_partial_metadata() {
        let exact = "[label](url) ".repeat(graph_search_types::limits::MAX_MARKDOWN_LINKS);
        assert_eq!(
            links(&exact).len(),
            graph_search_types::limits::MAX_MARKDOWN_LINKS
        );
        for text in [
            format!("{exact}[extra](url)"),
            "[".repeat(graph_search_types::limits::MAX_MARKDOWN_LINK_WORK),
            "a".repeat(graph_search_types::limits::MAX_MARKDOWN_LINK_WORK + 1),
            format!(
                "<https://example.test/{}>",
                "a".repeat(graph_search_types::limits::MAX_MARKDOWN_LINK_WORK.div_ceil(2))
            ),
        ] {
            let facts = facts(&text);
            assert!(facts.units.iter().any(|unit| unit.links_truncated));
            assert!(!facts.truncated);
            assert_eq!(
                facts.units.last().unwrap().span.end_byte as usize,
                text.len()
            );
            let mut coverage = graph_search_types::coverage::Coverage::default();
            super::super::coverage_from(std::iter::once(&facts), &mut coverage);
            assert_eq!(coverage.source_link_truncated_files, 1);
            assert_eq!(coverage.source_unit_truncated_files, 0);
        }
    }

    #[test]
    fn malformed_utf8_safe_markup_never_produces_invalid_metadata() {
        let alphabet = [
            "[", "]", "(", ")", "<", ">", "!", "`", "\\", "\"", "'", " ", "\n", "\r", "é", "😀",
            "a", ":",
        ];
        let mut seed = 7u64;
        for _ in 0..4_096 {
            let mut text = String::from("# Guide\n");
            for _ in 0..64 {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                text.push_str(alphabet[(seed >> 32) as usize % alphabet.len()]);
            }
            for link in links(&text) {
                for span in [
                    Some(link.span),
                    Some(link.label_span),
                    Some(link.destination_span),
                    link.title_span,
                ]
                .into_iter()
                .flatten()
                {
                    assert!(
                        text.get(span.start_byte as usize..span.end_byte as usize)
                            .is_some(),
                        "{text:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn file_link_allowance_is_shared_across_blocks_without_losing_body_terms() {
        let text =
            "[label](url)\n\n".repeat(graph_search_types::limits::MAX_MARKDOWN_LINKS_PER_FILE + 1);
        let facts = facts(&text);
        assert_eq!(
            facts
                .units
                .iter()
                .map(|unit| unit.links.len())
                .sum::<usize>(),
            graph_search_types::limits::MAX_MARKDOWN_LINKS_PER_FILE
        );
        let last = facts.units.last().unwrap();
        assert!(last.links_truncated);
        assert!(last.terms.contains_key("label"));
        assert!(!facts.truncated);
    }

    #[test]
    fn code_span_state_and_link_coordinates_survive_overlapping_windows() {
        let text = format!(
            "# Guide\n`\n{}` [visible](destination)\n",
            "[hidden](url)\n".repeat(100)
        );
        let found = links(&text);
        assert_eq!(found.len(), 1);
        assert_eq!(
            &text[found[0].label_span.start_byte as usize..found[0].label_span.end_byte as usize],
            "visible"
        );
        let text = format!(
            "# Guide\n{}[shared](url)\n{}",
            "prose\n".repeat(74),
            "prose\n".repeat(30)
        );
        let found = links(&text);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0], found[1]);
        let mut inconsistent = facts(&text);
        let repeated = inconsistent
            .units
            .iter_mut()
            .filter(|unit| !unit.links.is_empty())
            .nth(1)
            .unwrap();
        repeated.links[0].destination_span.start_byte += 1;
        assert!(!valid_file(&inconsistent));
    }

    #[test]
    fn persisted_links_reject_foreign_coordinates_and_legacy_fields_default_empty() {
        let text = "# Guide\n[link](destination \"title\")\n";
        let facts = facts(text);
        let file = Node {
            kind: NodeKind::File,
            path: "guide.md".into(),
            content_hash: Some("hash".into()),
            bytes: Some(text.len() as u64),
            ..Node::default()
        };
        for case in 0..6 {
            let mut invalid = facts.clone();
            let unit = invalid
                .units
                .iter_mut()
                .find(|unit| !unit.links.is_empty())
                .unwrap();
            match case {
                0 => unit.links[0].destination_span.end_byte = u32::MAX,
                1 => unit.links[0].label_span.start_byte = 0,
                2 => unit.links[0].span.end_line += 1,
                3 => unit.links[0].kind = MarkdownLinkKind::Autolink,
                4 => unit.links.push(unit.links[0]),
                _ => unit.kind = graph_search_types::source::SourceUnitKind::Code,
            }
            assert!(
                super::super::validate(&file, &invalid, |_| None).is_err(),
                "{case}"
            );
        }
        let mut old = serde_json::to_value(&facts).unwrap();
        old["version"] = 6.into();
        for unit in old["units"].as_array_mut().unwrap() {
            unit.as_object_mut().unwrap().remove("links");
            unit.as_object_mut().unwrap().remove("links_truncated");
        }
        let decoded: SourceFileUnits = serde_json::from_value(old).unwrap();
        assert!(super::super::validate(&file, &decoded, |_| None).is_ok());
        assert!(
            decoded
                .units
                .iter()
                .all(|unit| unit.links.is_empty() && !unit.links_truncated)
        );
    }
}
