//! Native source partitioning and token facts, produced during reconciliation.

use graph_search_types::{
    Language, Node,
    node::Span,
    source::{
        MarkdownBlock, MarkdownFence, MarkdownHeading, MarkdownTable, SourceFileUnits, SourceUnit,
        SourceUnitKind,
    },
};
use std::collections::{BTreeMap, BTreeSet};
#[path = "markdown_links.rs"]
pub(crate) mod links;

/// Builds bounded overlapping windows within declaration boundaries. Unknown
/// languages, configuration, Markdown and parser-quarantined text remain eligible.
#[must_use]
pub fn extract(
    path: &str,
    text: &str,
    hash: &str,
    language: Language,
    symbols: &[Node],
) -> SourceFileUnits {
    extract_documented(
        path,
        text,
        hash,
        language,
        symbols,
        DocumentationInput::default(),
    )
}

/// Parser-owned metadata supplied separately from lexical declaration ownership.
#[derive(Clone, Copy, Default)]
pub struct DocumentationInput<'a> {
    /// Recognized comment spans and conservative declaration associations.
    pub comments: &'a [graph_search_types::source::DocumentationComment],
    /// Whether metadata extraction omitted eligible comments.
    pub truncated: bool,
    /// Recognized framework script regions, extracted or explicitly omitted.
    pub embedded: &'a [graph_search_types::extraction::EmbeddedRegionFact],
    /// Whether the framework region scan reached an adapter bound.
    pub embedded_truncated: bool,
}

/// Builds source regions with validated parser-owned documentation metadata.
#[must_use]
#[allow(clippy::too_many_lines)] // one pass over ordered block boundaries
pub fn extract_documented(
    path: &str,
    text: &str,
    hash: &str,
    language: Language,
    symbols: &[Node],
    documentation: DocumentationInput<'_>,
) -> SourceFileUnits {
    let mut result = SourceFileUnits {
        documentation_truncated: documentation.truncated,
        embedded_regions: u32::try_from(documentation.embedded.len()).unwrap_or(u32::MAX),
        embedded_unextracted_regions: u32::try_from(
            documentation
                .embedded
                .iter()
                .filter(|fact| !fact.extracted)
                .count(),
        )
        .unwrap_or(u32::MAX),
        embedded_truncated: documentation.embedded_truncated,
        source_hash: hash.into(),
        version: graph_search_types::limits::SOURCE_INDEX_VERSION,
        ..SourceFileUnits::default()
    };
    let offsets = line_offsets(text);
    let mut boundaries = symbol_boundaries(text, symbols);
    let documents = documentation_boundaries(
        text,
        &offsets,
        &mut boundaries,
        documentation.comments,
        &mut result.documentation_truncated,
    );
    let source_kind = kind(path, language);
    let markdown = if source_kind == SourceUnitKind::Markdown {
        crate::markdown::refine(text, crate::markdown::regions(text))
    } else {
        Vec::new()
    };
    for region in &markdown {
        boundaries.entry(region.start).or_default();
        boundaries.entry(region.end).or_default();
    }
    let boundaries: Vec<_> = boundaries.into_iter().collect();
    let mut active = BTreeSet::new();
    let mut headings = Vec::new();
    let mut markdown_position = 0;
    let mut fence = None;
    let mut table = None;
    let mut block = None;
    let mut link_facts = (Vec::new(), false);
    let mut link_allowance = graph_search_types::limits::MAX_MARKDOWN_LINKS_PER_FILE;
    'regions: for pair in boundaries.windows(2) {
        let (start, events) = &pair[0];
        let end = pair[1].0;
        while let Some(region) = markdown.get(markdown_position)
            && region.start <= *start
        {
            if let Some(heading) = region.heading {
                update_headings(&mut headings, heading, &offsets);
            }
            fence = fence_metadata(text, region, &offsets);
            table = table_metadata(region, &offsets);
            block = block_metadata(region, &offsets);
            link_facts = links::region(text, region, &offsets, &mut link_allowance);
            markdown_position = markdown_position.saturating_add(1);
        }
        let owner = update_owners(&mut active, events, symbols);
        let mut cursor = *start;
        while cursor < end {
            if result.units.len() == graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE {
                result.truncated = true;
                break 'regions;
            }
            let first_line = offsets
                .partition_point(|&offset| offset <= cursor)
                .saturating_sub(1);
            let limit_line =
                first_line.saturating_add(graph_search_types::limits::SOURCE_UNIT_LINES);
            let finish = offsets
                .get(limit_line)
                .copied()
                .unwrap_or(text.len())
                .min(end);
            let (terms, identifiers) = analyze_lines(&text[cursor..finish], first_line);
            let documentation = documents
                .get(
                    documents.partition_point(|document| document.span.end_byte as usize <= cursor),
                )
                .filter(|document| document.span.start_byte as usize <= cursor)
                .cloned();
            result.units.push(SourceUnit {
                span: source_span(cursor, finish, &offsets),
                kind: if documentation.is_some() {
                    SourceUnitKind::DocumentationComment
                } else {
                    region_kind(source_kind, &markdown, cursor)
                },
                documentation,
                owner: owner.clone(),
                terms,
                identifiers,
                headings: headings.clone(),
                fence,
                table,
                block,
                links: links::within(&link_facts.0, cursor, finish),
                links_truncated: link_facts.1,
            });
            if finish == end {
                break;
            }
            cursor =
                offsets[limit_line.saturating_sub(graph_search_types::limits::SOURCE_UNIT_OVERLAP)];
        }
    }
    result
}

