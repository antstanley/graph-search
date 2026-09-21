# Normalized combination rejected by source-delivery gate

The top-50 candidate diagnostic nominated min-max normalization with 75% body /
25% metadata weight. This end-to-end native trial rejects its integration. It
improves required-file delivery from 50/58 to 51/58 but reduces complete-region
delivery from 30/58 to 27/58, with no newly complete tasks. The original production
ranking remains unchanged. No third-party component or dependency was added.

## Controlled comparison

The [predeclared protocol](PROTOCOL.md) and `capture/build.json` specify the exact
disposable native patch, source identities and binaries. The generic capture names
are unusual: **structured is baseline; fixed is the normalized candidate**.
Top-level `summary.json`, `changes.json`, `losses.json` and `decision.json` translate
those names. In the generic capture's paired-changes file, its field `control`
therefore denotes this normalized candidate; do not interpret that file backwards.

All 348 trials finish without protocol errors. Production, binary, sibling-source
and CodeGraph fingerprints remain stable. Repeats have identical evidence, source
lines and actions. Actions differ between policies, as expected when ranking
changes the evidence protocol's deterministic follow-up choices. No model produces
those actions. No claim about model debugging or patch success follows.

| Suite | Baseline complete regions | Normalized complete regions | Baseline mean region coverage | Normalized mean region coverage |
|---|---:|---:|---:|---:|
| Established (34) | 12 | 9 | 48.5792% | 47.4453% |
| Fresh routing (12) | 10 | 10 | 84.7222% | 84.7222% |
| Markdown/README (12) | 8 | 8 | 77.0833% | 77.7778% |

The candidate gains partial evidence on several tasks, including the nominated
blogwright secret-upsert task. Eleven tasks change labelled evidence, and 55 change
their delivered line sets. Mean region coverage falls overall, in nanus, and in the
debug task class. It fails both the complete-region and group-coverage gates.

## Every loss investigated

- `nanus.edit.debug`: the first relevant owner in `tools/edit.rs` moves from
  `edit_outcome` to a test. The follow-up begins at line 198 instead of 62; coverage
  of required lines 72–136 falls from complete to 50.77%.
- `nanus.read.debug`: `render_window` moves ahead of `read_outcome`. The same-file
  follow-up begins at 172 instead of 99. The second region stays complete but the
  first falls from complete to 19.67%.
- `nanus.write.change`: tests move into the first distinct-file follow-up slots.
  The implementation file remains among candidates but loses its follow-up read;
  required-region coverage falls from complete to 8.89%.
- `nanus.read.change`: follow-up actions are identical, while selected owners and
  initial context differ. The first region falls from 18.03% to zero and the second
  rises from zero to 2.56%, reducing mean coverage. This is a first-response
  selection/context loss, not a changed follow-up decision.

`losses.json` retains source-free item identities, spans, follow-up actions, byte
counts and region coverage for all four tasks. These traces establish how the
observed delivered evidence differs. They do not isolate numerical score scale,
metadata admission, owner order and context priors as separate causal effects.
The trial deliberately exercises their combined native behavior.

## Decision and validation

Reject this normalized policy; do not tune its weights on these labels. The
clean-generation candidate already fails, so implementing its mixed live/indexed
statistics is unnecessary for adoption. The experiment rejects nonempty live
body lanes explicitly and makes no compatibility promise for them. Production's
existing rank-based live/indexed merge remains intact.

The native candidate and baseline build release/offline/locked. All 28 evaluation
tests pass with `PYTHONPATH=evaluation`; the initial command without that module
path failed import discovery and is retained as a setup failure. Focused production
body/planner tests validate the retained implementation; terminal results are in
`final-checks.json`. No new full-workspace pass is claimed for this research-only
change. Source/parser/ranker versions remain 14/19/23.

Reproduce into a new output directory with:

```sh
python3 research/scripts/markdown_context.py --representation score_combination --output /tmp/score-combination-new --repeats 3 --suite research/fixtures/fresh-routing-2026-09-19 --suite research/fixtures/markdown-readme-2026-09-20
```

Archive results when sources change; the current report script expects the checked-in
capture and its printed temporary raw-transcript directory. Raw source bodies remain
outside the repository. This closes the normalized-combination decision, not the
independent model-answer/release requirements in recommendations 12/30.
