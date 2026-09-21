# Task-prompt cleanup versus verbatim input

348 completed public-API evidence trials: 58 tasks × two policies × three repeats.
All runs used the same binary and production source hashes. Every run's source
and production stability checks passed, evidence was identical across repeats,
and there were zero protocol errors. All sibling CodeGraph index hashes equal
the earlier Markdown capture and the post-comparison snapshot. Index files were
not opened for mutation or rebuilt.

Only `RetrievalOptions.query_policy` differs. Both arms use automatic ranking,
no per-file quota, semantic graph context and evidence-v1 budgets: four calls,
16 KiB per response, 48 KiB total, 180 seconds. Arms ran sequentially, so timings
are not a paired performance claim. Raw source transcripts remain temporary.

| Suite | Policy | Complete evidence | All required files | Mean region coverage | Mean response bytes |
|---|---|---:|---:|---:|---:|
| Established, 34 tasks | Verbatim | 12 | 28 | 50.51% | 34,799 |
| Established, 34 tasks | Task | 14 | 30 | 56.76% | 34,879 |
| Newer routing, 12 tasks | Both | 9 | 12 | 76.04% | 36,976 |
| README documentation, 12 tasks | Both | 8 | 11 | 78.75% | 34,439 |

The established task prompts were already exposed during implementation and
contain the vocabulary the policy targets. These gains are development evidence,
not independent confirmation. The newer routing/documentation prompts contain no
removable suffix, and have identical evidence **and response bytes** between arms.
They confirm noninterference for those queries, not generalization of cleanup gains.
No model answered a task and no debugging-success claim follows from these counts.

## Inspected tradeoffs

`changes.json` retains every changed established-task metric. Eight tasks improve
mean required-region coverage, four decline, and one gains a file hit without any
required-line coverage. No previously complete task becomes incomplete; the two
new complete tasks are `nanus.grep.change` and `whatsurvey.webhook-signature.change`.

Important losses:

- `nanus.edit.change`: 38/65 required lines to zero. Verbatim follow-up reads
  AGENTS.md, the edit tool and its filesystem port; task cleanup instead selects
  protocol.rs, the TUI view and a mechanisms report. Removing procedural terms
  does not guarantee that the remaining conceptual terms rank the implementation.
- `nanus.read.change`: one region falls from complete to 5/61 lines while the
  second rises from 17/39 to complete. The selected read.rs read begins at line
  165 instead of 99; this is a competing-region problem within a relevant file.
- `nanus.context.debug` and `nanus.glob.change` also lose partial evidence.

For `nanus.grep.change`, follow-up reads now include the grep tool implementation
and a different adapter region. For the webhook-signature task, all three follow-up
reads are unchanged; completion improves in the initial explore response instead.
Thus cleanup affects both candidate selection and initial context allocation.
`followup-actions.json` records paths/read windows without copying source snippets.

## Decision

Keep `verbatim` as default. Retain `task` as an explicit, explained option with its
narrow vocabulary and quoted-input protection. Do not expand the suffix list or
retune scoring solely to repair these exposed losses. The result satisfies the
initial policy comparison, while query-planner audit, multi-region context and
fresh independent quality evaluation remain distinct requirements. It does not
resolve the earlier Markdown ranking tradeoff on newer routing tasks.

Reproduction (new output directories required):

```sh
python3 research/scripts/ranking_review.py --output /tmp/policy-verbatim \
  --variants auto:0 --query-policy verbatim
python3 research/scripts/ranking_review.py --output /tmp/policy-task \
  --variants auto:0 --query-policy task
```

Repeat each with `--suite research/fixtures/fresh-routing-2026-09-19` and
`--suite research/fixtures/markdown-readme-2026-09-20`, using distinct outputs.
The six sibling `task-policy-*` result directories retain per-run provenance,
label validation, stability, protocol budgets, summaries and all trial rows.
`summary.json` combines their summaries; `all-suite-checks.json` verifies common
production/binary identity and per-run stability. Repeat-zero rows are the unit
of quality comparison; repeats are not additional independent observations.
