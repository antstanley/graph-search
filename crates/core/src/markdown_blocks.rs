//! Authored paragraph and flat-container boundaries; no rendered text is stored.
use super::Region;
use graph_search_types::source::SourceUnitKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Block {
    pub kind: SourceUnitKind,
    pub marker: Option<(usize, usize)>,
    pub unsupported: bool,
}

fn region(start: usize, end: usize, block: Block) -> Region {
    Region {
        start,
        end,
        fenced: false,
        frontmatter: false,
        heading: None,
        table: None,
        block: Some(block),
    }
}

fn blank(line: &str) -> bool {
    line.trim_matches([' ', '\t', '\r', '\n']).is_empty()
}

pub(super) fn thematic(line: &str) -> bool {
    b"*_-".iter().any(|&marker| {
        let mut count = 0usize;
        line.bytes().all(|byte| {
            if byte == marker {
                count = count.saturating_add(1);
                true
            } else {
                matches!(byte, b' ' | b'\t')
            }
        }) && count >= 3
    })
}

/// Marker indentation, marker width, and start column of authored content.
fn marker(line: &str) -> Option<(usize, usize, usize)> {
    let tail = super::unindent(line)?;
    if thematic(tail) {
        return None;
    }
    let indent = line.len().saturating_sub(tail.len());
    let width = if tail.starts_with(['-', '+', '*']) {
        1
    } else {
        let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
        if !(1..=9).contains(&digits) || !tail.get(digits..)?.starts_with(['.', ')']) {
            return None;
        }
        digits.saturating_add(1)
    };
    let suffix = &tail[width..];
    if !suffix.is_empty() && !suffix.starts_with([' ', '\t']) {
        return None;
    }
    let spaces = suffix.bytes().take_while(|byte| *byte == b' ').count();
    // A tab or unusually deep padding remains authored but is not interpreted.
    Some((
        indent,
        width,
        indent
            .saturating_add(width)
            .saturating_add(if (1..=4).contains(&spaces) { spaces } else { 1 }),
    ))
}

pub(super) fn list(text: &str, start: usize) -> Option<Region> {
    let mut lines = text[start..].split_inclusive('\n');
    let first = lines.next()?;
    let line = first.trim_end_matches(['\r', '\n']);
    let (indent, width, content) = marker(line)?;
    let mut end = start.saturating_add(first.len());
    let mut after_blank = false;
    let first_content = line[content.min(line.len())..].trim_start();
    let mut unsupported = line.contains('\t')
        || line[indent.saturating_add(width)..].starts_with("     ")
        || super::unsupported_block(first_content)
        || super::heading(first_content)
        || super::opener(first_content).is_some();
    for raw in lines {
        let line = raw.trim_end_matches(['\r', '\n']);
        let leading = line.bytes().take_while(|byte| *byte == b' ').count();
        if !blank(line) {
            if marker(line).is_some_and(|(next_indent, _, _)| next_indent <= indent)
                || (leading < content
                    && (after_blank
                        || super::heading(line)
                        || super::opener(line).is_some()
                        || line.trim_start().starts_with('<')
                        || thematic(line)))
            {
                break;
            }
            // Nested blocks remain a single source-bearing item. Do not infer
            // top-level heading ancestry from indentation stripped for this check.
            unsupported |= super::unsupported_block(line.trim_start());
            unsupported |= leading >= content
                && (super::heading(line.trim_start())
                    || super::opener(line.trim_start()).is_some());
        }
        after_blank = blank(line);
        end = end.saturating_add(raw.len());
    }
    Some(region(
        start,
        end,
        Block {
            kind: SourceUnitKind::MarkdownListItem,
            marker: Some((
                start.saturating_add(indent),
                start.saturating_add(indent).saturating_add(width),
            )),
            unsupported,
        },
    ))
}

pub(super) fn opaque(text: &str, start: usize) -> Option<Region> {
    let mut lines = text[start..].split_inclusive('\n');
    let first = lines.next()?;
    let line = first.trim_end_matches(['\r', '\n']);
    let html = super::html::opener(line);
    if html.is_none() && !super::unsupported_block(line) {
        return None;
    }
    let mut end = start.saturating_add(first.len());
    if !html.is_some_and(|stop| stop.reached(line)) {
        for raw in lines {
            let line = raw.trim_end_matches(['\r', '\n']);
            // Generic unsupported constructs use a conservative blank-delimited
            // block; only recognized HTML delimiters can consume blank lines.
            if html.is_none() && blank(line) {
                break;
            }
            end = end.saturating_add(raw.len());
            if html.is_some_and(|stop| stop.reached(line)) {
                break;
            }
        }
    }
    Some(region(
        start,
        end,
        Block {
            kind: SourceUnitKind::MarkdownOpaque,
            marker: None,
            unsupported: true,
        },
    ))
}

