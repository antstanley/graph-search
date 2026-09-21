# Native byte-trigram admission: adoption decision

**Decision: defer a persistent prefilter for the current live literal API.** The
native prototype proves useful on trusted stable snapshots, but the measured
strict-verification pipeline does not beat direct scanning. This closes the
conditional adoption decision in recommendation 16; it does not advertise a
production trigram index or rule out a future immutable-snapshot route.

## Experiment

The prototype indexes distinct raw byte trigrams into sorted file postings, keeps
reverse file-to-gram lists for replacement/removal, intersects candidate postings,
and verifies actual source with the existing production literal scanner. Short,
case-insensitive and weak constraints fall back; files without complete facts are
admitted rather than rejected. All grams being present is explicitly insufficient
for a match. Original source is always verified.

The driver builds a disposable source copy. A small recorded hook admits selected
paths immediately before the production scanner's existing source-open budget
check. Both arms retain the same walker, matcher, Unicode case behavior, source
checks, line/payload limits and response assembly. Unknown/binary/invalid-text paths
are always admitted to the verifier. The shipping scanner is unchanged. The hook
is **not** a freshness implementation: its experiment uses stable source snapshots.
A production integration would need current complete facts and conservative handling
of new, edited, unavailable and truncated files before permitting rejection.

Fifteen literals per repository cover common terms, short/non-ASCII literals,
case-insensitive fallback, an absent long literal, and eight long source tokens
selected by a fixed rule. This is a diagnostic workload, not observed user-query
frequency. All 45 production filtered/unfiltered hit lists agree at a 100-hit cap.
Resident reference checks also compare caps 0, 1, 100 and unlimited. The resident
`str::contains` diagnostic is separate from production matcher timing and must not
be used as a deployment speed estimate.

## Build, storage and maintenance

| Repository | Indexed text files | Source bytes | Build + teardown ms | Logical forward + reverse bytes | Median-file replacement + restore ms |
|---|---:|---:|---:|---:|---:|
| nanus | 163 | 3,341,891 | 50.03 | 3,420,504 | 0.844 |
| blogwright | 167 | 949,978 | 26.97 | 2,389,816 | 0.481 |
| whatsurvey | 1,224 | 8,215,575 | 175.35 | 12,154,688 | 0.137 |

Logical sizes assume four-byte gram keys/document ordinals and exclude dictionary,
vector, allocator and file-map overhead. The actual prototype uses native `usize`
ordinals and tree maps; these figures are not RSS or an implemented on-disk codec.
Build timing starts with resident source strings, includes dropping the built index,
and excludes initial source loading and serialization. Maintenance measures a full
replacement/restore cycle, not one update or a sustained-churn workload.

## Repeated-query result

Each batch below includes one resident-source gram-index build and teardown. The
trusted variant reuses its facts. The strict variant reopens and hashes every
indexed text file before each filtered query. It can read matched files again, so
this is an observed straightforward strict pipeline, **not** a lower bound on an
optimized shared-capture implementation. Timings are warm wall-clock medians, not
cold-device-I/O measurements.

| Repository / repeated literal | Queries | Direct scanner ms | Build + trusted queries ms | Build + strict queries ms |
|---|---:|---:|---:|---:|
| nanus / `DEEPSEEK_API_KEY` | 16 | 111.56 | 88.50 | 258.40 |
| nanus / same | 32 | 222.59 | 125.33 | 470.90 |
| blogwright / `createNodeFileSystem` | 16 | 62.45 | 50.72 | 118.69 |
| blogwright / same | 32 | 124.53 | 77.24 | 213.28 |
| whatsurvey / `AWS_ENDPOINT_URL` | 16 | 515.98 | 349.10 | 937.98 |
| whatsurvey / same | 32 | 1,036.28 | 506.98 | 1,731.50 |

Trusted batches lose at 1 and 4 queries and win at 16 and 32 on all three sampled
literals; the exact first winning query count was not measured. Strict batches
lose at every tested size. Separate read/hash-only medians are 10.75, 4.83 and
36.62 ms respectively. Per-literal selectivity, source reads, native timings and
estimated break-even counts are in the repository JSON files. Those estimates
come from measured medians and do not replace the cumulative curves.

A live scan cannot safely inherit a negative filter from an old hash or mtime-only
assumption. Removing that verification cost would change its source contract.
There is therefore no demonstrated justification to make a persistent gram index
part of the current default live route. Reopen adoption for an explicitly trusted
immutable snapshot, or a measured safe capture/invalidation design that beats the
same scanner through representative queries and updates. Positions, codecs and
more elaborate gram schemes are not justified by this result.

## Correctness and reproduction

Two prototype tests pass. An exhaustive finite test compares all 81 four-byte
patterns over `a/b/C` against 4,096 six-byte sources over `a/b/C/newline` (331,776
source/pattern pairs). A separate mutation test covers false positives, dirty facts,
replacement, deletion, newly added/missing facts, UTF-8, lowercase expansion,
short literals and result caps. This is not a proof over every byte string or a
production concurrency/cancellation test. The existing production work checks are
retained by the disposable hook; no new scanner or quota semantics ship.

Targeted strict Clippy passes. All source, executable and sibling CodeGraph-index
fingerprints remain stable through the capture. Exact instrumentation and hashes
are stored beside the results, with an archived `probe.rs`. No dependency changes.

```sh
python3 research/scripts/trigram_review.py --output /tmp/trigram-adoption-review
```

The root probe intentionally refuses standalone execution: only the driver installs
the disposable admission hook. Do not rebuild sibling CodeGraph indexes. The earlier
`trigram-economics` and `trigram-production-route` captures are intermediate diagnostics;
this cumulative capture is the decision evidence.
