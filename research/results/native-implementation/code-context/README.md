# Code structure versus fixed source windows

348 trials: 58 frozen/source-valid tasks × two arms × three repeats. The established
34 tasks, newer routing 12 and README 12 retain separate denominators. No model
answered tasks, and no task-success or latency conclusion follows from this capture.

## Intervention

`structured` uses production source representation 8, chunker 9 and ranker 16.
The disposable `fixed` control changes exactly two expressions in `units.rs`:
remove declaration-boundary events and pass no documentation boundaries. This
removes body-unit declaration associations too; graph declarations, metadata
retrieval and graph relationships remain available. Markdown structure remains
identical. Both use the current 80-line/8-line-overlap windows, analyzer, ranking,
limits and public API. This evaluates the combined code representation, not only
window endpoints, and is not a replay of the historical non-overlapping prototype.

Both arms use auto ranking, per-file diversification disabled, the same default
candidate limits, and evidence-v1: one explore plus up to three 100-line reads,
four calls, 16 KiB/response, 48 KiB cumulative, 180 seconds. Fresh stores precede
trials; cases are deterministically shuffled and arm order interleaved. Build
interventions and source/binary hashes are in `build.json`.

## Results

| Suite | Tasks | All files, structured / fixed | Complete regions, structured / fixed | Mean region coverage, structured / fixed | Mean response bytes, structured / fixed |
|---|---:|---:|---:|---:|---:|
| Established | 34 | 28 / 31 | 12 / 14 | 50.82% / 58.17% | 34,392 / 39,080 |
| Newer routing | 12 | 12 / 11 | 10 / 8 | 87.50% / 66.67% | 36,895 / 41,517 |
| README | 12 | 10 / 10 | 8 / 8 | 77.08% / 72.92% | 34,252 / 37,719 |

All 348 trials completed without errors. Evidence and response bytes are identical
across each arm/task's three repeats. Production inputs, binaries, sibling source
snapshots and all existing CodeGraph index files remained unchanged during capture.
The repeated tasks are correlated measurements, not 348 independent quality labels.

26 tasks change evidence between arms (`paired-changes.json`). Important examples:

- `nanus.glob.change`: fixed delivers the required glob implementation; structured
  follows the filesystem implementation, cap test and port declaration and misses
  the required file. Initial required-region coverage is 12% versus zero.
- `nanus.edit.change`: both reach the required file, but structured follows a read
  starting at line 198 rather than 63. Initial region coverage falls from 98.46%
  to zero; final complete coverage becomes zero.
- `blogwright.secret-upsert.debug`: fixed already delivers the required region in
  the initial response. Structured misses it and substitutes `pds/commands.ts`
  for `aws/secretsmanager.ts` among follow-up reads.
- `nanus.fresh-legacy-approval.debug`: structured initial evidence covers 90%
  versus zero; its later `agent_loop.rs` read starts at 2230 rather than 2151,
  and completes the required region.
- `blogwright.fresh-refresh-metadata.debug`: structured includes the required
  `build.ts` region in the initial response and reads it subsequently; fixed
  spends its three reads on documentation and misses the required file.

`action-analysis.json` records initial source-valid coverage, candidate headers and
follow-up actions without copying raw source transcripts into the repository.

## Decision

Structure is useful for source identity and association, but this capture does not
establish a universal retrieval improvement. Keep the demonstrated regressions as
release evidence for candidate/context work under recommendations 11/12/30. Do not
tune vocabulary or special-case the measured prompts. Recommendation 9 still has
package association dependent on 21; a separate documentation-only intervention
is needed to attribute comment segmentation's effects independently of declaration
boundaries and entity grouping.
