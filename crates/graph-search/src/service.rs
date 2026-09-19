//! The read API: every query a host makes, in-process (`SPEC.md` §4.7).
//!
//! Every method is bounded (`SPEC.md` §14) and reports its caps. Sourcing
//! follows `SPEC.md` §4.6: `files` is index-first only when the index has
//! been verified fresh in this process, `text` always scans, and the graph
//! modes read a snapshot.

use crate::error::{Error, Result};
use crate::index::{Index, Reconcile};
use graph_search_core::config::WalkPolicy;
use graph_search_core::ports::GraphSnapshot;
use graph_search_core::query::QueryEngine;
use graph_search_core::{files_search, text_search};
use graph_search_types::FilesQuery;
use graph_search_types::query::{
    DepsQuery, ExploreQuery, NeighborsQuery, PathQuery, RefQuery, SymbolQuery, TextQuery,
    TraversalQuery,
};
use graph_search_types::result::{
    ExploreResult, FilesResult, GraphResult, ImpactResult, IndexStatus, Staleness, StoreCounts,
    SyncReport, TextResult,
};

/// The query handle a host calls in-process.
pub struct SearchService<'a> {
    index: &'a Index,
}

impl<'a> SearchService<'a> {
    pub(crate) fn new(index: &'a Index) -> Self {
        Self { index }
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
        files_search::search_files(
            self.index.root(),
            query,
            &self.policy(query.include_hidden, query.no_ignore),
        )
        .map_err(Error::Core)
    }

    /// `search text`: a literal scan, never the index (`SPEC.md` §8.2).
    ///
    /// # Errors
    /// When the include filter is rejected or the root is missing.
    pub fn text(&self, query: &TextQuery) -> Result<TextResult> {
        text_search::search_text(
            self.index.root(),
            query,
            &self.policy(query.include_hidden, query.no_ignore),
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
    fn freshness(&self) -> Result<Staleness> {
        let manifest = self
            .index
            .store_read(|store| store.manifest().map_err(Error::Core))?;
        let Some(manifest) = manifest else {
            if self.index.reconcile() == Reconcile::Never {
                return Err(Error::Core(graph_search_core::Error::NoIndex));
            }
            if self.index.reconcile() == Reconcile::BeforeQuery {
                self.index.sync()?;
            } else {
                return Err(Error::Core(graph_search_core::Error::NoIndex));
            }
            return Ok(Staleness::default());
        };
        let search_root = graph_search_core::walk::resolve_search_root(self.index.root(), None)
            .map_err(Error::Core)?;
        let staleness =
            graph_search_core::stale::check(&search_root, &manifest, self.index.policy())
                .map_err(Error::Core)?;
        if staleness.changed > 0 && self.index.reconcile() == Reconcile::BeforeQuery {
            self.index.sync()?;
        }
        Ok(staleness)
    }

    fn graph(
        &self,
        run: impl FnOnce(&dyn GraphSnapshot) -> graph_search_core::Result<GraphResult>,
    ) -> Result<GraphResult> {
        self.freshness()?;
        self.index.store_read(|store| {
            let snapshot = store.snapshot().map_err(Error::Core)?;
            run(snapshot.as_ref()).map_err(Error::Core)
        })
    }

    /// `search symbol`: where is `<name>` defined (`SPEC.md` §8.3).
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn symbol(&self, query: &SymbolQuery) -> Result<GraphResult> {
        self.graph(|snapshot| QueryEngine::new(snapshot).symbol(query))
    }

    /// `search refs`: every reference to it.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn refs(&self, query: &RefQuery) -> Result<GraphResult> {
        self.graph(|snapshot| QueryEngine::new(snapshot).refs(query))
    }

    /// `search callers`.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn callers(&self, query: &TraversalQuery) -> Result<GraphResult> {
        self.graph(|snapshot| QueryEngine::new(snapshot).callers(query))
    }

    /// `search callees`.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn callees(&self, query: &TraversalQuery) -> Result<GraphResult> {
        self.graph(|snapshot| QueryEngine::new(snapshot).callees(query))
    }

    /// `search impact`: the blast radius.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn impact(&self, query: &TraversalQuery) -> Result<ImpactResult> {
        self.freshness()?;
        self.index.store_read(|store| {
            let snapshot = store.snapshot().map_err(Error::Core)?;
            QueryEngine::new(snapshot.as_ref())
                .impact(query)
                .map_err(Error::Core)
        })
    }

    /// `search deps`: imports and imported-by.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn deps(&self, query: &DepsQuery) -> Result<GraphResult> {
        self.graph(|snapshot| QueryEngine::new(snapshot).deps(query))
    }

    /// `search neighbors`.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn neighbors(&self, query: &NeighborsQuery) -> Result<GraphResult> {
        self.graph(|snapshot| QueryEngine::new(snapshot).neighbors(query))
    }

    /// `search path`: the shortest path between two nodes.
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn path(&self, query: &PathQuery) -> Result<GraphResult> {
        self.graph(|snapshot| QueryEngine::new(snapshot).path(query))
    }

    /// `search explore`: the one-call retrieval (`SPEC.md` §8.4).
    ///
    /// # Errors
    /// See [`Self::graph`].
    pub fn explore(&self, query: &ExploreQuery) -> Result<ExploreResult> {
        self.freshness()?;
        let root = self.index.root().to_path_buf();
        self.index.store_read(|store| {
            let snapshot = store.snapshot().map_err(Error::Core)?;
            QueryEngine::new(snapshot.as_ref())
                .explore_with_policy(query, &root, self.index.policy())
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
        let (exists, counts, indexed_at_ms): (bool, Option<StoreCounts>, Option<u64>) =
            self.index.store_read(|store| {
                let manifest = store.manifest().map_err(Error::Core)?;
                let exists = manifest.is_some();
                let indexed_at_ms = manifest.as_ref().map(|m| m.indexed_at_ms);
                if exists {
                    let snapshot = store.snapshot().map_err(Error::Core)?;
                    let counts = QueryEngine::new(snapshot.as_ref())
                        .counts()
                        .map_err(Error::Core)?;
                    Ok((exists, Some(counts), indexed_at_ms))
                } else {
                    Ok((exists, None, indexed_at_ms))
                }
            })?;
        let mut staleness = None;
        if exists
            && let Some(manifest) = self
                .index
                .store_read(|store| store.manifest().map_err(Error::Core))?
        {
            let search_root = graph_search_core::walk::resolve_search_root(self.index.root(), None)
                .map_err(Error::Core)?;
            let checked =
                graph_search_core::stale::check(&search_root, &manifest, self.index.policy())
                    .map_err(Error::Core)?;
            staleness = Some(checked);
        }
        Ok(IndexStatus {
            exists,
            store_path: self.index.store_dir().display().to_string(),
            root: self.index.root().display().to_string(),
            schema_version: graph_search_types::SCHEMA_VERSION,
            parser_version: graph_search_types::PARSER_VERSION,
            counts,
            indexed_at_ms,
            staleness,
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
