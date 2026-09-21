//! Rendering: the text output for people, the JSON envelope for agents
//! (`SPEC.md` §9, §10).
//!
//! The JSON payload is the contract; the text rendering is not. The
//! `files`/`text` renderings reproduce `nanus`'s strings verbatim so the
//! evaluation compares like with like (`SPEC.md` §8.1, §8.2).

use clap::ValueEnum;
use graph_search::Index;
use graph_search_types::result::{
    EdgeHit, ExploreResult, GraphResult, ImpactResult, IndexStatus, Stats, SymbolHit, SyncReport,
    TextHit, TextResult, Truncation, TruncationKind,
};
use graph_search_types::{Envelope, RESULT_SCHEMA_VERSION};
use serde::Serialize;
use std::fmt::Write as _;
use std::io::Write as _;

/// The output format.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// Human-readable text (not a contract).
    Text,
    /// One JSON document on stdout (the contract).
    Json,
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text => f.write_str("text"),
            Self::Json => f.write_str("json"),
        }
    }
}

/// Writes the rendered bytes to stdout.
fn emit(bytes: &str) -> graph_search::Result<()> {
    let mut out = std::io::stdout();
    out.write_all(bytes.as_bytes())
        .and_then(|()| out.flush())
        .map_err(|source| {
            graph_search::Error::Core(graph_search::core::Error::io(
                std::path::Path::new("<stdout>"),
                source,
            ))
        })
}

/// The truncation notices, verbatim.
fn truncations_text(truncations: &[Truncation]) -> String {
    let mut out = String::new();
    for truncation in truncations {
        writeln!(out, "{}", truncation.message).ok();
    }
    out
}

/// The approximation note, for graph and explore answers.
fn approximation_text(resolved: u64, unresolved: u64) -> String {
    format!(
        "(static approximation: {resolved} resolved, {unresolved} unresolved edges; dynamic/macro/generated edges may be missing)\n"
    )
}

/// Fills the shared envelope fields every answer carries.
fn finish<R: Serialize>(
    mut envelope: Envelope<R>,
    edges: Vec<EdgeHit>,
    approximation: Option<graph_search_types::result::Approximation>,
    truncations: Vec<Truncation>,
    stats: Stats,
) -> Envelope<R> {
    envelope.edges = edges;
    envelope.approximation = approximation;
    envelope.truncations = truncations;
    envelope.stats = stats;
    envelope
}

fn attach_context<R: Serialize>(
    envelope: &mut Envelope<R>,
    context: &graph_search_types::context::ResultContext,
) {
    envelope.stale = context.staleness.changed > 0;
    envelope.stale_paths = envelope
        .stale
        .then(|| context.staleness.changed_paths.clone());
    envelope.context = Some(context.clone());
}

fn context_text(context: &graph_search_types::context::ResultContext) -> String {
    use graph_search_types::context::SourceVerification;
    let mut text = String::new();
    if context.coverage.enumeration_complete == Some(false) {
        writeln!(
            text,
            "(source enumeration incomplete: {} unreadable entries, {} exhausted limits)",
            context.coverage.unreadable_entries,
            context.coverage.truncations.len()
        )
        .ok();
    }
    let coverage = &context.coverage;
    if coverage.source_read_errors > 0
        || coverage.binary_files > 0
        || coverage.invalid_utf8_files > 0
        || coverage.oversized_files > 0
        || coverage.quarantined_files > 0
    {
        writeln!(text, "(coverage: {} oversized, {} binary, {} invalid UTF-8, {} source read failures, {} parser quarantines)", coverage.oversized_files, coverage.binary_files, coverage.invalid_utf8_files, coverage.source_read_errors, coverage.quarantined_files).ok();
    }
    if coverage.source_budget_exceeded_files > 0 {
        writeln!(
            text,
            "(source read budgets withheld {} files)",
            coverage.source_budget_exceeded_files
        )
        .ok();
        for truncation in &coverage.truncations {
            if matches!(
                truncation.kind,
                graph_search_types::TruncationKind::SourceBytes
                    | graph_search_types::TruncationKind::SourceFileBytes
            ) {
                writeln!(text, "{}", truncation.message).ok();
            }
        }
    }
    if coverage.source_unit_truncated_files > 0 {
        writeln!(
            text,
            "(source-region indexing incomplete for {} files)",
            coverage.source_unit_truncated_files
        )
        .ok();
    }
    if context.staleness.changed > 0 {
        writeln!(
            text,
            "(indexed generation differs from {} observed source paths)",
            context.staleness.changed
        )
        .ok();
    }
    for (path, source) in &context.sources {
        let reason = match source.verification {
            SourceVerification::Mismatch => {
                "source changed since indexing; indexed snippets withheld"
            }
            SourceVerification::Unavailable => "source unavailable",
            SourceVerification::Binary => "binary source; snippet unavailable",
            SourceVerification::InvalidEncoding => "invalid UTF-8 source; snippet unavailable",
            SourceVerification::BudgetExceeded => "source read budget exceeded",
            _ => continue,
        };
        writeln!(text, "({path}: {reason})").ok();
    }
    text
}

