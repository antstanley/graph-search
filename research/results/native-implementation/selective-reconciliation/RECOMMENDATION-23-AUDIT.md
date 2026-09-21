# Recommendation 23 completion audit

Scope: recommendation 23 in `research/09-native-search-review.md`, not the entire
30-recommendation program. The user requested stopping when this recommendation
is complete. Requirements below retain the original scope.

| Requirement | Implementation / direct evidence | Completion gate |
|---|---|---|
| Preserve occurrence ownership and stable targets so replacements do not delete untouched incoming adjacency | `core/mutation.rs`, both native stores; source-owned occurrence representation; store contract and stable-target regression fixtures | Final regression suite |
| Distinguish physical edits from binding changes, including signatures and export surfaces | `binding_surface.rs` normalization and fingerprint; changed owners always reproject; 24 binding adapter/scenario combinations compare minimal upserts and complete rebuild state | Final binding suite |
| Maintain dependencies beyond resolved graph edges, including raw names, imports, module selection, export surfaces and unresolved references | Generation-owned `dependencies.rs`: reverse raw-name postings include unresolved facts, authored import/reexport records retain selection inputs, reverse selected-module and incoming-file postings, per-file normalized export/import surfaces. Export sets are indexed by owner/module rather than mixed into global definition names; the earlier differential test demonstrated that mixing aliases into definition names wrongly repaired leaf modules | Dependency unit tests, native cached-path assertions, generation corruption/migration tests, duplicate-name matrix |
| Publish dependency state coherently with graph state | Format-7 CURRENT authenticates dependency sidecar, header, extraction descriptor, graph, source and occurrence artifacts; failed preparation leaves prior generation intact | Failure/retry and reader tests |
| Split hot freshness metadata from large raw-fact payloads | Native manifest header and per-file fingerprinted extraction packs; unchanged query/no-op paths use header | Header, source-unit and extraction-fact tests |
| Load raw facts for affected files | Native Projector selects affected unchanged paths; explicit retention preserves all other cached payloads without typed decoding; compact JS surfaces support resolution; legacy fallback remains correct | Selected-only wrapper forbids full manifest calls; body edit requests zero unchanged facts, binding rename requests two; cold-record compaction fixture proves no hidden full hydration in writer |
| Benchmark no-op, body/API edits, rename, deletion, duplicate-name addition/removal and missing-cache repair against a clean rebuild | Release-mode matrix: eight cases, two synthetic fanout corpus sizes, three trials each. Captures requested facts, retained records, repair breadth, sync and full-reindex timings; reopens sync result and compares graph/source/occurrence/manifest state | Isolated matrix must finish with every equality assertion satisfied |
| No third-party components | No Cargo dependency/lock changes; native Rust implementation and existing test harness | Final source/dependency fingerprints |

HTML/CSS dependency selectors are explicitly a later narrowing in the original
recommendation. Conservative family-wide repair remains and is regression-tested.
Whole graph/retrieval-index reconstruction, occurrence publication and pack hash
verification still occur. Neither constant-time sync nor proportional disk I/O is
claimed. Signature-only edits do not seed consumers when signatures are not an
input to this resolver's target selection; their owner is still refreshed.

## Completion evidence

The final combined command passed **470 tests across 34 suites**. The selected-only
adapter tests also passed independently on the final test source. Strict workspace
Clippy and standalone probes passed. All **48 release-mode workload trials** passed
complete graph/source/occurrence/manifest comparisons after reopening, including
both duplicate-name transitions and missing-cache repair. [Matrix](MATRIX.md) and
[command outcomes](checks.json) retain timings, limitations and failed preliminary
attempts. All 165 recorded source/build/probe hashes match final files; dependency
manifests and locks have no changes.

Every requirement above has direct implementation and matching-scope evidence.
Recommendation **23 is achieved**. Recommendations 12, 21 and 30 remain open.
The user requested stopping at the recommendation-23 boundary and subsequently
requested committing and pushing the completed work to main. No further
recommendation work is undertaken.
