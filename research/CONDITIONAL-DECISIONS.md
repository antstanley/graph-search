# Conditional native-search adoption decisions

This records the conditions in recommendations 16, 17, 26, 27, 28 and 29 of
[the review](09-native-search-review.md). A deferred component is not an
implemented feature. Deferral does not waive the measurement, correctness or
lifecycle work explicitly retained below, nor close the overall implementation.
No new dependency or model is introduced by these decisions.

| Recommendation | Current decision | What remains |
|---|---|---|
| 16: byte trigrams | Measured deferral for the current live route; retain direct scanning. | Reopen for a trusted immutable-snapshot contract or a measured safe verification/invalidation pipeline that wins end to end. |
| 17: regex | Defer the separate feature; preserve the literal API. | Reopen only for a concrete regex requirement with an explicit supported syntax/work contract. |
| 26: score pruning | Measured deferral; retain exhaustive bounded posting evaluation. | Reopen for a workload with material posting cost, then prove bounds and update equivalence before adoption. |
| 27: compression | Retain current explicit packed records and uncompressed posting structures; reject unconditional posting-vector compaction. | Partial hot composition and RSS measured; complete heap attribution and codec decode/seek/maintenance evidence remain open. |
| 28: segments/concurrency | Retain generation snapshots and changed-file overlays; native reader leases protect lazy records during reclamation. | Native lifecycle accepted after churn/disk and process-memory captures; larger segments and owned shared snapshots remain deferred. |
| 29: neural/agentic retrieval | Defer models, ANN and generated repository representations. | Reopen only after first-order native gaps are addressed and a host supplies a justified representation/cost contract. |

## 16: a gram filter must earn its maintenance cost

The [native cumulative experiment](results/native-implementation/trigram-cumulative/README.md)
implements and tests gram admission without shipping a production index. It records
logical forward/reverse storage, build/teardown, replacement/restore, selectivity,
source reads, and production-scanner timing. All 45 filtered/direct hit lists agree;
finite exhaustive and mutation tests cover missing/dirty/new facts, deletion, case
fallback, UTF-8 and result caps. Stable sibling source and CodeGraph fingerprints
bound the measurement to an unchanged corpus.

Index build plus trusted queries wins at 16 and 32 repeated queries on the sampled
long literal in all three repositories, but loses at 1 and 4. A straightforward
strict read/hash-before-query pipeline loses at all four batch sizes. It rereads
matched files and therefore does not prove that every possible safe implementation
would lose. It does establish that removing verification to obtain the trusted
speedup would change the current live source contract. The storage figures are
logical u32 estimates, not RSS or a shipped codec; query frequencies are diagnostic,
not observed user workload statistics.

Recommendation 16's conditional adoption gate is satisfied by this measured decision:
defer the persistent live-route prefilter. Reopen for an explicit immutable-snapshot
API or a safe capture/invalidation design with demonstrated end-to-end gains and
maintenance economics. A future production index must still make dirty, missing,
truncated and incompatible-normalization facts fall back to scanning, validate its
publication/lifecycle, and preserve bounded execution. None of those unshipped
features is claimed complete by this deferral.

## 17: literal syntax stays literal

No current explicit query contract requires a regex compiler. `text` continues
to mean a single-line literal substring, and positional `phrase`/`near` routes
have separately documented whole-lexeme semantics. Punctuation does not infer
regex intent. A regression checks `a.*b`, `(?=x)`, `[a-z]+`, `^value$`, `\d+` and
an unmatched `[` in both case modes: each matches authored literal text, never
its regex interpretation. Existing empty/multiline rejection and source/work caps
remain applicable.

If regex is later required, it needs its own typed mode, declared syntax,
bounded native compilation/execution, and explicit rejection of unsupported
constructs. A future syntax-derived gram filter must satisfy true-match implies
candidate-admission, including alternation and optional branches. This decision
satisfies the review's conditional deferral; it does not advertise regex support.

## 26: preserve the exhaustive oracle and avoid premature bounds

Native metadata/body postings, shortest-list conjunction and bounded top-k heaps
are implemented. They bound work/selection; they are not WAND. The [historical
native probe](results/native-implementation/versions-native-probe.json) reports
about 4.77 ms to evaluate 50,050 postings in its 50k-symbol broad synthetic case,
versus about 0.0017 ms for 50 selective postings. These are historical isolated
measurements, not current end-to-end phase profiles or release latency targets.
They cannot show that broad scoring now dominates service latency.

The same artifact's algebraic counterexample shows why a stored locally winning
TF/length pair is not a bound under changing global averages: the short TF=1
pair beats the long TF=3 pair at average length 1, and loses at average length
1000. Current opt-in BM25F also changes the field-normalization contract. Any
future bound must be specific to the active fields, IDF/statistics, normalization,
nonnegative weights and complete tie rule. Default and BM25F exhaustive oracles
must stay available for update/merge/deletion/parameter equivalence checks.
The [current native phase profile](results/native-implementation/retrieval-phases/README.md)
now supplies that gate: 103 queries and 618 calls preserve normalized API results.
Median posting/service fractions are 4.19–10.08% for task prompts and 0.87–1.45%
for broad probes; no sampled request is posting-dominated. Freshness and result
context preparation cost more. Recommendation 26 closes as a measured deferral
for these local workloads. This is not a larger-scale performance guarantee;
reopen with representative evidence of material posting cost, followed by the
bound proofs and lifecycle equivalence above.

