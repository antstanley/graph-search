//! The ports: the projected store, its read view, and one language's
//! extraction (`SPEC.md` §4.2).
//!
//! Vendor-free in their signatures: no storage type and no tree-sitter type
//! crosses either trait. The read/write split is at the type level — `apply`
//! is the only mutator, and every read goes through a snapshot taken from the
//! store; a snapshot taken before an `apply` never sees that batch.

use crate::Result;
use graph_search_types::kind::{Direction, EdgeKind, NodeKind};
use graph_search_types::node::Node;
use graph_search_types::{Edge, NodeId, Scored, Subgraph};
use std::path::Path;

/// One source file handed to an extractor.
pub struct SourceFile<'a> {
    /// The workspace-relative path.
    pub path: &'a Path,
    /// The file's bytes (valid UTF-8; the walker rejects non-UTF-8).
    pub text: &'a str,
}

/// A parse or extraction failure. It quarantines a file; it never fails a
/// command (`SPEC.md` §6.4, §13).
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ParseError {
    /// A one-line reason for the quarantine record.
    pub message: String,
}

impl ParseError {
    /// Builds a parse error with `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// One language's extraction into nodes and edges (`SPEC.md` §4.2).
pub trait LanguageExtractor: Send + Sync {
    /// The language this extractor serves.
    fn language(&self) -> graph_search_types::Language;

    /// Whether this extractor claims `path` (by extension, including TSX/JSX
    /// spellings).
    fn supports(&self, path: &Path) -> bool;

    /// Extracts the file into symbols, references, and file annotations.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] when the file cannot be parsed; the caller
    /// quarantines the file rather than failing.
    fn extract(
        &self,
        file: &SourceFile<'_>,
    ) -> std::result::Result<crate::extraction::Extraction, ParseError>;
}

/// Answers "which extractor, if any, handles this path". The library wires a
/// registry over the language adapters; `core` never names one.
pub trait LanguageRegistry {
    /// Optional raw JSON/JSONC configuration projection over captured source bytes.
    /// This does not declare project membership or resolve inheritance/aliases.
    fn typescript_config(
        &self,
        _file: &SourceFile<'_>,
    ) -> Option<graph_search_types::typescript::TypeScriptConfig> {
        None
    }

    /// Optional manifest syntax adapter over the same captured source bytes.
    /// `None` means this registry does not recognize/support the manifest.
    fn package_manifest(
        &self,
        _file: &SourceFile<'_>,
    ) -> Option<graph_search_types::package::PackageManifest> {
        None
    }

    /// The extractor that claims `path`, when its language is enabled.
    fn extractor_for(&self, path: &Path) -> Option<&dyn LanguageExtractor>;
}

/// A trivial registry over an owned list.
pub struct ListRegistry {
    extractors: Vec<Box<dyn LanguageExtractor>>,
}

impl ListRegistry {
    /// A registry over `extractors`, consulted in order.
    #[must_use]
    pub fn new(extractors: Vec<Box<dyn LanguageExtractor>>) -> Self {
        Self { extractors }
    }
}

impl LanguageRegistry for ListRegistry {
    fn extractor_for(&self, path: &Path) -> Option<&dyn LanguageExtractor> {
        self.extractors
            .iter()
            .map(std::convert::AsRef::as_ref)
            .find(|ex| ex.supports(path))
    }
}

/// File-keyed raw extraction facts; absent keys mean no cached facts are available.
pub type ExtractionFacts =
    std::collections::BTreeMap<String, graph_search_types::extraction::SharedExtraction>;

/// The projected store the projector writes and the query engine reads
/// (`SPEC.md` §4.2).
pub trait GraphStore: Send {
    /// Applies one batch atomically; the manifest is committed separately,
    /// last (`SPEC.md` §6.4).
    ///
    /// # Errors
    ///
    /// Preparation failures abandon the batch whole. If a persistent adapter
    /// cannot confirm durability after publication, it must refuse subsequent
    /// reads until reopened. Use `publish` for graph and manifest coherence.
    fn apply(
        &mut self,
        batch: graph_search_types::WriteBatch,
    ) -> Result<graph_search_types::ApplyOutcome>;

    /// Publishes graph and manifest as one coherent generation. Persistent
    /// adapters must override this to prepare all state before making it visible.
    /// The default is suitable for infallible in-memory implementations.
    ///
    /// # Errors
    /// When preparation or publication fails.
    fn publish(
        &mut self,
        batch: graph_search_types::WriteBatch,
    ) -> Result<graph_search_types::ApplyOutcome> {
        let manifest = batch.manifest.clone();
        let outcome = self.apply(batch)?;
        self.commit_manifest(manifest)?;
        Ok(outcome)
    }

