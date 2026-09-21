# Native scalar posting codec: measured, not adopted

The native research codec passes round-trip and seek equivalence checks, but does
not justify replacing production posting vectors. A 128-entry restart design
reduces ordinal payload bytes while making sampled lower-bound seeks much slower.
Its directory allocations and inline headers are particularly expensive for tiny
lists. This is a decision about this implementation and workload, not a rejection
of all compression or a whole-query performance result.

## Contract and measurement

The codec uses unsigned canonical base-128 varints. Each block of at most 128
ordinals starts with an absolute ordinal; the remaining values are positive
deltas. A sorted `(first ordinal, byte offset)` restart directory supports binary
search followed by decoding at most one block. Safe Rust checks byte bounds,
integer overflow, canonical encodings and strictly increasing ordinals. No SIMD,
unsafe code, dependencies, persisted format, or production API was added.

The disposable release build captures all four native lanes: metadata split,
metadata identifiers, body split and body identifiers. Across nanus, blogwright
and whatsurvey this comprises **136,998 lists and 2,890,815 ordinals**. Every list
is encoded, decoded and compared element-by-element with its original plain
vector. Five lower-bound targets per list are checked independently using plain
vector binary search. Three fresh open processes per corpus reproduce the same
composition and checksums. Source strings are not exported in measurement output.

Timing uses a deterministic reservoir of at most 64 lists per lane and size
bucket (`<=1`, `2–4`, `5–16`, `17–128`, `>128`). Each selected list receives 64
seek targets: existing ordinals, adjacent values, distributed targets and misses
beyond the final ordinal. Sampled targets are independently checked before
timing. Six alternating plain/codec batches each perform 16 passes; three fresh
processes yield 18 paired batches per lane. Both scan and seek arms produce the
same checksums. Rust `black_box` prevents eliminating input/output work.

The scan baseline visits compact plain ordinals and folds the same checksum;
the codec also validates while decoding. The seek baseline is plain-vector binary
search. The build baseline copies an already extracted vector; the codec encodes
that vector. **None is a full query, extraction, index build, or incremental update
benchmark.** Actual production ordinals reside in posting structs with additional
fields, rather than these standalone compact vectors.

## Results

Decimal MB for the modeled standalone ordinal representations:

| Corpus | Plain live bytes + inline headers | Codec live bytes + inline headers | Codec capacity bytes + inline headers |
|---|---:|---:|---:|
| nanus | 6.34 MB | 2.66 MB | 4.25 MB |
| blogwright | 2.91 MB | 2.90 MB | 4.87 MB |
| whatsurvey | 17.16 MB | 7.92 MB | 12.81 MB |

These figures include restart-directory entries and per-list Rust struct sizes.
They exclude dictionary storage, allocator metadata, scoring fields, positions,
list sharing, other index objects and any persisted framing. They are **not RSS
or whole-index savings**. Plain live bytes assume exact-length ordinal vectors;
the final column exposes codec vector capacity, not a like-for-like comparison
against production vector capacities. The existing composition output separately
retains the actual production posting lengths, capacities and struct sizes.

For lists longer than 128 entries, median codec/plain paired ratios across the
equally weighted lane/process/batch observations are:

| Corpus | Scan | Lower-bound seek | Encode / plain copy |
|---|---:|---:|---:|
| nanus | 1.83× | 24.12× | 7.61× |
| blogwright | 1.79× | 23.50× | 7.69× |
| whatsurvey | 1.86× | 23.94× | 8.34× |

Ratios are not weighted by query frequency. All five buckets, extrema, counts
and raw timings are retained in `summary.json` and the nine `*-codec.json` files.
For singleton lists, a restart and two vector headers overwhelm the useful byte
saving. Even with many longer lists, this implementation provides no evidence
that its memory/latency tradeoff improves the hot query path. It remains research
code; production vectors are unchanged. A selective layout, smaller restart
blocks or a different codec would need a new measured comparison, including the
actual posting payload and end-to-end query/maintenance costs.

## Verification and limitations

- Three standalone Rust tests cover empty/singleton inputs, varint boundaries,
  `usize::MAX`, every gap around block boundaries, 10,000 deterministic nonuniform
  values, nonmonotonic inputs, truncation, noncanonical encodings, overflow,
  duplicate deltas, trailing bytes and inconsistent counts.
- `checks.json` proves frozen source/binary identity, unchanged sibling repository
  snapshots and unchanged original CodeGraph indexes. All three fresh-process
  composition captures per corpus agree apart from timings.
- All 140 production crate hashes still match the preceding verified workspace
  increment (477 passing tests). No production dependency or implementation changed.
- The [first attempt](../posting-codec/README.md) stopped when the sandbox denied
  `ps`. This successful run explicitly disables process-memory sampling; memory
  fields are null. No process-memory result is inferred.
- This experiment adds concrete scalar decode/seek economics to recommendation
  27. Complete heap attribution and a beneficial integrated compression design
  remain unproven; the recommendation is still open.

## Reproduce

Run sequentially, without competing builds/tests/benchmarks:

```sh
rustc --edition 2024 --test research/instrumentation/posting-codec/codec.rs -o /tmp/graph-search-posting-codec-tests
/tmp/graph-search-posting-codec-tests
python3 research/scripts/storage_composition.py --posting-codec --skip-process-memory --output /tmp/graph-search-codec-results
python3 research/scripts/posting_codec_summary.py /tmp/graph-search-codec-results
```

The output directory must not already exist. The driver uses existing offline
Cargo dependencies and builds/indexes in a disposable temporary directory. Input
hashes, injected-source hashes, platform, compiler, binary hash and temporary
paths are recorded in `provenance.json`.
