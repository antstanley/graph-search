# Native lexical Rust import bindings

## Change and source evidence

`before.txt` reproduces an unresolved call through an extracted `relay` alias.
The scope pass now registers named use leaves as `rust_import` bindings within
their original file/module/block scope. Imports participate from the beginning
of that scope. Selected calls preserve raw spelling, source span and binding
ordinal, while carrying the import target path into native module resolution.
Successful targets have `ExplicitImport` provenance.

Local values and inner declarations take precedence according to existing scope
and initialization rules. Explicit type-only imports are skipped during value
lookup; they remain available for namespace-qualified paths. Qualified module
aliases coexist with local value names. Competing named imports/declarations
remain explicitly ambiguous. This is a supported namespace subset: ordinary
imports whose target namespace is not yet known can conservatively remain
unresolved in mixed-namespace collisions.

Relative calls cannot inherit parent-module imports. Explicit crate/self/super
anchors bypass value lookup and follow native module resolution. Unsupported
glob export sets stop unknown-name lookup before global fallback; known local
bindings still win. Imported receiver calls remain explicitly unsupported.

Expansion is capped before string allocation: 4,096 bytes per expanded target and
8 MiB per file for added target plus import-provenance bytes. Exceeded references
retain their raw name and an explicit `rust_import_expansion_limit` reason. This
bounds this added text, not total parser memory. Scopes without named Rust import
candidates retain their existing binary-search selection path.

## Verification

Seven independent compiler fixtures check hoisting, value shadows, value/type
name coexistence, duplicate import errors, module boundaries and invalid type-only
calls. Run `python3 research/scripts/rust_import_oracle.py /tmp/rust-import-oracle.json`.
No fixture application code executes and no dependencies are fetched.

Public tests verify exact source targets for function/module aliases, block extent,
local shadows, conflicting bindings, namespace coexistence and decoy global names.
Alias edits and target deletion/restoration survive sync/reopen and match complete
clean-build occurrence records. Unit tests check byte/provenance accounting,
UTF-8 boundaries and an oversized authored alias that remains unresolved before
workspace lookup. The final source hashes are in `sources.json`.

Parser policy 17 refreshes persisted binding facts. Source/ranker versions and
dependencies are unchanged. The preceding parser-16 workspace run passed 433 tests;
this increment has its own final focused validation, recorded separately.

## Remaining scope

Recommendations 20/21/23 remain open: unanchored edition-dependent imports,
reexports, glob export sets, general target-derived namespace rules, type-use
binding and receiver/dataflow semantics, framework extraction and more precise
invalidation. This change resolves supported anchored aliases at call sites; it
does not establish complete Rust resolution or repository-wide precision/recall.

Final validation: 18 language unit tests and 33 module/scope/incremental integration
tests pass. Strict workspace/all-target Clippy, formatting, whitespace and source
hash checks pass. Cargo manifests/lockfiles are unchanged. See `checks.json`.
