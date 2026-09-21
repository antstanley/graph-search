# Current native retrieval phase profile

Decision: retain bounded exhaustive posting evaluation at this measured local
scale. Recommendation 26's conditional profiling gate is complete; WAND,
MaxScore and block-max bounds remain unshipped. The results do not establish
behavior on much larger repositories or a production query distribution.

## Method and correctness

`python3 research/scripts/retrieval_phases.py --output NEW_DIRECTORY`

This capture predates the request-inspection optimization. `driver.py` is the exact
archived driver (its hash matches `build.json`), and `service-before.rs` preserves
the pre-change service for the paired follow-up. The current driver times
`prepare_context` rather than the removed `freshness` method; replaying this
historical phase layout requires the matching recorded production sources.

The driver builds the unchanged release evaluation host and a disposable copy
with coarse RAII timers. It reads the 58 task prompts from the frozen current
context comparison and adds 45 deliberate broad probes: five queries (`return`,
`const`, `error`, `source`, `error response`) through Auto, Metadata and Body in
each of nanus, blogwright and whatsurvey. Each query receives one discarded
warmup in each arm and three measured calls in each arm: **618 measured calls**.
Query order is deterministically shuffled and arm order alternates. Each host
builds its own temporary native index; sibling CodeGraph indexes are read-only.
Defaults remain k=8, one graph hop and 16 KiB, with the declared ranking override.
These are first-search API timings, not the whole search/read task protocol.

All six measured normalized responses per query agree exactly, including scores,
node order, source identities, graph/evidence payload, truncations and all work
counters. Only `context.generation` and `stats.elapsed_ms` are normalized. Warmup
responses also agree between arms. Source, driver, binary, task-file, sibling and
CodeGraph fingerprints stay unchanged. Artifacts omit external source snippets.

Timers are inserted only into the copied source; `instrumentation.json` records
the exact method mapping, timer module and modified-source hashes. Events buffer
in thread-local memory until the outer service timer completes, avoiding stderr
writes inside measured phases. Timer construction/drop and buffering still add
some overhead. The median per-query profile/baseline external-time ratio is 1.009
for task queries and 1.013 for broad probes; individual ratios range 0.868–1.112
and 0.886–1.123 respectively. Three warm repeats cannot separate all scheduling,
filesystem-cache and instrumentation noise, so differences of this size are not
an optimization result. Both hosts remain resident during alternating calls.

## Findings

Percentages below are medians of per-query ratios of the summed metadata/body
posting-phase medians to the service-phase median. Maximum is the largest such
per-query service fraction. Phase timings are **inclusive**: query contains seed,
seed contains retrieval, and retrieval contains posting evaluation. Do not sum
nested phases or separately aggregated medians as disjoint wall time.

| Repository | Query group | Count | Median posting / service | Maximum posting / service | Median posting / core query |
|---|---|---:|---:|---:|---:|
| nanus | Tasks | 24 | 10.08% | 12.27% | 15.14% |
| blogwright | Tasks | 12 | 4.19% | 5.69% | 7.53% |
| whatsurvey | Tasks | 22 | 5.78% | 8.91% | 13.09% |
| nanus | Broad probes | 15 | 0.91% | 2.18% | 2.04% |
| blogwright | Broad probes | 15 | 0.87% | 2.70% | 1.91% |
| whatsurvey | Broad probes | 15 | 1.45% | 4.74% | 4.24% |

Posting-phase evaluation does not dominate any sampled request. Even within the
core query, the largest fraction is 19.17% for tasks and 10.56% for broad probes.
Task-prompt posting evaluation often costs more than the broad single-term probes
because it visits postings for many analyzed terms. The phase includes admission,
filtering, candidate accounting and score accumulation, not just BM25 arithmetic.
It excludes top-k/owner selection, exact-name handling and other seed preparation.
No zero-cost or guaranteed speedup is inferred for any proposed pruning algorithm.

Median inclusive phase milliseconds for the 58 task prompts:

| Repository | Service | Freshness | Result context | Core query | Seed | Body postings | Graph connect | Evidence extension |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| nanus | 11.074 | 1.819 | 1.811 | 7.407 | 5.367 | 1.075 | 0.207 | 0.592 |
| blogwright | 6.849 | 1.510 | 1.431 | 3.768 | 2.521 | 0.279 | 0.233 | 0.409 |
| whatsurvey | 36.497 | 10.172 | 9.963 | 16.458 | 14.109 | 2.131 | 0.379 | 0.543 |

`SearchService::freshness` and `SearchService::context` both invoke
`stale::inspect_with_work`; this is direct source evidence, not an inference from
similar timings. Their combined median per-query fraction across each repository's
sample is 34.45%, 46.09% and 57.07%. Request-scoped reuse of a verified inspection
is therefore a stronger next investigation than score pruning. Reconciliation
can change the generation, and source drift/coverage/work caps must remain honest;
this profile does not itself authorize reusing a stale observation after writes.
Seed time outside the measured posting/top-k phases is another profiling target.

## Conditional decision and limits

Keep the exact exhaustive metadata/body scorers and differential oracles. Reopen
pruning if a larger or representative workload demonstrates posting evaluation
as a material bottleneck, and only with bounds derived for current statistics,
normalization, nonnegative features and the complete tie rule. Validate updates,
deletions, merges, field changes and extreme length skew against the exhaustive
route before enabling pruning. The historical unsafe-bound counterexample remains
relevant; this experiment implements no bounds and makes no bound-correctness claim.

This is warm, single-request, unchanged-corpus wall time. It does not measure cold
I/O, sustained mutation, concurrent traffic, tail-latency SLOs, RSS, answer quality
or patch success. The repositories were authorized code-search examples, not a
random sample of repository sizes or user queries. Compression/lifecycle and
remaining retrieval-quality requirements stay open.
