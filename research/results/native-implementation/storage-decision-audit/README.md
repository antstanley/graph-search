# Recommendation 27: conditional compression decision

**Disposition: retain the current native vectors. Recommendation 27's conditional
measurement/adoption gate is satisfied; no compression candidate is adopted.**
This is not a claim that compression has shipped, that every heap allocation is
attributed, or that performance has been remeasured after parser revision 19.

The recommendation says to start with sorted vectors, measure the named storage
categories, and adopt native compression according to seek/decode workloads. It
explicitly describes alternative structures as alternatives, not a mandatory
checklist. The implementation ledger also says conditional techniques are not
required integrations. Requiring a successful codec or complete process heap
census before accepting *retention of the baseline* would add a requirement that
is absent from the recommendation. Stronger evidence would still be required to
adopt an optimization whose benefit depends on unmeasured memory.

## Requirement-to-evidence audit

| Requirement | Evidence and conclusion |
|---|---|
| Begin with sorted native vectors | Existing metadata/body posting implementations remain in production. Their three defining modules match both composition and codec captures byte-for-byte. |
| Measure terms and postings | Four analyzer lanes expose dictionary UTF-8 bytes, vector lengths/capacities, ABI payload sizes, tiny/long-list distributions and per-list headers. |
| Measure positions and norms | Native source facts retain absolute line occurrences, not token-position streams. Line-vector live/capacity bytes and both metadata norm arrays are measured. |
| Account for source blobs and facts | The measured native postings retain no raw source-text blob. Source/occurrence fact JSON sizes and generation disk categories are reported separately from resident-memory components. Query-time source caches are outside this open-index composition capture. |
| Measure adjacency and metadata | Incident ordinal vectors, inline edge/node capacities, exact-name ordinal vectors, dictionary bytes and serialized graph records are measured. Nested strings/map/allocator overhead are explicitly unattributed. |
| Treat tiny and long lists separately | All five list-length buckets are retained. Singleton restart/header costs differ sharply from long-list payload savings; no universal codec is adopted. |
| Evaluate actual scalar decoding and seek | A native checked delta-varint/restart implementation round-trips all 136,998 lists and 2,890,815 ordinals in its captures. Independent lower-bound checks and timed checksums agree. Long-list sampled seek ratios are unfavorable. |
| Consider construction/update economics | The allocation-compaction candidate has 360 paired full/delta measurements and exact ranking agreement. Its maintenance/RSS evidence does not justify adoption. Codec encode/copy microbenchmarks are separately labeled and are not relabeled as index-update measurements. |
| Version/checksum/bounds/position integrity for a persisted codec | No persisted codec is introduced, so no new persisted decoder or merge contract is claimed. The research codec checks scalar overflow, canonical bytes, truncation and monotonicity. Any future persisted format still needs the full stated gate. |
| No unsafe SIMD or third-party components | Neither experiment adds a production dependency or unsafe implementation. Production retains its existing representation. |

The machine-readable [audit](audit.json) pins every consumed artifact and checks
current `lexical.rs`, `body.rs` and `metadata.rs` against both measured captures.
A parser change can change corpus counts; matching layout source is not a claim
that all corpus data or query costs are unchanged.

## Composition, with units kept separate

Selected native components, decimal MB, from the frozen composition capture:

| Component | nanus | blogwright | whatsurvey |
|---|---:|---:|---:|
| Posting dictionary UTF-8 bytes | 0.319 | 0.237 | 0.663 |
| Live posting payload | 20.70 | 7.33 | 57.98 |
| Posting vector capacity | 30.26 | 11.69 | 87.29 |
| Source line-occurrence payload | 3.43 | 1.08 | 8.10 |
| Metadata norm payload | 0.834 | 0.288 | 2.488 |
| Incident adjacency ordinal payload | 0.290 | 0.082 | 0.652 |
| Edge inline capacity | 5.707 | 1.383 | 17.225 |
| Metadata node inline capacity | 2.032 | 1.016 | 8.126 |

Do not sum these into a process-memory estimate. Some fields describe capacity,
others live length; nested allocations, sharing and allocator overhead need their
own accounting. Serialized source facts are 14.18/4.60/30.75 MB and serialized
occurrence facts 13.07/3.51/44.52 MB. These are **JSON byte counts, not heap sizes**;
they are not added to the native components. Actual generation disk extension
categories and unique-inode allocated bytes are retained in `audit.json`.

## Why the candidates remain rejected

The [allocation-compaction experiment](../posting-compaction/README.md) removes
capacity slack but shows no reliable RSS win and increases measured synthetic
maintenance costs. Its patch was withdrawn and baseline source restored.

The [scalar codec experiment](../posting-codec-measured/README.md) includes actual
restart/header overhead. Modeled standalone ordinal live bytes nearly break even
on blogwright, before capacity slack. On lists longer than 128 entries, median
paired seeks are 23.50–24.12 times slower than compact-vector binary search;
encoding is 7.61–8.34 times slower than copying those vectors. These are kernel
measurements, not predictions of whole-query slowdown. They provide no positive
case for integrating this candidate.

Rejection does not require pretending that either experiment proves every possible
codec will fail. Complete retained-heap attribution, alternate restart sizes,
selective layouts, fixed-width ordinals, inactive-field removal and integrated
query/maintenance comparisons remain possible follow-up investigations. They
must be measured before adoption; they are not unconditional release requirements
introduced by this recommendation. Whole-release quality/resource gates under
recommendation 30 and lifecycle work under recommendation 28 remain open.

## Reproduce the audit

```sh
python3 research/scripts/storage_decision_audit.py
```

This audit reads the existing captures and current layout sources. It does not
rerun a benchmark, modify sibling sources/indexes or alter production code.
