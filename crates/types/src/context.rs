//! Provenance attached to library results. A filesystem check is per-file,
//! never a claim that the live workspace was atomically snapshotted.

use crate::result::Staleness;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How filesystem freshness was checked for this result.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessMethod {
    /// No filesystem freshness check was performed.
    #[default]
    Unchecked,
    /// File inclusion, size, modification time, and extraction versions only.
    Metadata,
    /// Content hashes as well as metadata; checks occur independently per file.
    Content,
    /// Evidence was read directly from the live tree, without a graph snapshot.
    Live,
}

/// Relationship between an indexed source identity and observed bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceVerification {
    /// An indexed fingerprint is known, but source was not read.
    #[default]
    NotRead,
    /// Observed bytes match the indexed source fingerprint.
    Verified,
    /// Observed bytes differ; indexed-symbol snippets must be withheld.
    Mismatch,
    /// Live evidence with no indexed source fingerprint.
    Live,
    /// Source could not be read or decoded.
    Unavailable,
    /// Source bytes contain a binary NUL marker.
    Binary,
    /// Source bytes are not valid UTF-8.
    InvalidEncoding,
    /// The source read would exceed its byte budget.
    BudgetExceeded,
}

/// Fingerprints and verification status for one result source path.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    /// Hash of the source bytes used to produce the selected graph generation.
    pub indexed_hash: Option<String>,
    /// Hash of the exact bytes read for this request, when read successfully.
    pub observed_hash: Option<String>,
    /// Whether the source is safe to pair with indexed coordinates.
    pub verification: SourceVerification,
}

/// The generation, freshness, and source boundary of a library result.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResultContext {
    /// Actual stored representation revisions, absent without an indexed generation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_versions: Option<crate::manifest::IndexVersions>,
    /// Query implementation revisions; separate from the selected persisted facts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_versions: Option<RuntimeVersions>,
    /// Observed source coverage under the configured inclusion policy.
    #[serde(default)]
    pub coverage: crate::coverage::Coverage,
    /// Identity of the selected immutable graph generation, when applicable.
    pub generation: Option<String>,
    /// Verification performed against the live tree.
    pub freshness: FreshnessMethod,
    /// Host reconciliation policy (`before_query`, `never`, or `explicit`).
    pub reconciliation: Option<String>,
    /// Observed drift from the selected generation, including source mismatches.
    pub staleness: Staleness,
    /// Identity of each returned source path; omitted paths were not materialized.
    pub sources: BTreeMap<String, SourceIdentity>,
    /// Repeated package identities referenced by source evidence in this result.
    /// Keys have result-local scope and remain stable when results are trimmed.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub packages: BTreeMap<String, crate::package::PackageIdentity>,
}

impl ResultContext {
    /// Remove unreferenced package entries after result trimming.
    /// Retained keys are not renumbered, so evidence references remain valid.
    pub fn retain_packages<'a>(&mut self, references: impl IntoIterator<Item = &'a str>) {
        let retained: std::collections::BTreeSet<_> = references.into_iter().collect();
        self.packages
            .retain(|key, _| retained.contains(key.as_str()));
    }

    /// Context for evidence read directly from a policy-constrained live scan.
    #[must_use]
    pub fn live(coverage: crate::coverage::Coverage) -> Self {
        Self {
            coverage,
            freshness: FreshnessMethod::Live,
            runtime_versions: Some(RuntimeVersions::current()),
            ..Self::default()
        }
    }
}

/// Current query policy. Historical results lacking this field remain unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeVersions {
    /// Native query analyzer policy.
    pub analyzer: u32,
    /// Unicode classification/lowercase tables.
    pub unicode: (u8, u8, u8),
    /// Live source partition/window policy.
    pub chunker: u32,
    /// Scoring, routing and context selection contract.
    pub ranker: u32,
    /// Result-wire contract, independent of stored representation.
    pub wire: u32,
}

impl RuntimeVersions {
    /// The executing library's revisions, not an assertion about indexed facts.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            analyzer: crate::limits::ANALYZER_VERSION,
            unicode: crate::limits::ANALYZER_UNICODE_VERSION,
            chunker: crate::limits::CHUNKER_VERSION,
            ranker: crate::limits::RANKER_VERSION,
            wire: crate::limits::RESULT_SCHEMA_VERSION,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn historical_context_does_not_claim_current_runtime_or_index_versions() {
        let old = serde_json::json!({
            "coverage": {}, "generation": "old-generation", "freshness": "unchecked",
            "reconciliation": "never", "staleness": { "changed": 0, "changed_paths": [] },
            "sources": {}
        });
        let context: ResultContext = serde_json::from_value(old).expect("legacy context");
        assert!(context.indexed_versions.is_none());
        assert!(context.runtime_versions.is_none());
        let current = ResultContext::live(crate::coverage::Coverage::default());
        assert_eq!(current.runtime_versions, Some(RuntimeVersions::current()));
        assert!(current.indexed_versions.is_none());
    }
}
