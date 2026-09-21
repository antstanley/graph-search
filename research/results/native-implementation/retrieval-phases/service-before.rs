//! The read API: every query a host makes, in-process (`SPEC.md` §4.7).
//!
//! Every method is bounded (`SPEC.md` §14) and reports its caps. Sourcing
//! follows `SPEC.md` §4.6: `files` is index-first only when the index has
//! been verified fresh in this process, `text` always scans, and the graph
//! modes read a snapshot.

use crate::error::{Error, Result};
use crate::index::{Index, Reconcile, Verification};
use graph_search_core::config::WalkPolicy;
use graph_search_core::ports::{GraphSnapshot, GraphStore};
use graph_search_core::query::QueryEngine;
use graph_search_core::{files_search, text_search};
use graph_search_types::FilesQuery;
use graph_search_types::context::{
    FreshnessMethod, ResultContext, SourceIdentity, SourceVerification,
};
use graph_search_types::query::{
    DepsQuery, ExploreQuery, NeighborsQuery, PathQuery, RefQuery, SymbolQuery, TextQuery,
    TraversalQuery,
};
use graph_search_types::result::{
    ExploreResult, FilesResult, GraphResult, ImpactResult, IndexStatus, Staleness, SyncReport,
    TextResult,
};

/// The query handle a host calls in-process.
pub struct SearchService<'a> {
    index: &'a Index,
    work_limits: graph_search_core::work::WorkLimits,
}

impl<'a> SearchService<'a> {
    pub(crate) fn new(index: &'a Index) -> Self {
        Self {
            index,
            work_limits: graph_search_core::work::WorkLimits::default(),
        }
    }

    /// Configures request-scoped retrieval limits, cancellation and deadline.
    /// Cancellation/deadlines also cover file/text scans and body-candidate walks.
    /// The entry allowance is shared by walks within each query.
    /// Source quotas cover literal scans, freshness verification and live evidence.
    /// Automatic maintenance and status freshness checks share these quotas.
    #[must_use]
    pub fn with_work_limits(mut self, limits: graph_search_core::work::WorkLimits) -> Self {
        self.work_limits = limits;
        self
    }

    fn validate_fields(targets: &[&str], path: Option<&str>) -> Result<()> {
        for target in targets {
            graph_search_core::work::validate_query_bytes("query target", target)
                .map_err(Error::Core)?;
        }
        if let Some(path) = path {
            graph_search_core::work::validate_query_bytes("path filter", path)
                .map_err(Error::Core)?;
        }
        Ok(())
    }

    /// The policy adjusted for one query's flags.
    fn policy(&self, include_hidden: bool, no_ignore: bool) -> WalkPolicy {
        let mut policy = self.index.policy().clone();
        if include_hidden {
            policy.include_hidden = true;
        }
        if no_ignore {
            policy.respect_ignore = false;
        }
        policy
    }

    // ------------------------------------------------------------------
    // Walk-served modes
    // ------------------------------------------------------------------

    /// `search files` (`SPEC.md` §8.1).
    ///
    /// # Errors
    /// When the root is missing or the pattern is invalid.
    pub fn files(&self, query: &FilesQuery) -> Result<FilesResult> {
        // The tree can change while an Index remains resident, and per-query
        // hidden/ignore flags can broaden the indexed set. Scan for parity.
        files_search::search_files_with_work(
            self.index.root(),
            query,
            &self.policy(query.include_hidden, query.no_ignore),
            &mut graph_search_core::work::WorkBudget::new(self.work_limits.clone()),
        )
        .map_err(Error::Core)
    }

    /// `search text`: a literal scan, never the index (`SPEC.md` §8.2).
    ///
    /// # Errors
    /// When the include filter is rejected or the root is missing.
    pub fn text(&self, query: &TextQuery) -> Result<TextResult> {
        text_search::search_text_with_work(
            self.index.root(),
            query,
            &self.policy(query.include_hidden, query.no_ignore),
            &mut graph_search_core::work::WorkBudget::new(self.work_limits.clone()),
        )
        .map_err(Error::Core)
    }

    // ------------------------------------------------------------------
    // Graph modes
    // ------------------------------------------------------------------

