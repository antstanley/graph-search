# Reuse source captures across strict explore phases

Public explore now shares captured raw bytes across freshness, automatic maintenance
and source evidence. Ranker policy is 20; wire 4, source representation 9 and parser
policy 7 remain unchanged. This comparison isolates capture reuse under strict
content verification, with a resident native index and unchanged source trees.

## Controlled comparison

54 trials: three queries per repository, two arms, three repeats. Both arms use
strict content verification, eight selected seeds, one graph hop and a 16,384-byte
response cap. The control disables only `work.enable_source_capture()` in the public
explore service. Build/source hashes and the exact intervention are in `build.json`.
Each process uses its own temporary native store; sibling trees and their existing
CodeGraph indexes are read-only.

All source-bearing snippets/excerpts were checked against original file lines and
full-file hashes. No duplicate source coordinate was returned. All nine paired
queries returned identical results after removing only statistics and generation
IDs, including identical delivered line sets. Each arm was repeat-identical in
normalized results and source-work counts. Production, binaries, sibling sources
and CodeGraph indexes were stable throughout the capture.

| Repository | File opens/query, capture | File opens/query, control | Source bytes/query, capture | Source bytes/query, control | Byte reduction |
|---|---:|---:|---:|---:|---:|
| nanus | 164 | 333–336 | 3,754,686 | 8,122,411–8,345,544 | 53.77–55.01% |
| blogwright | 169 | 345–347 | 956,204 | 1,986,625–2,008,296 | 51.87–52.39% |
| whatsurvey | 1,238 | 2,493–2,496 | 8,380,105 | 16,955,779–17,049,483 | 50.58–50.85% |

These are instrumented file-open attempts and source bytes consumed by the public
query, not physical-device I/O or a latency claim. Strict verification still reads
all indexed files once. Explicit setup/reindex and the probe's independent returned-
source checks occur outside query counters. The three queries per repository are
convenience samples; this is neither a ranking-quality study nor a model task-success
experiment. Raw capture retention adds request-scoped memory; RSS impact has not
been measured here and remains part of recommendation 27.

## Correctness and limits

The 387-test workspace run passed in 31 suites. Final focused checks after the last
metadata guard and insufficient-budget assertions passed all 152 core unit tests
and 21 public work-budget tests. Strict workspace/all-target Clippy and the release
probe's Clippy check pass. Formatting and whitespace checks pass. No dependency
manifests or lockfiles changed.

Five new core cases exercise exact-byte allowance reuse, shared allocation identity,
truncated-prefix rejection, the independent evidence byte cap, drift/missing-file
creation, cancellation, raw binary/invalid UTF-8 verification, bounded failed opens,
and unchanged accounting when capture is disabled. Two new public tests cover cold
builds, metadata edits, strict restored-mtime edits, one-read evidence delivery,
request isolation and failed verification leaving the published generation intact.
The existing freshness/work-budget regression now checks that explore needs one
read where the control needed three; other query modes retain their prior accounting.

A capture is a version observed during the request, not an atomic snapshot of the
live filesystem. Size/mtime changes cause an incomplete-verification error on reuse;
a concurrent writer that restores metadata can evade that guard. Returned hashes
still identify captured bytes, and the next strict request captures source anew.
A captured prefix cannot be promoted to a complete source file by a later larger
read request. Cancellation and deadlines remain cooperative and apply to cache hits.

## Reproduction

```sh
python3 research/scripts/source_capture_review.py --output <new-output-directory>
```

Queries are recorded in `queries.json`; `results.json` contains only source
coordinates/hashes, statistics and normalized-result hashes, not source text.
Temporary frozen binaries are at
`/var/folders/ft/6fdqp3c914xfdlk47w5cblzc0000gn/T/graph-search-source-capture-8padq9tf`.
See `CONTEXT-SELECTION-AUDIT.md` for recommendation 12's remaining requirements.