fn update_owners(
    active: &mut BTreeSet<(u32, usize)>,
    events: &[(bool, usize)],
    symbols: &[Node],
) -> Option<graph_search_types::NodeId> {
    for &(add, i) in events {
        let span = symbols[i].span.unwrap_or_default();
        let key = (span.end_byte.saturating_sub(span.start_byte), i);
        if add {
            active.insert(key);
        } else {
            active.remove(&key);
        }
    }
    active.first().map(|&(_, i)| symbols[i].id.clone())
}

fn line_offsets(text: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    offsets.extend(
        text.bytes()
            .enumerate()
            .filter_map(|(position, byte)| (byte == b'\n').then_some(position.saturating_add(1))),
    );
    if offsets.last() != Some(&text.len()) {
        offsets.push(text.len());
    }
    offsets
}

fn documentation_boundaries(
    text: &str,
    offsets: &[usize],
    boundaries: &mut BoundaryEvents,
    documentation: &[graph_search_types::source::DocumentationComment],
    truncated: &mut bool,
) -> Vec<graph_search_types::source::DocumentationComment> {
    let mut documents: Vec<graph_search_types::source::DocumentationComment> = Vec::new();
    let mut previous_end = 0;
    for (ordinal, document) in documentation.iter().enumerate() {
        let start = document.span.start_byte as usize;
        let end = document.span.end_byte as usize;
        if ordinal == graph_search_types::limits::MAX_DOC_COMMENTS_PER_FILE {
            *truncated = true;
            break;
        }
        if start < previous_end || start >= end || text.get(start..end).is_none() {
            *truncated = true;
            continue;
        }
        let mut document = document.clone();
        document.span = source_span(start, end, offsets);
        previous_end = end;
        if let Some(previous) = documents.last_mut()
            && previous.inner == document.inner
            && previous.documented_symbol == document.documented_symbol
            && text[previous.span.end_byte as usize..start]
                .trim()
                .is_empty()
        {
            previous.span.end_byte = document.span.end_byte;
            previous.span.end_line = document.span.end_line;
            continue;
        }
        documents.push(document);
    }
    for document in &documents {
        boundaries
            .entry(document.span.start_byte as usize)
            .or_default();
        boundaries
            .entry(document.span.end_byte as usize)
            .or_default();
    }
    documents
}

type BoundaryEvents = BTreeMap<usize, Vec<(bool, usize)>>;

fn symbol_boundaries(text: &str, symbols: &[Node]) -> BoundaryEvents {
    let mut boundaries = BoundaryEvents::new();
    boundaries.entry(0).or_default();
    boundaries.entry(text.len()).or_default();
    for (i, node) in symbols.iter().enumerate() {
        // A reexport member is a resolution alias, not an authored declaration;
        // it must not partition source regions.
        if node.attribute("rust_reexport").is_some() {
            continue;
        }
        let Some(span) = node.span else {
            continue;
        };
        let start = span.start_byte as usize;
        let end = span.end_byte as usize;
        if start >= end
            || end > text.len()
            || !text.is_char_boundary(start)
            || !text.is_char_boundary(end)
        {
            continue;
        }
        boundaries.entry(start).or_default().push((true, i));
        boundaries.entry(end).or_default().push((false, i));
    }
    boundaries
}

type TermLines = BTreeMap<String, Vec<u32>>;

fn source_span(start: usize, end: usize, offsets: &[usize]) -> Span {
    let location = if offsets.last() == Some(&start) {
        start.saturating_sub(1)
    } else {
        start
    };
    let first = offsets.partition_point(|&offset| offset <= location).max(1);
    let last = offsets.partition_point(|&offset| offset < end).max(first);
    Span::new(
        u32::try_from(first).unwrap_or(u32::MAX),
        u32::try_from(last).unwrap_or(u32::MAX),
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
    )
}

fn update_headings(
    ancestry: &mut Vec<MarkdownHeading>,
    heading: crate::markdown::Heading,
    offsets: &[usize],
) {
    ancestry.retain(|parent| parent.level < heading.level);
    ancestry.push(MarkdownHeading {
        level: heading.level,
        span: source_span(heading.start, heading.end, offsets),
        title_span: source_span(heading.title_start, heading.title_end, offsets),
    });
}

fn fence_metadata(
    text: &str,
    region: &crate::markdown::Region,
    offsets: &[usize],
) -> Option<MarkdownFence> {
    let fields = crate::markdown::fence_fields(text, region)?;
    Some(MarkdownFence {
        span: source_span(region.start, region.end, offsets),
        content_span: source_span(fields.content.start, fields.content.end, offsets),
        info_span: source_span(fields.info.start, fields.info.end, offsets),
        language_span: fields
            .language
            .map(|range| source_span(range.start, range.end, offsets)),
        closed: fields.closed,
    })
}

fn contains_span(outer: Span, inner: Span) -> bool {
    inner.start_byte >= outer.start_byte
        && inner.end_byte >= inner.start_byte
        && inner.end_byte <= outer.end_byte
        && inner.start_line >= outer.start_line
        && inner.end_line >= inner.start_line
        && inner.end_line <= outer.end_line
}

