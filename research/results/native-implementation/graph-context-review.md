# Graph enrichment: controlled evidence comparison

**Decision: retain semantic graph context by default, with explicit opt-out and
relation-family controls.** Neither arm improves complete-region delivery over
the other. Removing graph context has mixed partial-evidence effects on the
established suite and no effect on the newer suite. This is not evidence that
graph context is universally useful, or that a graph-free default is superior.

## Comparison

Both arms use the same ranker-revision-14 binary, source, driver, analyzer,
automatic lexical routing, field normalization, candidate limits and evidence
protocol. The only policy difference is `graph_context: semantic` versus `none`.
The protocol makes one explore request and reads up to three returned candidate
files; it does not run a model or make adaptive graph-tool calls.

There are 34 established source-valid tasks and 12 newer frozen tasks across
nanus, blogwright and whatsurvey. Both suites have been used before; neither is
blinded held-out evidence. Three repetitions per policy give **276 trials**, not
276 independent tasks. Every trial completed without protocol errors, and
per-task evidence dictionaries are deterministic across repetitions.

Budgets are four calls, 16,384 bytes per response, 49,152 cumulative response
bytes and 180 seconds, in addition to the native library's work/payload caps.
The source/candidate limits are identical; changing graph enrichment can change
which excerpts fit and which candidate files the fixed read policy follows.

| Suite | Required files: semantic → none | Complete regions | Mean per-task region coverage | Mean response bytes |
|---|---:|---:|---:|---:|
| Established 34 | 28 → 28 | 12 → 12 | 51.06% → 51.54% | 33,455 → 34,087 |
| Newer 12 | 12 → 12 | 10 → 10 | 83.33% → 83.33% | 37,679 → 38,033 |

The byte counts include the fixed follow-up source reads, not just explore's
JSON. Removing graph metadata need not reduce delivered source bytes. This
experiment does not isolate the contribution of impact counts from connecting
paths and graph-selected source regions: `none` omits all enrichment together.

## Individual effects

All five changed established tasks are in nanus. Required-file recall remains
one for each; none becomes fully evidence-ready under either policy.

| Task | Region | Semantic coverage | No graph coverage |
|---|---|---:|---:|
| `nanus.edit.change` | r1 | 13.85% | 49.23% |
| `nanus.glob.debug` | r1 | 12.00% | 5.33% |
| `nanus.context.debug` | r2 | 9.43% | 7.55% |
| `nanus.glob.change` | r1 | 16.00% | 5.33% |
| `nanus.grep.change` | r1 | 29.41% | 27.94% |

`nanus.context.debug` has zero r1 coverage in both arms.
`nanus.grep.change` has complete r2 coverage in both arms.
The newer suite's complete evidence dictionaries are identical, not merely its
aggregate counts. The partial improvements and losses argue for evaluating
graph selection by task intent, rather than treating degree or connectivity as
a universal relevance signal. The public Calls/Imports/Types controls now make
such experiments possible; those individual arms were not measured here.

## Correctness and provenance

The connection optimization preserves the old untruncated selected-edge union
on 5,120 exhaustive small-graph/seed/hop cases. It uses shared components to
avoid searches without a reachable peer and ends a BFS after its final target
is discovered. This equivalence does not require identical partial results at
tight caps: saved work can admit more evidence, with exhaustion still reported.

The semantic arm's per-task evidence dictionaries also match the previous
automatic-route combined-normalization capture on all 34 established and 12
newer tasks. That is regression evidence across these implementation changes,
not an isolated performance comparison with the old traversal.

All four captures have identical production/driver/host provenance. Before and
after production hashes match and were rechecked against the current checkout
after capture. External source snapshots are equal within and across runs.
Every file under the three sibling CodeGraph directories also matches its
pre-capture checksum. Temporary native stores leave the sibling repositories
and their CodeGraph indexes unchanged.

Arms ran sequentially; no causal latency or memory claim follows. There is no
agent answer, patch-success, relationship-resolution-precision or fresh-query
generalization claim. The 26 drifted historical tasks remain excluded rather
than having hashes silently updated.

## Artifacts and reproduction

[Paired comparison and provenance assertions](graph-context-comparison.json)
contains all changed evidence dictionaries, budgets and CodeGraph checksums.
`graph-context-{semantic,none}` and their `-fresh` counterparts contain sanitized
trial rows, summaries, manifests, label validation, build provenance and
source/production stability records. Raw source transcripts remain in temporary
directories listed by the driver.

```sh
python3 research/scripts/ranking_review.py --output /tmp/graph-semantic --variants auto:0 --graph-context semantic
python3 research/scripts/ranking_review.py --output /tmp/graph-none --variants auto:0 --graph-context none
```

Repeat with `--suite research/fixtures/fresh-routing-2026-09-19` and new output
directories for the newer suite. Freeze production and driver files throughout
capture. The driver now fails if those source hashes change during a run.

Remaining graph work includes reuse between connection expansion and incoming
impact cones, and broader intent-specific context selection. These measurements
complete the initial equal-budget graph ablation, not all graph recommendations.
