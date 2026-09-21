# Generation churn and retained-reader experiment

The native probe runs real extraction, resolution and persistent publication in
separate writer/reader processes over disposable generated Rust repositories.
No production instrumentation, dependency, sibling source or CodeGraph index is
changed. See `validation.json` for the completion certificate; partial runs alone
do not establish completion.

## Results

All **24 runs passed**, with **354 retained-reader fingerprint checks** and
**168 incremental/clean-rebuild comparisons**. All frozen inputs were unchanged.
The exact retained-directory assertion passed at every recorded state, including
individual reader releases. A final no-op sync preserved identity in every run.

| Files | Reader arm | Max generations | Median sync ms | Step 24 unique inode MiB |
|---:|---|---:|---:|---:|
| 64 | none | 2 | 79.91 | 1.58 |
| 64 | one | 3 | 83.68 | 2.70 |
| 64 | distinct | 5 | 82.86 | 4.95 |
| 64 | shared | 3 | 82.69 | 2.70 |
| 256 | none | 2 | 323.76 | 12.56 |
| 256 | one | 3 | 331.34 | 16.12 |
| 256 | distinct | 5 | 325.75 | 23.37 |
| 256 | shared | 3 | 325.69 | 16.12 |

Timings pool the 24 ordinary updates across three repeats per arm; mutation
kinds differ, so these are descriptive lifecycle costs, not statistically
established reader-overhead estimates. Step-24 bytes are the median across three
runs before readers release. Three readers sharing a generation have the same
retention count and file-byte footprint as one reader. Distinct pinned histories
retain more data. After all readers release and a publication runs, every arm
returns to two generation directories.

No production changes were needed to pass this experiment. Recommendation 28
remains open for the remaining resource assessment; process RSS, peak transient
space and concurrent query throughput were not measured here.

## Design

Two scales: 64 files × 8 functions and 256 files × 16 functions. Four arms retain
zero readers, one reader, three readers of distinct generations, or three readers
of one generation. Three fresh-process repeats rotate arm order. Each run makes
24 publications: four cycles of body edits, declaration addition/removal, file
rename, and file addition/removal. The body-edit fixture changes a source comment;
it exercises source/extraction publication without changing graph topology.

Readers deliberately leave extraction manifests lazy until at least two newer
publications. Repeated checks compare sorted node/edge records, complete source
facts, occurrence facts and extraction entries against the original generation.
Seven clean rebuild comparisons per run cover each mutation kind in the first
cycle and the final state. This is not a clean-rebuild comparison at every step.

After churn, readers exit individually, alternating abrupt `process::exit(87)`
without Rust destructors and normal exit. Each release is followed by a real
publication. At every recorded state, the exact set of retained directories must
equal current + previous + distinct actively leased generations. Finally a no-op
sync must preserve generation identity. Parent cleanup kills/reaps remaining
children on an assertion or protocol failure. No automatic child restarts occur.

## Measurement interpretation

`runs.json` preserves every stage, elapsed native sync time, logical file bytes,
unique-inode file bytes, allocated unique-inode blocks and generation IDs.
Hard links count repeatedly only in logical bytes. Allocated blocks are filesystem
accounting, not physical I/O or APFS clone/copy-on-write accounting. Directory
metadata is excluded. Fingerprinting and clean rebuild checks are outside sync
timing but may affect subsequent caches. Runs are sequential; no builds or tests
run concurrently. These are small synthetic local workloads, not production
throughput or simultaneous-reader query latency measurements.

`environment.json` records platform, Python and SHA-256 identities of all Rust
crate sources, probe, driver and executable. The completion check requires these
inputs to remain unchanged. This experiment does not sample RSS, peak transient
disk, failure-induced cleanup retention or arbitrary scheduler interleavings.
Existing reader-lease race/failure tests remain complementary evidence.

## Reproduce

```sh
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin generation_churn_probe
python3 research/scripts/generation_churn.py /tmp/graph-search-generation-churn
```

No finite global retention bound is implied: every distinct deliberately pinned
generation can retain additional history. Reclamation is triggered by a later
publication, not immediately by reader exit. The existing `GraphStore: Send`,
borrowed snapshot and writer exclusion contracts are unchanged.
