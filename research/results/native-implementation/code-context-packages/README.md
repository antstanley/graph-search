# Code structure with package context

This refreshes `../code-context/` on parser policy 7, source representation 9,
chunker policy 9 and ranker policy 16. It evaluates delivered source evidence,
not model task success. No sibling source or existing CodeGraph index was changed.

## Protocol and controls

348 trials: 58 frozen tasks across nanus, blogwright and whatsurvey, two arms,
three repeats. Each trial has four calls, a 16 KiB response limit, a 48 KiB
cumulative context limit and a 180-second deadline. The deterministic driver
performs one exploration and up to three 100-line follow-up reads. Both arms use
Auto retrieval, default candidate limits and `per_file=0`.

The disposable fixed control disables declaration boundaries and documentation
segmentation/associations in source-unit construction. It retains graph metadata,
Markdown representation and the current 80-line/8-line-overlap window policy.
This is not an exact replay of a historical implementation. Build hashes and the
exact two-edit patch are in `build.json`; tasks, oracles and source identities are
retained alongside results. Source snapshots and oracles match the prior capture.

All 348 trials completed without errors. Production, binaries, sibling source and
CodeGraph index stability checks passed. Evidence and actions repeat identically.
Response bytes differ by one byte in three fixed-arm task groups because
`stats.elapsed_ms` crosses a digit boundary. All response histories and delivered
source-line sets are identical after normalizing only that telemetry field.
Normalization also handles budget-clipped metadata JSON; parsing only complete
JSON initially produced a false determinism failure. `checks.json` records the
final check and deliberately retains the false strict byte-identity result.

## Results

Values below use repeat zero. Required files means tasks retrieving every required
file; complete means tasks covering every required source region. Mean region
coverage is averaged per task, not pooled across source lines.

| Suite | Tasks | Required files, structured / fixed | Complete, structured / fixed | Mean region coverage | Mean response bytes |
|---|---:|---:|---:|---:|---:|
| Established | 34 | 28 / 31 | 12 / 14 | 48.20% / 57.73% | 32,119 / 37,591 |
| Fresh routing | 12 | 12 / 11 | 10 / 8 | 84.72% / 73.44% | 34,825 / 39,586 |
| README | 12 | 10 / 10 | 8 / 8 | 77.08% / 72.92% | 33,154 / 36,098 |

The newer task sets are convenience samples, not a blind evaluation. Structured
context does not dominate fixed windows. `paired-changes.json` records 26 tasks
with different evidence between arms; `action-analysis.json` separates initial
source evidence from follow-up choices.

## Representation regressions

Compared with source representation 8, complete-task and required-file totals
remain unchanged in both arms. Five structured tasks lose partial source-region
coverage: `nanus.fresh-runtime-tools.debug`, `nanus.temporary-paths.change`,
`nanus.read.change`, `nanus.grep.change`, and `whatsurvey.contact-policy.debug`.
For all five, candidate headers and follow-up actions are unchanged. Their lost
coverage is already visible in the initial response. See
`package-format-context-losses.json` and `prior-capture-differences.json`.

These are observed representation regressions under a fixed response budget.
The comparison does not isolate package-field serialization from all other
implementation changes, so it cannot attribute the loss to a single mechanism.
Recommendations 11/12/30 retain these cases as candidate/context release work.
Package association correctness does not establish retrieval-quality superiority.

## Reproduction

```sh
python3 research/scripts/markdown_context.py --representation code \
  --output research/results/native-implementation/code-context-packages --repeats 3 \
  --suite research/fixtures/fresh-routing-2026-09-19 \
  --suite research/fixtures/markdown-readme-2026-09-20
```

Raw transcripts and disposable builds were captured outside the repository at
`/var/folders/ft/6fdqp3c914xfdlk47w5cblzc0000gn/T/graph-search-markdown-context-b09rr6nd`.
Repository artifacts retain sanitized evidence metrics and hashes, not copied
sibling source. No latency or model answer/patch-success claim follows.
