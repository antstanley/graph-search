# Residual source-fragment experiment — withdrawn

A native candidate attempted to use spare response capacity by bisecting rejected
structural intervals into bounded fragments. It retained package identity and all
source provenance. Complete structural windows ran first, then explicit match
neighborhoods/lines, then residual fragments. At most four subdivisions reached
the existing five-line scale. Fragment utility was scaled by source length.

The first ordering placed fragments before match evidence and failed the existing
`distant_body_matches_fit_when_the_whole_region_does_not` regression. That ordering
was corrected. The corrected candidate passed all 374 workspace tests (31 suites)
and strict workspace/all-target Clippy, including a synthetic fixture demonstrating
useful suffix evidence under a tight budget without duplicate or invented lines.

## Controlled outcome

348 trials: 58 tasks × two arms × three repeats, using the same frozen suites and
16 KiB response / 48 KiB cumulative evidence budget as the package-context study.
`structured` is the candidate; `fixed` disables only residual splitting. Both
retain package identity. Exact hashes and intervention are in `build.json`.

All trials complete without errors; source, binary and CodeGraph stability checks
pass. Both arms deliver **identical source-line sets and follow-up actions on all
58 tasks**. Evidence repeats identically and the control matches the pre-experiment
baseline. Established completion remains 12/34, routing 10/12, and README 8/12.
The five package-budget regressions remain unchanged.

The candidate increases context-window work on 44 tasks. Across repeat-zero queries,
work grows from 31,800 to 36,633 examined windows (+15.20%), with no delivered-source
gain. This is an operation-count comparison, not a latency measurement. Raw task
metrics, repeat/stability checks and per-task work differences are retained.

## Decision and reproducibility

**Withdrawn from production.** A synthetic improvement does not justify additional
work and complexity with no benefit in this controlled repository sample.
`candidate.patch` preserves the exact corrected implementation and its regression
fixture. The patch applies to the pre-experiment source recorded by
`../package-metadata-budget/build.json`; applying it to a disposable copy permits
reproduction with `markdown_context.py --representation context_fragments`, three
repeats, and the fresh-routing/Markdown README suites.

Production files were restored byte-for-byte to their pre-experiment hashes.
`production-restoration.json` records that every captured `crates/` file matches
that baseline; current production remains ranker policy 16. The candidate used
policy 17 only inside this experiment. No dependency was added.

Raw source-bearing transcripts and binaries remain outside the repository at
`/var/folders/ft/6fdqp3c914xfdlk47w5cblzc0000gn/T/graph-search-markdown-context-le98ljoa`.
No model task-success or universal retrieval-quality claim follows. The next
intervention should reduce repeated package metadata while preserving the same
identity information, then repeat the fixed-budget comparison.