fn table_metadata(region: &crate::markdown::Region, offsets: &[usize]) -> Option<MarkdownTable> {
    let table = region.table?;
    Some(MarkdownTable {
        span: source_span(region.start, region.end, offsets),
        header_span: source_span(region.start, table.header_end, offsets),
        delimiter_span: source_span(table.header_end, table.delimiter_end, offsets),
        columns: table.columns,
    })
}

fn valid_table(unit: &SourceUnit, file: &Node, version: u32) -> bool {
    let Some(table) = unit.table else {
        return version < 5 || unit.kind != SourceUnitKind::MarkdownTable;
    };
    unit.kind == SourceUnitKind::MarkdownTable
        && table.columns > 0
        && contains_span(table.span, unit.span)
        && contains_span(table.span, table.header_span)
        && contains_span(table.span, table.delimiter_span)
        && table.header_span.start_byte == table.span.start_byte
        && table.header_span.start_line == table.span.start_line
        && table.header_span.start_line > 0
        && table.header_span.end_byte > table.header_span.start_byte
        && table.header_span.end_line == table.header_span.start_line
        && table.delimiter_span.start_byte == table.header_span.end_byte
        && table.delimiter_span.end_byte > table.delimiter_span.start_byte
        && table.delimiter_span.end_line == table.delimiter_span.start_line
        && table.delimiter_span.start_line == table.header_span.end_line.saturating_add(1)
        && file
            .bytes
            .is_none_or(|bytes| u64::from(table.span.end_byte) <= bytes)
}

fn valid_fence(unit: &SourceUnit, file: &Node, version: u32) -> bool {
    let Some(fence) = unit.fence else {
        return version < 4 || unit.kind != SourceUnitKind::MarkdownCodeFence;
    };
    unit.kind == SourceUnitKind::MarkdownCodeFence
        && fence.span.start_line > 0
        && fence.span.end_byte > fence.span.start_byte
        && contains_span(fence.span, unit.span)
        && contains_span(fence.span, fence.content_span)
        && contains_span(fence.span, fence.info_span)
        && fence.info_span.end_byte <= fence.content_span.start_byte
        && fence.info_span.start_line == fence.span.start_line
        && fence.info_span.end_line == fence.span.start_line
        && fence.language_span.is_none_or(|label| {
            contains_span(fence.info_span, label) && label.start_byte < label.end_byte
        })
        && (!fence.closed || fence.content_span.end_byte < fence.span.end_byte)
        && (fence.closed || fence.content_span.end_byte == fence.span.end_byte)
        && file
            .bytes
            .is_none_or(|bytes| u64::from(fence.span.end_byte) <= bytes)
}

fn block_metadata(region: &crate::markdown::Region, offsets: &[usize]) -> Option<MarkdownBlock> {
    region.block.map(|block| MarkdownBlock {
        span: source_span(region.start, region.end, offsets),
        marker_span: block
            .marker
            .map(|(start, end)| source_span(start, end, offsets)),
        contains_unsupported: block.unsupported,
    })
}

fn valid_block(unit: &SourceUnit, file: &Node, version: u32) -> bool {
    let structured = matches!(
        unit.kind,
        SourceUnitKind::MarkdownParagraph
            | SourceUnitKind::MarkdownListItem
            | SourceUnitKind::MarkdownOpaque
    );
    let Some(block) = unit.block else {
        return version < 6 || !structured;
    };
    structured
        && contains_span(block.span, unit.span)
        && block.span.start_line > 0
        && block.span.end_byte > block.span.start_byte
        && file
            .bytes
            .is_none_or(|bytes| u64::from(block.span.end_byte) <= bytes)
        && match unit.kind {
            SourceUnitKind::MarkdownListItem => block.marker_span.is_some_and(|marker| {
                contains_span(block.span, marker)
                    && marker.start_byte < marker.end_byte
                    && marker.start_line == block.span.start_line
                    && marker.end_line == marker.start_line
                    && marker.end_byte.saturating_sub(marker.start_byte) <= 10
            }),
            SourceUnitKind::MarkdownOpaque => {
                block.marker_span.is_none() && block.contains_unsupported
            }
            _ => block.marker_span.is_none() && !block.contains_unsupported,
        }
}

fn valid_headings(unit: &SourceUnit, file: &Node) -> bool {
    if unit.headings.len() > 6
        || (!unit.headings.is_empty()
            && !matches!(
                unit.kind,
                SourceUnitKind::Markdown
                    | SourceUnitKind::MarkdownCodeFence
                    | SourceUnitKind::MarkdownTable
                    | SourceUnitKind::MarkdownParagraph
                    | SourceUnitKind::MarkdownListItem
                    | SourceUnitKind::MarkdownOpaque
            ))
    {
        return false;
    }
    let mut previous_level = 0;
    let mut previous_end = 0;
    for heading in &unit.headings {
        let span = heading.span;
        let title = heading.title_span;
        if heading.level <= previous_level
            || heading.level > 6
            || span.start_byte < previous_end
            || span.start_byte > unit.span.start_byte
            || span.start_line == 0
            || span.end_line < span.start_line
            || span.end_byte <= span.start_byte
            || title.start_byte < span.start_byte
            || title.end_byte > span.end_byte
            || title.end_byte < title.start_byte
            || title.start_line < span.start_line
            || title.end_line > span.end_line
            || title.end_line < title.start_line
            || file
                .bytes
                .is_some_and(|bytes| u64::from(span.end_byte) > bytes)
        {
            return false;
        }
        previous_level = heading.level;
        previous_end = span.end_byte;
    }
    true
}

