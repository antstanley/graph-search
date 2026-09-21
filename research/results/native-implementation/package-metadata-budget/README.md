# Package identity and context budget

348 controlled trials (58 tasks × two arms × three repeats) isolate the cost of
returned package identity under the existing 16 KiB public-API response budget.
`structured` is production with package identity. `fixed` is a disposable control
that clears only `SourceEvidence.package` while assembling an explore item.
Both retain package indexing, source boundaries, ranking, graph context, all
other provenance and identical budgets. The exact one-line intervention and
binary/source hashes are recorded in `build.json`.

All trials completed without errors. Sources, binaries and sibling CodeGraph
indexes stayed stable. Evidence repeats identically. The identity-free control
reproduces the source-version-8 structured evidence for every task, restoring all
five partial-coverage regressions observed after package context was introduced.
This isolates returned identity's budget cost as sufficient to produce those
losses on these tasks; it does not justify removing package identity.

| Suite | Tasks | Complete, identity / control | Mean region coverage, identity / control |
|---|---:|---:|---:|
| Established | 34 | 12 / 12 | 48.20% / 50.82% |
| Fresh routing | 12 | 10 / 10 | 84.72% / 87.50% |
| README | 12 | 8 / 8 | 77.08% / 77.08% |

Required-file totals are unchanged. `paired-changes.json` retains the five affected
tasks. This evaluation renders API-returned source and candidate headers, then
metadata; it does not score package identity's utility. The API budgets package
identity before rendering. Neither arm measures model answer/patch success or
latency. The newer task suites are convenience samples, not blind holdouts.

The source allocator currently tries full structural intervals, then five-line
match neighborhoods and individual matching lines. A whole structural interval
can cease to fit after a small metadata increase, explaining why recovered source
bytes can exceed removed metadata bytes. The next intervention retains identity
and offers bounded fragments of rejected structural intervals.

Reproduce using `research/scripts/markdown_context.py --representation package_metadata`
with `--repeats 3` and the `fresh-routing-2026-09-19` and `markdown-readme-2026-09-20`
suites. Raw source-bearing transcripts and disposable builds are outside the repo:
`/var/folders/ft/6fdqp3c914xfdlk47w5cblzc0000gn/T/graph-search-markdown-context-f36qesx6`.


The follow-up `raw-budget-breakdown.json` inspects ten unrendered public-API
responses for the five affected tasks. Sizes are UTF-8 compact JSON reconstructions
of parsed values, not a literal wire-byte capture. Identity accounts for
1,116–1,544 bytes in the production responses; reconstructed total sizes are
16,240–16,347 bytes under the 16,384-byte budget. Little capacity remains for new
fragments. Binary/adapter hashes and sibling source/index snapshots match the
original capture and remain stable (`raw-budget-checks.json`). Reproduce with
`research/scripts/package_budget_probe.py --capture <capture-directory> --binaries
<raw-build-directory>`.

The subsequent [residual-fragment experiment](../context-fragments/README.md)
adds work without changing delivered source, so that candidate was withdrawn.
Shared package identity records with explicit references are the next proposed
budget intervention; identity omission remains an experimental control only.
