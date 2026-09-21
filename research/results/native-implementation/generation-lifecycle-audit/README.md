# Recommendation 28: generation and lifecycle decision

**Accepted for the current design:** keep coherent immutable generations, native
metadata deltas and compact fact packs. Do not add a large lexical segment system
or an owned, freely shared snapshot API in this change. The original requirement
makes those changes conditional; it does require accounting for retained readers
and documenting the concurrency contract. Those obligations now have direct
implementation and experimental evidence.

This closes recommendation 28, not recommendation 23's narrower invalidation or
30's release-wide evaluation. It does not declare the overall implementation
ready to commit. Historical measurements remain historical; current-source
checks below establish correctness applicability, not new historical timings.

## Requirement-to-evidence mapping

| Original requirement | Current implementation and evidence | Disposition |
|---|---|---|
| Prefer immutable local snapshots and small deltas before elaborate segments | `MetadataIndex::updated` reuses immutable untouched postings, replaces changed document contributions, remaps ordinals, and updates corpus totals. [Metadata delta evidence](../metadata-deltas/README.md) compares complete postings/statistics/rank bits with reconstruction. Source/extraction packs reuse immutable bytes, repack low-live-density packs and bound small-pack accumulation. SPEC §6.4 and §8 document the limits. | Implemented; graph/body rebuilds remain explicitly accounted for under 23. |
| Suppress superseded base occurrences and merge changed evidence | Updated metadata views remove old document contributions before using new statistics. `QueryEngine` body retrieval masks known-changed, deleted or incompatible persisted paths before constructing bounded live-source evidence, including when overlay work is exhausted. Existing retrieval/update tests compare fresh and incremental views; the churn capture adds 168 complete graph/source/occurrence/extraction rebuild comparisons. | Implemented for existing delta routes; no claim of a new general LSM query layer. |
| Publish graph, text and source state together | Authenticated generation descriptor commits graph, manifest, source and occurrence facts; resident indexes are built from the prepared graph/facts before publication. Atomic CURRENT replacement and failure-state behavior are tested. | Implemented. |
| Consider segments, masks and bounded compaction if sustained churn warrants them | Current native packs already compact by live-byte density and small-pack count. At the tested scales, 24 publications per run remain correct with bounded cooperative retention. No measured segment candidate demonstrates an advantage over this architecture. | Defer the larger segment system; no segment merge-I/O or peak-space guarantee is invented. Future adoption requires those budgets and comparisons. |
| Tie scores to documented global/live statistics | Metadata corpus totals subtract removed/changed contributions and add the new analyzed delta; DF follows current posting membership. Metadata scores use the new population even for shared lists. Both analyzers/normalization policies have exact full-rebuild comparisons. Persisted-body and live-overlay lanes retain their own corpus statistics and merge by rank (SPEC §8); they do not add incomparable raw BM25 scores or claim globally recomputed live-body BM25. | Implemented and documented; metadata global/live statistics and body rank-fusion semantics remain distinct. |
| Distinguish visibility, durability and reclamation | SPEC §6.4 separates pointer visibility, file/directory synchronization, uncertain post-rename failures, and lease-aware later reclamation. Failure tests cover publication boundaries; they are not power-loss simulation. | Implemented/documented. |
| Account for old-reader disk retention | [24 churn runs](../generation-churn/README.md): exact current + previous + distinct leased directory set at every recorded state; 354 reader checks; additions, deletions, renames and source edits; individual normal/crash exit; logical and unique-inode disk metrics. All released histories reclaim on the next publication. | Measured; arbitrary pinned history and cleanup failures can retain more, so no universal disk cap. |
| Account for old-reader memory | [24 process-memory runs](../generation-memory/README.md): own-process RSS before/after lazy facts and reader release, 42 reader checks. Independent stores own resident graph/index data, including when disk generation is shared. | Measured at two synthetic scales, with RSS/shared-page/allocator limits explicit. No shared-memory saving or peak-RSS claim. |
| Deliberate concurrency contract; do not advertise unsupported sharing | `GraphStore: Send` remains non-`Sync`; `Index` holds its read guard through each callback and exclusive guard for maintenance. The compile-fail API example tests unsupported `Sync` use. Independent generation handles are stale until reopened. | Explicit contract retained. |
| An owned Arc snapshot is a lifecycle change requiring tests | No such API substitution was made. Native OS leases pin the existing borrowed/store-owned lifecycle; tests cover prepared/reopened readers, lazy records, two processes, crash release, selection races and bounded retries. | Existing lifecycle fixed and tested; future API adoption has a separate gate. |

## Validation and boundaries

`audit.json` verifies every current crate file against the final 484-test,
39-suite workspace capture, verifies strict Clippy success for those sources,
and hashes the evidence consumed here. Both new experiment certificates assert
unchanged source/driver/executable inputs and complete run counts. No production
code, dependency, persisted format or ranker policy changed in this audit.

The current design still copies a full graph during preparation, rebuilds body
indexes, and retains independent process allocations. There is no measured
production concurrent-query SLA, peak transient disk bound, or proof over every
filesystem/scheduler. These limits are explicit in the contract and remain part
of future optimization or recommendation 30's broad resource evaluation. They do
not make a large segment system or `Arc` snapshot mandatory in recommendation 28.