// ---------------------------------------------------------------------------
// files
// ---------------------------------------------------------------------------

/// Renders a `files` answer.
///
/// # Errors
/// Propagates write failures.
pub(crate) fn files(
    command: &str,
    index: &Index,
    query: &graph_search_types::FilesQuery,
    result: &graph_search_types::FilesResult,
    format: Format,
) -> graph_search::Result<()> {
    match format {
        Format::Json => {
            let envelope: Envelope<Vec<graph_search_types::result::FileHit>> = Envelope::new(
                RESULT_SCHEMA_VERSION,
                command,
                index.root().display().to_string(),
                serde_json::json!({
                    "pattern": query.pattern,
                    "path": query.path,
                    "limit": query.limit,
                }),
                result.items.clone(),
            );
            let mut envelope = finish(
                envelope,
                Vec::new(),
                None,
                result.truncations.clone(),
                result.stats,
            );
            attach_context(&mut envelope, &result.context);
            emit(&crate::budget::scan(&mut envelope, |hit| {
                hit.path.as_str()
            })?)?;
        }
        Format::Text => {
            let paths: Vec<String> = result.items.iter().map(|hit| hit.path.clone()).collect();
            let mut text = if paths.is_empty() {
                String::from("No files found.\n")
            } else {
                let mut text = paths.join("\n");
                text.push('\n');
                text
            };
            text.push_str(&context_text(&result.context));
            text.push_str(&truncations_text(&result.truncations));
            emit(&text)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// text
// ---------------------------------------------------------------------------

/// Renders a `text` answer with `nanus`'s exact strings.
///
/// # Errors
/// Propagates write failures.
pub(crate) fn text(
    command: &str,
    index: &Index,
    query: &graph_search_types::TextQuery,
    result: &TextResult,
    format: Format,
) -> graph_search::Result<()> {
    match format {
        Format::Json => {
            let envelope: Envelope<Vec<TextHit>> = Envelope::new(
                RESULT_SCHEMA_VERSION,
                command,
                index.root().display().to_string(),
                serde_json::json!({
                    "pattern": query.pattern,
                    "path": query.path,
                    "include": query.include,
                    "ignore_case": query.ignore_case,
                    "limit": query.limit,
                }),
                result.items.clone(),
            );
            let mut envelope = finish(
                envelope,
                Vec::new(),
                None,
                result.truncations.clone(),
                result.stats,
            );
            attach_context(&mut envelope, &result.context);
            emit(&crate::budget::scan(&mut envelope, |hit| {
                hit.path.as_str()
            })?)?;
        }
        Format::Text => {
            let mut text =
                render_text_matches(&result.items, result.truncations.as_slice(), query.limit);
            text.push_str(&context_text(&result.context));
            emit(&text)?;
        }
    }
    Ok(())
}

/// The match rendering, grouped by file with a cap notice — the same shape
/// `nanus` `grep` prints.
#[must_use]
pub(crate) fn render_text_matches(
    hits: &[TextHit],
    truncations: &[Truncation],
    _limit: u32,
) -> String {
    let truncated = !truncations.is_empty();
    if hits.is_empty() {
        return if truncated {
            format!(
                "No matches in the files reached; evidence is incomplete.\n{}",
                truncations_text(truncations)
            )
        } else {
            String::from("No matches found.\n")
        };
    }
    let mut rendered = String::new();
    let mut current: Option<&str> = None;
    for hit in hits {
        if current != Some(hit.path.as_str()) {
            if current.is_some() {
                rendered.push('\n');
            }
            rendered.push_str(&hit.path);
            rendered.push_str(":\n");
            current = Some(&hit.path);
        }
        writeln!(rendered, "  {}: {}", hit.line, hit.text).ok();
    }
    if truncated {
        rendered.push_str(&truncations_text(truncations));
    }
    rendered
}

// ---------------------------------------------------------------------------
// graph modes
// ---------------------------------------------------------------------------

/// Renders a `graph` answer.
///
/// # Errors
/// Propagates write failures.
pub(crate) fn graph(
    command: &str,
    index: &Index,
    query: &serde_json::Value,
    result: &GraphResult,
    format: Format,
) -> graph_search::Result<()> {
    match format {
        Format::Json => {
            let envelope: Envelope<Vec<SymbolHit>> = Envelope::new(
                RESULT_SCHEMA_VERSION,
                command,
                index.root().display().to_string(),
                query.clone(),
                result.nodes.clone(),
            );
            let mut envelope = finish(
                envelope,
                result.edges.clone(),
                result.approximation.clone(),
                result.truncations.clone(),
                result.stats,
            );
            attach_context(&mut envelope, &result.context);
            emit(&crate::budget::graph(&mut envelope)?)?;
        }
        Format::Text => {
            let mut text = String::new();
            if result.nodes.is_empty() && result.edges.is_empty() {
                text.push_str("Nothing found.\n");
            }
            for node in &result.nodes {
                let signature = node.signature.as_deref().unwrap_or_default();
                if node.kind == graph_search_types::kind::NodeKind::File {
                    writeln!(text, "{} [file]", node.path).ok();
                } else {
                    writeln!(
                        text,
                        "{}:{} {} {} — {signature}",
                        node.path, node.start_line, node.kind, node.qualified_name
                    )
                    .ok();
                }
            }
            for edge in &result.edges {
                let marker = if edge.resolved { "" } else { " (unresolved)" };
                writeln!(
                    text,
                    "{} -[{}]-> {} ({}:{}){marker}",
                    edge.from,
                    edge.kind,
                    edge.to_name,
                    edge.path.as_deref().unwrap_or("-"),
                    edge.line.map_or_else(|| "-".to_owned(), |l| l.to_string()),
                )
                .ok();
            }
            if let Some(approximation) = &result.approximation {
                text.push_str(&approximation_text(
                    approximation.resolved,
                    approximation.unresolved,
                ));
            }
            text.push_str(&context_text(&result.context));
            text.push_str(&truncations_text(&result.truncations));
            emit(&text)?;
        }
    }
    Ok(())
}

/// Renders an `impact` answer.
///
/// # Errors
/// Propagates write failures.
pub(crate) fn impact(
    command: &str,
    index: &Index,
    query: &serde_json::Value,
    result: &ImpactResult,
    format: Format,
) -> graph_search::Result<()> {
    match format {
        Format::Json => {
            let payload = crate::budget::ImpactPayload {
                by_depth: result.by_depth.clone(),
                top: result.top.clone(),
            };
            let envelope = Envelope::new(
                RESULT_SCHEMA_VERSION,
                command,
                index.root().display().to_string(),
                query.clone(),
                payload,
            );
            let mut envelope = finish(
                envelope,
                result.edges.clone(),
                result.approximation.clone(),
                result.truncations.clone(),
                result.stats,
            );
            attach_context(&mut envelope, &result.context);
            emit(&crate::budget::impact(&mut envelope)?)?;
        }
        Format::Text => {
            let mut text = String::new();
            if result.by_depth.is_empty() {
                text.push_str("No impact found.\n");
            }
            for ring in &result.by_depth {
                let kinds = ring
                    .by_kind
                    .iter()
                    .map(|(kind, count)| format!("{count} {kind}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                writeln!(text, "depth {}: {} ({kinds})\n", ring.depth, ring.total).ok();
            }
            for node in &result.top {
                writeln!(
                    text,
                    "{}:{} {} {}",
                    node.path, node.start_line, node.kind, node.qualified_name
                )
                .ok();
            }
            text.push_str(&context_text(&result.context));
            text.push_str(&truncations_text(&result.truncations));
            if let Some(approximation) = &result.approximation {
                text.push_str(&approximation_text(
                    approximation.resolved,
                    approximation.unresolved,
                ));
            }
            emit(&text)?;
        }
    }
    Ok(())
}

fn explore_source_text(text: &mut String, item: &graph_search_types::ExploreItem) {
    if let Some(retrieval) = &item.retrieval {
        writeln!(
            text,
            " retrieval: {}",
            serde_json::to_string(retrieval).unwrap_or_default()
        )
        .ok();
    }
    if let Some(snippet) = &item.snippet {
        for (offset, line) in snippet.lines.iter().enumerate() {
            let line_no = snippet
                .start_line
                .saturating_add(u32::try_from(offset).unwrap_or(0));
            writeln!(text, " {line_no}: {line}\n").ok();
        }
    }
    for excerpt in &item.excerpts {
        writeln!(text, " {:?} context:", excerpt.role).ok();
        for (offset, line) in excerpt.snippet.lines.iter().enumerate() {
            let line_no = excerpt
                .snippet
                .start_line
                .saturating_add(u32::try_from(offset).unwrap_or(0));
            writeln!(text, " {line_no}: {line}").ok();
        }
    }
}

/// Renders an `explore` answer.
///
/// # Errors
/// Propagates write failures.
pub(crate) fn explore(
    command: &str,
    index: &Index,
    query: &graph_search_types::ExploreQuery,
    result: &ExploreResult,
    format: Format,
) -> graph_search::Result<()> {
    match format {
        Format::Json => {
            let envelope: Envelope<Vec<graph_search_types::result::ExploreItem>> = Envelope::new(
                RESULT_SCHEMA_VERSION,
                command,
                index.root().display().to_string(),
                serde_json::json!({
                    "query": query.query,
                    "k": query.k,
                    "hops": query.hops,
                    "context_lines": query.context_lines,
                    "filters": query.filters,
                    "max_bytes": query.max_bytes,
                }),
                result.items.clone(),
            );
            let mut envelope = finish(
                envelope,
                result.edges.clone(),
                result.approximation.clone(),
                result.truncations.clone(),
                result.stats,
            );
            envelope.plan.clone_from(&result.plan);
            if query.retrieval != graph_search_types::RetrievalOptions::default() {
                envelope.query["retrieval"] =
                    serde_json::to_value(&query.retrieval).unwrap_or_default();
            }
            attach_context(&mut envelope, &result.context);
            emit_explore_envelope(&mut envelope, query.max_bytes)?;
        }
        Format::Text => {
            let mut text = String::new();
            if let Some(plan) = &result.plan {
                writeln!(
                    text,
                    "plan: {}",
                    serde_json::to_string(plan).unwrap_or_default()
                )
                .ok();
            }
            if result.items.is_empty() {
                text.push_str("Nothing found.\n");
            }
            for (position, item) in result.items.iter().enumerate() {
                if position > 0 {
                    text.push('\n');
                }
                let node = &item.node;
                writeln!(
                    text,
                    "{}:{} {} {}",
                    node.path, node.start_line, node.kind, node.qualified_name
                )
                .ok();
                if let Some(signature) = &node.signature {
                    writeln!(text, " {signature}").ok();
                }
                explore_source_text(&mut text, item);
                if let Some(impact) = &item.impact {
                    writeln!(
                        text,
                        " impact: {} direct callers, {} within depth\n",
                        impact.direct_callers, impact.total_callers
                    )
                    .ok();
                }
            }
            for edge in &result.edges {
                writeln!(text, "{} -[{}]-> {}\n", edge.from, edge.kind, edge.to_name).ok();
            }
            if let Some(approximation) = &result.approximation {
                text.push_str(&approximation_text(
                    approximation.resolved,
                    approximation.unresolved,
                ));
            }
            text.push_str(&context_text(&result.context));
            text.push_str(&truncations_text(&result.truncations));
            emit(&text)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// status and sync
// ---------------------------------------------------------------------------

/// Renders a `status` answer.
///
/// # Errors
/// Propagates write failures.
pub(crate) fn status(status: &IndexStatus, format: Format) -> graph_search::Result<()> {
    match format {
        Format::Json => {
            let mut envelope: Envelope<IndexStatus> = Envelope::new(
                RESULT_SCHEMA_VERSION,
                "status",
                status.root.clone(),
                serde_json::json!({}),
                status.clone(),
            );
            emit(&crate::budget::status(&mut envelope)?)?;
        }
        Format::Text => {
            let mut text = String::new();
            if status.exists {
                writeln!(text, "index: {}\n", status.store_path).ok();
                writeln!(
                    text,
                    "schema v{}, parser v{}\n",
                    status.schema_version, status.parser_version
                )
                .ok();
                if let Some(counts) = &status.counts {
                    writeln!(
                        text,
                        "{} nodes, {} edges\n",
                        counts.total_nodes, counts.total_edges
                    )
                    .ok();
                    for (kind, count) in &counts.files_by_language {
                        writeln!(text, " {kind}: {count} files\n").ok();
                    }
                }
                if let Some(at) = status.indexed_at_ms {
                    writeln!(text, "last indexed: {at} ms since epoch\n").ok();
                }
                if let Some(staleness) = &status.staleness {
                    if staleness.changed == 0 {
                        text.push_str("stale: no\n");
                    } else {
                        writeln!(text, "stale: {} changed paths\n", staleness.changed).ok();
                        for path in staleness.changed_paths.iter().take(10) {
                            writeln!(text, " {path}\n").ok();
                        }
                    }
                }
            } else {
                text.push_str("no index (run `graph-search index`)\n");
            }
            text.push_str(&truncations_text(&status.coverage.truncations));
            emit(&text)?;
        }
    }
    Ok(())
}

/// Renders a `sync`/`index` report.
///
/// # Errors
/// Propagates write failures.
pub(crate) fn sync(
    report: &SyncReport,
    format: Format,
    root: Option<&std::path::Path>,
) -> graph_search::Result<()> {
    let _ = root;
    match format {
        Format::Json => {
            let mut envelope: Envelope<SyncReport> = Envelope::new(
                RESULT_SCHEMA_VERSION,
                "sync",
                String::new(),
                serde_json::json!({}),
                report.clone(),
            );
            emit(&crate::budget::sync(&mut envelope)?)?;
        }
        Format::Text => {
            let mut text = String::new();
            if report.reindexed_all {
                text.push_str("re-indexed everything (parser or schema changed)\n");
            }
            for path in &report.added {
                writeln!(text, "added {path}\n").ok();
            }
            for path in &report.modified {
                writeln!(text, "modified {path}\n").ok();
            }
            for rename in &report.renamed {
                writeln!(text, "renamed {} -> {}\n", rename.from, rename.to).ok();
            }
            for path in &report.removed {
                writeln!(text, "removed {path}\n").ok();
            }
            for record in &report.quarantined {
                writeln!(text, "quarantined {}: {}\n", record.path, record.reason).ok();
            }
            let totals = report.totals();
            if text.is_empty()
                && totals.added == 0
                && totals.modified == 0
                && totals.removed == 0
                && totals.renamed == 0
                && totals.quarantined == 0
            {
                text.push_str("up to date\n");
            }
            if report
                .coverage
                .truncations
                .iter()
                .any(|t| t.kind == TruncationKind::Bytes)
            {
                writeln!(
                    text,
                    "{} added, {} modified, {} removed, {} renamed, {} quarantined",
                    totals.added,
                    totals.modified,
                    totals.removed,
                    totals.renamed,
                    totals.quarantined
                )
                .ok();
            }
            text.push_str(&truncations_text(&report.coverage.truncations));
            writeln!(
                text,
                "{} unchanged, {} ms\n",
                report.unchanged, report.elapsed_ms
            )
            .ok();
            emit(&text)?;
        }
    }
    Ok(())
}

/// Renders the staleness notice for `--fail-if-stale --no-reconcile`.
///
/// # Errors
/// Propagates write failures.
pub(crate) fn stale_notice(status: &IndexStatus, format: Format) -> graph_search::Result<()> {
    let staleness = status.staleness.clone().unwrap_or_default();
    let paths = &staleness.changed_paths;
    match format {
        Format::Json => {
            let envelope: Envelope<Vec<String>> = Envelope::new(
                RESULT_SCHEMA_VERSION,
                "search.stale",
                String::new(),
                serde_json::json!({}),
                paths.clone(),
            );
            let mut envelope = envelope;
            envelope.stale = true;
            envelope.stale_paths = Some(paths.clone());
            envelope.context = Some(graph_search_types::context::ResultContext {
                generation: status.generation.clone(),
                staleness: staleness.clone(),
                coverage: status.coverage.clone(),
                ..Default::default()
            });
            envelope
                .truncations
                .clone_from(&status.coverage.truncations);
            emit(&crate::budget::stale(&mut envelope)?)?;
        }
        Format::Text => {
            let mut text = format!("the index is stale: {} changed paths\n", staleness.changed);
            for path in paths.iter().take(10) {
                writeln!(text, " {path}\n").ok();
            }
            text.push_str(&truncations_text(&status.coverage.truncations));
            emit(&text)?;
        }
    }
    Ok(())
}

fn emit_explore_envelope(
    envelope: &mut Envelope<Vec<graph_search_types::result::ExploreItem>>,
    max_bytes: u32,
) -> graph_search::Result<()> {
    let cap = if max_bytes == 0 {
        graph_search_types::limits::MAX_TOTAL_BYTES
    } else {
        (max_bytes as usize).min(graph_search_types::limits::MAX_TOTAL_BYTES)
    };
    loop {
        if let Some(context) = &mut envelope.context {
            context
                .sources
                .retain(|path, _| envelope.results.iter().any(|item| &item.node.path == path));
            context.retain_packages(
                envelope
                    .results
                    .iter()
                    .filter_map(|item| item.evidence.as_ref()?.package_ref.as_deref()),
            );
        }
        let resolved = envelope.edges.iter().filter(|e| e.resolved).count() as u64;
        if let Some(approximation) = &mut envelope.approximation {
            approximation.resolved = resolved;
            approximation.unresolved = (envelope.edges.len() as u64).saturating_sub(resolved);
        }
        let json = serde_json::to_string(&envelope).unwrap_or_default();
        if json.len() <= cap {
            return emit(&json);
        }
        if !envelope.truncations.iter().any(|t| {
            t.kind == graph_search_types::result::TruncationKind::Bytes && t.cap == cap as u64
        }) {
            envelope.truncations.push(Truncation::new(
                graph_search_types::result::TruncationKind::Bytes,
                cap as u64,
                "JSON envelope exceeded its byte cap",
            ));
        }
        if let Some(item) = envelope
            .results
            .iter_mut()
            .rev()
            .find(|item| !item.excerpts.is_empty())
        {
            item.excerpts.pop();
            continue;
        }
        if envelope.edges.pop().is_some() {
            continue;
        }
        if let Some(item) = envelope
            .results
            .iter_mut()
            .rev()
            .find(|item| item.snippet.is_some())
        {
            item.snippet = None;
            continue;
        }
        if envelope.results.pop().is_none() {
            return Err(graph_search::Error::Core(
                graph_search::core::Error::ResultBudget(cap),
            ));
        }
    }
}

pub(crate) fn occurrences(
    index: &Index,
    query: &serde_json::Value,
    result: &graph_search_types::occurrence::OccurrenceResult,
    format: Format,
) -> graph_search::Result<()> {
    match format {
        Format::Json => {
            let payload = crate::budget::OccurrencePayload {
                items: result.items.clone(),
                indexed_files: result.indexed_files,
                extracted_files: result.extracted_files,
            };
            let mut envelope = Envelope::new(
                RESULT_SCHEMA_VERSION,
                "search.graph.occurrences",
                index.root().display().to_string(),
                query.clone(),
                payload,
            );
            envelope.stats = result.stats;
            envelope.truncations.clone_from(&result.truncations);
            attach_context(&mut envelope, &result.context);
            emit(&crate::budget::occurrences(&mut envelope)?)?;
        }
        Format::Text => {
            let mut text = String::new();
            for item in &result.items {
                let reference = &item.occurrence;
                writeln!(
                    text,
                    "{}:{} {} -[{}]-> {} [{:?}] {}",
                    item.path,
                    reference.line,
                    reference.owner,
                    reference.kind,
                    reference.target_name,
                    reference.resolution,
                    reference.reason.as_deref().unwrap_or_default()
                )
                .ok();
            }
            if result.items.is_empty() {
                text.push_str("Nothing found.\n");
            }
            writeln!(
                text,
                "Reference extraction succeeded for {} of {} files with occurrence metadata.",
                result.extracted_files, result.indexed_files
            )
            .ok();
            text.push_str(&context_text(&result.context));
            text.push_str(&truncations_text(&result.truncations));
            emit(&text)?;
        }
    }
    Ok(())
}
