# Stable-target update baseline

Recommendation 23 remains open. This native reproduction establishes the coupled
store/projector contract that must change before narrowing invalidation. It adds
no production behavior and makes no timing or memory claim.

## Identical target replacement

The probe indexes two generated JavaScript files: one exports `leaf`, the other
imports and calls it. It then submits only the target file's **identical complete
projection**, including its source and occurrence facts. No source file is edited.

Both MemoryStore and Grafeo remove the incoming Calls edge. The target node and
all caller-owned occurrence records bound to it remain byte-for-byte equal as
serialized values. Grafeo persists that state through close/reopen. The reproduction
asserts a real resolved caller and bound occurrence before the operation, so an
unresolved fixture cannot make this test vacuously pass.

This follows the current replacement semantics: node deletion removes incident
edges even if the same stable target is immediately recreated. Occurrence storage
is already source-owned and retains bindings to IDs that still exist. The probe
does not establish a new transaction-atomicity failure or claim that arbitrary
direct batches previously promised to preserve incoming edges. It demonstrates
why existing projection replacement prevents safe consumer-repair elimination.

## Real body edit and repair amplification

Separate generated chains contain 2, 8 and 32 files, with each function importing
and calling the preceding one. A single body literal changes from `return 1` to
`return 10` in the first file. Source hashes independently confirm exactly one
changed file. Capturing actual native GraphStore batches shows **2, 8 and 32 file
upserts**, respectively. `SyncReport.modified` also contains the repaired consumers;
it must not be treated as a physical-source-edit counter in this experiment.

All three sync results match separate clean rebuilds in full nodes, edges,
source units and occurrence records. This is conservative repair amplification,
not evidence that the public sync path currently returns an incorrect graph.
Only generated fixtures and temporary stores are used; no sibling sources or
CodeGraph indexes are touched.

An initial assertion expected `modified` to contain only the physical edit and
failed on the two-file chain. The retained failure log documents that assumption.
The corrected probe hashes actual source bytes separately, captures reported and
applied paths, and retains full clean-rebuild equivalence assertions.

## Consequences for the native implementation

The next change must preserve stable targets and their incoming source-owned
adjacency in both adapters before the projector stops repairing callers. Preserving
edges alone is insufficient: outgoing facts owned by an edited file must be replaced,
deleted targets must invalidate incoming bindings, and namespace/import/export
changes can invalidate callers even when a target ID survives. Conversely, simply
shrinking `extend_dependents` now would lose edges demonstrated here.

Edge ownership must remain explicit across generated cross-file edges, source-site
paths and pathless structural edges. Existing occurrence ownership is useful but
does not by itself define ownership for every aggregate edge. Reverse dependency
indexes must cover absent and ambiguous candidates, not just current resolved
targets, and publish with the graph/facts in one generation. Conservative fallback
for legacy/missing dependency facts remains necessary during migration.

This baseline is retained for the before/after contract tests: identical target
replacement should retain the caller edge after stable updates are implemented;
body-only sync should avoid transitive file upserts while matching clean rebuilds.
Signature/export/import edits, rename, deletion, duplicate-name addition/removal,
missing-cache repair, reopen and publication failure still require their own matrix.

## Reproduction

```sh
cargo run --release --offline --locked --manifest-path research/harness/Cargo.toml --bin stable_target_probe
```

The baseline intentionally asserts the current edge loss and repair amplification.
Convert those assertions to the new contract when implementing stable updates;
do not keep this diagnostic as a test that requires the old behavior forever.
Final source hashes, binary identity, build/lint results and terminal handles are
recorded in `checks.json`. No dependency or policy-version change was made.
