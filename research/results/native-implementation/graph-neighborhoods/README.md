# Request-local graph neighborhood reuse

Retained as ranker policy 23. Native request-local caching avoids repeated snapshot
adjacency reads between connection discovery and incoming impact traversals. The
control changes only `QueryEngine::read_edges` back to direct bounded snapshot
reads. No third-party component or dependency change is involved.

## Controlled evidence

348 trials cover 58 tasks, two arms and three repeats across nanus, blogwright and
whatsurvey. All trials completed without errors. Production, binary, sibling-source
and sibling CodeGraph-index fingerprints stayed stable. Every paired task retains
identical delivered source lines, labelled evidence metrics and tool actions.
Each arm also reproduces its own evidence, lines and actions across repeats. The
control matches the preceding context-proximity capture's evidence on all tasks.

| Suite | Tasks | Required files found | Complete regions | Mean region coverage |
|---|---:|---:|---:|---:|
| Established | 34 | 28 | 12 | 48.974474% |
| Fresh routing | 12 | 12 | 10 | 84.722222% |
| Markdown/README | 12 | 10 | 8 | 77.083333% |

Both arms have these figures. This is an evidence protocol, not a model task-success
experiment. All 696 paired tool responses match after normalizing generation IDs,
elapsed statistics and the changed graph-work counter. Three responses (the three
repeats of `whatsurvey.contact-policy.change`) have pre-existing transport-truncated
metadata tails in both arms. Their source prefixes are identical; their surviving
metadata differs only in the generation ID. They are separately recorded in
`transcript-comparison.json`, rather than being parsed as complete JSON metadata.

## Work and identity checks

A separate repeat-zero first-query probe captures 116 raw API responses:

| Repository | Queries | Control graph entries | Cached graph entries | Reduction |
|---|---:|---:|---:|---:|
| nanus | 24 | 3,687 | 2,139 | 41.99% |
| blogwright | 12 | 1,967 | 1,488 | 24.35% |
| whatsurvey | 22 | 6,754 | 5,936 | 12.11% |

55 queries reduce the counter and three leave it unchanged; none increases it.
The counter includes adjacency admissions and connecting-path work. Cached
selection and cloning still consume CPU, and retained adjacency consumes memory.
These figures are not latency, RSS, device-I/O or model-success measurements.

All captured item metadata, primary/excerpt coordinates, truncations, and other
work counters match. All 464 common-node package identities agree, all references
resolve and no orphan package entries remain. The probe does not retain every raw
API field; its equality checks should not be described as whole-API byte equality.
Its response-byte figures are compact Python JSON reconstructions, not wire bytes.
Per-task graph counts and equality checks are in `graph-work-comparison.json`.

## Validation and reproduction

393 workspace tests pass across 31 suites, including common MemoryStore/Grafeo
conformance checks for connection evidence, caller counts and request reset. Native
unit tests cover every subset of four relation kinds across all directions,
self-loops, unresolved edges, partial prefixes, cancellation and empty-key retention.
Strict workspace/all-target Clippy, formatting and whitespace checks pass.

```sh
python3 research/scripts/markdown_context.py --representation graph_neighborhoods --output /tmp/graph-neighborhood-review --repeats 3 --suite research/fixtures/fresh-routing-2026-09-19 --suite research/fixtures/markdown-readme-2026-09-20
```

The capture driver now checks repeat determinism and records all paired evidence
and line deltas automatically. Exact interventions, source and executable hashes
are in `build.json`. Freeze production and drivers during capture. Source snapshots
must match for historical comparisons; never rebuild the sibling CodeGraph indexes.

The [graph reuse audit](../../../GRAPH-REUSE-AUDIT.md) explains the bounded retention
and partial-prefix contract. Recommendation 22 remains open for its broader graph
quality and traversal-economics audit; this change reuses adjacency, not entire
per-seed BFS results.