    /// The staleness posture for one query: reconcile when the index is
    /// behind and the posture says so; report either way (`SPEC.md` §6.5.3).
    /// With `--no-reconcile` semantics and no usable index, this is the
    /// clear "not indexed" answer (`SPEC.md` §11.1, exit code 4).
    fn freshness(&self, work: &mut graph_search_core::work::WorkBudget) -> Result<Staleness> {
        work.check().map_err(Error::Core)?;
        let manifest = self
            .index
            .store_read(|store| store.manifest_header().map_err(Error::Core))?;
        let Some(manifest) = manifest else {
            if self.index.reconcile() == Reconcile::Never {
                return Err(Error::Core(graph_search_core::Error::NoIndex));
            }
            if self.index.reconcile() == Reconcile::BeforeQuery {
                self.index.maintain_with_work(false, work)?;
            } else {
                return Err(Error::Core(graph_search_core::Error::NoIndex));
            }
            return Ok(Staleness::default());
        };
        let search_root = graph_search_core::walk::resolve_search_root(self.index.root(), None)
            .map_err(Error::Core)?;
        let staleness = graph_search_core::stale::inspect_with_work(
            &search_root,
            &manifest,
            self.index.policy(),
            self.index.verification() == Verification::Content,
            work,
        )
        .map_err(Error::Core)?
        .0;
        if staleness.changed > 0 && self.index.reconcile() == Reconcile::BeforeQuery {
            if self.index.verification() == Verification::Content {
                // The metadata diff deliberately skips same-size/same-mtime
                // files. Strict verification must not reuse those cached facts.
                self.index.maintain_with_work(true, work)?;
            } else {
                self.index.maintain_with_work(false, work)?;
            }
        }
        Ok(staleness)
    }

    fn context(
        &self,
        store: &dyn GraphStore,
        work: &mut graph_search_core::work::WorkBudget,
    ) -> Result<ResultContext> {
        let manifest = store
            .manifest_header()
            .map_err(Error::Core)?
            .ok_or(Error::Core(graph_search_core::Error::NoIndex))?;
        let (staleness, mut coverage) = graph_search_core::stale::inspect_with_work(
            self.index.root(),
            &manifest,
            self.index.policy(),
            self.index.verification() == Verification::Content,
            work,
        )
        .map_err(Error::Core)?;
        graph_search_core::units::coverage(
            store.snapshot().map_err(Error::Core)?.source_files(),
            &mut coverage,
        );
        Ok(ResultContext {
            indexed_versions: Some(manifest.versions()),
            runtime_versions: Some(graph_search_types::context::RuntimeVersions::current()),
            coverage,
            generation: store.generation().map_err(Error::Core)?,
            freshness: match self.index.verification() {
                Verification::Metadata => FreshnessMethod::Metadata,
                Verification::Content => FreshnessMethod::Content,
            },
            reconciliation: Some(
                match self.index.reconcile() {
                    Reconcile::BeforeQuery => "before_query",
                    Reconcile::Never => "never",
                    Reconcile::Explicit => "explicit",
                }
                .to_owned(),
            ),
            staleness,
            ..ResultContext::default()
        })
    }

