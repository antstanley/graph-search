# One freshness observation per request

The [phase profile](results/native-implementation/retrieval-phases/README.md)
identified duplicate complete filesystem inspections in `SearchService::freshness`
and `SearchService::context`. Native `prepare_context` now obtains one complete
context and uses its staleness for the reconciliation decision. The graph,
occurrence, impact and explore entry points pass that context into their result.

## Verification certificate

**Claim.** For an unchanged source tree and selected generation, removing the
second pre-query inspection preserves result provenance and retrieval semantics,
while reducing actual enumeration and strict-verification work. It does not make
a stronger concurrent-filesystem consistency claim.

**Assumptions.** A request owns its work budget. A shared `Index` reference cannot
be concurrently mutated through another thread: the current store API is `Send`,
not `Sync`, and store access remains guarded. The request does not invoke a user
callback between observation and query. An external filesystem writer can still
race either implementation; neither takes an atomic filesystem snapshot.

**Cases.**

- Existing index, no drift: return the complete observed context. Its generation,
  versions, source-unit coverage and staleness come from the same guarded store
  used for that observation. The subsequent query reads the same handle.
- Existing index, drift, explicit/never reconciliation: return the complete stale
  observation, preserving the changed paths and verification method. Independent
  snippet/live-overlay verification remains responsible for delivered source bytes.
- Existing index, drift, automatic reconciliation: discard the first context,
  maintain using the shared work budget, then inspect again. Strict content mode
  retains forced refresh so restored-mtime edits cannot reuse old parsed facts.
- No index: automatic reconciliation builds it then observes the published state;
  explicit/never modes still return `NoIndex`.
- Incomplete walk/hash allowance or other inspection error: propagate the error;
  never manufacture a complete observation. Cancellation is checked before any
  inspection and at existing walker/verifier/query checkpoints.

No observation survives the request. Actual reads remain charged; this change
does not reset the budget when it passes to `QueryEngine`. No parser, wire, source
or scoring representation changes, so the corresponding versions remain unchanged.
Under a tight allowance, a previously rejected query can now complete because it
no longer pays for a redundant inspection; this is an intentional work-saving
behavior change, not equivalence of rejection behavior at every budget.

## Regression coverage

`each_graph_request_uses_one_complete_freshness_observation` exercises all four
entry points twice with exactly one complete enumeration and one source read.
It then restores the timestamp after an equal-size edit and verifies the next
request detects changed bytes without relabeling old graph facts as current.
Incomplete enumeration/read budgets and cancellation remain errors.

`freshness_and_retrieval_share_allowances_without_resetting_spent_work` now uses
one-observation limits and checks just-below-boundary failures. It also verifies
that source capture lets explore materialize the already-verified source without
resetting or overspending the allowance. Existing provenance and end-to-end tests
cover post-reconciliation generation changes, same-metadata strict refresh,
missing indexes, read-only behavior, changed/missing snippet sources and coverage.

The source profile predates this change. Its archived driver and pre-change
service preserve the measured implementation; the current profiling driver uses
`prepare_context` as its inclusive preparation phase. Phase labels across those
captures must not be mistaken for identical nesting.

## Release evidence

The [paired release capture](results/native-implementation/request-inspection/README.md)
compares the exact archived pre-change service with the new implementation, with
all other production sources identical and no timers. All 618 calls across 103
queries preserve normalized responses. Median paired task-query improvements are
14.89% / 18.48% / 26.45% for nanus / blogwright / whatsurvey; all source/index/binary
stability checks pass. These warm metadata-verification timings do not establish
cold, strict-content, concurrent or model-task performance.

Validation: 394 workspace tests in 31 suites, strict all-target workspace Clippy,
and 28 Python evaluation tests pass. Formatting and diff whitespace checks pass.
No dependency or representation-version changes.
