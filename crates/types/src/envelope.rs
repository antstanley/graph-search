//! The JSON envelope: exactly one document on stdout for `--json`
//! (`SPEC.md` §9.1).
//!
//! The envelope is the contract `nanus` parses from `bash`. `results` is
//! generic so each command serializes its own typed payload while the shell
//! of the document stays stable.

use crate::result::{Approximation, Stats, Truncation};
use serde::Serialize;

/// The `--json` document (`SPEC.md` §9.1).
#[derive(Clone, Debug, Serialize)]
pub struct Envelope<R: Serialize> {
    /// Bumped on any change to a result or edge field.
    pub schema_version: u32,
    /// The dotted command path (`search.files`, `search.graph.callers`).
    pub command: String,
    /// The absolute workspace root.
    pub root: String,
    /// The normalized arguments; the answer is reproducible from them.
    pub query: serde_json::Value,
    /// Whether the index was behind the tree at answer time.
    pub stale: bool,
    /// The changed paths, when stale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_paths: Option<Vec<String>>,
    /// The command-specific payload.
    pub results: R,
    /// The edges the command reports; empty for `files`/`text`.
    pub edges: Vec<crate::result::EdgeHit>,
    /// Every cap that fired; empty when nothing was dropped.
    pub truncations: Vec<Truncation>,
    /// The honesty block for `graph`/`explore`; `null` otherwise.
    pub approximation: Option<Approximation>,
    /// Counters.
    pub stats: Stats,
}

impl<R: Serialize> Envelope<R> {
    /// Assembles an envelope for `command` over `root`.
    #[must_use]
    pub fn new(
        schema_version: u32,
        command: impl Into<String>,
        root: impl Into<String>,
        query: serde_json::Value,
        results: R,
    ) -> Self {
        Self {
            schema_version,
            command: command.into(),
            root: root.into(),
            query,
            stale: false,
            stale_paths: None,
            results,
            edges: Vec::new(),
            truncations: Vec::new(),
            approximation: None,
            stats: Stats::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::result::TextHit;

    #[test]
    fn stale_paths_are_omitted_when_fresh() {
        let envelope: Envelope<Vec<TextHit>> = Envelope::new(
            crate::limits::SCHEMA_VERSION,
            "search.text",
            "/w",
            serde_json::json!({ "pattern": "x" }),
            Vec::new(),
        );
        let json = serde_json::to_value(&envelope).unwrap_or_default();
        assert!(json.get("stale_paths").is_none());
        assert_eq!(json.get("schema_version"), Some(&serde_json::json!(1)));
    }
}
