# Remaining end-to-end selective reconciliation

This was the continuation plan at the dependency-record increment. Its implementation
and required workload matrix are now complete; see
[selective reconciliation](../selective-reconciliation/README.md). The historical
plan is retained below; broader optional profiling is not claimed complete.

1. Make untouched raw-fact retention explicit at the publication boundary. A
   header entry with `extraction: None` cannot implicitly mean retain: callers
   already use that value to remove a cache. Carry a separate retained-path set
   with an expected prior generation/header identity; reject retention if identity
   or binding-relevant fingerprint differs. Timestamp-only refresh may update the
   header but must correctly rewrite the full cached record's fingerprint.
2. Keep the header in `Projector::sync` when the native dependency index exists.
   Physical edits produce their new facts; use cached binding surfaces to choose
   repair. Load only unchanged affected facts through `extraction_facts(paths)`.
   Missing caches retain the broad conservative fallback. Legacy adapters keep
   the existing full-manifest path.
3. Replace `js_modules::Modules`' map of whole `SharedExtraction` objects with
   authored `JsModule` surfaces. The resolver reads only `.js_module` from those
   payloads. Populate unchanged surfaces from dependency records and changed
   surfaces from pending extractions; preserve missing vs incomplete semantics.
4. Update dependency records for preserved paths from the prior committed index,
   replacing touched paths from new facts. Rebuild graph-dependent incoming maps
   from the final graph until a separately justified graph delta optimization.
   Do not accidentally call full hydration merely to reconstruct the new index.
5. The native pack writer must retain verified untouched descriptors/packs without
   requiring deserialized payload identities. Verify record fingerprints when
   requested; retain hash/range checks and generation leases. All failures before
   CURRENT preserve the old generation. Post-publication uncertainty still makes
   the handle unavailable.
6. Add a recording adapter that errors on full `manifest()` calls on the native
   selective path, records requested fact paths, and tests body edits, signature/
   binding edits, addition, removal, rename, package changes, no-op timestamps,
   missing caches, corrupted selected records and cold malformed records. Compare
   full graph, source, occurrence and manifest facts against clean rebuild/reopen.
7. After correctness and lint, run isolated fixed-source workload trials. Report
   per-phase reads/decoded facts, repair file counts, latency, memory and generation
   bytes separately. Current code still rebuilds full graph/source indexes; do not
   attribute those costs to dependency lookup or claim sublinear total sync.
