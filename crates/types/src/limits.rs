//! Named limits that bound every loop and payload (`SPEC.md` §14).
//!
//! The constants here are the defaults and the hard ceilings; per-workspace
//! overrides live in `config.toml` where the spec marks them with a dagger.

/// The largest file considered for parsing or search (1 MiB; config-overridable).
pub const MAX_FILE_BYTES: u64 = 1_048_576;

/// The most files one walk will consider (200 000).
pub const MAX_FILES: usize = 200_000;

/// The most policy-visible filesystem entries one walk yields.
pub const MAX_WALK_ENTRIES: usize = 1_000_000;

/// The most nodes one file may produce before it is quarantined (50 000).
pub const MAX_NODES_PER_FILE: usize = 50_000;

/// The most edges one file may produce before it is quarantined (200 000).
pub const MAX_EDGES_PER_FILE: usize = 200_000;

/// The default result cap for `files` (100).
pub const FILES_DEFAULT_LIMIT: u32 = 100;

/// The hard ceiling a caller may request for `files` (1 000).
pub const FILES_LIMIT_CEILING: u32 = 1_000;

/// The default result cap for `text` (250).
pub const TEXT_DEFAULT_LIMIT: u32 = 250;

/// The hard ceiling a caller may request for `text` (2 000).
pub const TEXT_LIMIT_CEILING: u32 = 2_000;

/// The default result cap for `graph` and `explore` (50).
pub const GRAPH_DEFAULT_LIMIT: u32 = 50;

/// The hard ceiling a caller may request for `graph` and `explore` (500).
pub const GRAPH_LIMIT_CEILING: u32 = 500;

/// The longest echoed match line, in characters (400).
pub const MAX_MATCH_LINE: usize = 400;

/// The longest stored signature, in characters (200).
pub const MAX_SIGNATURE_CHARS: usize = 200;

/// The most lines in the primary excerpt (10).
pub const MAX_SNIPPET_LINES: u32 = 10;

/// Largest source interval considered for implementation/body expansion.
pub const MAX_EVIDENCE_INTERVAL_LINES: u32 = 80;
/// Additional intervals retained across one explore response.
pub const MAX_EVIDENCE_INTERVALS: usize = 64;

/// The default snippet context for `explore` (2 lines).
pub const DEFAULT_CONTEXT_LINES: u32 = 2;

/// The largest whole JSON payload, in bytes (64 KiB).
pub const MAX_TOTAL_BYTES: usize = 65_536;

/// Default native metadata candidate admission budget per request.
pub const METADATA_CANDIDATES_DEFAULT: usize = 10_000;
/// Hard ceiling for native metadata candidate admissions.
pub const METADATA_CANDIDATES_CEILING: usize = 100_000;
/// Default native lexical posting examination budget per request.
pub const LEXICAL_POSTINGS_DEFAULT: usize = 200_000;
/// Hard ceiling for native lexical posting examinations.
pub const LEXICAL_POSTINGS_CEILING: usize = 1_000_000;

/// Default distinct graph node admission budget per request.
pub const GRAPH_WORK_NODES_DEFAULT: usize = 10_000;
/// Hard ceiling for distinct graph node admissions per request.
pub const GRAPH_WORK_NODES_CEILING: usize = 100_000;
/// Default adjacency-entry examination budget per request.
pub const GRAPH_WORK_EDGES_DEFAULT: usize = 50_000;
/// Hard ceiling for adjacency-entry examinations per request.
pub const GRAPH_WORK_EDGES_CEILING: usize = 500_000;

/// The deepest traversal honoured; over-ceiling requests are clamped (4).
pub const MAX_HOPS_CEILING: u8 = 4;

/// The default traversal depth for callers/callees (1).
pub const DEFAULT_TRAVERSAL_DEPTH: u8 = 1;

/// The default traversal depth for impact (2).
pub const DEFAULT_IMPACT_DEPTH: u8 = 2;

/// Minimum metadata candidate pool before final context selection.
pub const METADATA_POOL_MIN: usize = 64;
/// Metadata pool depth relative to the requested context count.
pub const METADATA_POOL_MULTIPLIER: usize = 4;

/// Native token/whole-lexeme boundary and lowercase policy revision.
pub const ANALYZER_VERSION: u32 = 2;
/// Unicode tables used by Rust's character classification and lowercase mapping.
pub const ANALYZER_UNICODE_VERSION: (u8, u8, u8) = std::char::UNICODE_VERSION;

