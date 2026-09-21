# Native caller counts without retained subgraphs

The default explore path previously constructed a complete incoming subgraph for
each function/method seed, then used only its node count for `ImpactSummary`.
`QueryEngine::incoming_caller_count` now keeps visited/frontier IDs and a count.
It still validates node existence through the same budgeted node reads; those
reads can temporarily clone node payloads. It does not retain the cone's node
payload map or edge map, and it borrows the already read root neighborhood.

## Equivalence argument

Both walks begin by attempting the same root read. They use the same ordered
frontier, the same pre-admitted root neighborhood, incoming Calls elsewhere, and
insert into the visited set before attempting each newly encountered node read.
A missing/denied node is never enqueued. Only successful non-root admissions add
to the count. Induction on BFS rings therefore preserves node-read order, edge
read order, reachable admitted nodes and the work/truncation report. The retained
edge map in the old walk never influenced its frontier or node count.

Direct-caller counting and the existing `max(total, direct)` rule are unchanged.
No source materialization, candidate selection, relationship selection, resolution,
or policy-version behavior changes. The broader `impact` endpoint still returns
its subgraph and uses its existing implementation.

The production regression compares both algorithms on all 512 directed three-node
graphs, including self-loops. Each graph includes an unrelated relation and an
unresolved edge. Four roots (including missing), five hop settings, five node/edge
limit pairs and both cold/pre-populated neighborhood situations exercise 51,200
comparisons. Counts and complete work reports must match. Common store conformance
and full workspace tests exercise the integrated default query.

## Measurements

See the pre-timing [protocol](PROTOCOL.md), `summary.json`, `trials.json`, binary
hashes and build logs. The whole-query comparison invokes actual native explore
with exact-name candidates, context disabled and unchanged graph context. Only
elapsed_ms is normalized before response hashing. Twelve cells cover wide,
separate, overlapping-layer and chain shapes with different seed counts and
symbol payload sizes. This is synthetic execution evidence, not repository task
latency, source-evidence quality or RSS measurement.

A separate research-only bitmask frontier tests multi-source feasibility against
independent BFS over prebuilt ordinal adjacency. All three-node graphs, one to
three roots and zero to five hops must agree (9,216 comparisons). This prototype
has no production partial-budget, cancellation or ID-conversion integration and
supports at most 64 seeds. Its timings cannot establish a production speedup.

No third-party component or production dependency was added. Source/parser/ranker
versions stay 14/19/23 because observable decisions and work admission are intended
to be equivalent. Recommendation 22 remains open for graph-answer usefulness and
the remaining conditional traversal decisions; the global ledger stays 24/6.

## Results and decision

All 168 cell/arm/round records have identical normalized responses within each
workload cell (3,360 timed whole queries). Candidate/control median ratios range
from 0.676 to 0.943: 5.7–32.4% lower median time in these synthetic workloads.
No cell crosses the predeclared regression gate. Retain the count-only traversal.

| Shape | Nodes | Seeds | Candidate/control query time | Shared/independent prototype time | Independent/shared arcs |
|---|---:|---:|---:|---:|---:|
| chain | 256 | 1 | 0.911 | 4.145 | 10/10 |
| chain | 256 | 8 | 0.931 | 1.094 | 17/13 |
| chain | 2048 | 8 | 0.943 | 5.351 | 17/13 |
| layered | 256 | 1 | 0.772 | 1.053 | 255/255 |
| layered | 256 | 8 | 0.826 | 0.236 | 1984/360 |
| layered | 2048 | 8 | 0.824 | 0.277 | 16320/2152 |
| separate | 256 | 1 | 0.677 | 1.166 | 255/255 |
| separate | 256 | 8 | 0.843 | 0.519 | 248/248 |
| separate | 2048 | 8 | 0.852 | 1.105 | 2040/2040 |
| wide | 256 | 1 | 0.676 | 0.944 | 255/255 |
| wide | 256 | 8 | 0.801 | 0.632 | 1984/1984 |
| wide | 2048 | 8 | 0.784 | 0.834 | 16320/16320 |

The shared bitmask prototype is strongly shape-dependent: overlapping layered
workloads benefit, whereas chain workloads reach 5.35 times independent traversal
time. Scanning dense node arrays can dominate sparse frontiers. Equal arc counts
can still have different times because the algorithms allocate and scan different
structures. Do not integrate it unconditionally. A future overlap-aware sparse
frontier would still require production work-limit/cancellation equivalence and
whole-query evidence; this experiment does not authorize that integration.

The workspace command passed 527 tests across 39 nonempty suites. Strict workspace
and probe Clippy, formatting and whitespace checks pass. All 158 recorded source
hashes match after measurement. The isolated `change.patch` is relative to this
increment’s saved source, not the much earlier repository HEAD. Logs and terminal
statuses are recorded in `checks.json`; initial probe lint failures are preserved
and the corrected probe was rebuilt in both arms before timing.
