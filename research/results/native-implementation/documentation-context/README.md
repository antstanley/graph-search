# Documentation segmentation and association: isolated comparison

348 trials over the same 58 frozen/source-valid tasks, two arms and three repeats.
No model task-success or latency claim. This follows the broader
[code-structure comparison](../code-context/README.md) and isolates its documentation
component: the disposable `fixed` control passes no documentation comments into
source partitioning, while retaining declaration boundaries, Markdown structure,
ordinary comment text in code bodies, graph/metadata retrieval and all budgets.
The historical arm label `fixed` here means **no separate documentation units**,
not removal of declaration structure.

The exact single-expression intervention, source identities and binaries are in
`build.json`. Both arms use current 80-line/8-line-overlap windows, auto ranking,
per-file diversification disabled, and the same public default candidate limits.
Evidence-v1 permits four calls: one explore and up to three 100-line reads, with
16 KiB per response, 48 KiB cumulative and 180 seconds. Cases and arm order are
interleaved deterministically; fresh index setup precedes trials.

| Suite | Tasks | All files, docs / control | Complete regions, docs / control | Mean region coverage, docs / control | Mean response bytes, docs / control |
|---|---:|---:|---:|---:|---:|
| Established | 34 | 28 / 28 | 12 / 12 | 50.82% / 50.51% | 34,392 / 34,798 |
| Newer routing | 12 | 12 / 12 | 10 / 9 | 87.50% / 76.04% | 36,895 / 36,975 |
| README | 12 | 10 / 11 | 8 / 8 | 77.08% / 78.75% | 34,252 / 34,438 |

All trials complete without errors; evidence and response bytes repeat exactly.
Production/binary/source/index stability checks pass. The structured binary and
its results are identical to the broader code comparison. Only the experiment
driver gained the additional control option between captures; production crate
hashes are identical. Task-success fields remain null. These are 58 correlated
retrieval tasks, not 348 independent successes.

16 tasks change evidence (`paired-changes.json`). Documentation units gain complete
regions on `whatsurvey.webhook-signature.change`, `whatsurvey.fresh-media-flow.debug`
and `blogwright.fresh-refresh-metadata.debug`, and lose completion on
`nanus.argument-types.debug` and `nanus.fresh-runtime-tools.debug`.

The traces distinguish several mechanisms:

- Argument-type initial coverage improves from 46.55% to 62.07%, yet the subsequent
  `args.rs` read moves from line 12 to 73. Final coverage decreases from complete
  to 67.24%. Better initial matching alone does not ensure better follow-up context.
- Runtime-tools initial coverage falls from complete to 50%; the `agent_loop.rs`
  read moves from line 296 to 406 and does not restore the missing region.
- Media-flow initial coverage rises from zero to complete; a types-file read
  replaces a validation-test read.
- Webhook-signature initial coverage rises only to 5%, but its follow-up now reads
  the required `core/whatsapp/signature.ts` instead of spending all three reads
  on mock signing, specification and tests. Final coverage becomes complete.
- The README survey-flow task loses its partial 20% coverage and required-file
  hit. Markdown boundaries were held fixed: changed body statistics/candidate
  competition can affect a documentation query even without changing its file's
  own representation. The capture does not separately attribute those effects.

`action-analysis.json` retains initial source-valid coverage and follow-up actions
without storing raw source transcripts. Aggregate gains do not erase these losses.
Keep the native association semantics, but carry these regressions into the open
candidate/context release gates (11/12/30). Package association under 21 remains
outstanding for recommendation 9. No task-vocabulary tuning was performed.
