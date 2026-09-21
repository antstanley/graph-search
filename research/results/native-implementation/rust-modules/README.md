# Native Rust module declarations

This increment resolves `mod name;` from Cargo roots and source-backed module
contexts. It does not implement `use` binding resolution, reexports, visibility or
qualified cross-module calls. Recommendations 20/21 remain open.

## Evidence

- `cargo-oracle.json`: 25 disposable offline Cargo metadata cases and a compiled
  Rust module fixture. Tests cover edition-specific discovery, explicit/empty
  target families, hidden files, build scripts, invalid declarations and path
  semantics. Wrong-path files contain `compile_error!` decoys. No dependencies are fetched,
  and no fixture build scripts or application code are executed. Production never invokes Cargo/rustc.
- `sources.json`: hashes of the final production, integration-test and oracle
  sources. Parser policy is 11; source representation is 11. No dependency change.
- Native root tests use independently captured target sets. Public integration
  tests cover custom/nested roots, inline/path attributes, ambiguous filenames,
  build scripts, multiple directory contexts, cycles and orphan files. Mutations
  and reopen are checked against complete clean-build occurrence records.
- `initial-workspace.txt` records a caught regression: rebinding all Rust files
  disturbed the existing selective-update contract. The final implementation
  rebinds external-module consumers, preserving unrelated files. Presence changes include nonstandard
  extensions referenced by explicit path attributes. This failed run
  is diagnostic, not final validation. `pre-inner-guard-sources.json` belongs to
  that earlier code and must not be confused with the final source hashes.

## Resolution argument

The root catalog uses bounded authored manifest facts and walked paths. The
worklist starts from known roots. Ordinary module loading contributes a physical
stem directory (`mod.rs` uses its parent); path attributes contribute the physical
parent. Thus each file can acquire at most two directory contexts. Every new state
is queued once, so diamonds and cycles terminate without recursive expansion.

Each external declaration is evaluated in every reached context. Only unanimous
successful targets resolve. Different targets or success in only some contexts
produce an explicit ambiguity. Unsupported attributes, missing/ambiguous files and
unreachable context remain unresolved. Inline ancestry is bounded to 256 entries;
path normalization cannot escape the workspace and performs no filesystem reads.
An explicit raw-fact tag identifies module declarations independently of generic
`use` spellings. Missing symbol projections cannot fall through to import guessing;
source spans select the exact declaration within its file.

These are source relationships over the supported syntax, not proof that Cargo can
compile the workspace or that a feature/configuration is active. Inner file path
attributes, escaped Rust path strings, unhandled attributes, block-local modules
and unresolved workspace edition inheritance remain explicit limits. No corpus
precision, latency, RSS or answer-success improvement is claimed.

## Final validation

All 418 workspace tests across 32 suites pass on the final parser/source revision
11 code. Strict workspace/all-target Clippy (`--locked -- -D warnings`), formatting
and whitespace checks pass. All 13 source hashes match; dependency manifests and
lockfiles are unchanged. See `checks.json`, `workspace-final.txt` and
`clippy-final.txt`. The earlier 416-test `workspace-pre-provenance.txt` run and its
`pre-provenance-sources.json` snapshot are historical evidence, not the final gate.

Recommendations 20, 21 and 23 remain open. This increment does not establish a
complete lexical/import model or precise module-dependency invalidation.
