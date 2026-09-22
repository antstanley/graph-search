//! Explicit boundaries of source enumeration, reading, and parsing.

use crate::result::Truncation;
use serde::{Deserialize, Serialize};

/// Coverage counters are observations within the configured inclusion policy,
/// not counts of every ignored descendant in the filesystem.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coverage {
    /// Source files with ambiguous or unavailable nearest package metadata.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub package_scope_incomplete_files: u64,
    /// Files with hash-bound native source retrieval facts in this generation.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_indexed_files: u64,
    /// Native source regions in this generation.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_units: u64,
    /// Files whose native region cap omitted source.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_unit_truncated_files: u64,
    /// Files in this generation whose Markdown link metadata reached a scan or record cap.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_link_truncated_files: u64,
    /// Files with incomplete parser-owned documentation metadata.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_documentation_truncated_files: u64,
    /// Files with at least one recognized framework script region.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_framework_region_files: u64,
    /// Recognized framework script regions across indexed source files.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_framework_regions: u64,
    /// Recognized framework regions omitted because their dialect is unmodeled.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_framework_unextracted_regions: u64,
    /// Framework files whose region scan reached an adapter bound.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_framework_truncated_files: u64,
    /// Inclusion policy summary and fingerprint of its complete configuration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<SourcePolicy>,
    /// Whether enumeration exhausted the policy-admitted tree without errors.
    /// `None` means no enumeration was performed for this result.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enumeration_complete: Option<bool>,
    /// Entries yielded by the policy-constrained walker, including directories.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub entries_visited: u64,
    /// Files admitted after the size filter.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub admitted_files: u64,
    /// Files skipped because they exceed the configured size ceiling.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub oversized_files: u64,
    /// Enumeration or metadata errors; affected paths cannot imply deletion.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unreadable_entries: u64,
    /// Readable files without a supported language extension.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unsupported_files: u64,
    /// Files whose language was explicitly disabled for extraction.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub disabled_language_files: u64,
    /// Cached paths whose bytes were withheld because a read budget was exceeded.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_budget_exceeded_files: u64,
    /// Source read failures after enumeration.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub source_read_errors: u64,
    /// NUL-containing source files rejected as binary.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub binary_files: u64,
    /// Source files that could not be decoded as UTF-8.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub invalid_utf8_files: u64,
    /// Files whose parser facts were quarantined in the selected generation.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub quarantined_files: u64,
    /// Work or output ceilings that prevented complete enumeration, reading or detail delivery.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncations: Vec<Truncation>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde predicate signature
fn is_zero(value: &u64) -> bool {
    *value == 0
}

/// Human-readable inclusion choices plus the hash of all policy fields.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePolicy {
    /// Whether hidden files may be included.
    pub include_hidden: bool,
    /// Whether ignore files are honored.
    pub respect_ignore: bool,
    /// Directory names excluded before descending.
    pub excludes: Vec<String>,
    /// Per-file source size ceiling.
    pub max_file_bytes: u64,
    /// SHA-256 of the complete policy, including extensions, languages and caps.
    pub fingerprint: String,
}
