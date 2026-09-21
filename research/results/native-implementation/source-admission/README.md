# Source-read admission under the response budget

Ranker 21 checks selected-item metadata and necessary response overhead before
materializing primary source. Oversized metadata cannot consume source allowance
needed by a later fitting candidate. An admitted item also needs room for at least
a full source hash, a positive line coordinate and one source line. Optional edges,
package/source tables and preceding primary text are excluded from the response
floor, preserving a conservative bound under deduplication and final trimming.
Wire 4, source representation 9 and parser policy 7 remain unchanged.

## Controlled corpus comparison

348 trials cover 58 frozen tasks, two arms and three repeats in nanus, blogwright
and whatsurvey. The control disables only the early metadata/source-admission guards;
its later exact payload checks remain intact. Both arms retain request source capture,
primary deduplication, package identity sharing and the existing ranking. The exact
interventions and source/binary hashes are recorded in `build.json`.

All trials complete without errors. Production, binaries, sibling source and
CodeGraph snapshots remain stable. Repeated source-line sets, actions and evidence
are identical. Every task has unchanged delivered lines and required-file/region
coverage between arms. The control reproduces the preceding primary-dedup capture's
oracle evidence on all 58 tasks.

| Suite | Tasks | Required files, both arms | Complete regions, both arms | Mean region coverage, both arms |
|---|---:|---:|---:|---:|
| Established | 34 | 28 | 12 | 48.97% |
| Fresh routing | 12 | 12 | 10 | 84.72% |
| README | 12 | 10 | 8 | 77.08% |

A separate raw probe covers 116 API responses. Selected node IDs and source-work
counts are identical between arms. All 464 common-node package associations agree,
every package reference resolves, and no orphan table entry survives. The probe
also verifies binary/adapter hashes and source/index snapshots against the capture.

The 16 KiB corpus protocol does not demonstrate fewer source reads for these queries.
The saved-read behavior is established by the focused tight-budget regression below.
No corpus speedup, RSS reduction, or model task-success claim follows. Source reads
required by candidate generation or strict freshness remain outside these admission
guards. Convenience samples and complete-region metrics are not blind task-success
measurements.

## Regression evidence

All 388 workspace tests pass in 31 suites, with no failures or ignored tests. Strict
workspace/all-target Clippy passes after test-only naming/style fixes. Formatting
and whitespace checks pass. No dependency manifests or lockfiles changed.

The new regression uses a native memory-store snapshot with two selected definitions
and a one-file source allowance. At a 4 KiB response cap, the first definition's
metadata cannot fit; the second definition must retain its verified source. The
result opens exactly one file, reads exactly its source length, and reports byte
truncation without inventing source-file/byte exhaustion. A second case isolates
mandatory response overhead when item metadata alone fits, verifies zero source
reads, and confirms source delivery returns when the response cap is raised.

`research/CONTEXT-SELECTION-AUDIT.md` records the admission argument, including why
the metadata prefix is necessary for an item to survive final tail trimming.
Recommendation 12 remains open for its remaining requirements.

## Reproduction

```sh
python3 research/scripts/markdown_context.py --representation source_admission \
  --output <new-output-directory> --repeats 3 \
  --suite research/fixtures/fresh-routing-2026-09-19 \
  --suite research/fixtures/markdown-readme-2026-09-20
python3 research/scripts/package_budget_probe.py --capture <capture-directory> \
  --binaries <raw-build-directory> --all-tasks --verify-sharing
```

Frozen binaries and source-bearing transcripts are outside the repository at
`/var/folders/ft/6fdqp3c914xfdlk47w5cblzc0000gn/T/graph-search-markdown-context-vldgywif`.
Repository artifacts retain sanitized evidence, work counts and identity/span data.
