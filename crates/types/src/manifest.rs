//! The store's self-fingerprint: one entry per walked file (`SPEC.md` §6.3).
//!
//! The manifest is what makes reconcile incremental. It is committed **last**,
//! after the batch that produced it, so an interrupted run leaves the old
//! manifest and the next run recomputes the same delta from content hashes.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One file's entry in the manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Size in bytes, from the last walk that saw the file.
    pub size: u64,
    /// Modification time in nanoseconds since the epoch.
    pub mtime_ns: u64,
    /// Hex SHA-256 of the content.
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
    pub extraction: Option<crate::extraction::Extraction>,
}

impl FileEntry {
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
    /// An empty manifest for the given parser and schema versions.
    #[must_use]
    pub fn new(parser_version: u32, schema_version: u32) -> Self {
        Self {
            schema_version,
            parser_version,
            indexed_at_ms: 0,
            entries: BTreeMap::new(),
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
