# Recommendation 22 requirement audit

This audit uses the original recommendation in `research/09-native-search-review.md`,
not a promise to integrate every algorithm mentioned by the research. Production
changes are native and add no dependency. Recommendation 22 is accepted on the mapped requirements below; release delivery remains open.

## Requirement mapping

| Original requirement | Implementation / evidence | Scope and conclusion |
|---|---|---|
| Retain graph relationships and use them for questions that need them | Explicit `GraphContext::{None,Calls,Imports,Types,Semantic}` controls, recorded in the query plan; `planner.rs` graph-context tests | Callers select intent without changing lexical seeds. This is explicit intent, not an English question classifier. |
| Share incoming neighborhoods and avoid repeatedly expanding the same cone | Request-local `Neighborhoods` shared by connections and caller summaries; count-only incoming traversal | Each admitted incident prefix is fetched once per endpoint/request. Counts are computed once per distinct output seed. Ordered per-seed reachability remains; no claim that every BFS operation is shared. |
| Explicit visited-node sets in adapters | MemorySnapshot already had one; this increment adds the missing set to GrafeoSnapshot | Both now enqueue an encountered node at most once. The earlier audit overstated Grafeo coverage; this was an actual remaining implementation gap. |
| Select relation types by intent, distinguish calls/imports/types from contains/references | Seven relation kinds × five explicit modes in `graph_context_admits_only_the_requested_relation_family` | Calls/imports/types modes do not admit unrelated kinds; Semantic excludes Contains but permits References. No claim that every reference is a call. |
| Keep edge direction in path answers | Connection traversal stores original edge indices; reconstruction uses `EdgeHit::from_edge` | Traversal may walk either direction, but returned `from`/`to` and kind remain authored graph facts. Tests verify reverse traversal without reversing edges. |
| Consider bidirectional BFS for one shortest path | New dependency-free feasibility experiment below | Conditional integration deferred: possible work gains coexist with overhead, changed equal-length choices, and unproven partial-budget behavior. |
| Investigate multi-source traversal/shared predecessors for many seeds | Dense ordinal connection adjacency, component pruning, predecessor maps; caller-count shared-frontier experiment | Investigation complete. Unconditional shared caller frontier rejected by measured shape dependence. Existing per-seed ordered predecessors preserve the union of selected explanations. |
| Preserve union of evidence for returned connections | `ConnectionGraph::paths` unions original edge indices from every seed; `connect` retains bridge endpoints; source evidence selects relationship sites | Exhaustive small-graph reference comparison checks ties/edge identity. Payload fitting removes edges whose endpoints were omitted and reports truncation; it does not invent complete paths after clipping. |
| Avoid high-degree utility domination | No centrality/PageRank feature added; graph bridges have zero lexical score and explicit relation controls | Degree-based reranking is conditional, not an unimplemented required dependency. |
| Evaluate graph gain with identical lexical candidates and source budgets | 348 faithful-native-JSON trials over 58 tasks, three repeats, graph vs no-graph | Candidate order/metadata/provenance and budgets agree. No labelled-region gain; one partial loss. Additional graph facts are not counted as proven semantic/model gains. |
| Do not infer GraphRAG/summary usefulness from unrelated research | No generated entity graph, model dependency, or repository-summary pipeline added | Adaptive model-task success remains explicitly required by recommendation 30. |

## Visited-set verification argument

Before: Grafeo remembered emitted edge IDs but could enqueue a previously expanded
node when it encountered a new edge returning to that node. A two-node cycle or
self-loop can therefore rescan adjacency. The memory adapter already suppressed
that second enqueue. Default QueryEngine queries use their own budgeted traversal;
the affected method is the direct GraphSnapshot expansion port.

After: `visited` starts with every seed; a newly encountered endpoint enters it
before node lookup. Only a first successful insertion and present node can enter
the next frontier. Since insertion is monotonic and each frontier is consumed
once, no node is expanded twice. Each node is first reached at its minimum ring;
all its matching incident edges are collected on that expansion. Later visits
could contribute no new edge, so skipping them preserves the returned subgraph.
Absent endpoints are not repeatedly looked up. No storage/policy version changes
are needed because returned facts and default query admission decisions do not change.

The new direct-port regression checks 648 combinations across both adapters:
four seed sets, three directions, three relation filters and nine hop depths.
It uses independent distance relaxation, checks complete node/edge ID sets, and
verifies full original edge records. It includes cycles, self-loops, parallel
relation kinds, dangling edges and empty seeds. This checks result semantics;
the monotonic-set argument establishes the eliminated revisit, not a wall-clock test.

## Bidirectional decision

Run `python3 research/scripts/bidirectional_decision.py`. All 132,096 comparisons
over all simple four- and five-node graphs, ordered endpoint pairs and bounded
depths agree with an independent Floyd-Warshall distance oracle. Reconstructed
paths are checked edge by edge and contain no repeated node. However, 3,324
comparisons choose different equally short paths than ordered forward BFS. The
first counterexample is retained in `bidirectional.json`.

The smaller-frontier, full-ring prototype scans 36 adjacency entries versus 304
for a four-hop wide-tree path, but 2,047 versus one for an immediately adjacent
star endpoint. Four-hop chain work is equal. Those are prototype operation counts,
not native budget counters or timings. Full-ring completion, tie rules and initial
frontier selection influence these results; they do not refute all bidirectional
variants. Two longer diagnostic paths are explicitly marked outside production's
four-hop ceiling and are not adoption evidence for the current API.

Retain forward BFS for the current path API. A future adaptive variant would need
whole-query evidence, explicit deterministic tie policy and production admission/
cancellation checks. The prototype excludes store access, budget exhaustion,
node existence checks, ID conversion and source payloads. It is not integrated.

## Evidence retained from earlier increments

- [Request-local neighborhoods](../graph-neighborhoods/README.md): 348 paired
  corpus trials; first-query graph-work decreases in 55 of 58 tasks and never
  increases. Repository aggregate reductions are 12.11–41.99%; no CPU claim.
- [Count-only summaries / multi-source investigation](../caller-counts/README.md):
  51,200 count/work comparisons, 3,360 paired whole queries with identical responses,
  workload-specific median improvements; shared-frontier prototype reaches a
  5.35× chain slowdown and is not integrated.
- [Fixed-candidate graph ablation](../graph-context-native-json/README.md): 348
  source-valid trials, identical candidates/budgets, 36 directed edges and 215
  summaries in the graph arm, no measured complete-region gain and one partial
  coverage loss. These are historical captured sources, not a fresh parser-19
  corpus run or a model-answer evaluation.

Relationship target precision/project resolution, current full-corpus release
evaluation, and actual model task success remain under recommendations 21/30.
This audit does not convert source coverage into graph semantic correctness.

## Final checks

The focused engine run passes 39 tests, including both adapter conformance
suites and the new 648-case regression. Both graph-context integration tests pass.
Strict workspace/all-target Clippy passes; all 157 frozen input hashes match. Only
`crates/engine/src/store.rs` and its integration test changed among production
crate files. The preceding 527-test workspace run predates this two-line change;
no fresh full-workspace pass is claimed here. Final global validation remains part
of delivery. See `checks.json` and retained logs for terminal results.
