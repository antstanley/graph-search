# Delivered-context comparison: combined length versus BM25F

**Decision: keep combined normalization as the default.** Native BM25F remains
an explicit metadata scoring option. It improves several metadata candidate and
partial-evidence results, but provides no automatic-route or complete-region
improvement in these suites and trades a target-file gain for a loss in the newer
suite. The evidence does not support a universal default switch.

## Controlled comparison

Both policies use the same native binary, source, driver, analyzer, field weights,
clipped IDF, corpus, file diversity, and query protocol. Automatic and metadata-only
routes were tested independently. Each of 34 established and 12 newer source-valid
tasks ran three times under each route and policy: **552 trials**, zero protocol
errors. Repeated evidence results are deterministic. These are 46 tasks, not 552
independent tasks; paired task families also remain correlated.

The evidence protocol permits four calls, 16,384 response bytes, 49,152 context
bytes and 180 seconds. The library's own candidate, work and serialized response
caps still apply. Complete-region results require all labeled regions, not merely
finding their files. Model answer success was not measured.

| Suite / route | Required files: combined → BM25F | Complete regions | Mean per-task region coverage | Mean response bytes |
|---|---:|---:|---:|---:|
| Established 34 / auto | 28 → 28 | 12 → 12 | 51.06% → 51.06% | 33,456 → 33,456 |
| Established 34 / metadata | 19 → 23 | 7 → 7 | 29.19% → 31.30% | 32,367 → 32,277 |
| Newer 12 / auto | 12 → 12 | 10 → 10 | 83.33% → 83.33% | 37,671 → 37,671 |
| Newer 12 / metadata | 8 → 8 | 1 → 1 | 9.52% → 11.28% | 31,256 → 30,423 |

All automatic-route evidence dictionaries are identical across policies. This is
consistent with these multiword tasks using the body-first route; it is not proof
that every automatic query ignores metadata normalization.

## Individual effects

Established metadata-only partial-region gains occur on
`nanus.argument-types.debug`, `nanus.context.debug`,
`whatsurvey.contact-policy.change`, and `whatsurvey.survey-draft.change`.
No established task loses region coverage, and none becomes fully evidence-ready.

In the newer metadata-only suite, `nanus.fresh-legacy-approval.debug` gains target
file presence but still delivers none of its labeled region.
`whatsurvey.fresh-media-flow.debug` loses target file presence; it also had zero
labeled-region coverage under the baseline. These effects cancel in aggregate
file recall and must not be hidden by the unchanged 8/12 total.
`whatsurvey.fresh-option-code-collision.debug` gains 21.05% of its labeled region,
but remains incomplete. No newer task gains complete-region coverage.

## Provenance and limits

Both policies and both suites have identical recorded production, driver and
host provenance. Current production hashes still match the captures. Every run's
sibling source snapshots are stable, and sibling CodeGraph files still match the
pre-experiment native-equivalence capture. The native backend uses temporary
stores; source repositories and their CodeGraph indexes were not modified.
Raw transcripts remain in the temporary directories printed in the run logs;
checked-in results retain sanitized metrics and transcript hashes.

The established labels were previously exposed, and the newer frozen suite was
also used in earlier routing/context experiments. This is controlled regression
evidence, not a fresh blinded test. Arms ran sequentially; their timing samples
must not be presented as a causal latency comparison. No RSS, update-cost or
model-success conclusion follows from these captures.

The native option and the factorial study satisfy the IDF/normalization
investigation. They reinforce the priority of body representation, query policy
and evidence selection over changing a metadata formula globally. Future default
changes need broader query-class and performance evidence, especially when
metadata participates in single-token fusion or body-empty fallback.

## Artifacts and reproduction

`normalization-context-comparison.json` contains verified provenance assertions,
all paired evidence changes, budgets and summaries. The four directories
`normalization-{combined,bm25f}` and
`normalization-{combined,bm25f}-fresh` retain individual trial rows, protocol
manifests, source stability and provenance.

```sh
python3 research/scripts/ranking_review.py --output /tmp/new-combined --variants auto:0,metadata:0 --normalization combined
python3 research/scripts/ranking_review.py --output /tmp/new-bm25f --variants auto:0,metadata:0 --normalization bm25f
```

Repeat with `--suite research/fixtures/fresh-routing-2026-09-19` and new output
directories for the newer suite. Freeze production/driver sources during capture.
