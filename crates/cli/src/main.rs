//! The `graph-search` binary: a thin client over the library
//! (`SPEC.md` §10).
//!
//! Parses arguments, opens the library, renders. It wires nothing itself:
//! the engine and the parser are the library's choices. Exit codes follow
//! `SPEC.md` §10.2: 0 success (an empty result is a success), 1 operational
//! failure, 2 usage error, 3 `--fail-if-stale` on a stale index, 4
//! `--no-reconcile` with no usable index.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
// A CLI's whole job is to print, and its argument struct is the command
// surface; the library code keeps the strict settings.
#![allow(
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::struct_excessive_bools
)]

mod budget;
mod render;

use clap::{Parser, Subcommand};
use graph_search::index::Reconcile;
use graph_search::{Error, Index, OpenOptions};
use graph_search_types::query::{
    DepsQuery, ExploreQuery, NeighborsQuery, PathQuery, RefQuery, SymbolQuery, TextQuery,
    TraversalQuery,
};
use graph_search_types::result::IndexStatus;
use graph_search_types::{FilesQuery, SyncReport};
use std::path::PathBuf;
use std::process::ExitCode;

/// One query surface for files, text, and the symbol graph.
#[derive(Debug, Parser)]
#[command(name = "graph-search", version, about, propagate_version = true)]
struct Cli {
    /// The workspace root (default: the current directory).
    #[arg(long, global = true, value_name = "DIR")]
    root: Option<PathBuf>,

    /// The store directory (default: `<root>/.graph-search/index`).
    #[arg(long, global = true, value_name = "DIR")]
    store: Option<PathBuf>,

    /// Output format: text or json.
    #[arg(long, global = true, value_name = "FORMAT")]
    format: Option<render::Format>,

    /// Shorthand for `--format json`.
    #[arg(long, global = true)]
    json: bool,

    /// Ignore `.gitignore` and friends while walking.
    #[arg(long, global = true)]
    no_ignore: bool,

    /// Refuse to reconcile before answering; staleness becomes exit code 3
    /// with `--fail-if-stale`.
    #[arg(long, global = true)]
    no_reconcile: bool,

    /// Verify source contents as well as metadata before graph queries.
    #[arg(long, global = true)]
    verify_content: bool,

    /// Fail with exit code 3 when the index was stale at answer time.
    #[arg(long, global = true)]
    fail_if_stale: bool,

    /// Include hidden files and directories.
    #[arg(long, global = true)]
    hidden: bool,

    /// Suppress diagnostics on stderr.
    #[arg(long, global = true)]
    quiet: bool,