/// Keep authored headings separate from paragraph bodies. Blank runs attach to
/// the preceding paragraph; leading blank runs remain plain source regions.
pub(crate) fn refine(text: &str, regions: Vec<Region>) -> Vec<Region> {
    let mut result = Vec::new();
    for mut original in regions {
        if result.len() > graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE {
            break;
        }
        if original.fenced
            || original.frontmatter
            || original.table.is_some()
            || original.block.is_some()
        {
            result.push(original);
            continue;
        }
        if let Some(heading) = original.heading.take() {
            result.push(Region {
                end: heading.end,
                heading: Some(heading),
                ..original
            });
            original.start = heading.end;
        }
        let mut start = original.start;
        let mut offset = start;
        let mut after_blank = false;
        let mut has_content = false;
        for raw in text[start..original.end].split_inclusive('\n') {
            if result.len() > graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE {
                return result;
            }
            let is_blank = blank(raw);
            if after_blank && !is_blank {
                result.push(if has_content {
                    region(
                        start,
                        offset,
                        Block {
                            kind: SourceUnitKind::MarkdownParagraph,
                            marker: None,
                            unsupported: false,
                        },
                    )
                } else {
                    Region {
                        start,
                        end: offset,
                        ..original
                    }
                });
                start = offset;
                has_content = false;
            }
            has_content |= !is_blank;
            after_blank = is_blank;
            offset = offset.saturating_add(raw.len());
        }
        if start < original.end {
            result.push(if has_content {
                region(
                    start,
                    original.end,
                    Block {
                        kind: SourceUnitKind::MarkdownParagraph,
                        marker: None,
                        unsupported: false,
                    },
                )
            } else {
                Region { start, ..original }
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_block_sequences_partition_original_utf8_without_gaps_or_overlap() {
        let atoms = [
            "# café\r\n",
            "élève\n",
            "\n",
            "===\n",
            "-\n",
            "- item\n",
            "  ## nested\n",
            "```\n",
            "<!--\n",
            "-->\n",
            "a|b\n-|-\n",
            "> quote\n",
        ];
        for a in atoms {
            for b in atoms {
                for c in atoms {
                    let text = format!("{a}{b}{c}");
                    let parts = refine(&text, super::super::regions(&text));
                    let mut end = 0;
                    for part in parts {
                        assert_eq!(part.start, end, "{text:?}");
                        assert!(part.end > part.start, "{text:?}");
                        assert!(text.get(part.start..part.end).is_some());
                        end = part.end;
                    }
                    assert_eq!(end, text.len(), "{text:?}");
                }
            }
        }
    }

    #[test]
    fn many_small_blocks_stop_at_the_source_unit_cap_with_explicit_truncation() {
        for atom in ["paragraph\n\n", "- item\n", "# heading\n"] {
            let text = atom.repeat(graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE + 100);
            let parts = refine(&text, super::super::regions(&text));
            assert!(parts.len() <= graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE + 3);
            let facts = crate::units::extract(
                "guide.md",
                &text,
                "hash",
                graph_search_types::Language::Unknown,
                &[],
            );
            assert!(facts.truncated);
            assert_eq!(
                facts.units.len(),
                graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE
            );
            for unit in facts.units {
                assert!(
                    text.get(unit.span.start_byte as usize..unit.span.end_byte as usize)
                        .is_some()
                );
            }
        }
    }

    #[test]
    fn paragraphs_and_list_items_partition_authored_bytes_and_keep_nested_blocks_opaque() {
        for newline in ["\n", "\r\n"] {
            let text = [
                "# Guide",
                "",
                "élève first",
                "continued",
                "",
                "Second paragraph.",
                "",
                "- first item",
                "  continuation",
                "  - nested item",
                "  ## nested heading",
                "",
                "  nested paragraph",
                "- second item",
                "",
                "After list",
                "",
                "<!--",
                "# hidden",
                "",
                "-->",
                "## Next",
                "last",
            ]
            .join(newline);
            let parts = refine(&text, super::super::regions(&text));
            assert_eq!(
                parts
                    .iter()
                    .map(|part| &text[part.start..part.end])
                    .collect::<String>(),
                text
            );
            let items: Vec<_> = parts
                .iter()
                .filter(|part| {
                    part.block
                        .is_some_and(|block| block.kind == SourceUnitKind::MarkdownListItem)
                })
                .collect();
            assert_eq!(items.len(), 2);
            assert!(items[0].block.unwrap().unsupported);
            assert!(!items[1].block.unwrap().unsupported);
            assert!(text[items[0].start..items[0].end].contains("nested paragraph"));
            let headings: Vec<_> = parts.iter().filter_map(|part| part.heading).collect();
            assert_eq!(
                headings
                    .iter()
                    .map(|heading| heading.level)
                    .collect::<Vec<_>>(),
                [1, 2]
            );
            let paragraphs: Vec<_> = parts
                .iter()
                .filter(|part| {
                    part.block
                        .is_some_and(|block| block.kind == SourceUnitKind::MarkdownParagraph)
                })
                .map(|part| &text[part.start..part.end])
                .collect();
            assert_eq!(paragraphs.len(), 4);
            assert!(paragraphs[0].starts_with("élève first"));
            assert!(paragraphs[2].starts_with("After list"));
        }
    }

    #[test]
    fn ordered_and_empty_items_keep_exact_markers_and_do_not_capture_next_headings() {
        for content in ["# nested", "```", "> quote", "- nested", "<div>"] {
            let text = format!("- {content}\n");
            assert!(list(&text, 0).unwrap().block.unwrap().unsupported);
        }
        for marker in ["-", "+", "*", "1.", "23)", "123456789."] {
            let text = format!("   {marker} café\n# After\n");
            let item = list(&text, 0).unwrap();
            let (start, end) = item.block.unwrap().marker.unwrap();
            assert_eq!(&text[start..end], marker);
            assert_eq!(&text[item.end..], "# After\n");
            let empty = format!("{marker}\n\ntext\n");
            assert_eq!(&empty[list(&empty, 0).unwrap().end..], "text\n");
        }
        for line in [
            "1234567890. too long",
            "-no space",
            "    - code",
            "- - -",
            "***",
            "* * *",
        ] {
            assert!(marker(line).is_none(), "{line}");
        }
    }
}
