//! Named limits that bound every loop and payload (`SPEC.md` §14).
//!
//! The constants here are the defaults and the hard ceilings; per-workspace
//! overrides live in `config.toml` where the spec marks them with a dagger.

/// The largest file considered for parsing or search (1 MiB; config-overridable).
pub const MAX_FILE_BYTES: u64 = 1_048_576;

/// The most files one walk will consider (200 000).
pub const MAX_FILES: usize = 200_000;

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

/// The most snippet lines one item may carry (10).
pub const MAX_SNIPPET_LINES: u32 = 10;

/// The default snippet context for `explore` (2 lines).
pub const DEFAULT_CONTEXT_LINES: u32 = 2;

/// The largest whole JSON payload, in bytes (64 KiB).
pub const MAX_TOTAL_BYTES: usize = 65_536;

/// The deepest traversal honoured; over-ceiling requests are clamped (4).
pub const MAX_HOPS_CEILING: u8 = 4;

/// The default traversal depth for callers/callees (1).
pub const DEFAULT_TRAVERSAL_DEPTH: u8 = 1;

/// The default traversal depth for impact (2).
pub const DEFAULT_IMPACT_DEPTH: u8 = 2;

/// The default seed count for `explore` (8).
pub const EXPLORE_DEFAULT_K: u32 = 8;

/// Bumps when the projection vocabulary or extraction changes, invalidating
/// every stored projection.
pub const SCHEMA_VERSION: u32 = 1;

/// Bumps when extractor behaviour changes in a way that alters output for an
/// unchanged file.
pub const PARSER_VERSION: u32 = 1;

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