    /// Verbose diagnostics on stderr.
    #[arg(short = 'v', long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create `.graph-search/` (configuration and store directory).
    Init,
    /// Full build: parse everything, build from scratch.
    Index {
        /// Rebuild even when an index exists.
        #[arg(long)]
        force: bool,
    },
    /// Incremental reconcile against the manifest.
    Sync,
    /// Whether an index exists, its counts, and how far behind it is.
    Status,
    /// One search with a mode. Modes, not tools.
    Search {
        #[command(subcommand)]
        mode: SearchMode,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum Normalization {
    Combined,
    Bm25f,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum GraphContextMode {
    Semantic,
    None,
    Calls,
    Imports,
    Types,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum QueryPolicyMode {
    Verbatim,
    Task,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum DetailMode {
    /// Node, primary snippet and impact only.
    Compact,
    /// Adds labelled excerpts and matched-body evidence.
    Full,
}

#[derive(Debug, Subcommand)]
enum SearchMode {
    /// Find files whose path matches a glob, anchored to the search root.
    Files {
        /// The glob pattern; `*` does not cross `/`, `**/` is every depth.
        pattern: String,
        /// Directory to search under, relative to the root.
        #[arg(long, value_name = "DIR")]
        path: Option<String>,
        /// Maximum matches to return (default 100, ceiling 1000).
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Find a literal substring (not a regex) with file and line.
    Text {
        /// The literal text to find.
        pattern: String,
        /// Directory to search under, relative to the root.
        #[arg(long, value_name = "DIR")]
        path: Option<String>,
        /// One positive glob narrowing the files searched.
        #[arg(long, value_name = "GLOB")]
        include: Option<String>,
        /// Maximum matches to return (default 250, ceiling 2000).
        #[arg(long)]
        limit: Option<u32>,
        /// Fold case when matching.
        #[arg(long)]
        ignore_case: bool,
    },
    /// Where is a symbol defined?
    Symbol {
        /// A symbol name, qualified name, or exact id.
        target: String,
        /// Restrict the matched definition's kind.
        #[arg(long)]
        kind: Option<String>,
        /// Restrict to one language.
        #[arg(long)]
        lang: Option<String>,
        /// Restrict by a glob over workspace-relative paths.
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        /// Maximum results (default 50, ceiling 500).
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Every reference to a symbol.
    Refs {
        /// A symbol name, qualified name, or exact id.
        target: String,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Individual source references, including repeated and unresolved sites.
    Occurrences {
        target: String,
        #[arg(long, default_value = "target", value_parser = ["target", "owner", "name"])]
        by: String,
        #[arg(long)]
        rel: Option<String>,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Who calls this symbol?
    Callers {
        /// A symbol name, qualified name, or exact id.
        target: String,
        /// How deep to follow (default 1, ceiling 4).
        #[arg(long, default_value_t = 1)]
        depth: u8,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// What does this symbol call?
    Callees {
        target: String,
        #[arg(long, default_value_t = 1)]
        depth: u8,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// The blast radius: transitive callers and references, counted by depth.
    Impact {
        target: String,
        #[arg(long, default_value_t = 2)]
        depth: u8,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// A file's imports and imported-by.
    Deps {
        /// A file path or id.
        target: String,
        /// Which direction to report.
        #[arg(long, value_name = "in|out|both", default_value = "both")]
        direction: String,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Nodes adjacent along chosen edge kinds.
    Neighbors {
        /// A node id, or a name resolved like `symbol`.
        target: String,
        /// Restrict to one edge kind.
        #[arg(long)]
        rel: Option<String>,
        /// How many hops (default 1, ceiling 4).
        #[arg(long)]
        hops: Option<u8>,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        #[arg(long)]
        limit: Option<u32>,
    },
    /// The shortest path between two nodes.
    Path {
        /// The start: a name or exact id.
        from: String,
        /// The goal: a name or exact id.
        to: String,
        /// The deepest path accepted (ceiling 4).
        #[arg(long)]
        max_hops: Option<u8>,
    },
    /// One-call context retrieval: seeds, snippets, connections, impact.
    Explore {
        /// Free-text terms.
        query: String,
        /// How many seeds to assemble (default 8).
        #[arg(long)]
        k: Option<u32>,
        /// How many hops to connect over (default 1).
        #[arg(long)]
        hops: Option<u8>,
        /// Primary match context radius (default 2, ceiling 10); 0 disables excerpts.
        #[arg(long)]
        context_lines: Option<u32>,
        /// Per-seed source evidence: `compact` (node, snippet, impact) or `full`
        /// (adds labelled excerpts and matched-body evidence).
        #[arg(long, value_enum, default_value_t = DetailMode::Compact)]
        detail: DetailMode,
        /// The whole-payload byte budget (default 64 KiB).
        #[arg(long)]
        max_bytes: Option<u32>,
        #[arg(long)]
        lang: Option<String>,
        #[arg(long, value_name = "GLOB")]
        path: Option<String>,
        /// Protect explicit navigation intent instead of broadening to discovery.
        #[arg(long, default_value = "auto", value_parser = ["auto", "exact-name", "exact-id", "name-prefix", "path", "terms", "phrase", "near"])]
        intent: String,
        /// Total intervening whole tokens for phrase intent (0 means adjacency).
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u16).range(0..=4096))]
        phrase_gap: u16,
        /// Inclusive whole-token window for near intent.
        #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u16).range(1..=4096))]
        near_window: u16,
        /// Compare split-only with whole-identifier and qualified-name fields.
        #[arg(long, default_value = "split", value_parser = ["split", "identifiers"])]
        analysis: String,
        /// Remove documented procedural suffixes in auto/terms queries (opt-in).
        #[arg(long, default_value = "verbatim", value_enum)]
        query_policy: QueryPolicyMode,
        /// Retrieval channels for ranked discovery.
        #[arg(long, default_value = "auto", value_parser = ["auto", "fusion", "metadata", "body"])]
        ranking: String,
        /// Metadata field-length normalization (does not change body scoring).
        #[arg(long, default_value = "combined", value_enum)]
        normalization: Normalization,
        /// Relationships used for context; none disables connections and caller impact.
        #[arg(long, default_value = "semantic", value_enum)]
        graph_context: GraphContextMode,
        /// Require every analyzed term in one candidate.
        #[arg(long, conflicts_with = "min_terms")]
        all_terms: bool,
        /// Require this many distinct analyzed terms in one candidate.
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..))]
        min_terms: Option<u16>,
        /// First-pass candidates per file; 0 disables file diversity.
        #[arg(long, default_value_t = 0)]
        per_file: u16,
        /// Stop automatic discovery when an exact name survives filtering.
        #[arg(long)]
        exact_fast_path: bool,
        /// Include executed routes, analyzed terms, and per-channel ranks.
        #[arg(long)]
        explain: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("graph-search: {error}");
            ExitCode::from(exit_code(&error))
        }
    }
}

/// The exit code an error fails with (`SPEC.md` §10.2).
fn exit_code(error: &Error) -> u8 {
    match error {
        // A rejected argument is a usage error; everything else operational.
        Error::Core(
            graph_search::core::Error::InvalidPattern { .. }
            | graph_search::core::Error::InvalidInclude(_)
            | graph_search::core::Error::InvalidQuery(_)
            | graph_search::core::Error::ResultBudget(_)
            | graph_search::core::Error::RootMissing { .. },
        ) => 2,
        Error::Core(graph_search::core::Error::NoIndex) => 4,
        _ => 1,
    }
}

fn run(cli: &Cli) -> Result<ExitCode, Error> {
    let format = if cli.json {
        render::Format::Json
    } else {
        cli.format.unwrap_or(render::Format::Text)
    };
    if !cli.quiet && cli.verbose {
        eprintln!("graph-search: root={:?} store={:?}", cli.root, cli.store);
    }

    match &cli.command {
        Command::Init => init(cli),
        Command::Index { force: _ } => {
            // `--force` is accepted for shell-script compatibility: a full
            // build is what `index` always does (`SPEC.md` §6.5.1).
            let index = open_index(cli, false)?;
            let report = index.reindex()?;
            render::sync(&report, format, Some(index.root()))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Sync => {
            let index = open_index(cli, false)?;
            let report: SyncReport = index.sync()?;
            render::sync(&report, format, Some(index.root()))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Status => {
            let index = open_index(cli, true)?;
            let status: IndexStatus = index.search().status()?;
            render::status(&status, format)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Search { mode } => {
            if let SearchMode::Explore {
                query,
                intent,
                phrase_gap,
                near_window,
                ..
            } = mode
            {
                let predicate = match intent.as_str() {
                    "phrase" => Some(graph_search::core::positional::Predicate::Ordered {
                        intervening: *phrase_gap,
                    }),
                    "near" => Some(graph_search::core::positional::Predicate::Unordered {
                        tokens: *near_window,
                    }),
                    _ => None,
                };
                if let Some(predicate) = predicate {
                    graph_search::core::positional::PositionalQuery::new(query, predicate)
                        .map_err(Error::Core)?;
                }
            }
            let index = open_index(cli, false)?;
            // `--fail-if-stale` with `--no-reconcile`: staleness is fatal
            // before answering (`SPEC.md` §6.5.3).
            if cli.fail_if_stale && cli.no_reconcile {
                let status = index.search().status()?;
                if status.staleness.as_ref().is_some_and(|s| s.changed > 0) {
                    render::stale_notice(&status, format)?;
                    return Ok(ExitCode::from(3));
                }
            }
            search(&index, mode, cli, format)
        }
    }
}

fn init(cli: &Cli) -> Result<ExitCode, Error> {
    let root = cli.root.clone().unwrap_or_else(|| PathBuf::from("."));
    let root = root
        .canonicalize()
        .map_err(|source| Error::Core(graph_search::core::Error::io(&root, source)))?;
    let dir = root.join(".graph-search");
    std::fs::create_dir_all(&dir)
        .map_err(|source| Error::Core(graph_search::core::Error::io(&dir, source)))?;
    let config = dir.join("config.toml");
    if !config.exists() {
        std::fs::write(
            &config,
            "# graph-search configuration (SPEC.md 12).\n# Defaults apply unless overridden here.\n",
        )
        .map_err(|source| Error::Core(graph_search::core::Error::io(&config, source)))?;
    }
    println!("{}", dir.display());
    Ok(ExitCode::SUCCESS)
}

fn open_index(cli: &Cli, _status_only: bool) -> Result<Index, Error> {
    let options = OpenOptions {
        root: cli.root.clone().unwrap_or_else(|| PathBuf::from(".")),
        store: cli.store.clone(),
        excludes: Vec::new(),
        languages: None,
        reconcile: if cli.no_reconcile {
            Reconcile::Never
        } else {
            Reconcile::BeforeQuery
        },
        verification: if cli.verify_content {
            graph_search::Verification::Content
        } else {
            graph_search::Verification::Metadata
        },
        read_only: false,
    };
    Index::open(options)
}

/// The search dispatch: one arm per mode, each three statements long — the
/// mode surface is the spec's (`SPEC.md` §8), so it stays in one place.
#[allow(clippy::too_many_lines)]
fn search(
    index: &Index,
    mode: &SearchMode,
    cli: &Cli,
    format: render::Format,
) -> Result<ExitCode, Error> {
    let service = index.search();
    let observed_stale;
    match mode {
        SearchMode::Files {
            pattern,
            path,
            limit,
        } => {
            let query = FilesQuery::new(pattern.clone())
                .with_limit(limit.unwrap_or(graph_search_types::limits::FILES_DEFAULT_LIMIT));
            let mut query = match path {
                Some(path) => query.with_path(path.clone()),
                None => query,
            };
            query.include_hidden = cli.hidden;
            query.no_ignore = cli.no_ignore;
            let result = service.files(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::files("search.files", index, &query, &result, format)?;
        }
        SearchMode::Text {
            pattern,
            path,
            include,
            limit,
            ignore_case,
        } => {
            let mut query = TextQuery::new(pattern.clone())
                .with_limit(limit.unwrap_or(graph_search_types::limits::TEXT_DEFAULT_LIMIT));
            if let Some(path) = path {
                query.path = Some(path.clone());
            }
            if let Some(include) = include {
                query.include = Some(include.clone());
            }
            query.ignore_case = *ignore_case;
            query.include_hidden = cli.hidden;
            query.no_ignore = cli.no_ignore;
            let result = service.text(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::text("search.text", index, &query, &result, format)?;
        }
        SearchMode::Symbol {
            target,
            kind,
            lang,
            path,
            limit,
        } => {
            let mut query = SymbolQuery::new(target.clone());
            if let Some(kind) = kind {
                query.kind = Some(
                    graph_search_types::kind::NodeKind::parse(kind)
                        .ok_or_else(|| usage_kind(kind))?,
                );
            }
            query.filters = filters(lang.as_ref(), path.as_ref())?;
            if let Some(limit) = limit {
                query.limit = *limit;
            }
            let result = service.symbol(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::graph(
                "search.graph.symbol",
                index,
                &query_echo(&query),
                &result,
                format,
            )?;
        }
        SearchMode::Refs {
            target,
            lang,
            path,
            limit,
        } => {
            let mut query = RefQuery::new(target.clone());
            query.filters = filters(lang.as_ref(), path.as_ref())?;
            if let Some(limit) = limit {
                query.limit = *limit;
            }
            let result = service.refs(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::graph(
                "search.graph.refs",
                index,
                &query_echo(&query),
                &result,
                format,
            )?;
        }
        SearchMode::Occurrences {
            target,
            by,
            rel,
            lang,
            path,
            limit,
        } => {
            use graph_search_types::occurrence::{OccurrenceBy, OccurrenceQuery};
            let query = OccurrenceQuery {
                target: target.clone(),
                by: match by.as_str() {
                    "owner" => OccurrenceBy::Owner,
                    "name" => OccurrenceBy::Name,
                    _ => OccurrenceBy::Target,
                },
                kind: rel.as_deref().map(parse_edge_kind).transpose()?,
                filters: filters(lang.as_ref(), path.as_ref())?,
                limit: limit.unwrap_or_default(),
            };
            let result = service.occurrences(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::occurrences(index, &query_echo(&query), &result, format)?;
        }
        SearchMode::Callers {
            target,
            depth,
            lang,
            path,
            limit,
        } => {
            let mut query = TraversalQuery::new(target.clone(), *depth);
            query.filters = filters(lang.as_ref(), path.as_ref())?;
            if let Some(limit) = limit {
                query.limit = *limit;
            }
            let result = service.callers(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::graph(
                "search.graph.callers",
                index,
                &query_echo(&query),
                &result,
                format,
            )?;
        }
        SearchMode::Callees {
            target,
            depth,
            lang,
            path,
            limit,
        } => {
            let mut query = TraversalQuery::new(target.clone(), *depth);
            query.filters = filters(lang.as_ref(), path.as_ref())?;
            if let Some(limit) = limit {
                query.limit = *limit;
            }
            let result = service.callees(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::graph(
                "search.graph.callees",
                index,
                &query_echo(&query),
                &result,
                format,
            )?;
        }
        SearchMode::Impact {
            target,
            depth,
            lang,
            path,
            limit,
        } => {
            let mut query = TraversalQuery::new(target.clone(), *depth);
            query.filters = filters(lang.as_ref(), path.as_ref())?;
            if let Some(limit) = limit {
                query.limit = *limit;
            }
            let result = service.impact(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::impact(
                "search.graph.impact",
                index,
                &query_echo(&query),
                &result,
                format,
            )?;
        }
        SearchMode::Deps {
            target,
            direction,
            lang,
            path,
            limit,
        } => {
            let mut query = DepsQuery::new(target.clone());
            query.direction = parse_direction(direction)?;
            query.filters = filters(lang.as_ref(), path.as_ref())?;
            if let Some(limit) = limit {
                query.limit = *limit;
            }
            let result = service.deps(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::graph(
                "search.graph.deps",
                index,
                &query_echo(&query),
                &result,
                format,
            )?;
        }
        SearchMode::Neighbors {
            target,
            rel,
            hops,
            lang,
            path,
            limit,
        } => {
            let mut query = NeighborsQuery::new(target.clone());
            if let Some(rel) = rel {
                query.rel = Some(parse_edge_kind(rel)?);
            }
            if let Some(hops) = hops {
                query.hops = *hops;
            }
            query.filters = filters(lang.as_ref(), path.as_ref())?;
            if let Some(limit) = limit {
                query.limit = *limit;
            }
            let result = service.neighbors(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::graph(
                "search.graph.neighbors",
                index,
                &query_echo(&query),
                &result,
                format,
            )?;
        }
        SearchMode::Path { from, to, max_hops } => {
            let mut query = PathQuery::new(from.clone(), to.clone());
            if let Some(max_hops) = max_hops {
                query.max_hops = *max_hops;
            }
            let result = service.path(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::graph(
                "search.graph.path",
                index,
                &query_echo(&query),
                &result,
                format,
            )?;
        }
        SearchMode::Explore {
            query,
            k,
            hops,
            context_lines,
            detail,
            max_bytes,
            lang,
            path,
            intent,
            phrase_gap,
            near_window,
            analysis,
            query_policy,
            ranking,
            normalization,
            graph_context,
            all_terms,
            min_terms,
            per_file,
            exact_fast_path,
            explain,
        } => {
            let mut query = ExploreQuery::new(query.clone());
            query.retrieval = graph_search_types::RetrievalOptions {
                query_policy: if matches!(query_policy, QueryPolicyMode::Task) {
                    graph_search_types::QueryPolicy::Task
                } else {
                    graph_search_types::QueryPolicy::Verbatim
                },
                analysis: if analysis == "identifiers" {
                    graph_search_types::AnalysisMode::Identifiers
                } else {
                    graph_search_types::AnalysisMode::Split
                },
                mode: match intent.as_str() {
                    "exact-name" => graph_search_types::ExploreMode::ExactName,
                    "exact-id" => graph_search_types::ExploreMode::ExactId,
                    "name-prefix" => graph_search_types::ExploreMode::NamePrefix,
                    "path" => graph_search_types::ExploreMode::PathGlob,
                    "terms" => graph_search_types::ExploreMode::Terms,
                    "phrase" => graph_search_types::ExploreMode::Phrase,
                    "near" => graph_search_types::ExploreMode::Near,
                    _ => graph_search_types::ExploreMode::Auto,
                },
                ranking: match ranking.as_str() {
                    "metadata" => graph_search_types::RankingStrategy::Metadata,
                    "body" => graph_search_types::RankingStrategy::Body,
                    "fusion" => graph_search_types::RankingStrategy::Fusion,
                    _ => graph_search_types::RankingStrategy::Auto,
                },
                normalization: if matches!(normalization, Normalization::Bm25f) {
                    graph_search_types::FieldNormalization::Bm25f
                } else {
                    graph_search_types::FieldNormalization::Combined
                },
                graph_context: match graph_context {
                    GraphContextMode::Semantic => graph_search_types::GraphContext::Semantic,
                    GraphContextMode::None => graph_search_types::GraphContext::None,
                    GraphContextMode::Calls => graph_search_types::GraphContext::Calls,
                    GraphContextMode::Imports => graph_search_types::GraphContext::Imports,
                    GraphContextMode::Types => graph_search_types::GraphContext::Types,
                },
                term_match: if *all_terms {
                    graph_search_types::TermMatch::All
                } else if let Some(n) = min_terms {
                    graph_search_types::TermMatch::AtLeast(*n)
                } else {
                    graph_search_types::TermMatch::Any
                },
                phrase_gap: *phrase_gap,
                near_window: *near_window,
                per_file: *per_file,
                exact_fast_path: *exact_fast_path,
                explain: *explain,
            };
            if let Some(k) = k {
                query.k = *k;
            }
            if let Some(hops) = hops {
                query.hops = *hops;
            }
            if let Some(lines) = context_lines {
                query = query.with_context_lines(*lines);
            }
            query = query.with_detail(match detail {
                DetailMode::Full => graph_search_types::ExploreDetail::Full,
                DetailMode::Compact => graph_search_types::ExploreDetail::Compact,
            });
            if let Some(bytes) = max_bytes {
                query.max_bytes = *bytes;
            }
            query.filters = filters(lang.as_ref(), path.as_ref())?;
            let result = service.explore(&query)?;
            observed_stale = result.context.staleness.changed > 0;
            render::explore("search.explore", index, &query, &result, format)?;
        }
    }
    Ok(if cli.fail_if_stale && observed_stale {
        ExitCode::from(3)
    } else {
        ExitCode::SUCCESS
    })
}

/// Parses `--direction`.
fn parse_direction(direction: &str) -> Result<graph_search_types::kind::Direction, Error> {
    match direction {
        "in" => Ok(graph_search_types::kind::Direction::In),
        "out" => Ok(graph_search_types::kind::Direction::Out),
        "both" => Ok(graph_search_types::kind::Direction::Both),
        other => Err(Error::Core(graph_search::core::Error::InvalidInclude(
            format!("--direction takes in, out, or both, but {other:?} was given"),
        ))),
    }
}

/// Parses `--rel`.
fn parse_edge_kind(rel: &str) -> Result<graph_search_types::kind::EdgeKind, Error> {
    graph_search_types::kind::EdgeKind::parse(rel).ok_or_else(|| {
        Error::Core(graph_search::core::Error::InvalidInclude(format!(
            "--rel takes an edge kind such as calls or imports, but {rel:?} was given"
        )))
    })
}

fn usage_kind(kind: &str) -> Error {
    Error::Core(graph_search::core::Error::InvalidInclude(format!(
        "--kind takes a node kind such as function or struct, but {kind:?} was given"
    )))
}

/// The normalized-arguments echo for the JSON envelope (`SPEC.md` §9.1).
fn query_echo<Q: serde::Serialize>(query: &Q) -> serde_json::Value {
    serde_json::to_value(query).unwrap_or(serde_json::json!({}))
}

fn filters(
    lang: Option<&String>,
    path: Option<&String>,
) -> Result<graph_search_types::query::GraphFilters, Error> {
    let language = lang
        .map(|name| {
            graph_search_types::Language::parse(name).ok_or_else(|| {
                Error::Core(graph_search::core::Error::InvalidInclude(format!(
                    "unknown language: {name}"
                )))
            })
        })
        .transpose()?;
    Ok(graph_search_types::query::GraphFilters {
        lang: language,
        path_glob: path.cloned(),
    })
}
