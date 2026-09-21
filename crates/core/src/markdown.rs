//! Native Markdown source boundaries; offsets always address the original UTF-8.
//! This is a block-boundary scanner, not a renderer or full `CommonMark` parser.
#[path = "markdown_blocks.rs"]
mod blocks;
#[path = "markdown_html.rs"]
mod html;
pub(crate) use blocks::refine;
#[path = "markdown_tables.rs"]
mod tables;
pub(crate) use tables::Table;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub start: usize,
    pub end: usize,
    pub fenced: bool,
    pub frontmatter: bool,
    pub heading: Option<Heading>,
    pub table: Option<Table>,
    pub block: Option<blocks::Block>,
}

impl Region {
    fn prose(start: usize, end: usize, heading: Option<Heading>) -> Self {
        Self {
            start,
            end,
            heading,
            fenced: false,
            frontmatter: false,
            table: None,
            block: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Heading {
    pub level: u8,
    pub start: usize,
    pub end: usize,
    pub title_start: usize,
    pub title_end: usize,
}

fn atx_heading(line: &str, offset: usize, end: usize) -> Heading {
    let content = line.trim_start_matches(' ');
    let depth = content.bytes().take_while(|byte| *byte == b'#').count();
    let title = content[depth..].trim_start_matches([' ', '\t']);
    let mut trimmed = title.trim_end_matches([' ', '\t']);
    let without_hashes = trimmed.trim_end_matches('#');
    if without_hashes.len() < trimmed.len()
        && (without_hashes.is_empty() || without_hashes.ends_with([' ', '\t']))
    {
        trimmed = without_hashes.trim_end_matches([' ', '\t']);
    }
    let title_start = offset.saturating_add(line.len().saturating_sub(title.len()));
    Heading {
        level: u8::try_from(depth).unwrap_or(6),
        start: offset,
        end,
        title_start,
        title_end: title_start.saturating_add(trimmed.len()),
    }
}

fn setext_heading(line: &str, start: usize, underline: usize, end: usize) -> Heading {
    Heading {
        level: if line.trim_start_matches(' ').starts_with('=') {
            1
        } else {
            2
        },
        start,
        end,
        title_start: start,
        title_end: underline,
    }
}

fn unindent(line: &str) -> Option<&str> {
    let spaces = line.bytes().take_while(|byte| *byte == b' ').count();
    (spaces <= 3).then(|| &line[spaces..])
}
fn opener(line: &str) -> Option<(u8, usize)> {
    let line = unindent(line)?;
    let marker = *line.as_bytes().first()?;
    if marker != b'`' && marker != b'~' {
        return None;
    }
    let count = line.bytes().take_while(|byte| *byte == marker).count();
    if count < 3 || (marker == b'`' && line[count..].contains('`')) {
        return None;
    }
    Some((marker, count))
}
fn closes(line: &str, marker: u8, count: usize) -> bool {
    let Some(line) = unindent(line) else {
        return false;
    };
    let run = line.bytes().take_while(|byte| *byte == marker).count();
    run >= count && line[run..].bytes().all(|byte| matches!(byte, b' ' | b'\t'))
}

pub(crate) struct Fence {
    pub content: std::ops::Range<usize>,
    pub info: std::ops::Range<usize>,
    pub language: Option<std::ops::Range<usize>>,
    pub closed: bool,
}

/// Recover authored field coordinates once per scanned fence, without copying it.
pub(crate) fn fence_fields(text: &str, region: &Region) -> Option<Fence> {
    if !region.fenced {
        return None;
    }
    let block = &text[region.start..region.end];
    let first = block.split_inclusive('\n').next()?;
    let line = first.trim_end_matches(['\r', '\n']);
    let (marker, count) = opener(line)?;
    let tail = &unindent(line)?[count..];
    let info = tail.trim_matches([' ', '\t']);
    let info_start = region.start.saturating_add(
        line.len()
            .saturating_sub(tail.trim_start_matches([' ', '\t']).len()),
    );
    let info_end = info_start.saturating_add(info.len());
    let language = info.split_ascii_whitespace().next().and_then(|label| {
        info.find(label).map(|at| {
            let start = info_start.saturating_add(at);
            start..start.saturating_add(label.len())
        })
    });
    let trimmed = block.strip_suffix('\n').unwrap_or(block);
    let last_start = trimmed.rfind('\n').map_or(0, |at| at.saturating_add(1));
    let closed = last_start >= first.len()
        && closes(trimmed[last_start..].trim_end_matches('\r'), marker, count);
    Some(Fence {
        content: region.start.saturating_add(first.len())..if closed {
            region.start.saturating_add(last_start)
        } else {
            region.end
        },
        info: info_start..info_end,
        language,
        closed,
    })
}
fn heading(line: &str) -> bool {
    let Some(line) = unindent(line) else {
        return false;
    };
    let count = line.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&count)
        && line
            .as_bytes()
            .get(count)
            .is_none_or(|byte| matches!(byte, b' ' | b'\t'))
}

fn setext(line: &str) -> bool {
    let Some(line) = unindent(line) else {
        return false;
    };
    let line = line.trim_end_matches([' ', '\t']);
    let Some(marker) = line.bytes().next() else {
        return false;
    };
    matches!(marker, b'=' | b'-') && line.bytes().all(|byte| byte == marker)
}

// Container/HTML parsing is not implemented yet. Conservatively suppress
// Setext recognition through such a block rather than inventing a title.
fn unsupported_block(line: &str) -> bool {
    let Some(line) = unindent(line) else {
        return true;
    };
    for marker in *b"*_-" {
        let mut count = 0usize;
        if line.bytes().all(|byte| {
            if byte == marker {
                count = count.saturating_add(1);
                true
            } else {
                matches!(byte, b' ' | b'\t')
            }
        }) && count >= 3
        {
            return true;
        }
    }
    if line.starts_with(['>', '\t'])
        || (line.starts_with('<') && !crate::units::links::uri_autolink(line))
        || (line.starts_with('[') && line.contains("]:"))
    {
        return true;
    }
    if let Some(rest) = line.strip_prefix(['-', '+', '*']) {
        return rest.is_empty() || rest.starts_with([' ', '\t']);
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    digits > 0
        && line.get(digits..).is_some_and(|tail| {
            tail.strip_prefix(['.', ')'])
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t']))
        })
}

/// Partition at top-level ATX headings and fenced blocks. Headings inside a
/// fence are code, and a missing closer keeps the fence open through EOF.
pub(crate) fn regions(text: &str) -> Vec<Region> {
    let mut result = Vec::new();
    let frontmatter_end = frontmatter_end(text);
    if frontmatter_end > 0 {
        result.push(Region {
            frontmatter: true,
            ..Region::prose(0, frontmatter_end, None)
        });
    }
    let mut start = frontmatter_end;
    let mut offset = frontmatter_end;
    let mut fence = None;
    let mut current_heading = None;
    let mut paragraph = None;
    for raw in text[frontmatter_end..].split_inclusive('\n') {
        // One omitted-region sentinel is sufficient for extraction to report
        // its unit cap; do not allocate one boundary per line of adversarial input.
        if result.len() > graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE {
            break;
        }
        let line = raw.trim_end_matches('\n').trim_end_matches('\r');
        let next = offset.saturating_add(raw.len());
        if offset < start {
            offset = next;
            continue;
        }
        if let Some((marker, count)) = fence {
            paragraph = None;
            if closes(line, marker, count) {
                result.push(Region {
                    fenced: true,
                    ..Region::prose(start, next, current_heading.take())
                });
                start = next;
                fence = None;
            }
        } else if html::opener(line).is_some()
            && let Some(region) = blocks::opaque(text, offset)
        {
            paragraph = None;
            push_prose(&mut result, start, offset, &mut current_heading);
            start = region.end;
            result.push(region);
        } else if let Some(open) = opener(line) {
            paragraph = None;
            push_prose(&mut result, start, offset, &mut current_heading);
            start = offset;
            fence = Some(open);
        } else if !(setext(line) && paragraph.is_some())
            && let Some(region) = blocks::list(text, offset)
        {
            paragraph = None;
            push_prose(&mut result, start, offset, &mut current_heading);
            start = region.end;
            result.push(region);
        } else if let Some((end, table)) = tables::scan(text, offset) {
            push_prose(&mut result, start, offset, &mut current_heading);
            result.push(Region {
                start: offset,
                end,
                fenced: false,
                frontmatter: false,
                heading: None,
                table: Some(table),
                block: None,
            });
            start = end;
            paragraph = None;
        } else if heading(line) {
            paragraph = None;
            push_prose(&mut result, start, offset, &mut current_heading);
            start = offset;
            current_heading = Some(atx_heading(line, offset, next));
        } else if line.trim_matches([' ', '\t']).is_empty() {
            paragraph = None;
        } else if setext(line) {
            if let Some(title) = paragraph.take() {
                push_prose(&mut result, start, title, &mut current_heading);
                start = title;
                current_heading = Some(setext_heading(line, title, offset, next));
            }
        } else if let Some(region) = blocks::opaque(text, offset) {
            paragraph = None;
            push_prose(&mut result, start, offset, &mut current_heading);
            start = region.end;
            result.push(region);
        } else {
            paragraph.get_or_insert(offset);
        }
        offset = next;
    }
    if start < text.len() {
        result.push(Region {
            fenced: fence.is_some(),
            ..Region::prose(start, text.len(), current_heading.take())
        });
    }
    result
}

fn push_prose(result: &mut Vec<Region>, start: usize, end: usize, heading: &mut Option<Heading>) {
    if start < end {
        result.push(Region::prose(start, end, heading.take()));
    }
}

/// Deliberate frontmatter dialect: an unindented first-line YAML/TOML marker,
/// closed by the matching marker (or YAML's `...`). Missing closers retain the
/// rest as frontmatter. No YAML/TOML value parsing or source rewriting occurs.
fn frontmatter_end(text: &str) -> usize {
    let mut lines = text.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return 0;
    };
    let marker = first.trim_end_matches(['\r', '\n']);
    if marker != "---" && marker != "+++" {
        return 0;
    }
    let mut end = first.len();
    for raw in lines {
        end = end.saturating_add(raw.len());
        let line = raw.trim_end_matches(['\r', '\n']);
        if line == marker || (marker == "---" && line == "...") {
            return end;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_delimiters_shield_headings_fences_and_tables_until_the_closer() {
        for (open, close) in [
            ("<!--", "-->"),
            ("<?processor", "?>"),
            ("<!DOCTYPE", ">"),
            ("<![CDATA[", "]]>"),
            ("<ScRiPt type='example'>", "</sTyLe>"),
            ("<pre", "</pre>"),
            ("<textarea>", "</textarea>"),
        ] {
            for newline in ["\n", "\r\n"] {
                let text = [
                    "# Parent", open, "", "# false", "```", "a|b", "-|-", "élève", "===", close,
                    "## Child", "body",
                ]
                .join(newline);
                let blocks = regions(&text);
                assert_eq!(blocks.len(), 3, "{text}");
                assert!(
                    blocks
                        .iter()
                        .all(|block| !block.fenced && block.table.is_none())
                );
                assert_eq!(blocks[2].heading.unwrap().level, 2);
                assert!(blocks[1].block.unwrap().unsupported);
                assert_eq!(
                    blocks
                        .iter()
                        .map(|block| &text[block.start..block.end])
                        .collect::<String>(),
                    text
                );
                let unclosed = format!("# Parent\n{open}\n\n# false\n```\n");
                let blocks = regions(&unclosed);
                assert_eq!(blocks.len(), 2, "{unclosed}");
                assert!(!blocks[1].fenced);
                assert!(blocks[1].block.unwrap().unsupported);
                assert_eq!(blocks[1].end, unclosed.len());
            }
        }
    }

    #[test]
    fn html_shielding_obeys_opener_and_same_line_closer_boundaries() {
        for line in [
            "<!-- done -->",
            "<?done?>",
            "<!A>",
            "<![CDATA[x]]>",
            "<PRE></pre>",
        ] {
            let text = format!("{line}\n# Visible\n");
            assert_eq!(regions(&text).last().unwrap().heading.unwrap().level, 1);
        }
        for line in [
            "<scripture>",
            "<pre/>",
            "    <!--",
            "\t<!--",
            "text <!--",
            "<![cdata[",
        ] {
            assert!(html::opener(line).is_none(), "{line}");
        }
        let text = "```\n<!--\n```\n# Visible\n";
        let blocks = regions(text);
        assert!(blocks[0].fenced);
        assert!(blocks[1].heading.is_some());
        let text = "<!--\n--> # still HTML\nTitle\n===\n";
        assert_eq!(
            regions(text).last().unwrap().heading.unwrap().title_start,
            text.find("Title").unwrap()
        );
    }

    #[test]
    fn table_delimiters_do_not_become_headings_and_opaque_blocks_stay_opaque() {
        let text = "# Guide\r\nintro\r\n| one |\r\n---\r\nrow\r\n\r\n# Next\r\nend\r\n";
        let blocks = regions(text);
        let table = blocks.iter().find(|part| part.table.is_some()).unwrap();
        assert!(table.heading.is_none());
        assert_eq!(&text[table.start..table.end], "| one |\r\n---\r\nrow\r\n");
        assert_eq!(
            blocks.iter().filter(|part| part.heading.is_some()).count(),
            2
        );
        assert_eq!(
            blocks
                .iter()
                .map(|part| &text[part.start..part.end])
                .collect::<String>(),
            text
        );
        for text in [
            "```\na|b\n-|-\n```\n",
            "---\na|b\n-|-\n---\n",
            "> quote\na|b\n-|-\n",
        ] {
            assert!(regions(text).iter().all(|part| part.table.is_none()));
        }
    }
    #[test]
    fn headings_keep_raw_title_coordinates_without_inline_rewriting() {
        for (source, title, level) in [
            ("  ## café *title* ###  \r\nbody", "café *title*", 2),
            ("### literal###\n", "literal###", 3),
            ("#", "", 1),
            ("# ###\n", "", 1),
            (
                "élève\r\ncontinued\r\n===\r\nbody",
                "élève\r\ncontinued\r\n",
                1,
            ),
            ("Title\n---\n", "Title\n", 2),
        ] {
            let blocks = regions(source);
            let heading = blocks[0].heading.unwrap();
            assert_eq!(heading.level, level);
            assert_eq!(&source[heading.title_start..heading.title_end], title);
            assert_eq!(heading.start, 0);
            assert!(heading.end <= source.len());
        }
    }
    #[test]
    fn setext_sections_preserve_multiline_titles_and_raw_offsets() {
        let text = "# Intro\r\nbody\r\n\r\nélève title\r\ncontinued\r\n  === \t\r\nsection body\r\n\r\nNext\r\n-\r\nlast";
        let blocks = regions(text);
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            &text[blocks[1].start..blocks[1].end],
            "élève title\r\ncontinued\r\n  === \t\r\nsection body\r\n\r\n"
        );
        assert_eq!(&text[blocks[2].start..blocks[2].end], "Next\r\n-\r\nlast");
        assert_eq!(
            blocks
                .iter()
                .map(|r| &text[r.start..r.end])
                .collect::<String>(),
            text
        );
    }

    #[test]
    fn setext_does_not_promote_code_containers_or_invalid_underlines() {
        for body in [
            "title\n= =",
            "title\n    ===",
            "title\n=== suffix",
            "title\n\n===",
            "- item\ncontinued\n---",
            "> quote\ncontinued\n===",
            "    code\n===",
            "<div>\ntext\n===",
            "***\n---",
            "_ _ _\n===",
            "1. item\ntext\n===",
        ] {
            let text = format!("# Intro\n\n{body}");
            assert_eq!(
                regions(&text)
                    .iter()
                    .filter(|region| region.heading.is_some())
                    .count(),
                1,
                "{body}"
            );
        }
        let text = "# Intro\n```\nTitle\n===\n```\n";
        assert_eq!(regions(text).len(), 2);
        assert!(regions(text)[1].fenced);
    }
    #[test]
    fn frontmatter_is_opaque_to_heading_and_fence_boundaries() {
        for (open, close) in [("---", "---"), ("---", "..."), ("+++", "+++")] {
            let prefix =
                format!("{open}\r\ntitle: élève\r\n# metadata comment\r\n```\r\n{close}\r\n");
            let text = format!("{prefix}# Authored heading\r\nbody\r\n");
            let parts = regions(&text);
            assert_eq!(parts.len(), 2);
            assert!(parts[0].frontmatter);
            assert!(!parts[0].fenced);
            assert_eq!(&text[parts[0].start..parts[0].end], prefix);
            assert!(!parts[1].frontmatter);
            assert_eq!(parts[1].start, prefix.len());
            assert_eq!(parts[1].end, text.len());
        }
    }

    #[test]
    fn frontmatter_requires_exact_initial_markers_and_retains_unclosed_bytes() {
        for text in [
            "---\nkey: value\n+++\n# still metadata",
            "+++\n...\n",
            "---",
        ] {
            let parts = regions(text);
            assert_eq!(parts.len(), 1);
            assert!(parts[0].frontmatter);
            assert_eq!(&text[parts[0].start..parts[0].end], text);
        }
        for text in [
            "\n---\nkey\n---",
            " ---\nkey\n---",
            "--- \nkey\n---",
            "\u{feff}---\nkey\n---",
        ] {
            assert!(regions(text).iter().all(|part| !part.frontmatter));
        }
        assert_eq!(frontmatter_end("---\nx\n---"), 9);
        assert_eq!(frontmatter_end(""), 0);
    }

    #[test]
    fn fences_preserve_original_bytes_and_ignore_embedded_headings() {
        let text = "# Intro\r\nélève\r\n````rust\r\n# not a heading\r\n```\r\n~~~~\r\n````\r\n## Next\r\nbody";
        let blocks = regions(text);
        assert_eq!(blocks.len(), 3);
        assert!(!blocks[0].fenced);
        assert!(blocks[1].fenced);
        assert!(!blocks[2].fenced);
        assert_eq!(
            &text[blocks[1].start..blocks[1].end],
            "````rust\r\n# not a heading\r\n```\r\n~~~~\r\n````\r\n"
        );
        assert_eq!(
            blocks
                .iter()
                .map(|part| &text[part.start..part.end])
                .collect::<String>(),
            text
        );
    }
    #[test]
    fn unmatched_and_indented_fences_have_deliberate_boundaries() {
        let text = "text\n   ~~~ language\n# code\n~~~ trailing\n## still code";
        let blocks = regions(text);
        assert_eq!(blocks.len(), 2);
        assert!(blocks[1].fenced);
        assert_eq!(blocks[1].end, text.len());
        assert!(
            regions("    ```\nindented\n")
                .iter()
                .all(|part| !part.fenced)
        );
        assert!(
            regions("```bad`info\ntext\n")
                .iter()
                .all(|part| !part.fenced)
        );
        assert_eq!(regions("#not-heading\n####### too many\nbody\n").len(), 1);
    }
}