/// Native source region boundary/window policy, independent of its serialized fields.
/// v10: `pub use` reexport members no longer partition source regions.
pub const CHUNKER_VERSION: u32 = 10;
/// Metadata/body scoring, automatic routing and context-selection policy revision.
/// Query-time only: changing this does not invalidate persisted extraction facts.
pub const RANKER_VERSION: u32 = 23;
/// Source-owned reference and binding representation.
pub const OCCURRENCE_VERSION: u32 = 1;
/// Source retrieval serialization revision, independent of parser and chunker policy.
pub const SOURCE_INDEX_VERSION: u32 = 15;
/// Maximum authored links retained per original Markdown block.
pub const MAX_MARKDOWN_LINKS: usize = 256;
/// Maximum distinct authored links retained across one source file.
pub const MAX_MARKDOWN_LINKS_PER_FILE: usize = 4_096;
/// Maximum byte inspections while extracting links from one Markdown block.
pub const MAX_MARKDOWN_LINK_WORK: usize = 1_048_576;
/// Maximum native source retrieval regions per file.
pub const MAX_SOURCE_UNITS_PER_FILE: usize = 8_192;
/// Maximum display lines in one source retrieval region.
pub const SOURCE_UNIT_LINES: usize = 80;
/// Overlap between consecutive windows within the same declaration region.
pub const SOURCE_UNIT_OVERLAP: usize = 8;

/// The default seed count for `explore` (8).
pub const EXPLORE_DEFAULT_K: u32 = 8;

/// Bumps when the projection vocabulary or extraction changes, invalidating
/// every stored projection.
pub const SCHEMA_VERSION: u32 = 3;

/// Wire result schema, independent of stored projection format.
pub const RESULT_SCHEMA_VERSION: u32 = 4;

/// Bumps when extractor behaviour changes in a way that alters output for an
/// unchanged file.
/// v22: Rust `type_uses` cover parameter, return, local and nested wrapper
/// types, not only direct field types; dangling display names are canonicalised.
/// v23: generic type parameters and `Self` are excluded from Rust `type_uses`;
/// dangling names are bounded and hash-disambiguated to keep edge identities 1:1.
pub const PARSER_VERSION: u32 = 23;

/// Default independent ceiling on edges delivered by one graph/explore query.
pub const RETURNED_EDGES_DEFAULT: usize = 1000;
/// Hard ceiling on delivered edges, separate from adjacency examination work.
pub const RETURNED_EDGES_CEILING: usize = 10_000;

/// Default maximum dictionary entries expanded by a prefix request.
pub const DICTIONARY_ENTRIES_DEFAULT: usize = 256;
/// Hard ceiling on dictionary entries expanded by a prefix request.
pub const DICTIONARY_ENTRIES_CEILING: usize = 4096;

/// Largest explore query before analysis.
pub const MAX_QUERY_BYTES: usize = 8192;
/// Maximum distinct analyzed terms; excess input is rejected, never silently weakened.
pub const MAX_QUERY_TERMS: usize = 128;

/// Maximum separately retained parser documentation-comment occurrences per file.
pub const MAX_DOC_COMMENTS_PER_FILE: usize = 8_192;
/// Maximum recognized framework script regions retained for one file.
pub const MAX_EMBEDDED_REGIONS_PER_FILE: usize = 64;
/// Maximum authored bytes inspected while scanning one framework region.
pub const MAX_EMBEDDED_REGION_BYTES: usize = 4_194_304;

/// Maximum manifest bytes passed to the existing syntax decoders.
pub const MAX_PACKAGE_MANIFEST_BYTES: usize = 262_144;
/// Maximum authored package-name bytes retained in metadata.
pub const MAX_PACKAGE_NAME_BYTES: usize = 512;

/// Maximum explicit Cargo targets retained from one manifest.
pub const MAX_CARGO_TARGETS: usize = 256;
/// Maximum authored feature gates on one target.
pub const MAX_CARGO_TARGET_FEATURES: usize = 64;
/// Maximum UTF-8 bytes in an authored target path.
pub const MAX_CARGO_TARGET_PATH_BYTES: usize = 4096;

/// Shared budget for native Node workspace ancestry, membership and override decisions.
pub const MAX_NODE_WORKSPACE_WORK: usize = 1_000_000;

/// Maximum nested conditional objects for proving one invariant package-map target.
pub const MAX_NODE_CONDITION_DEPTH: usize = 16;

/// Maximum JSON/JSONC source bytes admitted to configuration decoding.
pub const MAX_TYPESCRIPT_CONFIG_BYTES: usize = 262_144;

/// Maximum scalar/container values retained in one TypeScript configuration projection.
pub const MAX_TYPESCRIPT_CONFIG_VALUES: usize = 4096;
/// Maximum nested compiler-option/container depth retained in configuration facts.
pub const MAX_TYPESCRIPT_CONFIG_DEPTH: usize = 32;
/// Maximum total UTF-8 string/key bytes retained in configuration facts.
pub const MAX_TYPESCRIPT_CONFIG_TEXT_BYTES: usize = 131_072;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_sit_below_their_ceilings() {
        const { assert!(FILES_DEFAULT_LIMIT < FILES_LIMIT_CEILING) };
        const { assert!(TEXT_DEFAULT_LIMIT < TEXT_LIMIT_CEILING) };
        const { assert!(GRAPH_DEFAULT_LIMIT < GRAPH_LIMIT_CEILING) };
        const { assert!(DEFAULT_TRAVERSAL_DEPTH <= MAX_HOPS_CEILING) };
        const { assert!(DEFAULT_IMPACT_DEPTH <= MAX_HOPS_CEILING) };
        const { assert!(DEFAULT_CONTEXT_LINES <= MAX_SNIPPET_LINES) };
    }
}
