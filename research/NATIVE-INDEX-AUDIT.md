# Native inverted-index completion audit

This audit checks recommendation 8 against current implementation rather than
inferring completion from the presence of an index type. Recommendation 7's
per-file update requirement was incomplete at the initial audit; it is now
implemented and validated in the [metadata delta follow-up](results/native-implementation/metadata-deltas/README.md).

| Requirement | Current evidence | Conclusion |
|---|---|---|
| Native ordered dictionary and sorted postings | `LexicalIndex` owns an ordered map of immutable `Arc<Vec<Posting>>` lists. Construction appends monotonically increasing document ordinals; updates remap and sort affected lists. Posting records retain weighted frequency plus four split/whole field frequencies. | Implemented without a new component. |
| Per-document field lengths and corpus statistics | Split/whole lengths, combined lengths and field means live in the generation-owned index. DF is the complete term-list length; scoring uses current index populations. | Implemented; global statistics are not recomputed per query. |
| Sparse disjunction | `accumulate_inner` visits only requested term lists, filters before candidate allocation, and charges every examined posting. | Implemented, with visible candidate/posting exhaustion. |
| Conjunction and selective seeking | `accumulate_all_inner` delegates to the native shortest-list intersection with exponential seek/binary refinement. It scores in input-term order and requires complete membership before admission. | Implemented. Generated intersection and source-field differential tests cover correctness. |
| Deterministic bounded top-k | `MetadataIndex::search_top_with_policy` scores admitted compact ordinals, retains a bounded `BinaryHeap`, and clones full nodes only for winners. Candidate totals precede top-k. | Implemented; this is exhaustive candidate scoring, not WAND. |
| Independent exact oracle | Lexical unit tests reconstruct term maps and DF from original node fields. OR and AND scores agree bit-for-bit. Heap tests compare against a full sort across ties/cutoffs. | Implemented and exercised by the core test suite. |
| Filters, work and completion | Path/language filtering precedes candidate admission; work counters and truncations accompany the public result. Exact candidates survive an exhausted lexical lane. | Implemented; tests cover filtered records, independent caps and lane sharing. |
| Generation ownership | Both adapters own their metadata index. Snapshots borrow it; queries do not construct it. Grafeo preparation builds final indexes before publication, and opening a published generation reconstructs them from that generation's graph/source facts. | Implemented. No separate persisted posting file can drift from publication. |
| Update/reopen correctness | Engine test `native_metadata_rebuilds_on_publication_and_reopen` verifies renamed/qualified lookup and posting hits after reopen, then absence after deletion/reopen. | Passed on the current checkout. |

## Boundaries

The proposed `LexicalSnapshot::candidates` name in the review was an API sketch.
The actual native boundary is the snapshot's borrowed `MetadataIndex` plus
typed retrieval options and `WorkBudget`; compact candidates remain internal
until winners are materialized. It fulfills the separation between domain
scoring and storage without adding another abstraction merely to match a name.

Postings are resident sorted vectors built at open and incrementally maintained
at publication. Authenticated
graph, source and extraction records are persisted; an additional posting codec
is not required for this initial implementation. Compression, dense-bitset
selection and score pruning remain conditional optimizations under their own
measurement gates. No safe-pruning claim follows from the bounded heap.

Recommendation 7 now uses `MetadataIndex::updated` in both adapters. Changed
scoring fields are analyzed, affected term lists are updated/remapped, untouched
lists share storage, and corpus length totals change by document delta. Exact
maps and ordering still undergo global reconstruction. This implements posting
maintenance without claiming that the entire sync is proportional to the number
of changed files. Recommendation 23's graph/fact invalidation remains separate.

## Validation

The current lifecycle test passes in
`/tmp/native-postings-lifecycle-audit.log`. Current core-suite validation is
recorded in the implementation ledger after completion. Earlier independent
scoring-factorial/native captures add 2,740,992 legacy/BM25F score comparisons;
those historical measurements complement rather than replace current tests.

Recommendation 8 is satisfied by the implemented uncompressed native baseline;
the linked follow-up supplies recommendation 7's completion evidence. Neither
closes persistence economics, pruning, concurrency, or the whole implementation goal.
