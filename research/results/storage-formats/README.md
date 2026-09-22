# Storage and serialisation formats

**Outcome: generation format 8.** Source facts use a compact binary record codec
(`GSR1`) inside zstd-compressed packs. The three large JSON sidecars are
zstd-compressed, and every content hash is BLAKE3. The body-index build no longer
clones term maps. On this repository the index is **12.8× smaller** and one-shot
queries are **3.4× faster**, with identical query results. All 25 criterion
benchmarks are unchanged or faster.

Measured on 22 September 2026 on one Apple-silicon Mac with a warm page cache. No
x86 or Graviton host was available; see [Limitations](#limitations).

## The problem

Every one-shot CLI invocation opens the published generation, verifies it and
decodes all of it before answering. On this repository that took about 7 s and
held 1.4 GB. The store was 316 MiB for 151 MB of indexed source.

| Artifact (format 7) | Size | Content |
|---|---:|---|
| `source-records/` | 260 MiB | Per-file source facts, JSON, in 8 MiB packs |
| `occurrences.json` | 19 MiB | Reference occurrences, JSON |
| `extraction-records/` | 19 MiB | Cached extraction facts, JSON packs |
| `dangling.jsonl` | 6.1 MiB | Unresolved references, JSON lines |
| `graph.grafeo` | 3.9 MiB | Grafeo graph |
| `dependencies.json` | 3.0 MiB | Dependency index, JSON |
| Other | 2.1 MiB | Manifest and pack indexes |

Posting maps (`term -> [line, …]` per region) were 96% of source-record bytes.
JSON repeated every term string in every region that contained it and spelt
every line number in decimal. Term strings alone were 129.5 MB, but they reduce
to 21.5 MB with one dictionary per file, 5.8 MB per pack and 2.6 MB per generation.
Every line list was non-decreasing. Records were JSON facts about files, and the
indexed research JSON made them 1.8× the size of the source they describe.

Where a cold open spent its time ([raw](open-phases-before.txt)):

| Phase (format 7) | ms |
|---|---:|
| **`GrafeoStore::open` total** | **7,231** |
| Read source packs | 64 |
| SHA-256 of packs | 811 |
| SHA-256 of each record again | 816 |
| JSON decode of records | 1,403 |
| `BodyIndex::new` (derived postings) | 2,722 |
| Occurrences, dangling and other sidecars | ~80 |
| Graph open, id maps, validation, metadata/adjacency indexes | ~1,300 |

## Options evaluated

All numbers come from the prototype in
[`research/prototypes/storage-formats`](../../prototypes/storage-formats). It links the
real crates, round-trips every record of this repository's generation, and checks
decoded values for equality ([output](prototype-output.txt),
[JSON](prototype-results.json)). The GSR1 rows compile the production codec's
source file directly.

### Source records (2,515 files, 73k regions, 20.4M line entries)

| Format | Size | Encode | Decode | Round trip |
|---|---:|---:|---:|---|
| JSON (format 7) | 280.5 MB | 313 ms | 1,415 ms | ok |
| MessagePack (named fields) | 215.1 MB | 316 ms | 1,019 ms | ok |
| postcard | 187.3 MB | 251 ms | fails | `skip_serializing_if` fields |
| bincode 2 (serde) | 203.6 MB | 164 ms | fails | `skip_serializing_if` fields |
| JSON + zstd-3 per record | 45.4 MB | 794 ms | 1,507 ms | ok |
| MessagePack + zstd-3 per record | 44.1 MB | 642 ms | 1,163 ms | ok |
| **GSR1** | **68.7 MB** | **356 ms** | **491 ms** | ok |
| GSR1 + zstd-3 per record | 32.2 MB | 547 ms | 616 ms | ok |
| rkyv mirror, zero-copy | 327.9 MB | 336 ms | 74 ms validate and walk; 393 ms owned | mirror type only |
| JSON, zstd-3 per 8 MiB pack | 24.5 MB | +311 ms | +121 ms | bytes only |
| **GSR1, zstd-3 per 8 MiB pack** | **13.6 MB** | **+118 ms** | **+42 ms** | bytes only |
| GSR1, zstd-9 per 8 MiB pack | 12.5 MB | +406 ms | +39 ms | bytes only |

Conclusions:

- **Positional serde formats are unsafe here.** The types use
  `skip_serializing_if` over a hundred times, so postcard and bincode write
  records they cannot read back. They do round-trip occurrences, dangling
  references and dependencies today, but any later optional field would break
  them silently.
- **Generic self-describing formats don't fix the cause.** MessagePack still
  repeats every term per region, saving 23% of the size and 28% of decode time.
- **GSR1 targets the actual redundancy.** It is 4.1× smaller and decodes 2.9×
  faster, at about JSON's encode cost. Each record carries a sorted, front-coded
  dictionary of its own terms, identifiers and owners. Postings are ascending
  dictionary-id deltas with delta-coded lines, all as varints. The source hash is
  stored as 32 raw bytes, and JSON is kept only for rare Markdown, documentation
  and package fields, so their serde contracts stay authoritative.
- **Why a per-record dictionary.** A dictionary per generation would store term
  strings in 8× fewer bytes (2.6 MB against 21.5 MB), but record bytes would then
  depend on the rest of the generation. That
  breaks the existing design, where records are hashed individually, reused
  across generations by hash, and read selectively.
- **Compress per pack, not per record.** One zstd frame per 8 MiB pack shrinks
  GSR1 a further 5×, to 13.6 MB, and inflates in 42 ms. Records are still hashed
  individually and addressed within the inflated pack. A selective read inflates
  the one pack holding the record, at most about 8 MiB.
- **Zero-copy (rkyv) is the ceiling, not a drop-in.** Validation plus a full walk
  costs 74 ms instead of 491 ms of decoding, but at 24× GSR1+zstd's size. It only
  pays off if the query path reads archived data directly instead of owned
  `BTreeMap`s. That is an API change, recorded below.

### Other artifacts

| Artifact | JSON | JSON + zstd-3 | bincode 2 | Chosen |
|---|---:|---:|---:|---|
| Occurrences | 21.3 MB, dec 33 ms | 3.2 MB, dec 45 ms | 11.4 MB, dec 9 ms | JSON + zstd |
| Dangling references | 11.7 MB, dec 13 ms | 0.6 MB, dec 18 ms | 8.8 MB, dec 4 ms | JSON + zstd |
| Dependencies | 3.2 MB, dec 8 ms | 0.4 MB, dec 10 ms | 2.4 MB, dec 5 ms | JSON + zstd |

Compression keeps each serde contract unchanged and makes these 7–20× smaller.
It adds about 20 ms of decompression on open. Hashing 7× fewer committed bytes
recovers most of that, and the end-to-end open benchmarks still improved. bincode
would decode faster but has the positional-format hazard above.

### Storage engines (same GSR1 record bytes, fsync on every commit)

| Engine | Disk | Bulk write | Update one record | Read all | Read 50 |
|---|---:|---:|---:|---:|---:|
| Packs (format 7 design) | 69.4 MB | 168 ms | 18 ms | 15 ms | 1.9 ms |
| SQLite (WAL, `fullfsync`) | 70.8 MB | 402 ms | 13 ms | 18 ms | 0.9 ms |
| redb | 101.3 MB | 138 ms | 27 ms | 45 ms | 13.0 ms |

Keep the packs. SQLite is similar except for slower bulk writes and would replace
a verified, generation-atomic design for no measured gain. redb is larger and
slower to read. The storage engine was never the problem; the encoding was.

### Hashing

The same 280 MB, single-threaded:

| Hash | ms |
|---|---:|
| SHA-256, `sha2` 0.10 software path (format 7) | 833 |
| SHA-256, `sha2` 0.11 using ARMv8 SHA2 instructions | 132 |
| BLAKE3 (NEON) | 156 |

On this machine, hardware SHA-256 is slightly faster than BLAKE3. The index must
also run on Intel, AMD and Graviton. SHA-256 is fast only where dedicated
instructions exist, and older Intel Xeons (Skylake and Cascade Lake, e.g. AWS
c5/m5) lack SHA-NI. BLAKE3's speed comes from generic SIMD, dispatched at runtime
between NEON, SSE4.1, AVX2 and AVX-512. BLAKE3 was adopted as the portable choice,
for both workspace file fingerprints and artifact integrity. Hashing each record
a second time on open was kept, because on 4× fewer bytes it now costs about 40 ms.

## What shipped

- **`crates/engine/src/record_codec.rs`:** GSR1 for source records, JSON for
  extraction records, behind an encode/decode record trait. Record-index format 3
  means native records.
  - Decoding is bounded. It rejects truncation, trailing bytes, out-of-range ids,
    non-ascending ids, unsorted dictionaries, and counts larger than the remaining
    record, before allocating.
  - Each string occurrence is hashed once with the randomly keyed std `HashMap`;
    only distinct keys are sorted.
- **`crates/engine/src/compress.rs`:** zstd level 3 frames.
  - Pack names hash the compressed file, so corruption is caught before inflation.
  - Pack inflation is bounded by the frame's declared size and a 1 GiB ceiling.
  - Sidecars decode by streaming.
  - Sidecars are renamed `occurrences.json.zst`, `dangling.jsonl.zst` and
    `dependencies.json.zst`.
- **Hashing:** `graph_search_core::hash::content_hash` is BLAKE3, and generation
  format 8 is required. Older generations fail verification and must be rebuilt,
  which is safe because the index is derived data. Two tests that simulated
  format-5 and format-6 migrations were removed.
- **`BodyIndex::new`:** the build now produces identical postings with less work.
  - It computes each posting's frequency and first line without cloning
    per-region term maps.
  - It merge-joins terms with lowercased identifiers.
  - It numbers entities per file instead of cloning the path for every region.
  - It keeps exact-term postings in a `HashMap`, which is only ever looked up.
  - It went from 2,722 ms to 1,042 ms. Retained heap is 339 MB, against 341 MB
    before, with no transient peak above it.
- **`crates/graph-search/benches/storage.rs`:** a new criterion suite. Its corpus
  includes hash-heavy JSON. It covers full publication, read-only re-open (bare
  and followed by symbol, text and explore queries), and no-op, Rust-edit and
  JSON-edit syncs. Benchmark ids are stable; the store size is printed.

## Verification

- **Tests and lint:** 579 workspace tests pass, including new codec round-trip,
  truncation, late-index and bounded-count tests. Strict `clippy -D warnings`
  and `rustfmt` pass on all changed files.
- **Criterion:** the release gate's `search` suite, the `evaluation` suite and the
  new `storage` suite ran against `--save-baseline before` on unmodified `main`
  ([output](criterion-final.txt)). The evaluation baseline was recorded from a
  clean `main` worktree.

| Benchmark | Time | Change | Verdict |
|---|---:|---:|---|
| `storage_publish/reindex_full` | 493.24 ms | -26.7% | improved |
| `storage_open/published_read_only` | 170.43 ms | -52.1% | improved |
| `storage_open/open_then_symbol` | 171.05 ms | -52.0% | improved |
| `storage_open/open_then_explore` | 173.87 ms | -51.6% | improved |
| `storage_open/open_then_text` | 176.02 ms | -51.3% | improved |
| `storage_sync/noop` | 873.72 µs | -0.2% | no change |
| `storage_sync/rust_body_edit` | 316.21 ms | -27.6% | improved |
| `storage_sync/json_edit` | 320.05 ms | -27.0% | improved |
| `build/cold_index` | 585.10 ms | -25.0% | improved |
| `sync/one_file_edit` | 417.97 ms | -25.8% | improved |
| `lookup/exact_symbol` | 1.3916 ms | -1.4% | within noise |
| `lookup/references` | 1.4017 ms | -1.2% | within noise |
| `explore/body_multiword` | 6.4943 ms | -0.2% | no change |
| `explore/metadata_single_term` | 2.2651 ms | -0.0% | no change |
| `explore/positional_route` | 2.9251 ms | -3.3% | improved |
| `occurrences/by_name` | 1.3992 ms | -0.0% | no change |
| `scan/text_literal` | 7.5747 ms | -0.8% | no change |
| `scan/files_glob` | 1.4840 ms | -0.7% | no change |
| `scan/filtered_explore` | 6.3911 ms | +0.3% | no change |
| `open/published_store` | 59.925 ms | -28.7% | improved |
| `graph/refs_type` | 1.0681 ms | -1.7% | improved |
| `graph/impact_type` | 1.0668 ms | -0.9% | within noise |
| `payload/explore_compact` | 2.5496 ms | -2.1% | improved |
| `payload/explore_full` | 2.6204 ms | -1.4% | within noise |
| `payload/text_search` | 5.0190 ms | -5.6% | improved |

The storage-suite store shrank from 22.3 MB to 3.3 MB.

**This repository, release CLI** (median of 5 per query and 3 per index, interleaved):

| Measure | Format 7 | Format 8 |
|---|---:|---:|
| Store size | 316 MiB | 24.7 MiB |
| `GrafeoStore::open` | 7,231 ms | 2,244 ms |
| `search symbol GrafeoStore` | 7.15 s | 2.05 s |
| `search text content_hash` | 7.00 s | 2.15 s |
| `search refs SourceFileUnits` | 6.84 s | 2.04 s |
| `search explore "storage generation pack"` | 7.25 s | 2.07 s |
| `sync` after a one-file edit | 14.97 s | 6.38 s |
| Full `index` | 12.64 s | 8.95 s |
| Peak RSS, full `index` | 1,736–1,794 MB | 1,696–2,064 MB |

Seventeen queries across `symbol`, `text`, `refs`, `callers`, `callees`, `impact`,
`deps`, `occurrences`, `explore` and `files` return byte-identical JSON from both
binaries. The comparison normalises only hash values, `elapsed_ms`, generation ids
and store paths.

The first version of the body-index change raised peak RSS during a full index by
about 200 MB at equal heap size. A counting allocator traced it to posting lists
starting at capacity 1 and reallocating on the second posting. Starting at
capacity 4, as the old `push` did, returned peak RSS to the old run-to-run range.
That range is wide (one run of each binary sits well outside the rest), so
indexing peak memory is best described as unchanged within noise.

## Remaining opportunities

1. **Stop retaining complete term maps after building the indexes.** They are
   about 700 MB of the 1.4 GB RSS. The CLI also spends about 350 ms freeing them
   at exit. Publication currently compares and re-encodes records from these
   in-memory facts, so this needs care with reuse.
2. **Persist or lazily build `BodyIndex`.** Its 1.0 s rebuild is now the largest
   open cost. rkyv-style zero-copy postings are the measured ceiling above.
3. **The rest of open, about 550 ms.** Graph load, id maps, fact-owner
   validation, and metadata and adjacency indexes. Not investigated.
4. **Positional encodings for the sidecars.** They would save about 30 ms of
   decode, but only with guarding tests for every optional field.
5. **zstd level 9 for packs.** 8% smaller for about 290 ms more encode on a full
   index.

## Limitations

- **One machine.** Apple silicon, warm page cache; no x86 or Graviton measurement.
  BLAKE3 and zstd both dispatch SIMD at runtime; their relative behaviour on
  those CPUs is taken from their design, not measured here.
- **Criterion baselines.** Evaluation payload ids embed result byte counts, which
  vary by one or two bytes with `elapsed_ms` width. Comparing required aliasing
  the saved baselines for ±2 bytes; no benchmark content changed.
- **Old indexes.** They are not migrated; delete `.graph-search/index` or run
  `graph-search index`.

## Reproduce

```sh
# Criterion (save a baseline on main first, then compare on this branch)
cargo bench -p graph-search --bench storage -- --save-baseline before   # on main
cargo bench -p graph-search --bench storage -- --baseline before        # on this branch
# likewise --bench search and --bench evaluation

# Format, engine and open-phase measurements on a copy of a real index
cp -R .graph-search/index /tmp/index-copy
cargo run --release --manifest-path research/prototypes/storage-formats/Cargo.toml -- \
  /tmp/index-copy results.json
```
