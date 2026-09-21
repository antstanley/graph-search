# Native anchored Rust paths

## Reproduction and behavior

The two-package public regression initially left `crate::send` unresolved
(`before.txt`). Native path resolution now follows discovered Cargo roots and
source module links for `crate`, `self` and repeated leading `super`. Direct
anchored use leaves resolve to the actual source symbol with explicit-import
provenance. Calls and type uses retain qualified provenance. Raw call spelling
is retained while syntax nodes normalize whitespace/comments in ordinary paths.

The module graph separates member declarations from module interiors. A module
name maps to its inline scope or the resolved external source file. All reachable
root contexts are retained; every possible context must find a unique, visible
member and all terminal targets must agree. Missing context, conflicting members,
missing files and different terminal targets stay unresolved, without global-name
fallback. Raw identifier prefixes are normalized for member lookup.

Basic module-item visibility is checked at each path segment: public, crate,
parent and private. Arbitrary authored restrictions stay explicitly unsupported.
Private ancestry proofs require a unique parent chain, so shared ambiguous physical
contexts may conservatively remain unresolved. This is not a complete logical
crate-instance model or compiler privacy checker.

## Bounds and update correctness

Root-context propagation marks pairs before enqueueing, deduplicating diamonds
and cycles. At most 65,536 scope/root pairs are admitted; reaching the cap makes
all anchored resolution unavailable with `rust_module_context_limit`. Path lookup
admits at most 256 components after its anchor. A focused core test checks cycles,
duplicate edges and exact/over context limits. Source graph storage remains
proportional to the existing nodes/links; this is not a whole-parser memory bound.

Rust source or file-presence changes conservatively rebind external-module,
anchored-path and structured-use consumers from cached facts. Package-boundary
changes retain their existing broad invalidation. Integration tests create,
remove, duplicate and change target visibility, then compare complete occurrence
records after sync/reopen against clean reindex. Unrelated source parsing remains
on the existing selective path; recommendation 23's narrower dependency indexes
remain unfinished.

## Evidence and limits

The public fixtures separate two packages' same-named functions, check local and
parent paths, private sibling rejection, parent/crate visibility, shared-root
ambiguity and dynamic file changes. `compiler-oracle.json` independently compiles
five disposable fixtures without running application code. Reproduce with
`python3 research/scripts/rust_path_oracle.py /tmp/rust-path-oracle.json`.

The focused run passed 164 core tests and 29 public module/scope/incremental tests.
It precedes the final whitespace-path assertion and type-only target restriction;
final full-workspace validation is recorded separately when complete. `sources.json`
is the frozen final source set. Parser policy is 16; source representation remains
11. No dependency changes were made.

Still open under recommendations 20/21: lexical import aliases at call sites,
unanchored edition-dependent import paths, reexports and glob export sets, full
namespace/receiver rules, general restricted visibility and framework regions.
Associated-item paths are unsupported here. This increment resolves anchored
source paths; it does not establish repository-wide target precision or recall.

Final strict workspace/all-target Clippy passes (`clippy-final.txt`). Formatting,
whitespace and source-hash checks pass; dependencies remain unchanged. The workspace run launched for this increment completed: 433 tests across 32
suites pass (`workspace.txt`). The subsequent parser-17 import-binding change is
validated separately; this is not its final full-workspace gate.
