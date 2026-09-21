# Incremental native metadata postings

**Decision: use delta maintenance for resident metadata generations.** It
preserves full-rebuild scores and ordering, avoids reanalyzing unchanged scoring
fields, and improves the measured update work. Both memory and Grafeo adapters
use it; opening persisted state still builds a fresh resident index.

## Implementation and invariants

`MetadataIndex::updated` creates a new immutable view. It matches old symbols by
stable ID and verifies the fields used by analysis: name, qualified name, path
and signature. Each old ordinal can be claimed only once, including when callers
construct duplicate IDs. New/changed documents go through the ordinary analyzer.
Unchanged documents retain their split/whole frequencies and lengths.

Compact ordinals still follow the deterministic path/line/ID order. If additions,
deletions or span changes move an ordinal, affected term lists are remapped and
sorted. Lists with no changed membership, frequencies or ordinal positions share
their immutable allocation through standard-library `Arc`. Removed terms vanish;
new terms enter the ordered dictionary. No term list is mutated in the old view.

Corpus length totals are updated by subtracting removed/changed documents and
adding the analyzed delta. Average lengths use those integer totals and the new
population, avoiding floating-point update drift. Document frequencies are the
updated list lengths. Query-time scoring uses the new statistics, even when a
posting list is shared. Both split and identifier-aware indexes follow this path.

Name maps, file-language metadata and compact ordering are reconstructed for the
new view. These global passes remain; this is not an O(changed-files)-only sync
algorithm. Body indexes, graph reconstruction and source/fact persistence retain
their separate implementations. There is no new persistent format or dependency.

## Correctness

All **337 workspace tests** pass on the final implementation. New tests compare
complete posting contents, integer totals, field lengths, averages and score
bits with full reconstruction, including empty corpora, additions, deletions,
reordering and four field configurations. A 48-step metadata mutation sequence
checks exact maps and ranked results under both analyzers and both normalization
policies, while checking the prior generation remains intact. Pointer-identity
checks prove that untouched lists share storage rather than merely returning the
same scores. Existing publication/failure/reopen and retrieval tests also pass.

Workspace/all-target strict Clippy, the release research probe's strict Clippy,
formatting and diff checks pass. Logs are
`/tmp/lexical-update-workspace-final.log`,
`/tmp/lexical-update-clippy-complete.log`, and
`/tmp/metadata-update-probe-clippy-final.log`.

## Resident metadata experiment

One warmup precedes five alternating-order pairs for each of six changes at
1,000 and 50,000 symbols: **60 pairs, 120 measured constructions**. Both arms
retain the old generation; input cloning is inside the timed boundary. Graph
work, source parsing and persistence are excluded. Every pair checks candidate
counts and top-50 ID/score bits for four fixed queries under both analyzers and
normalization policies. The old generation is checked after every workload.

| Symbols | Change | Full median, ms | Delta median, ms | Median paired delta, ms |
|---:|---|---:|---:|---:|
| 1,000 | Body span only | 7.32 | 1.28 | −6.04 |
| 1,000 | One signature | 6.35 | 1.34 | −4.99 |
| 1,000 | Append symbol | 6.33 | 1.22 | −5.11 |
| 1,000 | Insert before existing paths | 6.39 | 1.48 | −4.91 |
| 1,000 | Delete middle symbol | 6.33 | 1.24 | −5.09 |
| 1,000 | Half of signatures | 6.19 | 4.26 | −1.93 |
| 50,000 | Body span only | 372.05 | 74.45 | −293.66 |
| 50,000 | One signature | 360.42 | 89.13 | −271.83 |
| 50,000 | Append symbol | 365.03 | 77.86 | −287.17 |
| 50,000 | Insert before existing paths | 369.79 | 96.00 | −273.98 |
| 50,000 | Delete middle symbol | 361.75 | 80.82 | −280.83 |
| 50,000 | Half of signatures | 358.56 | 263.49 | −96.06 |

Delta maintenance was faster in every measured pair. These are synthetic resident
construction measurements, not a service SLA or total-sync speedup. Both arms
use the current immutable posting representation; this does not measure the
allocation overhead relative to the earlier non-shared representation. `Arc`
introduces reference-count/header and allocation costs; total RSS and retained
reader memory remain separate measurement work.

## Publication/sync experiment

The existing storage driver builds a control in a disposable checkout. Its only
production change replaces `prepared.refresh_indexes(Some(&self.metadata))`
with `prepared.refresh_indexes(None)`, restoring full metadata reconstruction.
The original checkout is untouched. Each corpus has one warmup per variant and
seven alternating-order pairs, with a fresh disposable store per run.

| Corpus | Full sync median, ms | Delta sync median, ms | Median paired delta, ms | Faster pairs |
|---|---:|---:|---:|---:|
| Frozen graph-search archive (`bdcbecef…`) | 370 | 365 | −5 | 6/7 |
| Synthetic Rust: 128 files × 64 functions | 622 | 605 | −50 | 7/7 |

The median of paired differences is not the difference of the two marginal
medians; the synthetic runs show substantial shared timing variation. Initial
build paired medians were −15 ms and −3 ms, respectively. Reopen medians were
273/273 ms on the archive and 395/399 ms on synthetic Rust (full/delta). Opening
always reconstructs resident metadata, so no reopen improvement is claimed.
Seven pairs on these corpora do not establish tail latency or general speedups.

Every run passes graph/fact equality against a clean reindex, reopen equality
and no-op artifact stability checks. Generation and manifest byte sizes match
between variants in every pair. The change affects resident maintenance, not
the persisted representation. Raw rows and the exact control patch are retained
in [metadata-delta-publication](../metadata-delta-publication/paired-summary.json).

## Provenance and reproduction

`results.json` and `summary.json` retain the native pairs and derived summaries.
`source-before.json` equals `source-after.json`, and those hashes were rechecked
against current source after both experiments. The native host hash is stable;
the publication comparison records both binaries, the isolated control edit and
its source hashes. No sibling repository or CodeGraph index was used or changed.

```sh
cargo run --release --offline --locked --manifest-path research/harness/Cargo.toml --bin metadata_update_probe
python3 research/scripts/storage_compare.py --output /tmp/metadata-publication --pairs 7 --synthetic-files 128 --synthetic-kind rust --control metadata-rebuild
```

The adjacent `metadata-deltas-initial` capture predates incremental corpus totals
and uses slightly different signature fixtures. It is preliminary evidence,
not an isolated comparison with this final capture.

This completes recommendation 7's generation caching and changed-document
posting/statistic maintenance. It does not complete recommendation 23's narrower
graph/fact invalidation or the remaining memory/concurrency/performance gates.
