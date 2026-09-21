# Context proximity candidate: controlled evaluation

Status: retained as ranker policy 22. Focused tests establish the compactness preference; this corpus capture establishes no aggregate quality improvement.

The native ranker adds a bounded shortest-line-span bonus for at least two distinct, undelivered query terms. The control disables only that bonus and retains the same fixed-point scale and other behavior. No dependency was added.

## Results

348 trials cover 58 tasks, two arms and three repeats across nanus, blogwright and whatsurvey. All trials completed without errors. Production, executable, source and sibling CodeGraph-index fingerprints remained stable. Every task has identical evidence metrics across repeats and arms. The control reproduces the prior source-admission capture's evidence metrics for all tasks.

| Suite | Tasks | Required files found | Complete regions | Mean region coverage |
|---|---:|---:|---:|---:|
| Established | 34 | 28 | 12 | 48.974474% |
| Fresh routing | 12 | 12 | 10 | 84.722222% |
| Markdown/README | 12 | 10 | 8 | 77.083333% |

Both arms have the figures above. These are evidence-protocol measurements, not model answer or patch success.

The separate raw probe checked the first query at repeat zero (116 responses). All 464 common node identities agree, all package references resolve, and no orphan package entries remain. Selected node order and source-read counters are identical. Candidate context-window work totals 34,553 versus 34,630 in the control, but this counter excludes the added proximity calculations and does not establish a CPU or latency saving.

Delivered line sets differ in 5 first-query pairs. `raw-context-comparison.json` records every added and removed coordinate. Unchanged labelled coverage does not prove those substitutions are harmless or helpful. Across the full four-call protocol, four task line sets differ (`protocol-line-changes.json`). All actions are identical between arms, and each arm has identical delivered lines and actions across its three repeats.

The retention decision rests on implementing the requested bounded proximity preference with oracle-tested semantics and no measured labelled-evidence regression. It is not a claim that the changed, unlabelled lines improve answers. Independent model-success measurement remains outstanding.

## Verification and scope

The candidate passed 391 workspace tests and strict workspace/all-target Clippy. Following a semantic-preserving early-return optimization, the final core suite passed all 155 tests, including 65,536 comparisons with an exhaustive minimum-interval oracle. This is not exhaustive testing of all 128-bit masks; separate tests cover full masks, high bits, coordinate translation, repeated terms and delivered-term removal.

Build fingerprints and exact intervention are in `build.json`; raw response metadata and identity checks are in `raw-budget-*` and `raw-identity-validation.json`. The raw byte figures are compact Python JSON reconstructions, not literal wire captures.

Reproduce the paired capture:

```sh
python3 research/scripts/markdown_context.py --representation context_proximity --output /tmp/context-proximity-review --repeats 3 --suite research/fixtures/fresh-routing-2026-09-19 --suite research/fixtures/markdown-readme-2026-09-20
```

The source snapshots must match for comparisons with this historical capture. Freeze production and drivers during measurement. Do not rebuild or modify the sibling CodeGraph indexes.