fn region_kind(
    default: SourceUnitKind,
    markdown: &[crate::markdown::Region],
    cursor: usize,
) -> SourceUnitKind {
    match markdown.get(markdown.partition_point(|region| region.end <= cursor)) {
        Some(region) if region.frontmatter => SourceUnitKind::MarkdownFrontmatter,
        Some(region) if region.fenced => SourceUnitKind::MarkdownCodeFence,
        Some(region) if region.table.is_some() => SourceUnitKind::MarkdownTable,
        Some(region) if region.block.is_some() => region.block.map_or(default, |block| block.kind),
        _ => default,
    }
}

fn analyze_lines(text: &str, first_line: usize) -> (TermLines, TermLines) {
    let mut terms: TermLines = BTreeMap::new();
    let mut identifiers: TermLines = BTreeMap::new();
    for (line, text) in text.lines().enumerate() {
        let number =
            u32::try_from(first_line.saturating_add(line).saturating_add(1)).unwrap_or(u32::MAX);
        for word in crate::analyzer::lexemes(text) {
            identifiers
                .entry(word.text.to_owned())
                .or_default()
                .push(number);
        }
        for token in crate::lexical::tokens(text) {
            terms.entry(token).or_default().push(number);
        }
    }
    (terms, identifiers)
}

/// Adds generation-owned source retrieval coverage to a query or index report.
pub fn coverage(
    files: &BTreeMap<String, SourceFileUnits>,
    coverage: &mut graph_search_types::coverage::Coverage,
) {
    coverage_from(files.values(), coverage);
}

/// Computes coverage from a prepared generation without cloning its source facts.
pub(crate) fn coverage_from<'a>(
    files: impl Iterator<Item = &'a SourceFileUnits>,
    coverage: &mut graph_search_types::coverage::Coverage,
) {
    coverage.package_scope_incomplete_files = 0;
    coverage.source_indexed_files = 0;
    coverage.source_units = 0;
    coverage.source_unit_truncated_files = 0;
    coverage.source_link_truncated_files = 0;
    coverage.source_documentation_truncated_files = 0;
    coverage.source_framework_region_files = 0;
    coverage.source_framework_regions = 0;
    coverage.source_framework_unextracted_regions = 0;
    coverage.source_framework_truncated_files = 0;
    for file in files {
        coverage.package_scope_incomplete_files = coverage
            .package_scope_incomplete_files
            .saturating_add(u64::from(file.package_scope_incomplete));
        coverage.source_documentation_truncated_files = coverage
            .source_documentation_truncated_files
            .saturating_add(u64::from(file.documentation_truncated));
        coverage.source_link_truncated_files =
            coverage
                .source_link_truncated_files
                .saturating_add(u64::from(
                    file.units.iter().any(|unit| unit.links_truncated),
                ));
        coverage.source_framework_region_files = coverage
            .source_framework_region_files
            .saturating_add(u64::from(file.embedded_regions > 0));
        coverage.source_framework_regions = coverage
            .source_framework_regions
            .saturating_add(u64::from(file.embedded_regions));
        coverage.source_framework_unextracted_regions = coverage
            .source_framework_unextracted_regions
            .saturating_add(u64::from(file.embedded_unextracted_regions));
        coverage.source_framework_truncated_files = coverage
            .source_framework_truncated_files
            .saturating_add(u64::from(file.embedded_truncated));
        coverage.source_indexed_files = coverage.source_indexed_files.saturating_add(1);
        coverage.source_units = coverage
            .source_units
            .saturating_add(file.units.len() as u64);
        coverage.source_unit_truncated_files = coverage
            .source_unit_truncated_files
            .saturating_add(u64::from(file.truncated));
    }
    if coverage.source_unit_truncated_files > 0
        && !coverage
            .truncations
            .iter()
            .any(|t| t.kind == graph_search_types::result::TruncationKind::SourceUnits)
    {
        coverage
            .truncations
            .push(graph_search_types::result::Truncation::new(
                graph_search_types::result::TruncationKind::SourceUnits,
                graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE as u64,
                "source-region indexing reached its per-file cap; source retrieval is partial",
            ));
    }
}

/// Validates source facts against the complete post-batch node universe.
/// Package identities can refer to another file; lexical owners still must be local.
/// # Errors
/// If replacements or removals leave any source record with an invalid association.
pub fn validate_batch<'a>(
    batch: &'a graph_search_types::WriteBatch,
    nodes: impl Iterator<Item = &'a Node>,
    sources: &BTreeMap<String, SourceFileUnits>,
) -> crate::Result<()> {
    let replaced: BTreeSet<_> = batch
        .removed_files
        .iter()
        .map(String::as_str)
        .chain(batch.upserts.iter().map(|item| item.file.path.as_str()))
        .collect();
    let mut post_nodes: BTreeMap<_, _> = nodes
        .filter(|node| !replaced.contains(node.path.as_str()))
        .map(|node| (&node.id, node))
        .collect();
    for item in &batch.upserts {
        post_nodes.insert(&item.file.id, &item.file);
        post_nodes.extend(item.symbols.iter().map(|node| (&node.id, node)));
    }
    for item in &batch.upserts {
        if let Some(source) = &item.source {
            validate(&item.file, source, |id| post_nodes.get(id).copied())?;
        }
    }
    for (path, source) in sources {
        if !replaced.contains(path.as_str()) {
            let file = post_nodes
                .get(&graph_search_types::NodeId::file(path))
                .ok_or_else(|| {
                    crate::Error::Store(format!("source facts without file owner: {path}"))
                })?;
            if !crate::packages::valid(file, source, &|id| post_nodes.get(id).copied()) {
                return Err(crate::Error::Store(format!(
                    "package facts do not match {path}"
                )));
            }
        }
    }
    Ok(())
}