    fn source_identities<'p>(
        snapshot: &dyn GraphSnapshot,
        context: &mut ResultContext,
        paths: impl Iterator<Item = &'p str>,
    ) -> Result<()> {
        for path in paths {
            if context.sources.contains_key(path) {
                continue;
            }
            let hash = snapshot
                .node_by_id(&graph_search_types::NodeId::file(path))
                .map_err(Error::Core)?
                .and_then(|node| node.content_hash);
            let verified = context.freshness == FreshnessMethod::Content
                && !context
                    .staleness
                    .changed_paths
                    .iter()
                    .any(|changed| changed == path);
            context.sources.insert(
                path.to_owned(),
                SourceIdentity {
                    indexed_hash: hash.clone(),
                    observed_hash: if verified { hash } else { None },
                    verification: if verified {
                        SourceVerification::Verified
                    } else {
                        SourceVerification::NotRead
                    },
                },
            );
        }
        Ok(())
    }

    fn graph(
        &self,
        run: impl FnOnce(
            &dyn GraphSnapshot,
            graph_search_core::work::WorkBudget,
        ) -> graph_search_core::Result<GraphResult>,
    ) -> Result<GraphResult> {
        let mut work = graph_search_core::work::WorkBudget::new(self.work_limits.clone());
        self.freshness(&mut work)?;
        self.index.store_read(|store| {
            let snapshot = store.snapshot().map_err(Error::Core)?;
            let context = self.context(store, &mut work)?;
            let mut result = run(snapshot.as_ref(), work).map_err(Error::Core)?;
            result.context = context;
            Self::source_identities(
                snapshot.as_ref(),
                &mut result.context,
                result
                    .nodes
                    .iter()
                    .map(|node| node.path.as_str())
                    .chain(result.edges.iter().filter_map(|edge| edge.path.as_deref())),
            )?;
            graph_search_core::payload::fit_graph(&mut result).map_err(Error::Core)?;
            Ok(result)
        })
    }

    /// `search symbol`: where is `<name>` defined (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn symbol(&self, query: &SymbolQuery) -> Result<GraphResult> {
        Self::validate_fields(&[&query.target], query.filters.path_glob.as_deref())?;
        self.graph(|snapshot, work| QueryEngine::with_work_budget(snapshot, work).symbol(query))
    }

    /// `search refs`: every reference to it.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn refs(&self, query: &RefQuery) -> Result<GraphResult> {
        Self::validate_fields(&[&query.target], query.filters.path_glob.as_deref())?;
        self.graph(|snapshot, work| QueryEngine::with_work_budget(snapshot, work).refs(query))
    }

    /// Individual indexed source references, including unresolved raw-name matches.
    /// # Errors
    /// On unavailable indexes, ambiguous targets, invalid filters or exhausted byte metadata budget.
    pub fn occurrences(
        &self,
        query: &graph_search_types::occurrence::OccurrenceQuery,
    ) -> Result<graph_search_types::occurrence::OccurrenceResult> {
        Self::validate_fields(&[&query.target], query.filters.path_glob.as_deref())?;
        let mut work = graph_search_core::work::WorkBudget::new(self.work_limits.clone());
        self.freshness(&mut work)?;
        self.index.store_read(|store| {
            let snapshot = store.snapshot().map_err(Error::Core)?;
            let context = self.context(store, &mut work)?;
            let mut result = QueryEngine::with_work_budget(snapshot.as_ref(), work)
                .occurrences(query)
                .map_err(Error::Core)?;
            result.context = context;
            Self::source_identities(
                snapshot.as_ref(),
                &mut result.context,
                result.items.iter().map(|item| item.path.as_str()),
            )?;
            graph_search_core::payload::fit_occurrences(&mut result).map_err(Error::Core)?;
            Ok(result)
        })
    }

    /// `search callers`.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn callers(&self, query: &TraversalQuery) -> Result<GraphResult> {
        Self::validate_fields(&[&query.target], query.filters.path_glob.as_deref())?;
        self.graph(|snapshot, work| QueryEngine::with_work_budget(snapshot, work).callers(query))
    }

    /// `search callees`.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn callees(&self, query: &TraversalQuery) -> Result<GraphResult> {
        Self::validate_fields(&[&query.target], query.filters.path_glob.as_deref())?;
        self.graph(|snapshot, work| QueryEngine::with_work_budget(snapshot, work).callees(query))
    }

    /// `search impact`: the blast radius.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn impact(&self, query: &TraversalQuery) -> Result<ImpactResult> {
        Self::validate_fields(&[&query.target], query.filters.path_glob.as_deref())?;
        let mut work = graph_search_core::work::WorkBudget::new(self.work_limits.clone());
        self.freshness(&mut work)?;
        self.index.store_read(|store| {
            let snapshot = store.snapshot().map_err(Error::Core)?;
            let context = self.context(store, &mut work)?;
            let mut result = QueryEngine::with_work_budget(snapshot.as_ref(), work)
                .impact(query)
                .map_err(Error::Core)?;
            result.context = context;
            Self::source_identities(
                snapshot.as_ref(),
                &mut result.context,
                result
                    .top
                    .iter()
                    .map(|node| node.path.as_str())
                    .chain(result.edges.iter().filter_map(|edge| edge.path.as_deref())),
            )?;
            graph_search_core::payload::fit_impact(&mut result).map_err(Error::Core)?;
            Ok(result)
        })
    }

    /// `search deps`: imports and imported-by.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn deps(&self, query: &DepsQuery) -> Result<GraphResult> {
        Self::validate_fields(&[&query.target], query.filters.path_glob.as_deref())?;
        self.graph(|snapshot, work| QueryEngine::with_work_budget(snapshot, work).deps(query))
    }

    /// `search neighbors`.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn neighbors(&self, query: &NeighborsQuery) -> Result<GraphResult> {
        Self::validate_fields(&[&query.target], query.filters.path_glob.as_deref())?;
        self.graph(|snapshot, work| QueryEngine::with_work_budget(snapshot, work).neighbors(query))
    }

    /// `search path`: the shortest path between two nodes.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn path(&self, query: &PathQuery) -> Result<GraphResult> {
        Self::validate_fields(&[&query.from, &query.to], None)?;
        self.graph(|snapshot, work| QueryEngine::with_work_budget(snapshot, work).path(query))
    }

    /// `search explore`: the one-call retrieval (`SPEC.md` §8.4).
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn explore(&self, query: &ExploreQuery) -> Result<ExploreResult> {
        Self::validate_fields(&[&query.query], query.filters.path_glob.as_deref())?;
        graph_search_core::positional::PositionalQuery::for_retrieval(
            &query.query,
            &query.retrieval,
        )?;
        let mut work = graph_search_core::work::WorkBudget::new(self.work_limits.clone());
        work.enable_source_capture();
        self.freshness(&mut work)?;
        let root = self.index.root().to_path_buf();
        self.index.store_read(|store| {
            let snapshot = store.snapshot().map_err(Error::Core)?;
            let context = self.context(store, &mut work)?;
            QueryEngine::with_work_budget(snapshot.as_ref(), work)
                .explore_with_context(query, &root, self.index.policy(), context)
                .map_err(Error::Core)
        })
    }

    // ------------------------------------------------------------------
    // Status
    // ------------------------------------------------------------------

    /// `status`: whether an index exists and how far behind it is
    /// (`SPEC.md` §8.5).
    ///
    /// # Errors
    /// When the store or the walk fails.
    pub fn status(&self) -> Result<IndexStatus> {
        let mut work = graph_search_core::work::WorkBudget::new(self.work_limits.clone());
        work.check().map_err(Error::Core)?;
        self.index.store_read(|store| {
            let manifest = store.manifest_header().map_err(Error::Core)?;
            let mut result = IndexStatus {
                generation: store.generation().map_err(Error::Core)?,
                exists: manifest.is_some(),
                store_path: self.index.store_dir().display().to_string(),
                root: self.index.root().display().to_string(),
                schema_version: graph_search_types::SCHEMA_VERSION,
                parser_version: graph_search_types::PARSER_VERSION,
                ..IndexStatus::default()
            };
            if let Some(manifest) = manifest {
                let snapshot = store.snapshot().map_err(Error::Core)?;
                result.counts = Some(snapshot.counts().clone());
                result.indexed_at_ms = Some(manifest.indexed_at_ms);
                let (staleness, coverage) = graph_search_core::stale::inspect_with_work(
                    self.index.root(),
                    &manifest,
                    self.index.policy(),
                    self.index.verification() == Verification::Content,
                    &mut work,
                )
                .map_err(Error::Core)?;
                result.staleness = Some(staleness);
                result.coverage = coverage;
                graph_search_core::units::coverage(snapshot.source_files(), &mut result.coverage);
            }
            work.check().map_err(Error::Core)?;
            graph_search_core::payload::fit_status(&mut result).map_err(Error::Core)?;
            work.check().map_err(Error::Core)?;
            Ok(result)
        })
    }

    /// Sync passthrough, for hosts that want the report through the service.
    ///
    /// # Errors
    /// See [`Index::sync`].
    pub fn sync(&self) -> Result<SyncReport> {
        self.index.sync()
    }
}
