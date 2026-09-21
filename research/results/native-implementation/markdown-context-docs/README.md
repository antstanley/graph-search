# Documentation-inclusive Markdown comparison

This extends the [first controlled comparison](../markdown-context/README.md)
with 12 manually source-backed documentation questions, four per sibling repo.
Prompts and exact line/hash oracles were frozen before their first retrieval
trial in [the fixture](../../../fixtures/markdown-readme-2026-09-20/freeze.json).
The sample uses three README files and is not representative of all documentation.
The author had seen the earlier code-task results; this is not a blinded study.

348 trials completed with zero protocol errors: 58 tasks × two arms × three
repeats. All evidence metrics are repeat-stable. The 46 earlier tasks reproduce
exactly, including the code regressions. Both arms use the same public API,
ranking, graph policy, work allowances and evidence-v1 response budgets. Only
Markdown structural partitioning differs, through the recorded one-line patch
in a disposable checkout. No new third-party implementation is involved.

| Suite | Structured complete evidence | Fixed complete evidence | Structured all files | Fixed all files |
|---|---:|---:|---:|---:|
| Established code tasks | 12/34 | 12/34 | 28/34 | 28/34 |
| Newer routing tasks | 9/12 | 10/12 | 12/12 | 12/12 |
| Documentation sample | 8/12 | 2/12 | 11/12 | 2/12 |

Documentation mean region coverage is 78.75% structured versus 16.67% fixed;
mean delivered bytes are 34,439 versus 31,905. These are evidence retrieval
metrics, not model answers or task success. The samples must not be pooled into
a general quality score: repository balance does not correct convenience sampling
or equal weighting of unrelated tasks.

## Documentation differences and remaining failures

- Nanus provider/tool replacement, component unload and stdio contracts are fully
  delivered only with structure. Named-session resumption retrieves the prose
  explanation but misses the command region: 50% mean region coverage.
- Blogwright's package table and documentation-link questions become complete.
  Emulator testing is complete in both arms. Initial setup regresses from complete
  to 75% required-line coverage, so structure is not uniformly beneficial even
  within documentation.
- WhatSurvey's local loop and trace-correlation questions become complete.
  Survey lifecycle delivers only one of five required lines. Neither arm delivers
  the required README evidence for stack layout and environment naming.

`paired-changes.json` preserves all changed evidence metrics, including losses;
`results.json` includes unchanged and failed tasks. No labels were altered after
seeing results. File/hash labels assert the chosen source regions, not uniqueness
of the answer: another document may contain equivalent information, which this
strict oracle does not credit.

## Decision

Retain the native structural representation for source integrity and its measured
benefit on this documentation sample. Do not present it as a universal ranking
improvement. Simply restoring fixed windows would remove much of the observed
documentation gain. The remaining work is candidate/context policy: code evidence
can lose follow-up slots to documentation, and small structural blocks can deliver
only part of a multi-region answer. Keep those release gates open under the query
planning and context-selection recommendations. A future policy must be evaluated
on both code and documentation without changing source labels or response budgets.

Source, driver and binary fingerprints remained stable during measurement; all
sibling source snapshots and every CodeGraph index file were unchanged. The new
versioned-document regression test was added only after the capture finished.

Reproduce:

```sh
python3 research/scripts/markdown_context.py \
  --output /tmp/markdown-context-docs-new \
  --suite research/fixtures/fresh-routing-2026-09-19 \
  --suite research/fixtures/markdown-readme-2026-09-20
```
