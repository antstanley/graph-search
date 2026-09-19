# Natural-language retrieval experiments, v1

Frozen before measurements. Base: bdcbece (PR #2). Source repositories are read-only.

Primary selection set: the task suite's 36 development prompts. The 24 held-out
prompts are evaluated only after choosing a configuration; the existing set is
public and has been used for earlier baseline reporting, so it is a confirmation
set rather than a newly secret generalization test. No fixture labels or prompts
will be changed in response to ranker outcomes.

Measure required-file recall@8 and reciprocal rank, plus whether returned symbol
spans overlap the labelled source regions. These are retrieval proxies, never
agent task success. Report per-repository results and every miss. Compare each
variant to metadata retrieval over the same graph snapshot. Production baseline
and end-to-end task evidence measurements are separate from prototype rankings.

Fields and limits for the first experiment:
- Metadata: existing identifier-aware BM25 over name/path/signature (8/2/1).
- Comments: leading comment block, at most 32 lines / 2,048 characters per symbol;
  first prototype uses an explicitly approximate line recognizer.
- Bodies: function/method spans only, at most 64 lines / 4,096 characters each.
- Documentation: Markdown/MDX/RST/text/AsciiDoc passages, 64 lines / 4,096 characters,
  at most 64 KiB per file. Passage matches return the file and source location.
- Source reads: only indexed file paths, UTF-8, at most 1 MiB per source file.

Ablations: metadata; metadata+comments; metadata+bodies; metadata+documentation;
all content fields. Score field indexes separately to avoid diluting metadata
length normalization. Test content field weights 0.25, 0.5 and 1.0 on development
only. Document results have no exact-symbol lane; exact bare/qualified symbol
matches stay ahead of every nonexact match.

Then test query expansion and result diversity independently against the selected
content configuration. Expansion is frozen generic English inflection matching
(no task-specific synonym dictionary). Diversity is a file-level second-hit
penalty, keeping exact matches unpenalized; test factors 0.5 and 0.25. Include a
combined arm only after reporting individual effects. Ties are stable by path,
line and node ID. Runtime/candidate counts are recorded; prototype timings do
not imply production speedups.

Regression gates: original 90 exact and 90 split-name queries, the 60 held-out
split-name queries, and the original 15 task-language/30 discovery prompts.
Compare target name+path for identifier labels and path for task/discovery labels.
No production promotion unless exact-symbol retrieval is preserved. Keep all
failed or rejected ablations, not only the chosen configuration.

After prototype selection, implement the justified changes through the public
library with bounded/fresh content handling, deterministic ranking, filter and
output-budget regressions, and source-stability checks. Re-run relevant frozen
queries and the task evaluation protocol through the actual implementation.
Any necessary departures from the prototype must be measured and documented.

Before measurements, nine additional source-authored documentation queries were
frozen in `documentation-queries.json` (six development, three confirmation).
They diagnose whether documentation indexing retrieves documentation targets;
they are reported separately and do not replace code-task selection metrics.
This prevents evaluating a documentation field solely against code-file labels.

## Prototype correction before confirmation

The first development-only run is preserved under `results/pilot/`. Its separate
documentation index incorrectly included thousands of empty symbol placeholders
in average passage length and document count, suppressing every documentation
hit in the six diagnostic queries. The corrected index computes those statistics
over documentation passages only, preserving the candidate-index offset when
merging scores. Metadata, comment and body symbol universes remain unchanged.
Re-run the predeclared field weights and expansion/diversity arms before selecting
or opening confirmation tasks. This is a corpus-normalization correction, not a
query-label change. No held-out measurements were inspected before correction.

## Production regression correction

The first public-core check preserved exact spelling but exposed a collision
between complete split names and multi-target queries. For example, `load pds
secret` could prioritize every individual symbol called `secret`. The partial
failed run is preserved in `results/rejected-split-priority.json`; it completed
nanus and blogwright and was deliberately stopped before the last repository.
Complete split-name matches now disable the separate-name interpretation.
A synthetic regression adds all individual component names alongside the full
identifier. This is a compatibility correction, not a retuning of content
weights, expansion, diversity, or task labels. Frozen confirmation labels have
now been seen, so subsequent runs are regression confirmation, not a fresh
held-out selection exercise. The final probe copies binaries before running to
prevent concurrent rebuilds from changing an arm midway through a comparison.

The final implementation audit also corrected the prototype documentation-file
limit from 65,536 Unicode characters to the predeclared 65,536 UTF-8 bytes. This
affects only the documentation ablations, not the chosen body/diversity ranker.
The development ablations were rerun; compare their selection with the frozen
configuration above rather than selecting again using confirmation results.
