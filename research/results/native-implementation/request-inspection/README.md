# Request freshness observation: paired release comparison

Retain the native change. One complete observation supplies the reconciliation
decision and returned context; automatic maintenance still forces a new
post-publication inspection. There is no cross-request freshness cache.

## Comparison

The control restores the exact pre-change `service.rs` archived in the
[phase profile](../retrieval-phases/README.md), verified against that capture's
source hash. All other production sources are identical. No profiling timers are
present in either binary. Both release hosts use temporary native indexes and
read the authorized sibling repositories without modifying their sources or
CodeGraph indexes.

The workload is the same 58 task prompts plus 45 deliberate broad Auto/Metadata/
Body probes. Each arm gets one discarded warmup per query, then three measured
calls: **103 queries, 618 measured calls**. Query order is deterministically
shuffled; arm order alternates. k=8, one graph hop and 16 KiB stay fixed.

All six normalized responses for every query agree exactly. Normalization removes
only `context.generation` and `stats.elapsed_ms`; scores, node order, graph payload,
source evidence, coverage, truncations and every other emitted counter agree.
Warmups agree between arms too. All production/driver/binary/sibling/index/control/
query fingerprint checks pass. Captured artifacts contain no external source text.

| Repository | Queries | Median old / new latency | Median paired improvement |
|---|---|---:|---:|
| nanus | 24 task prompts | 9.656 / 8.196 ms | 14.89% |
| blogwright | 12 task prompts | 6.085 / 4.956 ms | 18.48% |
| whatsurvey | 22 task prompts | 37.244 / 27.163 ms | 26.45% |
| nanus | 15 broad probes | 7.806 / 6.171 ms | 20.86% |
| blogwright | 15 broad probes | 4.960 / 3.845 ms | 24.03% |
| whatsurvey | 15 broad probes | 33.590 / 23.503 ms | 31.01% |

All 103 per-query three-repeat medians are lower with the change. The percentage
column is the median of per-query relative improvements, not a ratio of the
independently aggregated latency columns. Raw repeats and every per-query result
remain in `results.json` and `comparison.json`; `summary.json` retains the ranges.
These are warm wall-clock timings through the Python/JSONL adapter, including
response transport and decoding. They are not cold-I/O or tail-latency guarantees,
concurrent-load measurements, whole-task execution times or model-success scores.
Metadata verification is the benchmark default; strict-content correctness and
read accounting are covered by regression tests, not these latency numbers.

## Validation and reproduction

`python3 research/scripts/inspection_review.py --output NEW_DIRECTORY`

`build.json` records source/control/binary hashes and the disposable directory.
The control source is `../retrieval-phases/service-before.rs`. The archived
profiling driver there is separate from this timer-free paired comparison.

Before measurement, `cargo test --workspace --locked` passed **394 tests in 31
suites**, strict workspace/all-target Clippy passed, and all **28 Python evaluation
tests** passed. The new public-library regression uses exactly one complete walk
and one strict source read across symbol, impact, occurrence and explore requests.
It checks repeated requests, an equal-size restored-mtime edit, insufficient work
and cancellation. Existing tests cover post-reconciliation generation/provenance.
See the [verification certificate](../../../REQUEST-INSPECTION-AUDIT.md).

Under a tight budget the optimized route can complete where the redundant route
failed. That is intentional saved work, not a claim of identical failure behavior
under every budget. Source drift can still race filesystem observations; unchanged
snippet/hash checks remain necessary and no atomic filesystem snapshot is claimed.
