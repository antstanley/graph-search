# Caller-count decision protocol

Frozen before timing: twelve synthetic workload cells (four shapes, three
node/seed/payload configurations), seven alternating arm-order rounds and twenty
whole native explore calls per cell. Store construction and warm-up are excluded;
the measured loop includes query execution, result equality checking and disposal.
Only elapsed_ms is normalized for response equality. Source and binary hashes
must remain unchanged, and no builds/tests run during measurement.

Required for retaining count-only production traversal: complete normalized
response identity in every paired cell; count and work-report equivalence in the
exhaustive graph/budget regression; no unexplained repeat-stable whole-query median
regression greater than 10%. Small changes within this tolerance are neutral;
latency benefit is reported by workload, not as a general repository speedup.

The independent/shared frontier comparison is a feasibility experiment on already
built dense ordinal adjacency. It excludes conversion from production graph IDs,
neighbor retrieval, budget/cancellation integration, node existence validation and
payload work. It may reject an unconditional shared strategy but cannot establish
that integrating it improves production. Exhaustive three-node comparisons must
verify its complete-graph counts. Production truncation-order semantics remain a
separate constraint; the prototype does not claim equivalence under work caps.

No memory/RSS measurement, source-evidence improvement, graph-answer usefulness,
or model task-success claim follows from these timing measurements.