/// Validates that retrieval facts belong to the projected source version.
/// # Errors
/// On mismatched hash/version, invalid coordinates, or an invalid declaration owner.
pub fn validate<'a>(
    file: &Node,
    source: &SourceFileUnits,
    lookup: impl Fn(&graph_search_types::NodeId) -> Option<&'a Node>,
) -> crate::Result<()> {
    if file.content_hash.as_deref() != Some(source.source_hash.as_str())
        || !crate::packages::valid(file, source, &lookup)
        || source.typescript_config.as_ref().is_some_and(|config| {
            source.version < 14
                || !config.valid()
                || !std::path::Path::new(&file.path)
                    .extension()
                    .is_some_and(|ext| ext == "json" || ext == "jsonc")
        })
        || source.version == 0
        || source.version > graph_search_types::limits::SOURCE_INDEX_VERSION
        || source.embedded_unextracted_regions > source.embedded_regions
        || source.units.iter().any(|unit| {
            unit.owner.as_ref().is_some_and(|id| {
                lookup(id).is_none_or(|owner| {
                    owner.is_file()
                        || owner.path != file.path
                        || owner.span.is_none_or(|span| {
                            span.start_byte > unit.span.start_byte
                                || span.end_byte < unit.span.end_byte
                                || span.start_line > unit.span.start_line
                                || span.end_line < unit.span.end_line
                        })
                })
            }) || unit.span.start_line == 0
                || !valid_documentation(unit, file, &lookup)
                || !valid_headings(unit, file)
                || !valid_fence(unit, file, source.version)
                || !valid_table(unit, file, source.version)
                || !valid_block(unit, file, source.version)
                || !links::valid(unit)
                || unit.span.end_line < unit.span.start_line
                || unit.span.end_byte < unit.span.start_byte
                || unit
                    .terms
                    .iter()
                    .chain(&unit.identifiers)
                    .any(|(term, lines)| {
                        term.is_empty()
                            || lines.is_empty()
                            || !lines.is_sorted()
                            || lines.iter().any(|&line| {
                                line < unit.span.start_line || line > unit.span.end_line
                            })
                    })
                || file
                    .bytes
                    .is_some_and(|bytes| u64::from(unit.span.end_byte) > bytes)
        })
        || !valid_documentation_file(source)
        || !links::valid_file(source)
    {
        return Err(crate::Error::Store(format!(
            "source retrieval facts do not match {}",
            file.path
        )));
    }
    Ok(())
}

fn valid_documentation_file(source: &SourceFileUnits) -> bool {
    let mut documents = BTreeMap::new();
    for unit in &source.units {
        if let Some(document) = &unit.documentation {
            if let Some(previous) = documents.insert(document.span.start_byte, document)
                && previous != document
            {
                return false;
            }
            if documents.len() > graph_search_types::limits::MAX_DOC_COMMENTS_PER_FILE {
                return false;
            }
        }
    }
    let mut end = 0;
    for document in documents.values() {
        if document.span.start_byte < end {
            return false;
        }
        end = document.span.end_byte;
    }
    true
}

fn valid_documentation<'a>(
    unit: &SourceUnit,
    file: &Node,
    lookup: &impl Fn(&graph_search_types::NodeId) -> Option<&'a Node>,
) -> bool {
    let Some(document) = &unit.documentation else {
        return unit.kind != SourceUnitKind::DocumentationComment;
    };
    unit.kind == SourceUnitKind::DocumentationComment
        && document.span.start_line > 0
        && document.span.start_byte < document.span.end_byte
        && contains_span(document.span, unit.span)
        && file
            .bytes
            .is_none_or(|bytes| u64::from(document.span.end_byte) <= bytes)
        && document.documented_symbol.as_ref().is_none_or(|id| {
            lookup(id).is_some_and(|node| {
                !node.is_file()
                    && node.path == file.path
                    && node.span.is_some_and(|span| {
                        if document.inner {
                            contains_span(span, document.span)
                        } else {
                            document.span.end_byte <= span.start_byte
                                && document.span.end_line <= span.start_line
                        }
                    })
            })
        })
}