    /// Publishes while explicitly retaining cached facts from an observed header.
    /// The compatibility path loads only retained facts before normal publication.
    /// Native persistent adapters can retain verified packed records directly.
    /// # Errors
    /// On stale retention identity, missing records or publication failure.
    fn publish_retaining(
        &mut self,
        mut batch: graph_search_types::WriteBatch,
        retention: &crate::retention::FactRetention,
    ) -> Result<graph_search_types::ApplyOutcome> {
        retention.validate_batch(self, &batch)?;
        let facts = self.extraction_facts(&retention.paths)?;
        if facts.len() != retention.paths.len() {
            return Err(crate::Error::Store("missing retained extraction".into()));
        }
        for (path, facts) in facts {
            if let Some(entry) = batch.manifest.entries.get_mut(&path) {
                entry.extraction = Some(facts);
            }
        }
        self.publish(batch)
    }

    /// Identity of the currently opened committed generation, if supported.
    /// This must describe the same state as this handle's snapshots.
    ///
    /// # Errors
    /// When publication left the handle unavailable.
    fn generation(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// A point-in-time read view.
    ///
    /// # Errors
    /// When a snapshot cannot be taken.
    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot + '_>>;

    /// The committed manifest, when the store holds one.
    ///
    /// # Errors
    /// When the stored manifest cannot be read.
    fn manifest(&self) -> Result<Option<graph_search_types::Manifest>>;

    /// Compact dependencies from the same committed generation as the snapshot.
    /// `None` selects conservative compatibility repair.
    /// # Errors
    /// When the selected generation is unavailable.
    fn dependency_index(&self) -> Result<Option<&dyn crate::dependencies::DependencyLookup>> {
        Ok(None)
    }

    /// Raw extraction facts for the requested paths in this committed generation.
    /// Unknown paths and unavailable caches are omitted. Native adapters avoid
    /// cloning/decoding unrequested facts; the compatibility default hydrates the
    /// full manifest for a nonempty request. An empty request performs no I/O.
    /// # Errors
    /// On unavailable generation, unreadable facts or mismatched fingerprints.
    fn extraction_facts(
        &self,
        paths: &std::collections::BTreeSet<String>,
    ) -> Result<ExtractionFacts> {
        if paths.is_empty() {
            return Ok(ExtractionFacts::new());
        }
        let manifest = self.manifest()?;
        Ok(paths
            .iter()
            .filter_map(|path| {
                manifest
                    .as_ref()?
                    .entries
                    .get(path)?
                    .extraction
                    .as_ref()
                    .map(|facts| (path.clone(), facts.clone()))
            })
            .collect())
    }

    /// Freshness/version metadata without raw extraction facts. Native adapters
    /// serve this from the selected generation without reading its raw-fact file.
    /// # Errors
    /// When the selected generation is unavailable.
    fn manifest_header(&self) -> Result<Option<graph_search_types::Manifest>> {
        Ok(self
            .manifest()?
            .as_ref()
            .map(graph_search_types::Manifest::header))
    }

    /// Commits the manifest after a successful apply (`SPEC.md` §6.4).
    ///
    /// # Errors
    /// When the manifest cannot be persisted.
    fn commit_manifest(&mut self, manifest: graph_search_types::Manifest) -> Result<()>;
}

/// A point-in-time read view of the store (`SPEC.md` §4.2).
pub trait GraphSnapshot {
    /// Exact cached counts from this generation; does not enumerate graph facts.
    fn counts(&self) -> &graph_search_types::result::StoreCounts;

    /// Exact cached source coverage from this generation; does not load the
    /// source facts.
    fn source_coverage(&self) -> &crate::units::SourceCoverage;

    // The fact and index accessors below may load and verify generation data
    // on first use, so each one reports a read or integrity failure.

    /// File-owned reference occurrences from the selected graph generation.
    ///
    /// # Errors
    /// When the facts cannot be read or fail verification.
    fn occurrence_files(
        &self,
    ) -> Result<&std::collections::BTreeMap<String, graph_search_types::occurrence::OccurrenceFile>>;
    /// Native occurrence lookup positions from the same generation.
    ///
    /// # Errors
    /// When the facts cannot be read or fail verification.
    fn occurrences(&self) -> Result<&crate::occurrences::OccurrenceIndex>;
    /// Known occurrences of one aggregate relationship; `None` when none are
    /// recorded. Adapters may answer from a published count table without
    /// loading the occurrence facts.
    ///
    /// # Errors
    /// When the counts cannot be read or fail verification.
    fn occurrence_count(&self, edge_id: &str) -> Result<Option<usize>> {
        Ok(self.occurrences()?.count_for_edge(edge_id))
    }
    /// Immutable source-region postings from the same generation.
    ///
    /// # Errors
    /// When the facts cannot be read or fail verification.
    fn body(&self) -> Result<&crate::body::BodyIndex>;

    /// Hash-bound native source facts from this same committed generation.
    ///
    /// # Errors
    /// When the facts cannot be read or fail verification.
    fn source_files(
        &self,
    ) -> Result<&std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>>;

    /// The source facts of one file, when it has any. Stores that can read one
    /// file's facts without the rest override this.
    ///
    /// # Errors
    /// When the facts cannot be read or fail verification.
    fn source_file(
        &self,
        path: &str,
    ) -> Result<Option<graph_search_types::source::SourceFileUnits>> {
        Ok(self.source_files()?.get(path).cloned())
    }

