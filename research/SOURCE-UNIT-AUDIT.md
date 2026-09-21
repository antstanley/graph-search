# Source-unit recommendation audit

Recommendation 9 in the native review requires indexed body discovery, several
source representations, separate doc comments, source/package associations,
nonduplicated ancestry, explicit fallback and a controlled fixed-window comparison.
This audit separates those requirements from broader ranking/resolution work.

| Requirement | Current implementation and direct evidence |
|---|---|
| Indexed body channel replaces longest-token scan fallback | `core/body.rs` builds native postings over source regions; `query::seed` queries the analyzed term union/Boolean requirement. `graph-search/tests/body.rs` verifies indexed discovery beyond the old scan prefix, metadata/body competition and source-bound excerpts. |
| Declarations and bodies for supported code | `units::symbol_boundaries` partitions original bytes; `update_owners` selects the smallest physical enclosing declaration. Overlap is limited to the declared bounded-window policy. |
| Configuration, strings/errors and unsupported source without symbols | `units::kind` distinguishes configuration/plain text, and `extract` analyzes all admitted readable source even when no declaration exists. Body/source-unit integration tests cover JSON, text, Markdown, strings and parser-independent retrieval. |
| Separate documentation candidates | Parser-owned Rust/JS/TS comment facts partition into documentation regions; adjacent same-association/style comments separated by whitespace group together. Terms are not copied into declaration fields. Public documentation tests cover all-term queries across Rust line comments, phrase queries, long comments, identity and persistence. |
| Symbol, package, file, language and original source span | A unit has its lexical owner and original span; a documentation descriptor has a distinct documented declaration. Source-file/node records supply file and language. `SourceFileUnits.package` and resolved indexed evidence carry manifest path/hash, ecosystem and authored name. Wire 4 shares repeated identities through `context.packages` and `package_ref`; inline historical evidence remains readable. Package integration tests distinguish same-named packages, virtual workspaces, ambiguous scope and observed unavailable manifests. |
| Avoid repeated ancestor bodies | Primary declaration/document boundaries divide original text; parent/owner references associate it. No file/class/method ancestor copy is indexed. Bounded long-region overlap remains intentional and separately documented. |
| Explicit missing-parser fallback | Unrecognized source remains plain-text evidence; coverage reports unsupported/disabled extraction and quarantine at generation/query level. Supported Markdown/configuration representations do not require declaration extraction. This does not claim complete embedded-framework/parser-construct coverage, which remains under 21. |
| Package association lifecycle and validation | Package creation/invalid edits/removal/relocation match clean rebuilds. Source-size-excluded manifest presence participates in freshness and blocks outer inheritance. Both stores reject retained source associations whose manifest is removed or changes identity, before visible mutation. |
| Fixed-window comparison at equal candidate/context budgets | The source-version-8 `code-context` capture compares structure against current 80-line/8-line-overlap windows, and `documentation-context` isolates comments. Each has 348 trials, frozen task denominators and explicit mixed results. The refreshed source-version-9 `code-context-packages` capture adds 348 trials with the same snapshots/oracles. Stability and repeated source-evidence checks pass; five partial-context regressions remain explicitly recorded under 11/12/30. |

Manifest discovery is a declared Cargo/package.json subset over the observed
inclusion universe, using syntax decoders already in the library. It does not
install dependencies, execute configuration or resolve package imports. Full
workspace membership, language module resolution, reexports, aliases and embedded
framework regions remain recommendation 21. Candidate/context quality and broader
release evaluation remain 11/12/30; mixed retrieval results are not erased by
source-identity correctness.

**Status:** recommendation 9 complete within the declared native representation
contracts. Final workspace verification, including shared-identity transport checks, passed 376 tests; strict workspace/all-target
Clippy and formatting passed. The [current controlled comparison](results/native-implementation/code-context-packages/README.md)
exercises the representation requirement and preserves mixed quality results.
This closes source representation and association work, not the separate retrieval
quality, package-resolution or full-release gates.
