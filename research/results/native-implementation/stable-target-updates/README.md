# Stable-target updates

Recommendation 23 remains open. This increment establishes stable replacement
semantics in both native adapters; it does not narrow projector invalidation or
remove whole-generation graph preparation.

## Contract and implementation

`core::mutation::Mutation` derives a plan from the old graph. An upsert preserves
an existing node when ID, path and kind agree, unless its file is explicitly
removed. Missing or incompatible nodes are deleted. MemoryStore replaces complete
node values; Grafeo updates all properties in its isolated prepared graph, including
removing properties absent from the replacement. This does not promise preservation
of Grafeo's internal IDs across generation reconstruction.

Edges owned by touched source paths are replaced. An edge's explicit path defines
ownership; otherwise the pre-update source node's path does. Untouched owners'
edges survive only when both endpoints survive. This rule also governs dangling
references, and matches the existing complete-projection reconstruction rule.
Explicit removal remains destructive even when a batch recreates the same IDs.

The new adapter regression caught a separate persistence loss: the old sidecar
writer silently excluded pathless dangling records. Generation publication now
writes the complete already-filtered prepared edge set. The public path-filtering
helper keeps its existing behavior. The initial failing regression log is retained.

## Native before/after evidence

The same generated JavaScript caller/leaf fixture used in the
[baseline](../stable-target-baseline/README.md) submits only an identical complete
leaf projection. Both adapters retain the full target value, all three caller-owned
bound occurrences, and the incoming call. Grafeo retains the same state after reopen.

| Adapter | Baseline calls after replacement | Current calls after replacement | Current nodes deleted |
|---|---:|---:|---:|
| MemoryStore | 0 | 1 | 0 |
| Grafeo, including reopen | 0 | 1 | 0 |

The probe now asserts retention instead of requiring the historical loss.
Generated 2/8/32-file chains still perform 2/8/32 upserts for one hash-verified body
edit. Every case equals a clean rebuild in nodes, edges, source facts and occurrence
records. Consumer invalidation deliberately remains conservative in this increment.

The production regression additionally checks removed optional node properties,
replacement of pathless outgoing edges, explicit source ownership different from
endpoint ownership, untouched pathless dangling edges, target deletion, explicit
removal/reinsertion, and exact persistent node/edge equality after reopen. Existing
engine tests cover generation failure/retry, publication and reader lifetimes.

## Validation

- Workspace tests before the pathless-sidecar fix: **528 passed / 39 nonempty suites**.
- Full engine rerun after that fix and the new regression: **42 passed**.
- Strict workspace Clippy across all targets: passed.
- Native release probe: passed, including clean-rebuild comparisons and reopen.
- Strict probe Clippy and source/binary identities: recorded in `checks.json`.

The source capture covers 157 Rust/Cargo/probe inputs. Historical workspace results
are not represented as a fresh complete run of the final sidecar change; the final
engine rerun and strict lint cover that change. No performance or model-task-success
claim is made. No dependency, source-layout, parser or ranker version was changed.

## Remaining work

Stable targets remove the store-level reason to repair every incoming caller.
They do not establish that its binding remains valid after import/export, package,
visibility, ambiguity or file-presence changes. Native dependency tracking must
include absent and ambiguous candidates and conservatively handle missing/legacy
facts. Whole graph preparation, whole occurrence persistence, derived-index rebuilds
and broad raw-fact reads also remain costs. Narrower invalidation needs its own
incremental-versus-clean matrix and measured workload evidence before acceptance.