    /// Every file's TypeScript configuration facts, reduced to what project
    /// selection reads (the config, source hash and version).
    ///
    /// # Errors
    /// When the facts cannot be read or fail verification.
    fn typescript_configs(
        &self,
    ) -> Result<std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>>
    {
        Ok(self
            .source_files()?
            .iter()
            .filter_map(|(path, source)| {
                crate::units::typescript_config_facts(source).map(|facts| (path.clone(), facts))
            })
            .collect())
    }

    /// Every package manifest's facts, reduced to what package context reads
    /// (the definition and source hash).
    ///
    /// # Errors
    /// When the facts cannot be read or fail verification.
    fn package_manifests(
        &self,
    ) -> Result<std::collections::BTreeMap<String, graph_search_types::source::SourceFileUnits>>
    {
        Ok(self
            .source_files()?
            .iter()
            .filter_map(|(path, source)| {
                crate::units::package_manifest_facts(source).map(|facts| (path.clone(), facts))
            })
            .collect())
    }

    /// Immutable native metadata retrieval structures owned by this generation.
    ///
    /// # Errors
    /// When the graph cannot be read.
    fn metadata(&self) -> Result<&crate::metadata::MetadataIndex>;

    /// The node with this exact id.
    ///
    /// # Errors
    /// When the read fails.
    fn node_by_id(&self, id: &NodeId) -> Result<Option<Node>>;

    /// Every node `path` owns, its file node included, sorted by id. Stores
    /// that can read one file's nodes without the rest override this.
    ///
    /// # Errors
    /// When the read fails.
    fn nodes_in(&self, path: &str) -> Result<Vec<Node>> {
        Ok(self
            .all_nodes()?
            .into_iter()
            .filter(|node| node.path == path)
            .collect())
    }

    /// The symbols indexed under `key` in `index` (see [`crate::symbols::keys`]),
    /// in no particular order. Stores that index symbol keys override this.
    ///
    /// # Errors
    /// When the read fails.
    fn symbol_rows(
        &self,
        index: crate::symbols::SymbolIndex,
        key: &str,
    ) -> Result<Vec<crate::symbols::SymbolRow>> {
        Ok(self
            .all_nodes()?
            .iter()
            .filter(|node| crate::symbols::indexed(node, index, key))
            .map(crate::symbols::SymbolRow::of)
            .collect())
    }

    /// Every node of one structure (see [`crate::symbols::structure`]), in no
    /// particular order. Stores that index structures override this.
    ///
    /// # Errors
    /// When the read fails.
    fn structure(&self, structure: crate::symbols::Structure) -> Result<Vec<Node>> {
        Ok(self
            .all_nodes()?
            .into_iter()
            .filter(|node| crate::symbols::structure(node) == Some(structure))
            .collect())
    }

    /// Symbols whose `name` or `qualified_name` equals `name`, best first,
    /// at most `k`. An empty `kinds` slice admits every kind.
    ///
    /// # Errors
    /// When the read fails.
    fn find_by_name(&self, name: &str, kinds: &[NodeKind], k: usize) -> Result<Vec<Scored<Node>>>;

    /// Edges incident to `id` along `kinds` in `dir`. An empty `kinds`
    /// slice admits every kind.
    ///
    /// # Errors
    /// When the read fails.
    fn edges_from(&self, id: &NodeId, kinds: &[EdgeKind], dir: Direction) -> Result<Vec<Edge>>;

    /// Bounded adjacency. Implementations must stop before materializing omitted
    /// edges and charge every examined candidate, including filtered kinds.
    ///
    /// # Errors
    /// On read failure, cancellation, or deadline.
    fn edges_bounded(
        &self,
        id: &NodeId,
        kinds: &[EdgeKind],
        dir: Direction,
        budget: &mut crate::work::WorkBudget,
    ) -> Result<Vec<Edge>>;

    /// The subgraph within `hops` of `seeds` along `kinds` in `dir`.
    ///
    /// # Errors
    /// When the read fails.
    fn expand(
        &self,
        seeds: &[NodeId],
        hops: u8,
        kinds: &[EdgeKind],
        dir: Direction,
    ) -> Result<Subgraph>;

    /// Files whose path matches the glob, from the *indexed* set, at most
    /// `k`. This is the resident path set (`SPEC.md` §4.6).
    ///
    /// # Errors
    /// When the read or the glob fails.
    fn files_matching(&self, glob: &str, k: usize) -> Result<Vec<Node>>;

    /// Every node; the bulk read the reconcile's symbol table and `status`
    /// counts are built from. v1-scale by design (`SPEC.md` §19.3).
    ///
    /// # Errors
    /// When the read fails.
    fn all_nodes(&self) -> Result<Vec<Node>>;

    /// Every edge; the bulk read the cross-language match tables are built
    /// from (`SPEC.md` §7.3).
    ///
    /// # Errors
    /// When the read fails.
    fn all_edges(&self) -> Result<Vec<Edge>>;
}
