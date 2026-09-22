# Native Rust public reexports

Scope: recommendation 21's Rust clause ("model ... aliases, and reexports in
their actual package scope").

## Implemented contract

- A `pub use` leaf whose target is anchored (`crate::`, `self::`, `super::`) and
  whose local name is known is emitted as an `Export` symbol owned by the
  republishing module, with `rust_reexport` (authored target), optional
  `rust_reexport_type_only` and the exact visibility modifier.
- Private `use`, `use ... as _`, globs and unanchored targets publish nothing.
- Anchored path resolution follows a selected reexport from the republishing
  module's own scope, so chains work (`pub use crate::a::X as Y;` then
  `pub use crate::Y;`). Visibility checks reuse the existing module rules.
- Traversal is bounded at 16 hops; cycles or exhaustion stay unresolved with an
  explicit reason. Glob reexports keep `rust_glob_exports_unavailable`.
- Reexport symbols are graph nodes, so unchanged consumers do not require raw
  extraction facts to be re-hydrated before resolution.

## Evidence

- `crates/graph-search/tests/rust_reexports.rs`: direct `pub use`
  (`crate::reexported_target`), a one-hop chain (`crate::hop` via
  `pub use ... as hop`), two resolved call occurrences, and a private-`use`
  counterfactual that leaves the import unresolved after sync.
- Full validation: 564 Rust tests across 50 suite reports pass, strict
  workspace/all-target Clippy and `cargo fmt --all --check` pass. Dependency
  manifests and lockfiles unchanged.

## Limits

Glob reexports remain unavailable by design. Restricted visibility paths
(`pub(in ...)`) keep an explicit unsupported reason rather than a public guess.
Reexport chains are followed through physical module scopes; macro-generated
reexports and configuration-dependent (`cfg`) module trees are not expanded.
Visibility is checked for the reexport node and its eventual target; intervening
`pub(crate)`/`pub(super)` restrictions are modeled with the existing rules.

`provenance.json` records the exact versions and source hashes for this capture.
