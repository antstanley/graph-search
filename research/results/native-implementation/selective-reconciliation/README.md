# Selective native reconciliation and explicit fact retention

Recommendation 23 implementation increment. No new third-party component, parser
revision, semantic index revision or storage-format change.

## Behavior

Native changed-sync now keeps the hot manifest header. It determines repair from
the committed dependency index and loads raw extraction facts only for affected
unchanged files. Physically changed files are parsed normally. Unchanged
JavaScript/TypeScript module binding uses compact authored surfaces, not whole
parser payloads. Legacy/no-index adapters retain the compatibility path.

`FactRetention` separates preservation from absence. It records the observed
generation, complete old header and retained paths. Publication rejects stale
identity, changed representation/policy, missing caches, changed fingerprints
(other than mtime), and overlap with upserted or removed owners before mutation.
Ordinary publication still interprets absent extraction values as cache removal.

Grafeo retains authenticated packed descriptors for unchanged headers. Timestamp
changes load/rewrite only those records because each packed record includes its
fingerprint. Compaction copies verified serialized slices without typed decoding;
it keeps existing packing, deduplication, liveness, hash and reader-lease rules.
The new generation reuses per-file dependency records and reconstructs reverse
maps against the final graph/file set. MemoryStore retains shared payloads and
reuses compact records, rather than walking all retained parser facts again.
Transient in-memory Grafeo stores now retain their manifest values explicitly so
later sync can use them even though no raw-fact sidecar exists.

## Verification contract

The selected-only test adapter rejects `manifest()` outright. On both native
adapters, a leaf body edit requests zero unchanged raw facts; a binding rename
requests only caller and outer files. Unrelated cold facts are explicitly retained.
Graph nodes, edges, source units, reference occurrences and full manifest entries
are compared with a clean rebuild, including rename, timestamp refresh and reopen.
No-op sync leaves the full manifest unchanged. The transient engine is tested too.

A storage fixture commits authenticated JSON bytes that are not a typed FileEntry
for one cold file. Full hydration fails, but retaining and compacting that file
while publishing another succeeds; later explicitly reading the malformed file
still fails. This directly exercises the absence of hidden full typed hydration
inside native publication, not only the projector's port calls.

Retention tests reject stale generations, incompatible headers, missing owners,
conflicting upserts and absent cache requests without changing the old state.
Timestamp refresh preserves payloads, while ordinary None publication removes them.
Eight prepublication fault points exercise native retention and retry; the old
reader and a reopened handle both preserve the old generation on failure.
Existing crash, packed-data, reader-lifetime, missing-cache, binding differential,
legacy migration and clean-rebuild suites remain part of validation.

Initial lint found sync's expanded function length; metadata refresh was extracted
into a helper that also avoids cloning a header during a true no-op. A later lint
found a test-only assigning-clone style issue, corrected to clone_from. The source
snapshots before and after that test-only correction are retained.

## Limits and remaining work

This removes workspace-wide *typed raw-fact hydration*, not every workspace-wide
operation. Reconciliation still walks files and snapshots graph nodes/edges.
Publication still prepares a complete graph, retrieval structures and occurrence
facts. Reused packs are read and hash-verified during publication, so no proportional
disk-read or end-to-end latency claim follows from fewer decoded parser payloads.
Older generations without compact dependencies retain the full hydration fallback.

Recommendation 23's required matrix is complete: no-op, body edit, public API edit,
rename, deletion, duplicate-name addition/removal and missing-cache repair. All
48 release-mode trials match full reindex after reopening. [Measured results](MATRIX.md)
include per-case medians/ranges, requested facts and repair breadth. Further phase,
large-repository and memory profiling belongs to later performance/release work;
no such measurements are claimed here. Recommendations 12, 21 and 30 remain open.

`checks.json` records actual command outcomes. `sources.json` fingerprints the final
production, tests, build inputs and affected standalone probe wrapper. No timing
experiment runs concurrently with validation.

## Final validation and completion

- Final core, engine and graph-search regression: **470 tests in 34 suites passed**.
- Final selected-only path: **2 tests passed**, independently rerun after the
  test-only clone style correction.
- Strict workspace/all-target and both affected standalone probe Clippy passed.
- Optimized workload matrix: **48/48 rebuild/reopen comparisons passed**; direct
  execution exited 0. The failed OS memory profiler attempt is retained separately.
- All **165 final source/build/probe fingerprints match**. No dependency changes.
- [Requirement-by-requirement audit](RECOMMENDATION-23-AUDIT.md) confirms the original
  recommendation's scope. Recommendation **23 is complete**; ledger is **27/30**.

The user requested stopping at this boundary. No further recommendation is started,
and recommendations 12, 21 and 30 remain open. The user subsequently requested
committing and pushing the completed work to main.