pub(crate) fn kind(path: &str, language: Language) -> SourceUnitKind {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    match extension {
        "md" | "mdx" | "markdown" => SourceUnitKind::Markdown,
        "json" | "jsonc" | "toml" | "yaml" | "yml" | "ini" | "env" => SourceUnitKind::Configuration,
        _ if language != Language::Unknown => SourceUnitKind::Code,
        _ => SourceUnitKind::Text,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use graph_search_types::{NodeId, NodeKind};
    #[test]
    fn malformed_documentation_metadata_preserves_body_and_reports_omission() {
        let text = "/** café zirconium */";
        let malformed = graph_search_types::source::DocumentationComment {
            span: Span::new(1, 1, 0, u32::MAX),
            documented_symbol: None,
            inner: false,
        };
        let fallback = extract_documented(
            "a.rs",
            text,
            "hash",
            Language::Rust,
            &[],
            DocumentationInput {
                comments: &[malformed],
                truncated: false,
                ..DocumentationInput::default()
            },
        );
        assert!(fallback.documentation_truncated);
        assert!(
            fallback
                .units
                .iter()
                .any(|unit| unit.terms.contains_key("zirconium"))
        );
        assert!(
            fallback
                .units
                .iter()
                .all(|unit| unit.documentation.is_none())
        );
        let empty = extract_documented(
            "a.rs",
            "",
            "hash",
            Language::Rust,
            &[],
            DocumentationInput {
                comments: &[graph_search_types::source::DocumentationComment {
                    span: Span::new(1, 1, 0, 1),
                    documented_symbol: None,
                    inner: false,
                }],
                truncated: false,
                ..DocumentationInput::default()
            },
        );
        assert!(empty.documentation_truncated);
        assert!(empty.units.is_empty());
    }

    #[test]
    fn documentation_fragments_preserve_bytes_and_validate_separate_associations() {
        use graph_search_types::source::DocumentationComment;
        let text = format!(
            "/**\n{}*/\nfn work() {{}}\n",
            "café zirconium\n".repeat(160)
        );
        let start = text.find("fn work").unwrap();
        let symbol = Node {
            id: NodeId::new("work"),
            path: "a.rs".into(),
            kind: NodeKind::Function,
            span: Some(Span::new(
                163,
                163,
                u32::try_from(start).unwrap(),
                u32::try_from(text.len()).unwrap(),
            )),
            ..Node::default()
        };
        let document = DocumentationComment {
            span: Span::new(1, 162, 0, u32::try_from(start - 1).unwrap()),
            documented_symbol: Some(symbol.id.clone()),
            inner: false,
        };
        let file = Node {
            kind: NodeKind::File,
            path: "a.rs".into(),
            content_hash: Some("hash".into()),
            bytes: Some(text.len() as u64),
            ..Node::default()
        };
        let facts = extract_documented(
            "a.rs",
            &text,
            "hash",
            Language::Rust,
            std::slice::from_ref(&symbol),
            DocumentationInput {
                comments: &[document],
                truncated: true,
                ..DocumentationInput::default()
            },
        );
        let lookup = |id: &NodeId| (id == &symbol.id).then_some(&symbol);
        assert!(validate(&file, &facts, lookup).is_ok());
        let documents: Vec<_> = facts
            .units
            .iter()
            .filter(|unit| unit.documentation.is_some())
            .collect();
        assert_eq!(documents.len(), 3);
        for unit in documents {
            assert!(unit.owner.is_none());
            assert!(unit.span.end_line - unit.span.start_line < 80);
            assert!(
                text[unit.span.start_byte as usize..unit.span.end_byte as usize].contains("café")
            );
        }
        assert!(
            facts
                .units
                .iter()
                .filter(|unit| unit.documentation.is_none())
                .all(|unit| !unit.terms.contains_key("zirconium"))
        );
        let mut coverage = graph_search_types::coverage::Coverage::default();
        coverage_from(std::iter::once(&facts), &mut coverage);
        assert_eq!(coverage.source_documentation_truncated_files, 1);
        assert_eq!(coverage.source_unit_truncated_files, 0);
        for case in 0..6 {
            let mut invalid = facts.clone();
            let unit = &mut invalid.units[0];
            match case {
                0 => unit.documentation = None,
                1 => unit.kind = SourceUnitKind::Code,
                2 => {
                    unit.documentation.as_mut().unwrap().documented_symbol =
                        Some(NodeId::new("foreign"));
                }
                3 => unit.documentation.as_mut().unwrap().inner = true,
                4 => unit.documentation.as_mut().unwrap().span.end_byte = u32::MAX,
                _ => unit.documentation.as_mut().unwrap().documented_symbol = None,
            }
            assert!(validate(&file, &invalid, lookup).is_err(), "case {case}");
        }
    }

    #[test]
    fn block_metadata_is_bounded_validated_and_legacy_fields_remain_optional() {
        let text = format!(
            "# Guide\n\nparagraph\n\n1. item\n{}\n> unsupported quote\n",
            "   continuation\n".repeat(160)
        );
        let facts = extract("guide.md", &text, "hash", Language::Unknown, &[]);
        let file = Node {
            kind: NodeKind::File,
            path: "guide.md".into(),
            content_hash: Some("hash".into()),
            bytes: Some(text.len() as u64),
            ..Node::default()
        };
        assert!(validate(&file, &facts, |_| None).is_ok());
        let items: Vec<_> = facts
            .units
            .iter()
            .filter(|unit| unit.kind == SourceUnitKind::MarkdownListItem)
            .collect();
        assert_eq!(items.len(), 3);
        let descriptor = items[0].block.unwrap();
        for unit in items {
            assert_eq!(unit.block, Some(descriptor));
            assert!(unit.span.end_line - unit.span.start_line < 80);
        }
        let marker = descriptor.marker_span.unwrap();
        assert_eq!(
            &text[marker.start_byte as usize..marker.end_byte as usize],
            "1."
        );
        for case in 0..7 {
            let mut invalid = facts.clone();
            let unit = invalid
                .units
                .iter_mut()
                .find(|unit| unit.kind == SourceUnitKind::MarkdownListItem)
                .unwrap();
            match case {
                0 => unit.block = None,
                1 => unit.block.as_mut().unwrap().marker_span = None,
                2 => unit.block.as_mut().unwrap().span.end_byte = u32::MAX,
                3 => unit.block.as_mut().unwrap().span.start_line = 0,
                4 => {
                    unit.block
                        .as_mut()
                        .unwrap()
                        .marker_span
                        .as_mut()
                        .unwrap()
                        .start_byte = 0;
                }
                5 => {
                    unit.block
                        .as_mut()
                        .unwrap()
                        .marker_span
                        .as_mut()
                        .unwrap()
                        .end_line += 1;
                }
                _ => unit.kind = SourceUnitKind::Code,
            }
            assert!(validate(&file, &invalid, |_| None).is_err(), "case {case}");
        }
        let mut legacy = serde_json::to_value(&facts).unwrap();
        legacy["version"] = 5.into();
        for unit in legacy["units"].as_array_mut().unwrap() {
            unit["kind"] = "markdown".into();
            unit.as_object_mut().unwrap().remove("block");
        }
        let decoded: SourceFileUnits = serde_json::from_value(legacy).unwrap();
        assert!(validate(&file, &decoded, |_| None).is_ok());
        assert!(decoded.units.iter().all(|unit| unit.block.is_none()));
    }

    #[test]
    fn opaque_html_keeps_searchable_bytes_without_fabricating_ancestry() {
        let text = format!(
            "# Parent\r\n<!--\r\n{}-->\r\n## Child\r\nbody\r\n",
            "# concealed\r\n\r\n".repeat(50)
        );
        let facts = extract("guide.md", &text, "hash", Language::Unknown, &[]);
        assert!(facts.units.len() > 2);
        assert!(
            facts
                .units
                .iter()
                .any(|unit| unit.terms.contains_key("concealed"))
        );
        for unit in &facts.units {
            let titles: Vec<_> = unit
                .headings
                .iter()
                .map(|heading| {
                    &text[heading.title_span.start_byte as usize
                        ..heading.title_span.end_byte as usize]
                })
                .collect();
            assert!(titles == ["Parent"] || titles == ["Parent", "Child"]);
            assert!(unit.fence.is_none());
        }
        assert_eq!(facts.units.last().unwrap().headings.len(), 2);
        let file = Node {
            kind: NodeKind::File,
            path: "guide.md".into(),
            content_hash: Some("hash".into()),
            bytes: Some(text.len() as u64),
            ..Node::default()
        };
        assert!(validate(&file, &facts, |_| None).is_ok());
    }

    #[test]
    fn fence_metadata_preserves_fields_and_rejects_invalid_coordinates() {
        for (text, label, closed, content) in [
            (
                "```rust title=\"café\"\r\nα();\r\n```\r\n",
                Some("rust"),
                true,
                "α();\r\n",
            ),
            ("~~~ text\n~~~\n", Some("text"), true, ""),
            ("```\n", None, false, ""),
            ("```", None, false, ""),
            ("```js\ncode\n~~\n", Some("js"), false, "code\n~~\n"),
            (
                "~~~ \u{000b}rust more\nbody",
                Some("\u{000b}rust"),
                false,
                "body",
            ),
            ("~~~ \u{000c}rust more\nbody", Some("rust"), false, "body"),
        ] {
            let facts = extract("guide.md", text, "hash", Language::Unknown, &[]);
            let fence = facts.units[0].fence.unwrap();
            assert_eq!(fence.closed, closed);
            assert_eq!(
                fence
                    .language_span
                    .map(|span| &text[span.start_byte as usize..span.end_byte as usize]),
                label
            );
            assert_eq!(
                &text[fence.content_span.start_byte as usize..fence.content_span.end_byte as usize],
                content
            );
            let file = Node {
                kind: NodeKind::File,
                path: "guide.md".into(),
                content_hash: Some("hash".into()),
                bytes: Some(text.len() as u64),
                ..Node::default()
            };
            assert!(validate(&file, &facts, |_| None).is_ok(), "{text:?}");
            for case in 0..7 {
                let mut invalid = facts.clone();
                let metadata = invalid.units[0].fence.as_mut().unwrap();
                match case {
                    0 => metadata.span.end_byte = u32::MAX,
                    1 => metadata.content_span.start_byte = u32::MAX,
                    2 => metadata.info_span.start_line = 0,
                    3 => metadata.language_span = Some(metadata.content_span),
                    4 => metadata.closed = !metadata.closed,
                    5 => invalid.units[0].kind = SourceUnitKind::Text,
                    _ => invalid.units[0].fence = None,
                }
                assert!(validate(&file, &invalid, |_| None).is_err(), "case {case}");
            }
            let mut legacy = serde_json::to_value(&facts).unwrap();
            legacy["version"] = 3.into();
            for unit in legacy["units"].as_array_mut().unwrap() {
                unit.as_object_mut().unwrap().remove("fence");
            }
            let legacy: SourceFileUnits = serde_json::from_value(legacy).unwrap();
            assert!(validate(&file, &legacy, |_| None).is_ok());
        }
    }
    #[test]
    fn heading_references_are_bounded_validated_and_backward_readable() {
        let text = "# Parent\n## Child\nbody\n";
        let facts = extract("guide.md", text, "hash", Language::Unknown, &[]);
        let file = Node {
            kind: NodeKind::File,
            path: "guide.md".into(),
            content_hash: Some("hash".into()),
            bytes: Some(text.len() as u64),
            ..Node::default()
        };
        assert!(validate(&file, &facts, |_| None).is_ok());
        for case in 0..7 {
            let mut invalid = facts.clone();
            let unit = invalid.units.last_mut().unwrap();
            match case {
                0 => unit.headings[1].level = 1,
                1 => unit.headings[1].level = 7,
                2 => unit.headings[1].span.end_byte = u32::MAX,
                3 => unit.headings[1].title_span.start_byte = 0,
                4 => unit.headings[1].span.start_byte = unit.span.end_byte,
                5 => unit.kind = SourceUnitKind::Text,
                _ => unit.headings[1].title_span.start_line = 0,
            }
            assert!(validate(&file, &invalid, |_| None).is_err(), "case {case}");
        }
        let mut old = serde_json::to_value(&facts).unwrap();
        old["version"] = 2.into();
        for unit in old["units"].as_array_mut().unwrap() {
            unit.as_object_mut().unwrap().remove("headings");
        }
        let decoded: SourceFileUnits = serde_json::from_value(old).unwrap();
        assert!(decoded.units.iter().all(|unit| unit.headings.is_empty()));
        assert!(validate(&file, &decoded, |_| None).is_ok());
    }
    #[test]
    fn windows_cover_original_unicode_crlf_bytes_and_index_all_terms() {
        let text = "élève cache\r\n".repeat(170);
        let units = extract("data.txt", &text, "hash", Language::Unknown, &[]);
        assert!(!units.truncated);
        assert_eq!(units.units.len(), 3);
        assert_eq!(units.units[0].span.start_byte, 0);
        assert_eq!(
            units.units.last().unwrap().span.end_byte as usize,
            text.len()
        );
        for unit in units.units {
            assert!(unit.span.end_line.saturating_sub(unit.span.start_line) < 80);
            assert!(
                text.get(unit.span.start_byte as usize..unit.span.end_byte as usize)
                    .is_some()
            );
            assert!(unit.terms.contains_key("élève"));
            assert!(unit.terms.contains_key("cache"));
        }
    }
    #[test]
    fn nested_declarations_partition_without_losing_source_lines() {
        let text = "before\nouter\ninner\nouter\nafter\n";
        let outer = Node {
            id: NodeId::new("outer"),
            kind: NodeKind::Function,
            span: Some(Span::new(2, 4, 7, 25)),
            ..Node::default()
        };
        let inner = Node {
            id: NodeId::new("inner"),
            kind: NodeKind::Function,
            span: Some(Span::new(3, 3, 13, 19)),
            ..Node::default()
        };
        let units = extract("a.rs", text, "hash", Language::Rust, &[outer, inner]);
        assert_eq!(
            units
                .units
                .iter()
                .map(|u| u.owner.as_ref().map(NodeId::as_str))
                .collect::<Vec<_>>(),
            [None, Some("outer"), Some("inner"), Some("outer"), None]
        );
        assert_eq!(
            units
                .units
                .iter()
                .map(|u| u.span.start_line)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
    }
    #[test]
    fn same_line_declarations_keep_exact_byte_ownership() {
        let text = "fn a(){} fn b(){}";
        let a = Node {
            id: NodeId::new("a"),
            span: Some(Span::new(1, 1, 0, 8)),
            ..Node::default()
        };
        let b = Node {
            id: NodeId::new("b"),
            span: Some(Span::new(1, 1, 9, 17)),
            ..Node::default()
        };
        let facts = extract("a.rs", text, "hash", Language::Rust, &[a, b]);
        assert_eq!(facts.units.len(), 3);
        assert_eq!(facts.units[0].owner.as_ref().map(NodeId::as_str), Some("a"));
        assert_eq!(facts.units[1].owner, None);
        assert_eq!(facts.units[2].owner.as_ref().map(NodeId::as_str), Some("b"));
        assert_eq!(
            &text[facts.units[2].span.start_byte as usize..facts.units[2].span.end_byte as usize],
            "fn b(){}"
        );
    }

    #[test]
    fn region_overflow_is_reported_without_partial_unit_bytes() {
        let text =
            "x".repeat(graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE.saturating_add(1));
        let symbols: Vec<_> = (0..text.len())
            .map(|i| Node {
                id: NodeId::new(format!("n{i}")),
                span: Some(Span::new(
                    1,
                    1,
                    u32::try_from(i).unwrap(),
                    u32::try_from(i.saturating_add(1)).unwrap(),
                )),
                ..Node::default()
            })
            .collect();
        let source = extract("a.rs", &text, "hash", Language::Rust, &symbols);
        assert!(source.truncated);
        assert_eq!(
            source.units.len(),
            graph_search_types::limits::MAX_SOURCE_UNITS_PER_FILE
        );
        let mut observed = graph_search_types::coverage::Coverage::default();
        coverage(&BTreeMap::from([("a.rs".into(), source)]), &mut observed);
        assert_eq!(observed.source_unit_truncated_files, 1);
        assert_eq!(observed.truncations.len(), 1);
    }
}
