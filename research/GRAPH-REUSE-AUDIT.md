# Request-local graph reuse audit

Scope: recommendation 22, now accepted after the [requirement audit](results/native-implementation/graph-acceptance/README.md).
Earlier sections below record historical increments and their then-open decisions.
Resolver precision and model task-success evaluation remain separate requirements.

## Contract and reasoning

`QueryEngine` owns one immutable `GraphSnapshot`. It clears `Neighborhoods` at the
start of every public query, including a query supplied with inherited freshness
work. A cache key is the exact `NodeId`; an entry contains the incident prefix
returned by `edges_bounded(id, [], Both, work)`.

For the two native adapters, adjacency entries are charged before direction and
kind filtering. Therefore the first cached read captures the same admitted prefix
that an uncached filtered read would inspect. Selecting by `from == id`, `to == id`,
or both, and then by kind, preserves the native adapter's output order and exact
edge identities. Unresolved edges have no incoming endpoint; self-loops are stored
once in each incident list. Relation filters never turn an unresolved edge into a
resolved one or reverse an edge's recorded direction.

Subsequent phases may use that already admitted prefix even if the edge allowance
has since been exhausted. They do not fetch omitted suffix entries. An exhausted
first read has already recorded a request-level graph truncation, which stays in
the final result. Cached filtering still checks cancellation/deadlines for each
entry. A read failure propagates before cache insertion. Empty and denied reads
allocate no persistent cache record.

Each retained edge was charged once at its endpoint's first read. Thus retained
edge records are bounded by the request's adjacency allowance, and nonempty keys
are bounded by those records. An edge can occur in both endpoint lists and pays
for both. This bound does not include transient selected-vector clones, graph
subgraphs, string allocation overhead, or allocator capacity; it is not an RSS
measurement. Cached selection still scans its retained list. Connecting-path
search keeps its own work charges. `graph_edges_examined` savings measure avoided
adjacency admissions and do not establish CPU speedups.

## Evidence

- Native unit comparison covers all 16 subsets of four relation kinds and all
  three directions over fixture nodes, including self-loops and unresolved edges.
- A partial-prefix test checks stable reuse, persistent truncation, bounded empty
  key retention, cancellation on a hit, and a fresh complete read after reset.
- Public-query conformance checks connection evidence, direct/total caller counts,
  bounded work, and a second query on the same engine; it runs against MemoryStore
  and Grafeo through the common suite.
- All 393 workspace tests pass across 31 suites, including both native adapters.
  Strict workspace/all-target Clippy passes. The [paired corpus capture](results/native-implementation/graph-neighborhoods/README.md)
  completes 348 stable trials with unchanged delivered lines, evidence and actions.
  Across 58 first-query pairs, graph-entry work falls in 55 and never increases;
  repository aggregate reductions range from 12.11% to 41.99%. The candidate is
  retained as ranker 23. No latency, RSS or model-success result is claimed.

## Remaining recommendation 22 audit

Relation-intent selection, explicit traversal visited sets, connection evidence
union and dense shortest-path predecessor work predate this change. Reusing
adjacency does not eliminate independent BFS computation for each seed's incoming
cone. The [same-candidate graph comparison](results/native-implementation/graph-context-native-json/README.md)
now records 348 stable trials through faithful native JSON delivery. Graph context
returns additional relationship/impact information but improves no labelled region
and reduces one task's partial coverage. Independent graph-answer usefulness,
impact-cone economics and conditional multi-source/bidirectional decisions remain
open; the evidence protocol does not grade graph semantics or model answers. This document does not mark recommendation 22 complete.

## Count-only impact summaries and shared-frontier decision

The default explore path now counts incoming callers without retaining a complete
subgraph per seed. Ordered reads, admission and truncation remain equivalent to
the materializing traversal in 51,200 exhaustive graph/budget comparisons. It
retains visited/frontier IDs and temporarily reads node payloads for existence;
it is not allocation-free and does not eliminate independent BFS per seed.

[Paired synthetic evidence](results/native-implementation/caller-counts/README.md)
contains 3,360 whole queries across twelve workloads and seven alternating rounds.
Every normalized response matches; candidate median time is 5.7–32.4% lower by
workload. The current workspace passes 527 tests across 39 nonempty suites. These
measurements do not replace corpus latency or answer-usefulness evaluation.

A separate complete-adjacency shared-frontier prototype passes 9,216 exhaustive
count comparisons but is shape-dependent, reaching 5.35 times independent BFS time
on a chain. Unconditional integration is rejected; any future adaptive variant
needs its own production admission-order and end-to-end evidence. Bidirectional
search and graph-answer usefulness remain separate open decisions under 22/30.

## Acceptance audit and corrected adapter coverage

Direct inspection found that Grafeo’s port-level expansion still used edge-only
suppression; the earlier statement that all adapters had visited-node sets was
too broad. A two-line native fix now aligns it with MemorySnapshot. The new test
checks 648 cyclic-expansion combinations against independent distance relaxation
on both adapters; strict lint and focused engine/graph-intent checks pass.

The [final mapping](results/native-implementation/graph-acceptance/README.md) covers
every sentence of recommendation 22. Bidirectional feasibility passes 132,096
independent distance comparisons but changes 3,324 equal-length path choices and
has mixed work economics; conditional integration is deferred. Multi-source
investigation is likewise complete, with unconditional adoption rejected by the
prior measurements. The fixed-candidate graph ablation satisfies the requested
gain evaluation without claiming an improvement. Model-answer usefulness stays
open under recommendation 30. Recommendation 22 is accepted; ledger 25/5.
