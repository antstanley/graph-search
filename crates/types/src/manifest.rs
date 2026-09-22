//! The store's self-fingerprint: one entry per walked file (`SPEC.md` §6.3).
//!
//! The manifest is what makes reconcile incremental. It is committed **last**,
//! after the batch that produced it, so an interrupted run leaves the old
//! manifest and the next run recomputes the same delta from content hashes.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One file's entry in the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Size in bytes, from the last walk that saw the file.
    pub size: u64,
    /// Modification time in nanoseconds since the epoch.
    pub mtime_ns: u64,
    /// Hex BLAKE3 of the content.
    pub content_hash: String,
    /// The `parser_version` that produced the stored projection.
    pub parser_version: u32,
    /// The `schema_version` the stored projection was built for.
    pub schema_version: u32,
    /// Why the file was quarantined, when extraction failed; its `file` node
    /// is still indexed, but it has no symbols (`SPEC.md` §6.4).
    pub quarantine: Option<String>,
    /// Raw parser facts used to rebind unchanged files without parsing them.
    /// Absent in older manifests and for quarantined parse failures.
    #[serde(default)]
    pub extraction: Option<crate::extraction::SharedExtraction>,
}

impl FileEntry {
    /// Copies only the small per-file fingerprint.
    #[must_use]
    pub fn header(&self) -> Self {
        Self {
            extraction: None,
            ..self.clone()
        }
    }

    /// Whether the stored projection was produced by the current parser and
    /// schema. When it was not, the file must be re-parsed regardless of its
    /// content hash (`SPEC.md` §6.3, `unchanged-global`).
    #[must_use]
    pub const fn matches_versions(&self, parser_version: u32, schema_version: u32) -> bool {
        self.parser_version == parser_version && self.schema_version == schema_version
    }
}

/// A file the reconcile detected as moved: same content hash, different path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rename {
    /// The path that disappeared.
    pub from: String,
    /// The path that appeared with the same content.
    pub to: String,
}

/// The manifest: the fingerprint of the last completed projection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Observed manifest boundaries, including bodies excluded by the size ceiling.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub package_boundaries: BTreeSet<String>,
    /// Source-owned reference occurrence representation.
    #[serde(default)]
    pub occurrence_version: u32,
    /// Region partition/window policy used for persisted retrieval facts.
    #[serde(default)]
    pub chunker_version: u32,
    /// Tokenization policy used for persisted retrieval facts.
    #[serde(default)]
    pub analyzer_version: u32,
    /// Unicode table identity, so toolchain changes cannot silently reuse old analysis.
    #[serde(default)]
    pub analyzer_unicode_version: (u8, u8, u8),
    /// Native source retrieval representation used by this generation.
    #[serde(default)]
    pub source_version: u32,
    /// Inclusion/extraction policy that produced this generation.
    #[serde(default)]
    pub policy_fingerprint: Option<String>,
    /// The schema the stored projections were built for.
    pub schema_version: u32,
    /// The parser that produced the stored projections.
    pub parser_version: u32,
    /// When the last complete index finished, in milliseconds since the epoch.
    pub indexed_at_ms: u64,
    /// One entry per walked file, keyed by workspace-relative path. A
    /// `BTreeMap` so serialized manifests are byte-stable.
    pub entries: BTreeMap<String, FileEntry>,
}

impl Manifest {
    /// Copies freshness/version metadata without cloning raw extraction facts.
    /// This view is for query checks, not for incremental rebinding or publication.
    #[must_use]
    pub fn header(&self) -> Self {
        Self {
            package_boundaries: self.package_boundaries.clone(),
            occurrence_version: self.occurrence_version,
            chunker_version: self.chunker_version,
            analyzer_version: self.analyzer_version,
            analyzer_unicode_version: self.analyzer_unicode_version,
            source_version: self.source_version,
            policy_fingerprint: self.policy_fingerprint.clone(),
            schema_version: self.schema_version,
            parser_version: self.parser_version,
            indexed_at_ms: self.indexed_at_ms,
            entries: self
                .entries
                .iter()
                .map(|(path, entry)| (path.clone(), entry.header()))
                .collect(),
        }
    }

    /// An empty manifest for the given parser and schema versions.
    #[must_use]
    pub fn new(parser_version: u32, schema_version: u32) -> Self {
        Self {
            package_boundaries: BTreeSet::new(),
            occurrence_version: crate::limits::OCCURRENCE_VERSION,
            chunker_version: crate::limits::CHUNKER_VERSION,
            source_version: crate::limits::SOURCE_INDEX_VERSION,
            analyzer_version: crate::limits::ANALYZER_VERSION,
            analyzer_unicode_version: crate::limits::ANALYZER_UNICODE_VERSION,
            policy_fingerprint: None,
            schema_version,
            parser_version,
            indexed_at_ms: 0,
            entries: BTreeMap::new(),
        }
    }

    /// Persisted representation identities, without query-only ranking policy.
    #[must_use]
    pub const fn versions(&self) -> IndexVersions {
        IndexVersions {
            occurrences: self.occurrence_version,
            parser: self.parser_version,
            schema: self.schema_version,
            source: self.source_version,
            analyzer: self.analyzer_version,
            unicode: self.analyzer_unicode_version,
            chunker: self.chunker_version,
        }
    }

    /// The entry for `path`, when the file was walked before.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&FileEntry> {
        self.entries.get(path)
    }

    /// How many files the manifest knows about.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the manifest records no files.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Identities of facts in an indexed generation. Zero denotes an unknown legacy revision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexVersions {
    /// Source-owned reference and binding facts.
    #[serde(default)]
    pub occurrences: u32,
    /// Parser extraction contract.
    pub parser: u32,
    /// Graph projection vocabulary.
    pub schema: u32,
    /// Serialized source fields.
    pub source: u32,
    /// Term/whole-lexeme analysis policy.
    pub analyzer: u32,
    /// Unicode tables used by analysis.
    pub unicode: (u8, u8, u8),
    /// Region partition and overlap policy.
    pub chunker: u32,
}

impl IndexVersions {
    /// Whether existing source facts have the current retrieval representation.
    #[must_use]
    pub fn retrieval_is_current(self) -> bool {
        self.source == crate::limits::SOURCE_INDEX_VERSION
            && self.analyzer == crate::limits::ANALYZER_VERSION
            && self.unicode == crate::limits::ANALYZER_UNICODE_VERSION
            && self.chunker == crate::limits::CHUNKER_VERSION
    }
}