## 27: packing/reuse is not compression

**Current disposition:** the [requirement audit](results/native-implementation/storage-decision-audit/README.md)
accepts the conditional gate with native vectors retained. The chronological
notes below preserve the measurements and limitations; their earlier statements
that recommendation 27 remains open are superseded by this audit. No codec or
total-heap-accounting completion is claimed. Adoption of a future representation
still requires its own positive evidence.

The [paired extraction-storage experiment](results/native-implementation/shared-extraction-paired/comparison.json)
measured whole-generation logical bytes falling from 19,481,378 to 16,789,811 on
the frozen archive, and 32,786,737 to 23,116,912 on the Rust fixture, through fact
separation, sharing and native pack reuse. Those results do not measure a posting
codec. The older serialized composition measurement in the implementation ledger
also does not establish resident dictionary/tiny-list overhead or decode cost.

Keep these distinctions explicit. A codec decision still needs term/posting/norm/
position/fact/adjacency composition and workload-specific seek, decode, build,
update and memory measurements. Rare tiny lists and common long lists should not
be forced into one scheme on the strength of total JSON bytes. A future native
format must retain version/checksum/bounds checks and position alignment. The workspace forbids unsafe code. A favorable byte count alone does not justify
a codec without its seek/decode and maintenance measurements. This gate remains open.

The [native composition and compaction experiment](results/native-implementation/posting-compaction/README.md)
now measures posting length/capacity, dictionary bytes, norm arrays, source line
occurrences, partial adjacency/metadata composition and pre-diagnostic process
RSS on all three siblings. Posting vectors hold 4.37–29.30 MB of spare capacity,
but an unconditional native `shrink_to_fit` candidate does not demonstrate a
reliable RSS benefit and slows synthetic maintenance. It was withdrawn after
correctness checks and 360 timed full/delta pairs. That is a measured rejection
of one allocation strategy, not evidence against all compression. BTree/allocator/
nested-record heap attribution and actual codec seek/decode economics remain
unmeasured; serialized fact bytes are not relabeled as resident memory.

The subsequent [scalar codec experiment](results/native-implementation/posting-codec-measured/README.md)
now measures actual delta-varint encode/decode/lower-bound-seek kernels, including
128-entry restart directories and per-list headers. It round-trips every native
posting list from the three siblings, but sampled long-list seeks are 23.50–24.12
times slower than compact plain-vector binary search. Tiny-list overhead nearly
eliminates modeled live-byte savings on blogwright. This codec stays outside
production. The results close the lack of *any* scalar codec measurement, but do
not establish end-to-end query economics, a complete heap census or an acceptable
integrated layout. Recommendation 27 remains open.

## 28: publication does not imply lock-free concurrent readers

The [lifecycle audit](results/native-implementation/generation-lifecycle-audit/README.md)
accepts recommendation 28 for the current generation/delta architecture. Native
metadata deltas update live statistics; source/extraction packs reuse immutable
bytes and compact by live density. Graph/body reconstruction remains explicitly
outside the narrower incremental improvements. A larger lexical segment system
has not demonstrated an adoption advantage and remains conditional.

The [reader lease protocol](results/native-implementation/reader-retention/README.md)
protects lazy extraction records through later publications. Current-source
[churn measurements](results/native-implementation/generation-churn/README.md)
cover mutations, exact directory retention, normal/crash release and 168 fresh
rebuild comparisons. Separate [memory samples](results/native-implementation/generation-memory/README.md)
account for resident reader processes before/after lazy facts. Independent
handles own resident state even when they share one disk generation. Neither
arbitrary retained history nor arbitrary reader count has a global resource cap.

`Index::store_read` holds its shared `RwLock` guard through the complete callback;
maintenance holds an exclusive guard. `GraphStore: Send` does not provide a `Sync`
contract. The compile-fail API example guards this boundary. Read-only queries
omit the writer file lock but are not freely shared concurrent queries on one
handle. Existing readers retain their selected generation until reopened.

An owned `Arc` snapshot or a large segment system still requires lifecycle tests,
merge I/O/peak-space budgets and a measured advantage before adoption. The current
48-run lifecycle evidence does not claim peak RSS, peak transient disk, production
concurrent throughput or all scheduler/filesystem behavior. Those measurements
remain future adoption gates or part of recommendation 30's release evaluation;
they do not make the conditional architecture changes mandatory here.

## 29: solve the measured native gaps before adding model machinery

The [latest integrated evidence comparison](results/native-implementation/normalization-context-review.md)
still delivers complete regions for only 12/34 established tasks and 10/12 newer
tasks on the automatic route. These are tool/evidence metrics; there is no model
answer-success result. Body/query policy, source assembly, parser coverage and
scope/package resolution remain explicit unfinished native work. Introducing a
model now would not establish which of those failures it fixes, and a reranker
cannot recover candidates it never receives.

No embedding producer, learned sparse model, late-interaction index, cross-encoder,
LLM rewrite loop, GraphRAG summary generator or ANN component is added. If an
existing host later supplies representations, begin with native exact scoring
under explicit representation, filtering, work, update and cost contracts before
considering approximation or quantization. Model production and host call/token
cost remain separate from the scorer's implementation. This fulfills the review's
instruction to keep such machinery conditional; it does not claim model-based
retrieval quality or close the native evaluation gaps.
