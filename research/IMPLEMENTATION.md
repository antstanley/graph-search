# Native search implementation tracking

Scope: implement the recommendations in [the review](09-native-search-review.md), validate them, then commit and push. Conditional techniques retain their stated evidence gates; they are not automatically required integrations. No new third-party runtime components; the benchmark-only Criterion dev-dependency is the single addition and exists for the release gate's performance objective.

## Requirement ledger

- [x] 1. Make failed writes leave a coherent generation — P0
- [x] 2. Return freshness and source identity from the library — P0
- [x] 3. Bind snippets to indexed bytes and fix coordinates — P0
- [x] 4. Replace quadratic literal-hit bookkeeping — P0
- [x] 5. Enforce work budgets and graph payload limits inside execution — P0
- [x] 6. Report coverage loss from the walker and extractors — P0
- [x] 7. Cache exact-name and lexical structures by generation — P1
- [x] 8. Add a native inverted lexical index — P1
- [x] 9. Make bodies, comments, configuration, and Markdown first-class candidates — P1
- [x] 10. Add a small, native query planner and protect explicit intent — P1
- [x] 11. Redesign candidate selection separately from final context selection — P1
- [x] 12. Select source around matches and relationships — P1 (implementation and measurable evaluation complete; model answer success owned by the release gate)
- [x] 13. Preserve whole identifiers and improve analyzer contracts — P1
- [x] 14. Evaluate field normalization and positive IDF, without conflating them — P1/P2
- [x] 15. Add native phrase and proximity verification — P2
- [x] 16. Add a sound native trigram prefilter when scan economics justify it — P2/conditional (measured; live-route adoption deferred)
- [x] 17. Add regex only with a clearly bounded native contract — P2/conditional (deferred by the explicit contract decision)
- [x] 18. Add prefix lookup before fuzzy matching — P2
- [x] 19. Preserve reference occurrences separately from graph adjacency — P1
- [x] 20. Make resolution scope-aware before increasing its reach — P1
- [x] 21. Model packages, imports, and framework regions natively — P1/P2 (declared native subset; template/JSX/DI edges and condition loaders explicitly unresolved)
- [x] 22. Make graph expansion evidence-driven and avoid repeated traversal — P1/P2
- [x] 23. Reduce incremental invalidation and separate facts from the hot manifest — P1/P2
- [x] 24. Version policy, representations, and publication independently — P1
- [x] 25. Implement Markdown structure for evidence integrity — P1/P2
- [x] 26. Add safe block pruning only after postings are correct and profiled — P2/conditional (current phase gate measured; pruning deferred)
- [x] 27. Compress only after measuring index composition — P2/conditional (composition and two candidates measured; retain native vectors)
- [x] 28. Use generations and compact deltas before building a large segment system — P2 (native deltas and reader lifetimes validated; larger segments deferred)
- [x] 29. Keep neural and agentic retrieval conditional — later (deferred; no model component added)
- [x] 30. Turn the evaluation suite into the release decision mechanism — P1 (decision record; model-success objective remains external/unmeasured)

## Validation and delivery

- [x] Reproduced correctness failures covered by production regression tests.
- [x] Workspace tests and strict lint checks pass (automated by the release gate).
- [x] External source-valid evidence evaluation and fresh-query coverage (34 dev tasks plus the frozen fresh suites; 26 drifted tasks remain excluded pending source review).
- [x] Differential incremental/rebuild and scorer checks.
- [x] Review each numbered recommendation against implementation evidence (audits and conditional decisions are recorded per recommendation).
- [x] Commit intended files, excluding the user-owned `.codegraph/` index.
- [x] Push and verify remote commit.

## Completed increments

### Recommendation 12 complete: source selection and its release-gate evaluation

Every implementation clause is in place: matched locations from lexical
retrieval, distinct-term coverage and bounded proximity, declaration identity
plus matching body, relationship call sites with endpoint context, complete small
functions and labelled intervals for large ones, marginal-value-per-byte packing,
overlap/duplicate-line removal, metadata and edge reservation, one source read per
request, and admission or rejection before materialization. The
[context-selection audit](CONTEXT-SELECTION-AUDIT.md) maps each clause to its
implementation and to the controlled experiments that accepted or rejected a
candidate policy.

The measurable evaluation clauses are now part of the release gate rather than
ad-hoc measurements: complete-region delivery, mean region coverage and response
bytes are thresholds in `results/native-implementation/release-gate-v3`, and
citations are source-hash verified by the evidence protocol. Model answer/patch
success cannot be established in this environment, so the gate records
`model_task_success: not_measured` and returns `conditional_pass`; closing
recommendation 12 does not assert answer success.

Validation: the gate ran 564 passing workspace tests, strict workspace/all-target
Clippy, and 34 source-valid evidence tasks with zero protocol errors.

### Recommendation 30 complete: release decision mechanism with Criterion benchmarks

`research/scripts/release_gate.py` runs workspace tests, strict Clippy, task-label
validation, the evidence-v1 protocol on source-valid tasks, the new Criterion
benchmark suite and a real index/resident probe, then applies predeclared
thresholds and writes `release-decision.json` plus `RELEASE-GATE.md`. Objectives
stay separate: exact-matching correctness, candidate/evidence metrics,
performance p50/p95/p99, resource envelope, and model task success.

The first run correctly failed on a stale coverage threshold; the actual
pre-increment baseline was measured from commit `a0cbf7d` (28 required files,
12 complete regions, 0.4862 mean coverage) and the thresholds were re-frozen
against that calibration rather than loosened. The current decision is
**conditional_pass**: everything measured passes, and the model task-success
objective is `not_measured` because no model driver or blind reviewer exists in
this environment, so a full release claim is refused.

Criterion (dev-dependency only, https://docs.rs/criterion/latest/criterion/) adds
11 benchmarks covering cold index build, one-file sync, exact/reference lookup,
metadata/body/positional explore, occurrence lookup, live literal/file scans and
filtered explore. On the generated 200-file corpus, p95 values are 717 ms cold
build, 475 ms sync, and 1.3–8.2 ms for lookups/scans/explores; the nanus resource
probe reports 57,716,498 index bytes and a 450,000 KiB resident sample.

Validation: the gate ran 564 passing workspace tests and strict workspace/all-target
Clippy; the accuracy arm covers 34 source-valid tasks with zero protocol errors and
unchanged sibling source snapshots. 26 drifted tasks remain excluded pending
explicit source review. Full clause mapping: [audit](RECOMMENDATION-30-AUDIT.md);
[decision record](results/native-implementation/release-gate-v3/README.md);
[calibration and the failed pre-calibration run](results/native-implementation/release-gate-v1/README.md).
The only added dependency is the benchmark-only Criterion crate; production
dependencies are unchanged.

### Recommendation 21 complete: framework regions, default TypeScript aliases, Rust reexports

Native Svelte/Vue/Astro adapters now extract declared script regions through the
offset-translating embedded-script bridge and publish parser-coverage facts for
unmodeled dialects. A generation-owned TypeScript project selector resolves the
nearest admitted `tsconfig.json`/`jsconfig.json`, applies bounded inheritance and
`paths`/`baseUrl` before package maps in supported bundler/Node16 modes, and
leaves unsupported modes unresolved. Rust `pub use` leaves are published as
module-owned exports and followed through `as` aliases and chains with a 16-hop
bound; private uses publish nothing. The [clause-by-clause
audit](RECOMMENDATION-21-AUDIT.md) records the declared subset and remaining
template/JSX/DI/conditional-loader limits.

Validation: 564 Rust tests across 50 suite reports pass
(`cargo test --workspace --locked`), strict workspace/all-target Clippy passes,
and `cargo fmt --all --check` passes. Dependency manifests and lockfiles are
unchanged; parser/source/schema are 20/15/3. Evidence:
[framework regions](results/native-implementation/framework-regions/README.md),
[TypeScript aliases](results/native-implementation/typescript-aliases/README.md)
and [Rust reexports](results/native-implementation/rust-reexports/README.md).

### Recommendation 23 complete: selective reconciliation and explicit retention

Native changed-sync now loads only affected unchanged extraction facts and resolves
unchanged ECMAScript surfaces from compact dependency records. `FactRetention`
validates the observed generation/header and retained owners before publication;
ordinary absent facts still remove caches. Native pack compaction preserves verified
serialized records without typed decoding, and timestamp changes rewrite only
changed fingerprints. Both stores reuse compact dependency records for retained
files. No third-party components or new storage/semantic representation revision.

Final validation passed **470 tests in 34 suites**, strict workspace/probe Clippy,
and **48 release-mode workload comparisons** against full reindex and reopen.
Body edits at 16 and 64 files requested zero unchanged raw facts and one upsert.
At 64 files the body case measured 188.70 ms sync versus 244.70 ms reindex; no-op
measured 0.63 ms versus 160.51 ms. Rename was 172.10 versus 169.03 ms: whole-generation
work remains. These are three-trial warm synthetic medians, not production tail
latency or a comparison against the old sync implementation. Peak RSS was unavailable
because the OS profiler's sysctl request was denied; no memory gain is claimed.

[Implementation and evidence](results/native-implementation/selective-reconciliation/README.md),
[workload matrix](results/native-implementation/selective-reconciliation/MATRIX.md),
[completion audit](results/native-implementation/selective-reconciliation/RECOMMENDATION-23-AUDIT.md).
All 165 source/build/probe fingerprints match. Recommendation 23 is complete; the
ledger was **27 checked / 3 open** (12, 21, 30) at that point. Recommendation 21 is
now complete for its declared native subset, recommendation 30 is complete as the
release decision mechanism, and recommendation 12's implementation and measurable
evaluation are complete with model success owned by the gate: the ledger is
**30 checked / 0 open**. Earlier increment notes retain their historical commit/push
status.


### Generation-owned native dependency records and cached repair selection

Both native adapters now publish compact binding dependency records. Generation
format 7 authenticates those records with the graph/header/raw-fact descriptors;
legacy generations retain conservative fallback. Incremental repair uses cached
raw-name and module/incoming relationships, including unresolved references and
selected modules without a resolved symbol edge. Differential tests assert the
cache is present and preserve the required minimal upsert sets. The first run
caught excess barrel-retarget repair; export surfaces remain separate from ordinary
definition-name invalidation after correction.

[Implementation, limits and validation](results/native-implementation/dependency-records/README.md).
Final focused tests passed 11 (including 24 binding adapter/scenario combinations),
plus strict workspace and standalone-probe Clippy. Final broad validation passed
465 tests in 32 suites across core, engine and graph-search. One format-1
emulation fixture needed removal of the new artifact; its repaired suite and all
remaining suites passed. All 161 final source/build fingerprints match.
No new third-party components. This still hydrates full manifests and snapshots;
explicit untouched-record retention and selective actual reconciliation remain.
Recommendation 23 remains open; ledger remains **26 checked / 4 open**. Commit and
push are pending the complete goal.


### Native selective raw-fact reads with authenticated byte ranges

`GraphStore::extraction_facts(paths)` now has native selected lookup in MemoryStore
and generation-pinned range reads in Grafeo. Requested record hashes and complete
manifest fingerprints are checked; unknown or missing caches remain absent. The
range reader consumed 10 bytes from a 4,108-byte pack for two paths sharing a record,
without decoding cold records or opening unrelated packs. Old readers retain their
facts across publication; identity caches retain only weak payload references.
[Contract and evidence](results/native-implementation/selective-extraction-reads/README.md).
Final engine tests passed 49; incremental integration tests passed 6, including
24 adapter/scenario combinations and missing-cache fallback. No dependency/layout
change. This is a native read capability, not yet a changed-sync optimization:
reconciliation still hydrates all facts until persisted reverse dependencies and
explicit untouched-record retention are integrated. Recommendation 23 stays open;
ledger remains **26 checked / 4 open**, with commit and push pending completion.


### Binding-surface invalidation removes body-edit repair amplification

Rust/JS/TS consumer repair now starts from binding-relevant symbol and authored
module-surface changes. Body, documentation, outgoing-call and signature changes
still fully update their owner, while stable targets preserve incoming adjacency.
Visibility/import/export/identity changes, missing facts and broader package or
file-presence cases retain conservative repair.
[Contract, correctness argument and evidence](results/native-implementation/binding-surface-invalidation/README.md).
Generated 2/8/32-file body edits now submit exactly one upsert in both adapters,
including Grafeo reopen, and match clean rebuilds in all graph/source/occurrence
facts. The production matrix validates 24 adapter/scenario combinations; the
missing-cache regression forces repair even without a binding change. Broad
validation passed 265 tests; the final strengthened focused run passed 6, and
strict workspace/probe lint passed. No latency claim. Recommendation 23 remains
open for persisted reverse dependencies, selective fact loading and workload
acceptance; ledger stays **26 checked / 4 open**. Commit and push remain pending.


### Stable-target replacement implemented in both adapters

Native mutation planning now preserves same-ID/path/kind nodes and untouched
source-owned incoming edges, replaces edited owners' edges, and deletes references
to removed targets. Grafeo replaces complete properties in its isolated prepared
graph. The new regression exposed and fixed pathless dangling-edge loss on reopen;
explicit file-removal semantics remain destructive.
[Implementation and evidence](results/native-implementation/stable-target-updates/README.md).
The native caller probe retains its incoming call and all three occurrences in both
adapters, including reopen; identical replacement deletes zero nodes. Generated
2/8/32-file body edits still cause 2/8/32 upserts and match clean rebuilds exactly.
Workspace tests passed 528 before the sidecar follow-up; the final full engine
rerun passed 42, and strict workspace Clippy passed. No timing claim or dependency
change. Recommendation 23 remains open for narrower binding-dependency invalidation
and remaining broad persistence/read work. Ledger stays **26 checked / 4 open**;
commit and push remain pending completion.


### Stable-target replacement and repair amplification baseline

A native two-file reproduction shows that identical target-only replacement drops
one incoming call edge while preserving all three caller-owned bound occurrences
and the complete target node, in MemoryStore and Grafeo including reopen. Generated
2/8/32-file chains show one hash-verified body edit causing 2/8/32 projection upserts;
all nodes, edges, source facts and occurrences still match clean rebuilds.
[Evidence and required implementation order](results/native-implementation/stable-target-baseline/README.md).
The first chain assertion exposed that `SyncReport.modified` includes rebound
consumers; source hashes now measure physical edits independently. Strict probe
Clippy and the corrected/frozen reproduction pass; all 157 source hashes match.
No production change. This establishes the coupled store/projector contract for
recommendation 23; stable updates and narrower invalidation remain unimplemented.
Ledger stays **26 checked / 4 open**. Commit and push remain pending completion.

### Recommendation 11 acceptance after native normalized-score rejection

The nominated 75% body min-max combination was tested through native source
delivery under equal query/work/payload budgets. Across 348 stable trials it gains
one required-file task but loses three complete-region tasks (30/58 → 27/58),
with no complete-region gains. Every loss is traced to selected owners, delivered
context and deterministic follow-up reads. The production default is retained.
[Evidence and decision](results/native-implementation/score-combination-context/README.md).
[Requirement audit](CANDIDATE-SELECTION-AUDIT.md) maps the deeper pool, protected
navigation, provenance, independent channel/diversity/representation experiments,
source deduplication and complementary evidence. All 27 body/planner tests and 28
evaluation tests pass; captured sources and sibling indexes remain stable. No
production or dependency change. Ledger: **26 checked / 4 open** (12, 21, 23, 30).
Final global validation, commit and push remain outstanding.

### Normalized channel-combination candidate experiment

Captured current native top-50 metadata/body lane scores on 58 source-validated
sibling tasks. Nine predeclared combinations keep entity representation and file
diversity fixed. Query-local min-max normalization with 75% body weight delivers
all required files at eight on 51/58 tasks versus body-only 50/58, gaining one task
without losses. It meets the gate for a subsequent end-to-end source-delivery
trial, not a default-ranking change. [Protocol and evidence](results/native-implementation/channel-combinations/README.md).
Strict probe lint/build and boundary checks pass; all 162 input hashes and sibling
source/CodeGraph fingerprints remain stable. No production or dependency changes.
Recommendation 11 remains open, including mixed live/indexed statistics and full
context validation for the promising policy. Ledger stays **25 checked / 5 open**.

### Recommendation 22 acceptance and Grafeo visited-node fix

The requirement audit found and fixed missing visited-node suppression in Grafeo’s
direct expansion port. A new independent oracle covers 648 cyclic-expansion cases
across both adapters. All focused engine and graph-intent tests and strict workspace
lint pass. A 132,096-case bidirectional prototype verifies shortest distances while
showing different equal-length choices and mixed work costs; integration remains
conditional and is deferred. The prior multi-source investigation and identical-
candidate graph ablation complete the remaining investigation/evaluation clauses.
[Full requirement mapping and evidence](results/native-implementation/graph-acceptance/README.md).
No dependency or policy-version change. Model-task success remains open under 30.
Ledger: **25 checked / 5 open** (11, 12, 21, 23, 30). Final global validation,
commit and push remain outstanding.

### Count-only caller summaries and multi-source feasibility

The default explore path avoids retaining complete caller-cone subgraphs when it
needs only their counts. All 51,200 exhaustive count/work comparisons pass; all
3,360 paired synthetic queries preserve normalized responses. Median whole-query
time is 5.7–32.4% lower by workload. A separate shared-frontier prototype is mixed
and reaches a 5.35× slowdown on chains, so unconditional integration is rejected.
[Protocol, results and limitations](results/native-implementation/caller-counts/README.md).
Workspace validation passes 527 tests across 39 nonempty suites, strict Clippy,
formatting and whitespace checks; all 158 source hashes match. No dependency or
policy-version change. Recommendation 22 remains open; ledger **24 checked / 6
open**. Global acceptance, commit and push remain outstanding.

### Recommendation 20 acceptance audit

Reviewed the original lexical-scope requirements separately from project/package
work in recommendation 21 and dependency indexing in recommendation 23. The
existing native implementation satisfies the named scope/binding/order/provenance
requirements; the review explicitly defers transitive value dataflow and full
compiler semantics. New independent evidence checks 50 JS/TS call sites: all 22
published targets match compiler declaration sites and all 28 expected refusals
carry reasons. Seven Rust compiler premises and 26 focused regressions pass.
[Requirement mapping and evidence](results/native-implementation/scope-acceptance/README.md).
No production code or dependency change. Ledger: **24 checked / 6 open** (11, 12,
21, 22, 23, 30). Final global validation, commit and push remain outstanding.

### Native TypeScript root enumeration (project integration open)

Native root enumeration now interprets inherited files/include/exclude using each
field's declaring directory, explicit roots, output exclusions, JS/JSON defaults,
and extension priority. The bounded native pattern matcher models hidden/package
paths, minified files, exclusion prefixes and UTF-16 wildcards. It requires a
complete case-sensitive namespace; it does not infer missing files from admission.
[Evidence and limitations](results/native-implementation/typescript-project-roots/README.md).
Validation passes 205 focused tests, strict workspace linting, and 54 compiler
root-file-set comparisons; all 158 recorded input hashes match.
Imported-file closure, project discovery/overlap, default bindings and persisted
presence invalidation remain open. The ledger and policy versions are unchanged.

### Native TypeScript candidate presence (publication integration open)

The file loader now requires evidence for absent unindexed candidates and refuses
unavailable/unknown preferred files. A bounded native metadata adapter captures
and rechecks namespace kinds without reading excluded contents. Oversized and
ignored preferred-file mutations match clean rebuilds; the previous false fallback
reproduction now returns an explicit unavailable reason. The compiler matrix
retains 173 supported matches and two explicit package-directory refusals.
[Evidence](results/native-implementation/typescript-presence/README.md).
Project-aware publication and persisted presence invalidation remain open;
source/parser/ranker stay 14/19/23 and the ledger stays 23 checked / 7 open.

### Native modern TypeScript file loading (project integration open)

The native file loader now composes with inherited aliases for explicit bundler
and modern Node modes. It preserves authored versus wildcard extension priority,
source/runtime families, suffix order, index-directory/ESM differences and JSON
rules. Distinct positive and negative probes share one bound across substitutions.
Unavailable package boundaries cannot become source files or permit index guesses.

The compiler matrix passes 173 supported cases and 2 explicit package-directory
refusals. Seven core tests and two published-fact mutation tests cover bounds,
priority changes and clean-rebuild parity. This is a callable selected-context
loader, not automatic default alias edges. [Evidence and supported scope](results/native-implementation/typescript-file-loading/README.md).
Final validation: 195 focused tests / 6 suites, strict workspace/all-target Clippy,
173 supported compiler matches plus 2 explicit refusals, and unchanged hashes for
152 crate files and both harness inputs. A separate oversized-candidate probe
reproduces the need for policy-aware presence facts before default bindings.
Project/mode selection, package-directory rules, context propagation and persisted
invalidation remain open. No dependency or persisted-version change is introduced;
ledger remains 23 checked / 7 open.

### Native TypeScript alias dispatch (module/project integration open)

Native `Aliases` dispatches paths/baseUrl from a selected inherited configuration,
retaining exact/wildcard precedence, authored ties, ordered substitutions, declaring
directories and the difference between an unmatched map and a matched map that
finds no file. A bounded loader callback receives the original substitution as
well as its expanded candidate, preserving extension-priority information.

Six core regressions and 17 compiler comparisons cover this dispatch stage. The
research callback deliberately loads exact files only; the comparison is not a
claim of complete native module resolution. Two compiler-only probes exposed and
corrected an insufficient boolean callback interface before final validation.
[Evidence, exact source identities and limitations](results/native-implementation/typescript-alias-dispatch/README.md).
Final validation: 186 focused tests / 5 suites, strict workspace/all-target Clippy,
17 compiler comparisons and matching hashes for all 150 crate files and both
harness inputs. Default project selection, module-mode loading, alias edges, reexport context and
invalidation remain open under 21/23. No persisted-version/dependency change or
ledger closure is implied by this prerequisite. Ledger: 23 checked / 7 open.

### Native TypeScript configuration inheritance (project integration open)

A new core helper merges a caller-selected configuration using only indexed source
facts. Ordered bases merge compiler options individually; `paths` replaces as a
whole and retains wildcard precedence order. Every retained option and membership
field carries its declaring file, and every visited base carries its source hash.
Project references stay local. Missing/unavailable parents, cycles, unsupported
package inheritance and exhausted bounds return a reason without partial results.

Sixteen independent TypeScript 6.0.3 comparisons cover seven copied sibling configs
and nine synthetic cases, including dotted-name `.json` fallback. A public-index
mutation test checks parent edits, malformed input, deletion and recreation against
clean rebuilds after reopen. Full validation and exact source identities are in
[the evidence report](results/native-implementation/typescript-inheritance/README.md).
Final gate: 497 tests / 41 suites, strict Clippy, all 16 compiler cases and matching
hashes for 149 crate files and both harness inputs. No dependency or persisted-version
change is introduced. Automatic project
selection, alias edges, output/source mapping and binding invalidation remain open;
this helper does not change default search resolution. Ledger: 23 checked / 7 open.

### Native TypeScript configuration facts (resolution integration open)

The production registry now projects bounded JSON/JSONC configuration inputs into
hash-bound source facts. It preserves inheritance arrays, raw compiler options,
membership/reference fields, and wildcard path declaration order without adding
a parser dependency. Syntax/budget failures leave an explicit unavailable record;
excluded inputs remain excluded. Source representation 14 refreshes indexes.

[Evidence and exact scope](results/native-implementation/typescript-config-facts/README.md)
cover production publication/reopen, mutation/rebuild equality, independent
persisted validation and a TypeScript parser oracle using disposable source copies.
Final validation passes 491 tests / 40 suites, strict workspace/all-target Clippy,
and 19 TypeScript 6.0.3 oracle cases. All 147 crate files and both driver/probe
sources match the final capture; no production dependency was added.
Project discovery, inheritance application, alias/output mapping and dependency
invalidation remain open under 21/23. The ledger remains 23 checked / 7 open.

### Recommendation 28 lifecycle and retention audit

The [requirement-to-evidence audit](results/native-implementation/generation-lifecycle-audit/README.md)
accepts the current native generation/delta design and explicit concurrency
contract. The original recommendation conditionally proposes larger segments and
owned snapshots; neither is required without evidence supporting adoption.
The native reader lease fix now has 24 mutation/disk runs, 354 retained-reader
checks and 168 clean-rebuild comparisons. A separate 24-run memory capture adds
42 retained-reader checks and own-process RSS samples before/after lazy facts.
Shared disk generations do not imply shared resident graph/index memory.

All 143 current crate files match the final 484-test, 39-suite and strict-Clippy
capture. No production code or dependency changed in this audit. Peak transient
space, broader concurrency performance and release-wide workload evaluation
remain explicit limits under 30; narrower invalidation remains under 23.
Twenty-three recommendations are now checked; seven remain open.

### Generation churn resource experiment

The [churn experiment](results/native-implementation/generation-churn/README.md)
adds a native writer/reader protocol and disposable mutation driver. It checks
retained generation identities, delayed manifest reads, independent reader exit
and crash release, and selected clean-rebuild equivalence while recording sync
time and disk allocation without double-counting hard links. Its completion
certificate and exact scope live with the captured results. The later lifecycle audit above combines this with the separate RSS capture.
This disk experiment itself does not claim RSS, peak transient disk or concurrent
query throughput.

### Recommendation 27 requirement audit

The [requirement-to-evidence audit](results/native-implementation/storage-decision-audit/README.md)
accepts the conditional decision to retain native vectors. It maps every named
storage category to measured evidence, keeps native capacities, serialized facts
and disk allocation separate, and verifies that current lexical/body/metadata
layout modules match both frozen captures. Allocation compaction and an actual
scalar codec were measured and rejected; no production compression is claimed.

This supersedes earlier ledger statements that left 27 open pending a complete
heap census or an acceptable codec. Neither is an unconditional requirement for
retaining the baseline in the original recommendation. Unattributed heap overhead
and alternate codecs remain explicit research gaps; they would matter to a future
adoption claim. The audit does not close recommendation 28's lifecycle work or
30's release-wide quality/resource gates, and does not relabel historical numbers
as parser-19 benchmarks. Twenty-two recommendations are checked; eight remain open.

### Embedded script coordinates and binding-pattern expressions (framework integration open)

The [embedded coordinate adapter](results/native-implementation/embedded-script-coordinates/README.md)
reuses native JS/TS extraction for an explicitly selected source range. It shifts
all coordinate-bearing facts, including resolver lexical bounds, and namespaces
declaration identities consistently through references, parent/binding keys and
documentation associations. Existing merge logic offsets scope/binding ordinals
without asserting one complete ESM surface. Framework extensions are not yet
registered: boundary discovery, directional module/instance visibility, template
relations and embedded parser coverage remain part of recommendation 21.

An independent Svelte-parser comparison against copied whatsurvey source exposed
an ordinary JS/TS adapter bug: destructuring defaults/computed keys contributed
false declaration names, while calls in those expressions were omitted. The
adapter now shares the native scope binding-pattern traversal and walks executable
pattern expressions separately. Parser version 19 expires the old extraction
facts; source version remains 13. The focused regressions check bound-name sets,
call counts, original spans, lexical targets and explicit import provenance.
Final verification passes **484 tests in 39 suite reports**, strict Clippy,
formatting and diff checks. All 143 crate hashes match. On original and
Unicode/CRLF-prefixed copies, the installed Svelte 5.56.10 oracle agrees with three
function spans and ten identifier-call spans; the core resolver preserves
`onTagKeydown → addTag`. Original source remains unchanged. No dependency manifest
or lockfile changed. The pre-fix oracle failure is retained separately; the obsolete
pre-fix workspace run was explicitly terminated and is not final-source evidence.

### Native scalar ordinal codec economics

The [posting codec experiment](results/native-implementation/posting-codec-measured/README.md)
implements a research-only safe Rust delta-varint codec with 128-entry restart
blocks and measures it on all four posting lanes from the three sibling repos.
All 136,998 lists and 2,890,815 ordinals round-trip exactly; independent plain-vector
lower-bound checks, sampled timed-result checksums and three fresh-process
composition captures agree. Three boundary/corruption tests pass. Original
sources and CodeGraph indexes remain unchanged, and all 140 production crate
hashes still match the preceding verified workspace increment.

The codec is not adopted. For lists longer than 128 entries, median paired seeks
are 23.50–24.12 times slower than compact plain-vector binary search; encoding is
7.61–8.34 times slower than copying existing ordinals. Per-list headers and restart
allocations nearly erase modeled live-byte savings on blogwright, before capacity
slack. These are explicit ordinal-kernel measurements, not whole-query latency,
RSS, complete posting layouts or maintenance costs. The initial `ps`-denied run is
preserved separately; the successful run records process-memory fields as null.
Recommendation 27 remains open for complete attribution and an integrated design
that demonstrates a useful memory/latency tradeoff.

### Native workspace dependency identity

- Added bounded native pnpm workspace projection and package.json workspace
  membership, with pnpm-file precedence, explicit manager handling, exclusions,
  hidden-directory rules, duplicate/incomplete-member guards and a shared work cap.
- Workspace-protocol dependencies select unique admitted package identities and
  explicit exports. Conditional targets are retained as invariant only when all
  branches share the same path and include defaults. Potentially relevant root
  overrides prevent unsupported resolved bindings; unrelated overrides do not.
- Persisted validation binds workspace roles to their manifest filenames. Source
  representation is 13; parser 18 is unchanged. Existing manifest rebinding covers
  declaration/dependency edits, additions/removals and reopening.
- Validation: 477 workspace tests across 37 reports, strict all-target Clippy,
  formatting and whitespace checks pass; 140 crate-file hashes match. Independent
  pnpm membership and Node condition probes pass without installs or app execution.
- The six-file whatsurvey fixture resolves five actual `contactNameParts` calls.
  Removing/restoring membership and dependency declarations in the copy removes/
  restores those bindings. Original files remain unchanged. [Evidence and limits](results/native-implementation/node-workspaces/README.md).
- Recommendation 21 remains open for aliases, authored source/build-output mappings,
  framework regions and the recorded Rust/module/receiver gaps. More precise
  invalidation and broader quality/release gates remain open. No dependencies added.

### Configured index-store subtree exclusion

- Reproduced a one-file custom-store build admitting its own lock and WAL files.
- Added precise normalized store-subtree exclusions to the shared walk policy,
  preserving same-named source siblings and external-store behavior. Hidden and
  ignore overrides cannot admit storage; the policy fingerprint binds these paths.
- Store paths equal to or containing the source root now fail before engine open.
  The policy fingerprint invalidates older contaminated source projections.
- Validation: 466 workspace tests across 36 reports; all six focused store tests,
  strict workspace/all-target Clippy, formatting and whitespace checks pass.
  The four-file whatsurvey replay now uses an in-tree store and retains exactly
  four indexed files through all three mapping states; originals are unchanged.
  [Evidence](results/native-implementation/store-exclusion/README.md) binds all
  137 crate files. No dependencies changed; the broader delivery gates remain open.

### Native Node package self-references and private import maps

- Added bounded authored package.json entry-point/import/export/workspace/dependency
  facts, preserving absent, blocked and unsupported targets. Independent persisted
  validation rejects malformed or partial unavailable projections. Source version
  is 12; parser 18 and ranker 23 remain unchanged.
- Package self-references and exact `#` maps now feed the native ESM resolver.
  Nearest boundaries, exports encapsulation, traversal rejection and ambiguous
  runtime/source substitutes prevent speculative targets. File-set changes rebind
  unresolved package consumers, including default imports.
- Focused integration and adapter tests pass; thirteen independent Node probes
  confirm expected package rules and explicitly retain the conditional-export
  feature gap. [Evidence](results/native-implementation/node-package-maps/README.md)
  records the supported subset, source identities and final checks.
- Final frozen validation: 460 workspace tests across 35 suite reports, strict
  all-target Clippy, formatting and whitespace checks pass; 136 crate hashes match.
- A source-backed four-file whatsurvey probe resolves two actual calls through
  `#core`; deleting/restoring only the copied mapping removes/restores both exact
  bindings after reopen. Original source hashes remain unchanged.
- New observed delivery blocker: a custom in-tree store outside `.graph-search`
  is admitted by the source walker. The contaminated first setup is retained as
  a reproduction, excluded from package acceptance evidence. The subsequent
  configured-store exclusion increment addresses it.
- Recommendation 21 remains open for workspace selection, declared aliases,
  framework regions and the other stated module/receiver gaps; recommendation 23
  still requires more precise invalidation. No dependencies were added.

### Native JavaScript/TypeScript ESM bindings

- Replaced unrestricted target-file name matching for ESM imports with typed
  module facts and native export traversal. Named/default/namespace imports,
  aliases, local import forwarding and explicit/star reexports preserve defining
  symbol identity. Private declarations, conflicting stars and type-only runtime
  calls remain unresolved.
- Corrected export-clause AST handling and all declaration bindings; module
  facts now produce import/export leaf references consistently. Escaped module
  strings no longer resolve through an arbitrary string fragment. `require()`
  module edges accept only the actual first literal argument.
- Added record/text/depth/visit bounds, byte-range declaration lookup and
  indexed import normalization. Parser policy advances to 18; no dependency,
  source or ranker policy changes. Reopen/mutation tests compare full occurrence
  output with clean rebuilds.
- Ten independent Node ESM probes confirm aliases, default/namespace access,
  private rejection, cycles, diamonds, ambiguity, explicit-over-star priority
  and default exclusion. Validation is recorded in
  [the evidence directory](results/native-implementation/js-module-bindings/README.md).
- Final frozen validation passes 454 workspace tests across 35 suites, strict
  all-target Clippy, formatting and whitespace checks; all 134 crate hashes match.
- Recommendations 20/21/23 remain open for complete namespace/type/receiver
  semantics, package conditions and aliases, framework regions, invalidation
  precision and corpus-wide quality/economics. This increment does not claim a
  complete JavaScript/TypeScript module loader.

### Native generation leases protect lazy readers

[Reader-retention evidence](results/native-implementation/reader-retention/README.md)
reproduces a missing-file error when an older open store loads extraction facts
after two newer publications reclaim its generation. The engine now pins the
mandatory immutable dangling sidecar with a native shared file lock before
generation validation, and pins prepared stores before CURRENT publication.
Reclamation removes old directories only under a nonblocking exclusive lease.
Current/previous and every distinct live-reader generation remain available;
normal/process exit releases leases, and a later publication retries reclamation.

The new tests cover reopened/prepared stores, multiple processes retaining
original graph and lazy facts through publication, last-reader exit without
destructors, pointer retirement before selection, stable corruption errors and
an eight-attempt retry bound under continuous pointer changes. One extra read-only
descriptor per store is required; no new files, components, unsafe code, persisted
formats or shared-thread `Index` contract are introduced. `SPEC.md` distinguishes
this independent-handle lifetime guarantee from automatically refreshed or
freely shared concurrent queries. Recommendation 28 remains open for broader
mutation/churn and resource measurements; the commit/push gate remains open.

Validation of the parser-17 reader-retention increment: the complete workspace run
passes **444 tests across 33 suites**, strict workspace/all-target Clippy passes,
and formatting/diff checks pass. Crate hashes match the captured source state.
The isolated four-file patch and failing-before/passing-after evidence are saved
with the reader-retention report.

### Hot posting composition and rejected allocation compaction

The [storage composition experiment](results/native-implementation/posting-compaction/README.md)
measures all four native posting lanes, term bytes, metadata norms, source line
occurrences, partial adjacency/node composition and process RSS across nanus,
blogwright and whatsurvey. Disposable instrumentation leaves production APIs,
dependency manifests, sibling sources and existing CodeGraph indexes unchanged.

Live posting payload is 20.70/7.33/57.98 MB; allocated vector capacity is
30.26/11.69/87.29 MB. Many term lists have at most four postings. A native
`shrink_to_fit` candidate removes the capacity slack but fails to demonstrate
a reliable RSS improvement and adds maintenance cost. At 50k synthetic symbols,
full rebuild medians rise 3.09–4.17% and update medians rise 0.67–8.18%.
The candidate is withdrawn; its exact patch and unsuccessful evidence remain
reviewable. Production source hashes are restored to the measured baseline.

Candidate validation: 164 core tests, 48 public integration tests, strict
workspace/all-target Clippy and formatting pass. The maintenance comparison
has 12 mutation/scale cases, three process repeats per arm and five alternating
full/delta pairs per case: 360 pairs, 720 timed builds/updates. Result hashes
agree across builds/repeats, full versus incremental scores agree, and retained
old-generation results stay unchanged. All three composition opens per corpus
are identical except process timing/RSS; cross-arm composition differs only in
posting capacity. Source/binary/sibling/index stability checks pass.

Recommendation 27 remains open: this is partial native allocation attribution,
not a complete heap census or an implemented codec. Doc-ID varint sizes are
explicitly lower bounds without skips/payload/decoding costs. The next candidates
should avoid duplicated analyzer payloads or inactive fields, with checked
ordinal widths and measured memory/maintenance behavior. The full goal and
commit/push gates remain open.

### Bounded native authored-link fields

- Added original spans for inline links, images and URI autolinks, including
  labels, destinations and optional quoted titles. No decoding, URL fetching,
  third-party parser or inferred graph edge is introduced. Destination bytes
  remain in the original body channel without duplicate analyzed fields.
- The declared dialect handles escapes, Unicode/CRLF, empty and angle-delimited
  destinations and bounded balanced path parentheses. Code spans across windows,
  HTML attributes (including multiline quotes) and opaque blocks suppress false
  link metadata. Reference/email/multiline links and recursive inline rendering
  remain outside this initial dialect, informed by the
  [primary link specification](https://spec.commonmark.org/0.31.2/#links).
- Block/file record limits and a byte-inspection budget bound metadata work.
  Partial link metadata has its own stored flag and generation coverage counter;
  body terms remain available after the link cap. Source representation is now 7
  and chunker policy 8; old facts remain readable and policy drift triggers refresh.
- Validation checks field containment, line coordinates, ordering, allowed source
  kinds, record bounds and consistency across overlapping windows. Tests include
  4,096 deterministic malformed/Unicode inputs, capped workloads, shared-window
  fields, old field defaults, publication/reopen, incremental/fresh equality and
  searchable body text after metadata truncation.
- The 352-test workspace run passed. Subsequent final core validation passed 138
  unit and two property tests; all 10 Markdown integration tests passed, including
  the additional file-cap case. The final URI-budget regression passes. Strict
  workspace/all-targets and release-probe Clippy, formatting and whitespace checks
  pass. Cargo manifests/lockfiles remain unchanged.
- [Final corpus evidence](results/native-implementation/markdown-link-statistics/README.md)
  validates 405 recognized link spans across the three sibling repositories, with
  no link cap reached and six exhaustive-scorer comparisons passing. Production,
  sibling source and CodeGraph hashes are stable throughout the capture.
- Recommendations 9/25 remain open for doc comments, the remaining dialect audit
  and controlled final-context evaluation. The review explicitly allows a declared
  native Markdown subset; these fields do not claim full CommonMark conformance.

### Native Markdown paragraphs, flat list items and opaque blocks

- Added separate paragraph and heading units with authored heading ancestry;
  paragraphs split on blank lines, preserving the original UTF-8/CRLF bytes.
- Native flat list items retain exact bullet/ordered marker coordinates and
  continuation lines. Nested or unsupported constructs stay searchable within
  their item and are explicitly flagged rather than promoted to document headings.
  Setext underlines take precedence over empty dash items; thematic breaks are
  excluded from list-marker recognition.
- Opaque HTML/comments, quote, indented-code and reference-like blocks receive an
  explicit evidence kind. Recognized HTML delimiters retain their exact termination
  rules. Other unsupported blocks use a conservative blank-delimited fallback.
- Persisted `MarkdownBlock` descriptors hold original block/marker spans shared
  across 80-line fragments. Validation rejects malformed coordinates, foreign
  kinds and missing required fields. Version-5 facts without descriptors remain
  readable; current source representation is 6 and chunker policy is 7.
- Temporary block boundaries are capped before source-unit construction; overflow
  remains explicit. Tests cover 1,728 mixed block sequences for gap-free original
  UTF-8 partitioning, ordered/empty/nested items, long-block fragmentation,
  malformed metadata, legacy fields and more than 8,192 tiny blocks.
- Public API regressions verify paragraph/list evidence after publication/reopen
  and live source edits. Existing heading/fence/table tests now verify the finer
  paragraph spans and their separate parent references.
- Validation: the 344-test workspace run passed; the final core suite passed
  131 unit and two property tests, including two additional block-partition/cap
  tests added after the workspace run started. Strict workspace/all-targets Clippy,
  formatting and whitespace checks pass. No dependency changes.
- A [three-repository corpus diagnostic](results/native-implementation/markdown-block-statistics/README.md)
  compares current structure with native fixed windows and checks all six complete
  rankings against an exhaustive scorer. All pass; source and CodeGraph hashes
  stay unchanged. Document count/DF/average length and candidate ordering change
  materially, reinforcing the need for the final-context quality gate.
- Recommendations 9/25 remain open for separate doc comments, links/recursive
  containers and the controlled equal-budget quality comparison. Finer structural
  units change body document statistics; no ranking improvement is claimed yet.

### Opaque Markdown HTML boundary shielding

- Fixed a concrete evidence-integrity gap: top-level ATX headings and fences
  inside HTML comments or raw HTML could previously change heading ancestry or
  hide subsequent authored Markdown behind an invented open fence.
- Added native delimiter tracking for CommonMark HTML block types 1–5, guided by
  the [primary block specification](https://spec.commonmark.org/0.31.2/#html-blocks).
  The scanner retains original bytes and shields content across blank lines;
  terminator matching resumes Markdown on the next line. Unclosed blocks extend
  through EOF. Raw tag delimiters are ASCII case-insensitive; comments, CDATA and
  processing instructions use their exact delimiters. No third-party component.
- Regression fixtures cover LF/CRLF, Unicode, false headings/fences/tables,
  same-line closers, mismatched raw-tag closers, malformed prefixes, indentation,
  fenced-code precedence and searchable multi-window blocks with valid ancestry.
- Chunker policy advances from 5 to 6; serialized source representation remains
  5. Existing policy mismatch reconciliation/live-overlay handling applies.
- Validation: all 340 workspace tests pass (`cargo test --workspace --locked`),
  including 126 core tests. Workspace/all-targets strict Clippy, formatting and
  diff whitespace checks pass. Cargo manifests and lockfiles remain unchanged.
- Recommendations 9/25 remain open for paragraphs, lists, links, generic HTML
  and nested containers, and equal-budget evidence evaluation. Prior benchmark
  artifacts retain their captured source hashes and describe earlier code.

### Literal execution and JS/TS coordinates

- Literal matching compiles one finder per request, scans each line once, and stops on the first omitted matching line. No repeated prefix counts or line lookups remain.
- The line-oriented contract rejects CR/LF patterns in both case modes. Unicode lowercase preserves original evidence; it does not claim full case folding.
- Regression cases cover CRLF, repeated matches, final lines, zero and positive limits, dense matches, Unicode expansion, and multiline rejection.
- JS/TS delegates spans to the common zero-based half-open byte helper; source-slice tests include a byte-zero declaration and multibyte CRLF input. Parser revision 3 invalidates prior extraction caches.
- `cargo test --workspace --locked`: 100 tests passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- Recommendation 3 remains open for source hash binding and the remaining coordinate audit. Other unchecked recommendations remain outstanding; no completion or delivery claim is made.

### Coherent native generation publication

- Reconciliation now calls `GraphStore::publish`; the Grafeo adapter prepares an isolated replacement and publishes graph, dangling references, and manifest together through a checksummed `CURRENT` descriptor.
- Existing Grafeo storage remains the graph representation. Native code owns generation allocation, visibility, file/directory sync, integrity validation, and retention. No dependency or lockfile changes.
- Prepared graph files open read-only. Failed preparation leaves the old handle and pointer untouched. A failure after pointer rename makes the handle unavailable until reopen, because claiming rollback at that point would be false.
- Legacy roots migrate on publication. Explicit stored paths replace display-ID parsing for projection ownership, covering `#` in paths.
- Tests inject failures after deletion, insertion, edges, graph persistence, dangling persistence, manifest persistence, and pointer publication. Separate child-process exits bypass destructors at six boundaries. Reopen, changed-source retry against a clean build, corrupted artifacts, orphan directories, migration, and two-generation retention are covered. This verifies process interruption, not physical power loss.
- The original filesystem-failure probe now returns `old_present=true`, `new_present=false`. See [probe](results/native-implementation/generation-probe.json).
- `cargo test --workspace --locked`: 109 tests passed. Strict workspace/all-target Clippy passed. Native-only reproduction completed at `/tmp/graph-search-generation-review-20260919`.
- Frozen 193-file source profile: initial publication 144 ms, exact symbol query 10.0 ms, explore 22.2–22.7 ms. These are a single warm small-corpus profile, not an update/load benchmark. [Profile](results/native-implementation/generation-repository-probe.json), [source provenance](results/native-implementation/generation-provenance.json).
- Initial publication copies a full graph in memory; subsequent publications do too. Recommendation 28 remains open for measuring changed-file overlays and publication costs under churn. Recommendation 24 remains open for analyzer/chunker/ranker/policy identities beyond the now-independent generation storage format.

Next: attach generation/freshness context to public library results and bind served snippets to indexed source fingerprints; then finish execution/coverage budgets before retrieval changes.

### Result provenance and verified source excerpts

- All library search result types now carry generation/freshness/source context. Graph and impact identities come from the selected store snapshot; explore adds fingerprints from its shared source cache. Live text hits carry their observed file hash.
- `Verification::Metadata` states its heuristic boundary. `Verification::Content` and CLI `--verify-content` detect restored-mtime edits; automatic strict-mode drift triggers a full rebuild, while never/explicit modes report drift. No whole-workspace atomicity claim is made.
- Indexed snippets are withheld on hash mismatch. Missing/invalid source and byte-budget exhaustion have distinct statuses. Live body spans and snippets use the same captured bytes. Snippet lines remain verbatim; source hashes refer to original file bytes including line endings.
- The broader coordinate audit found the same off-by-one byte conversion in Rust. All five language adapters now pass raw-source slice, multibyte, and CRLF coordinate tests. Parser revision is 4. Result wire schema is independently versioned at 3.
- CLI envelopes preserve library context, stale flags reflect that context, and text output explains withheld evidence. Context participates in explore's serialized byte limit.
- Regression tests cover stale/missing snippets, restored mtimes, strict refresh, generation continuity, graph/impact context, live body/text identities, single-version request caching, budget exhaustion, and CLI JSON behavior.
- Workspace: 117 tests passed; evaluation: 24 Python tests passed; strict workspace/all-target Clippy passed. Native probe completed successfully, with stale evidence now reporting mismatch and no indexed snippet. [Probe](results/native-implementation/provenance-probe.json).
- Recommendation 2 remains open for explicit coverage metadata, which is implemented with the walker/execution budgets next. Recommendation 12 remains open for matched-region selection, larger useful contexts, and evidence packing.

- Native-only reproduction also passed at `/tmp/graph-search-provenance-review-20260919`; [profile](results/native-implementation/source-repository-probe.json) and [build provenance](results/native-implementation/source-provenance.json) are retained.

### Coverage and safe reconciliation

- Searches, status, and sync report the inclusion policy, its fingerprint, enumeration completeness, work truncations, and observed coverage losses. Zero counters are omitted; unperformed enumeration is distinct from complete enumeration. Ignored descendants are not misleadingly counted as searched.
- Enumeration sorts paths before file limits and bounds directory entries as well as files. Incomplete enumeration cannot imply deletion: sync/reindex refuse to publish. A source that disappears during extraction aborts preparation and preserves the previous graph and manifest. Live text reads are bounded even if a file grows after enumeration.
- Binary and invalid UTF-8 source has explicit status; index extraction quarantines it. Disabled languages produce file nodes only. Policy changes invalidate cached parser facts, including enable/disable cycles.
- Body filters now precede scan quotas. The native late-file reproduction finds both early and late targets, scanning one file in each filtered request.
- Tests cover deterministic/exact file caps, directory-entry caps, invalid ignore patterns, unknown/disabled languages, binary/invalid source, incomplete reconciliation, policy changes, and disappearance between enumeration and extraction.
- Workspace: 125 tests passed; evaluation: 24 Python tests passed; strict workspace/all-target Clippy passed. Native reproduction passed. [Probe](results/native-implementation/coverage-probe.json), [profile](results/native-implementation/coverage-repository-probe.json), [provenance](results/native-implementation/coverage-provenance.json). The graph payload, body longest-token, metadata starvation, and local-shadowing probes still reproduce outstanding findings and remain tracked under their respective recommendations.
- Recommendation 5 remains open for graph work/edge/payload limits, request cancellation and broader execution budgets. Recommendation 24 now includes a policy fingerprint; analyzer/chunker/ranker identities remain outstanding.

### Graph and impact payload packing

- Core graph query entry points enforce the 64 KiB compact serialized result ceiling. The service enforces it again after attaching generation, freshness, and source provenance. Impact uses the same native packer.
- Packing removes tail edges before ranked nodes, reports byte truncation, preserves candidate/depth counts, and recomputes approximation counts from retained edges. Unreferenced source identities are removed; required metadata is never silently stripped. Metadata that cannot fit produces an explicit `ResultBudget` error.
- The removal pass accounts for serialized tail bytes in batches, avoiding a full serialization per removed edge. Determinism, repeated fitting, impact counts, metadata-only failure, and the public service boundary have regression coverage.
- The native 2,000-edge star with a one-node result cap is now 65,522 bytes with 457 edges, down from 282,400 bytes. This fixes the core payload overflow; it does not yet bound traversal work or CLI envelope/pretty-print overhead. Recommendation 5 remains open for those boundaries, independent edge/work limits, and cancellation.
- Workspace: 129 Rust tests passed; strict workspace/all-target Clippy passed; native reproduction passed. [Probe](results/native-implementation/payload-probe.json), [profile](results/native-implementation/payload-repository-probe.json), [provenance](results/native-implementation/payload-provenance.json).

### Native adjacency and graph execution budgets

- Both adapters now prepare native adjacency with the store generation. Stable ordered edges are stored once; per-node adjacency holds integer positions. Bounded reads stop before cloning omitted entries and charge filtered candidates. Query execution does not sort or materialize the complete high-degree neighborhood first.
- Public core graph queries and library `SearchService::with_work_limits` share node/edge work accounting across traversals, connection search, and impact summaries. Defaults are 10,000 nodes / 50,000 adjacency entries, hard ceilings 100,000 / 500,000. Reused engines reset budgets per query; stats expose charged work and truncations identify partial counts/evidence.
- A native cloneable cancellation token and monotonic deadline are checked cooperatively. Already-cancelled/expired service queries stop before reconciliation. These controls do not claim hard latency bounds on index opening, source walking, lexical construction, or filesystem calls.
- Core expansion now uses a visited set across BFS rings, replacing the persistent adapter's repeated cycle expansion in query paths. Shortest-path search stops on first arrival and charges node admission before extending its predecessor map.
- Adapter conformance covers zero/exact budgets, per-query reset, and node exhaustion. Additional tests cover high-degree filtered reads, self-loops/deduplication, cancellation checkpoints, public cyclic traversal, partial path search, and cancellation/deadline behavior before automatic reconciliation.
- The 2,000-edge native star under a four-node/eight-edge budget reports exactly four nodes visited and eight adjacency entries examined, with both work truncations. The regular payload is 65,435 bytes after adding work stats. [Probe](results/native-implementation/work-probe.json).
- Workspace: 133 Rust tests passed; 24 evaluation Python tests passed; strict workspace/all-target Clippy passed; native reproduction passed. Frozen 193-file profile: reindex 161 ms, exact symbol 14.6 ms, explore 36.9–37.9 ms. [Profile](results/native-implementation/work-repository-probe.json), [provenance](results/native-implementation/work-provenance.json). This is a warm small-corpus measurement, not a churn or peak-memory benchmark.
- Recommendation 5 remains open for CLI envelope/formatting overhead, independent returned-edge limits, full-query source/work accounting, and postings budgets. Recommendations 7/8 will remove per-query metadata/index construction before adding their candidate-work accounting. Recommendation 22 still needs evidence-driven selection and reuse across impact/connection subqueries.

### Generation-owned metadata and native postings

- Both adapters build immutable bare-name, qualified-name, file-language, lexical-statistics and posting structures during generation preparation/open. Snapshots borrow them directly; neither exact lookup nor explore reconstructs name/token maps per request. Queries compile their path filter once and reuse cached file languages.
- An ordered native term dictionary owns sorted ordinal/frequency postings. Retrieval accumulates only matching posting unions, using compact ordinals until candidate materialization. The existing field weights, combined length normalization, clipped IDF, exact-name priority and score transform remain unchanged.
- `WorkLimits` now includes candidate and posting ceilings. Exact-name candidates reserve admission first; filters precede candidate allocation; all examined postings consume work. Stats and truncations distinguish candidate admissions from posting examinations. Defaults are 10,000 candidates / 200,000 postings, hard ceilings 100,000 / 1,000,000.
- Differential tests independently reconstruct the original exhaustive document maps and scorer, then require identical floating-point bits across empty/single/mixed corpora, repeated terms, stopwords, missing terms and Unicode. Additional tests cover sparse work counts, cap exhaustion, exact evidence under posting exhaustion, cached name precedence, filters, publication/removal and reopen cache lifecycle.
- Workspace: 138 Rust tests passed; strict workspace/all-target Clippy passed. Native score maps equal the exhaustive reference and the independent review prototype. On 50,000 symbols: selective native postings score 50 entries in 0.0016 ms; a broad query scores 50,050 entries in 4.68 ms. The full-size benchmark explicitly raises candidate limits so it measures exact execution rather than a truncated default run.
- Frozen 193-file warm profile: reindex 158 ms, exact symbol 9.6 ms, explore 26.1–27.2 ms, versus the preceding 161 / 14.6 / 36.9–37.9 ms profile. These single-corpus figures are not a large-index memory or churn benchmark.
- Recommendations 7/8 remain open for per-file posting/statistic maintenance and independent representation identities, per-field statistics, conjunction planning, bounded top-k candidate selection, and the broader lifecycle/evaluation gates. The current cache deliberately rebuilds from each coherent graph generation; no persisted codec or score pruning is claimed.
- Native reproduction artifacts: [probe](results/native-implementation/postings-probe.json), [profile](results/native-implementation/postings-repository-probe.json), [provenance](results/native-implementation/postings-provenance.json).

### Bounded metadata selection and field statistics

- Native metadata search now selects the best compact ordinals with a deterministic bounded heap before cloning node properties. Selection is O(C log k) after scoring C admitted candidates; there is no score pruning. Score ties use the generation's path/line/id order.
- Explore's metadata pool is separate from its final context count: at least 64, otherwise four times k, capped at 500. It is always at least the clamped final k. This preserves the existing metadata/body union's top k while avoiding full-node materialization for every metadata match. Candidate statistics retain the pre-selection count.
- Postings now retain unweighted name/path/signature term counts and separate field lengths, alongside the existing weighted frequencies. Production scoring is unchanged; independent reference tests still require identical float bits. This prepares the field-normalization experiments without silently adopting a new formula.
- A full-sort differential test covers exact-name priority, score ties, missing terms, k=0/1/7/64, and k beyond the candidate count. Field-statistic tests distinguish raw counts from scoring weights.
- Workspace: 140 Rust tests passed; strict workspace/all-target Clippy passed; native reproduction passed. Frozen 193-file profile: reindex 178 ms, exact symbol 9.5 ms, explore 26.9–29.7 ms. Separate field data adds memory/preparation work; these warm single-corpus timings do not establish the large-corpus memory tradeoff.
- [Probe](results/native-implementation/selection-probe.json), [profile](results/native-implementation/selection-repository-probe.json), [provenance](results/native-implementation/selection-provenance.json).
- Recommendations 8/11 remain open for conjunction planning, retrieval-channel fusion, evidence diversity and larger lifecycle/evaluation gates. Source-valid external evidence evaluation completed for this increment.
- External regression: all 306 evidence-v1 trials completed (34 source-valid tasks × three arms × three repeats), with no protocol errors. Every per-trial evidence score equals the original baseline, including graph-search's required-file delivery on 19/34 tasks and complete-region delivery on 6/34. No agent/model task-success claim is made. These are previously exposed labels; fresh-query validation remains outstanding. [Comparison](results/native-implementation/selection-external/comparison.json), [results](results/native-implementation/selection-external/external-results.json), [source stability](results/native-implementation/selection-external/candidate-source-stability.json).

### Persisted native source regions

- Reconciliation now produces hash-bound source-region facts for every readable UTF-8 file, including configuration, Markdown, unsupported languages and parser-quarantined text. Original source blobs are not copied into the index. Token facts retain absolute source-line occurrences.
- Regions partition at exact declaration byte boundaries, including nested or same-line declarations. The smallest enclosing declaration owns each region. Long regions use 80-line windows with eight-line overlap. UTF-8/CRLF boundaries remain valid; the 8,192-unit per-file ceiling is explicit in persisted facts and coverage reports.
- Source facts are part of each file projection and are preserved during dependency-only rebinding. They replace/remove with their file and match clean rebuilds after edit/remove/rename sequences. File byte counts now describe the captured bytes rather than potentially stale enumeration metadata.
- `source-units.json` participates in native generation preparation, hashing, sync and atomic publication. Descriptor format 2 requires this artifact; format 1 remains readable. The independent manifest source representation revision triggers a rebuild of legacy source coverage under automatic reconciliation.
- Source indexed-file/unit counts and cap losses are exposed in sync/status/result coverage. Zero statistic counters now omit their wire representation and default to zero on decode, avoiding gratuitous metadata overhead in small payload budgets.
- Tests cover source hash binding, non-parser content, Unicode/CRLF windows, nested/same-line declarations, unit overflow, byte ownership, dependency reuse, clean rebuild equivalence, reopen, legacy migration, failed source publication and checksum corruption. Existing fault/process-interruption matrices now include the source persistence stage.
- Workspace: 148 Rust tests passed; strict workspace/all-target Clippy passed; native reproduction passed. Frozen 193-file profile: 4,042 source units with no unit-cap loss; graph artifact 762,354 bytes, manifest 2,647,402 bytes, source facts 3,800,722 bytes. Reindex 271 ms, exact symbol 9.9 ms, explore 26.1–26.9 ms. Source preparation adds cost that must be assessed with the body-query gains and later storage work.
- [Probe](results/native-implementation/source-regions-probe.json), [profile](results/native-implementation/source-regions-repository-probe.json), [provenance](results/native-implementation/source-regions-provenance.json).
- Recommendation 9 remains open: persisted regions are prepared but explore still uses the live longest-token fallback. Next: generation-owned body postings, source-bound region candidates, changed-file overlays, channel fusion, and match-centered context packing. Comment-specific and Markdown-structure treatment also remain outstanding.

### Native indexed body retrieval and channel selection

- Both store adapters prepare immutable source-region posting lists with each generation. Queries enumerate the union of all matching terms and charge postings/candidate admissions; filters and live-file masks precede accumulator allocation. Positive-IDF BM25 ranks source regions. Best-region deduplication occurs per graph owner or unowned file before body top-k selection.
- Known changed files replace indexed body facts with bounded request-local facts. The live overlay reads at most 512 eligible files / 8 MiB and shares captured bytes with snippet verification. Unchanged indexed files require no body scan; complete enumeration masks removed paths. Unchecked direct core queries may verify a bounded prefix without claiming freshness for unobserved facts.
- Metadata reserves at most half the request's lexical work, leaving body capacity. The live overlay has a further fractional reservation; unused capacity remains available. Work counters now use the accurate name `retrieval_candidates_admitted`, since admissions include both metadata and body candidates.
- Metadata/body ranks combine through native reciprocal-rank fusion (constant 60), preserving a separate exact-name priority tier. Indexed/live body scores merge by rank because their corpus statistics differ. A two-entities-per-file first selection pass prevents repeated declarations in one file from consuming all final slots; deferred entities fill spare slots and exact names are exempt.
- Body evidence carries its own source hash, region span, owner, kind, matched line and live/indexed origin. These coordinates never overwrite graph declaration coordinates. Snippets center on the line with the most distinct query terms, earliest on ties, and require the captured source hash to match.
- The independent exhaustive body scorer agrees with postings; tests cover filters before allocation, masked files, examined-posting limits, matches beyond the old 512-file boundary, deep-function body locations, exact-name priority, metadata crowding, single stopword queries, and changed-source replacement. Existing provenance/atomicity suites remain passing. Workspace: 155 Rust tests passed. Strict workspace/all-target Clippy passed after two test-only style corrections.
- Both outstanding native body reproductions now pass: `cache invalidation` retrieves a file containing only `cache`, and the body guide survives eight metadata matches. The late filtered-file reproduction continues to scan one eligible file. [Probe](results/native-implementation/body-probe.json).
- Frozen 193-file warm profile: reindex 292 ms, exact symbol 9.2 ms, explore 11.2–13.7 ms, versus source-region preparation's 271 / 9.9 / 26.1–26.9 ms. No unchanged body files were scanned for these queries. These remain single small-corpus measurements. [Profile](results/native-implementation/body-repository-probe.json), [provenance](results/native-implementation/body-provenance.json).
- External evidence-v1 evaluation: all 306 trials completed without protocol errors; sibling source fingerprints were unchanged. Required-file delivery improved from 19/34 to 26/34 graph-search tasks. Complete-region delivery regressed from 6/34 to 5/34. There were eight newly delivered required-file tasks and one lost task; region outcomes also include gains and losses. This is a mixed result, not an accepted overall quality win. Context packing and ranking regressions must be addressed before release. The labels were previously exposed, and this protocol does not measure agent/model task success. [Detailed comparison](results/native-implementation/body-external/comparison.json).
- Recommendations 9/11/12 remain open for structural Markdown/comment treatment, query routing and controlled fusion/diversity ablations, larger/multiple evidence intervals, relationship-site context, and fresh-query quality gates. No third-party components were introduced.

### Verified multi-interval context assembly

- Explore now reserves its required metadata and primary snippets, then considers complete declarations up to 80 lines, body-match regions, headers of larger declarations, and up to four returned relationship sites per item. Additional intervals carry a source hash, original start line, verbatim lines and a declaration/body/reference role.
- A native deterministic packer orders candidates by rank-adjusted value per estimated byte and admits them using exact serialized item costs. It removes lines already delivered from the same source version, splitting at gaps rather than fabricating contiguous source. The response admits at most 64 additional intervals, reports omitted context, and retains the existing compact serialized byte ceiling. Final library/CLI packing removes optional intervals before edges or nodes.
- `context_lines=0` suppresses all source excerpts. The existing primary excerpt remains at most ten lines. Additional intervals use only the request's captured, hash-verified bytes: a mismatched or unavailable source cannot gain extra context through this path. No new file reads are hidden in the packer.
- CLI text renders labeled intervals. The evaluation adapter renders their tool-provided source lines; it never fetches additional source. Its new regression test verifies source gaps and forbids source reads. Evidence scoring, labels, call limits and response/context byte budgets are unchanged.
- Tests cover complete small implementations, exact original lines, extra-context deduplication, multiple byte budgets, context suppression, distant call sites, headers plus deep matches, and hash mismatch suppression. The full workspace passed 158 Rust tests with the 200-line ablation; the retained 80-line implementation passed the preceding 157-test full run plus the new connection test and all eight body/context integration tests. Strict workspace/all-target Clippy passed; evaluation Python suite passed 25 tests.
- Controlled external ablation: the 80-line and 200-line variants produced identical evidence scores and response sizes across all 306 trials. The retained ceiling is 80; raising it had no observed benefit on this set. All trials completed without protocol errors and sibling source snapshots stayed unchanged.
- Relative to the original baseline, the retained body/context implementation delivers all required files on 26/34 tasks (was 19/34), all required regions on 6/34 (was 6/34), and mean per-task region coverage 30.96% (was 23.20%). Mean delivered response bytes per graph-search trial increased from 24,704 to 33,017 across the protocol's bounded calls. These are mixed, previously exposed-label retrieval results, not agent/model task success or a fresh heldout win. Individual regressions remain documented in the comparison.
- [Ablation and per-task changes](results/native-implementation/context-ablation.json), [80-line results](results/native-implementation/evidence80-external/external-results.json), [200-line results](results/native-implementation/evidence200-external/external-results.json), [80-line profile](results/native-implementation/evidence80-repository-probe.json), [80-line provenance](results/native-implementation/evidence80-provenance.json). The 80-line warm profile reports explore 11.7–13.5 ms on the frozen 193-file corpus.
- Recommendation 12 remains open for complementary regions within large owners, adaptive context selection and broader quality gates. Recommendations 10/11 next require query-intent routing and controlled channel/diversity ablations: adding context alone did not resolve all ranking losses. Structural Markdown, scope-aware resolution, occurrence identity, incremental-storage optimization, and the remaining review ledger are still outstanding. Commit/push remains pending completion of that work.

### Explicit query routes and measured automatic ranking

- Added typed exact-name, exact-ID, path-glob and term modes; explicit navigation misses remain misses. Optional exact fast-path lookup applies filters before deciding whether to stop. Query input is bounded to 8,192 bytes and 128 distinct terms, with errors instead of silent term loss.
- Metadata, body and fusion strategies remain selectable. AND/minimum-term coverage uses the same charged posting pass; work exhaustion cannot claim unobserved terms matched. This is union accumulation with coverage filtering, not yet a shortest-posting intersection executor.
- Optional plans explain actual attempted routes and original query/options. Per-item diagnostics report channel ranks and exact evidence. CLI exposes intent, ranking, minimum/all terms, per-file diversity, fast path and explanation controls.
- Controlled ranking × diversity experiment: 918 evidence-v1 trials over the existing 34 source-valid tasks, nine configurations, three repeats. Body ranking with diversity disabled delivered all required files on 28/34 and all required regions on 10/34; mean region coverage was 49.45%. The preceding fusion/diversity-two default delivered 26/34 and 6/34, with 30.96% mean region coverage. [Full ablation](results/native-implementation/ranking-ablation/summary.json).
- Before the first retrieval run, froze 12 newly authored source/hash-bound queries across the three sibling repositories. These form a small convenience sample, not a blinded benchmark. Across 324 trials, body ranking without diversity delivered required files on 12/12 and complete regions on 10/12, compared with 10/12 and 3/12 for the preceding default. [Frozen suite](fixtures/fresh-routing-2026-09-19/freeze.json), [first-run ablation](results/native-implementation/ranking-fresh/summary.json).
- Adopted automatic multiword body-first retrieval, with metadata fallback when the body pool is empty. Single-token automatic queries retain exact-priority fusion. Per-file diversity is disabled by default but remains an explicit soft selection control. Follow-up automatic-routing runs match body-only evidence on every one of 102 existing-set and 36 fresh-suite trials. The follow-ups reuse now-exposed labels and are implementation checks, not further independent validation. All runs had zero protocol errors and unchanged sibling source fingerprints. [Existing-set verification](results/native-implementation/routing-auto/comparison.json), [new-suite verification](results/native-implementation/routing-auto-fresh/comparison.json).
- Workspace: 165 Rust tests passed, strict all-target Clippy passed, and 25 evaluation Python tests passed. Native reproductions passed. Frozen 193-file warm profile: reindex 286 ms, exact lookup 10.1 ms, explore 11.6–13.4 ms. [Profile](results/native-implementation/planner-repository-probe.json), [provenance](results/native-implementation/planner-provenance.json).
- Recommendations 8/10/11 remain open for true conjunction execution, literal/phrase integration, complementary regions and broader release gates. No model task-success claim is made, and no third-party components were added.

### Source ownership and query-boundary integrity

- Source facts now validate declaration ownership in both stores before mutation, and against the selected graph on persistent reopen. Missing owners, file nodes, foreign-file owners and owners that do not enclose the region in both bytes and lines are rejected.
- Shared adapter conformance proves that rejection preserves existing nodes/source facts even when the rejected batch requests deletions. A reopen regression recomputes the artifact checksum around invalid owner facts, proving semantic validation works independently of checksums.
- Request-local source coverage now counts distinct budget-withheld paths and distinguishes total-read from per-file byte truncations, including paths absent from final results. Repeated reads of a cached path do not inflate that count.
- Primary snippet execution clamps oversized direct/decoded context radii around the match. Regression coverage checks ordinary, maximum-builder and `u32::MAX` radii against a deep body match; all preserve the anchor within ten lines.
- Workspace: 167 Rust tests passed; strict workspace/all-target Clippy passed. Rebuilding all three sibling indexes and running 102 evidence-v1 trials produced identical per-trial evidence to automatic routing, zero protocol errors and unchanged source fingerprints. [Comparison](results/native-implementation/source-boundaries/comparison.json), [provenance](results/native-implementation/source-boundaries/provenance.json).
- These close specific integrity gaps; the broader work-budget, context-selection and release ledger remains open.

### Native bounded whole-name prefixes

- Added explicit `name_prefix` library intent and CLI `--intent name-prefix`, using range traversal over generation-owned ordered bare/qualified dictionaries. Spelling is case-sensitive and unchanged, including punctuation and combining marks. Empty input is rejected; a miss does not broaden or trigger correction. This mode is a whole-name prefix, explicitly distinct from token prefixes, globs and substrings.
- Prefix expansion has a separate configurable work ceiling: 256 dictionary entries by default, 4,096 hard maximum. Every expanded dictionary entry and examined posting is charged; filters and deduplication precede candidate admission. Stats/truncations distinguish expansion, posting and candidate limits. Bare dictionary traversal precedes qualified traversal; duplicate entities are returned once.
- Differential tests compare native range lookup with an exhaustive name predicate across case, qualification, acronym/digit, accented and combining-mark examples. Tests cover zero/exact expansion budgets, filtered candidate admission, sync/reopen cache replacement, explicit-route isolation, diagnostics and CLI wiring.
- Workspace: 170 Rust tests passed; strict workspace/all-target Clippy passed. Recommendation 18's prefix-before-fuzzy step is complete. Token-prefix expansion and edit-distance suggestions are not implied by this whole-name contract; fuzzy rewriting remains deferred as recommended. Recommendation 13 remains open for whole-identifier and qualified lexical fields inside conceptual queries.

### Preserve metadata when source excerpts exceed the output budget

- Early explore packing now omits an oversized primary excerpt before rejecting its entity, and continues considering later candidates when an individual item cannot fit. It reports the effective byte ceiling, including the default ceiling when `max_bytes=0`, without shortening or fabricating source lines.
- Final library and CLI envelope packing remove optional intervals and edges, then primary snippets, before dropping symbol metadata. Required-metadata overflow uses the shared `ResultBudget` error; CLI treats an insufficient requested budget as a usage error.
- Text output now reports source-read budget losses for paths that never reached the final result, including the same total/per-file truncation messages exposed in JSON coverage.
- New library and process-level CLI regressions exercise a 20,000-character source line with 4/8/16 KiB payload ceilings, ensure both symbols survive, verify explicit truncation and empty excerpts, and check that an impossible one-byte envelope fails without partial stdout.
- Full workspace: 171 Rust tests passed before the final CLI contract addition. The final ten-test CLI contract suite passed, bringing the verified total to 172 distinct Rust tests; strict workspace/all-target Clippy passed after all changes.
- External checks: all 102 existing-set and 36 newly added-suite trials retained identical evidence and response byte counts versus the adopted automatic-routing policy, with zero protocol errors and unchanged sibling sources. [Existing-set comparison](results/native-implementation/packing/comparison.json), [new-suite comparison](results/native-implementation/packing-fresh/comparison.json). These suites are now exposed regression evidence, not fresh independent validation.
- Recommendation 5 remains open for graph CLI envelope/formatting overhead, returned-edge controls and full-query source/work accounting. Recommendation 12 remains open for complementary large-owner regions and adaptive selection. Commit and push remain pending the rest of the requirement ledger.

### Native posting-list conjunction

- AND and minimum coverage equal to all distinct terms now drive from the shortest posting list and perform monotonic galloping/binary seeks through the rest. Missing term lists prove an empty lexical intersection immediately. Each inspected posting, including seek probes, consumes work; interrupted verification cannot admit an unproven candidate.
- Filters precede intersection probes on other lists and candidate admission. Only complete matches allocate score/coverage entries. After a verified match, advancing all cursors past that ordinal avoids re-reading dense matches. Exact metadata navigation retains its existing separate priority contract; lower minimum-term thresholds retain union/coverage execution.
- Metadata preserves input-term scoring order and repeated-term weights; body scoring preserves rarity order, corpus statistics and owner deduplication. Independent exhaustive metadata scores agree bit-for-bit, body conjunction scores agree with complete union scoring, and generated sorted-list intersections agree with exhaustive membership across 8,192 list/filter/order cases. Tests sweep every probe cap of a selective fixture and verify that only fully established matches are emitted.
- Candidate-budget regressions show that one allowed candidate suffices to retrieve a late complete match among 999 partial matches in both metadata and body retrieval. Dense execution charges each matching posting once. Full workspace: 178 Rust tests passed. Strict workspace/all-target Clippy passed before the final cursor advance; the release research harness strict lint passed, and final lint is repeated with the next boundary increment.
- Release microbenchmark, 50,000 symbols: selective AND admits 50 candidates and examines 1,150 postings in 0.0059 ms, versus union filtering's 50,000 admissions / 50,050 postings / 9.78 ms. Dense AND examines 100,000 postings in both implementations, at 7.68 ms for intersection versus 14.09 ms for union filtering. An absent term proves no matches without posting probes. All complete score maps equal the independent native prototype oracle. These warm synthetic measurements exclude index construction and are not end-to-end latency claims.
- [Probe](results/native-implementation/conjunction-native-probe.json), [frozen-corpus profile](results/native-implementation/conjunction-repository-probe.json), [provenance](results/native-implementation/conjunction-provenance.json). Recommendation 8 still requires per-file maintenance and broader lifecycle/scale gates; conjunction execution is now implemented.

### Final graph JSON envelopes and independent delivered-edge limits

- Graph and impact CLI output now uses compact JSON and measures the complete transport envelope, including query/root/provenance fields, against 64 KiB before emitting stdout. Native batch trimming removes tail edges before ranked nodes, then rechecks actual bytes. Required metadata is retained; an impossible envelope fails with `ResultBudget` instead of emitting partial JSON.
- Impact depth totals and candidate/work counters survive output packing. Approximation counts are recomputed from delivered edges, and source identities are retained for delivered nodes or edge sites. Process-level regressions cover a 200-caller graph with long identifiers; both graph and impact envelopes stay within the ceiling while preserving complete measured impact totals and generation identity.
- Added `WorkLimits.returned_edges`, default 1,000/hard maximum 10,000, independently of adjacency examination work and node limits. All graph, impact and explore result paths apply it before final serialization. Zero is valid; `returned_edges` truncation appears only when relationships are omitted. Tests cover zero, partial and exact limits while preserving traversal work, graph nodes, impact depth counts and path nodes.
- Full workspace: 181 Rust tests passed; strict workspace/all-target Clippy passed after the final cursor/CLI/edge changes. The first new path test assumed directed traversal; it was corrected to compare against the API's existing undirected path result, then the full suite passed. No production path semantics were changed.
- All 102 existing-set and 36 newly added-suite evidence-v1 trials retained identical evidence scores, with zero protocol errors and unchanged sibling source fingerprints. These are exposed regression suites. [Existing-set comparison](results/native-implementation/edge-output/comparison.json), [new-suite comparison](results/native-implementation/edge-output-fresh/comparison.json).
- Recommendation 5's graph JSON transport overhead and independent returned-edge controls are implemented. Full-query source/work accounting remains open. Other outstanding recommendations remain in the numbered ledger; no completion, commit or push claim is made.

### Native whole-identifier analysis and source representation v2

- Added opt-in `AnalysisMode::Identifiers` / CLI `--analysis identifiers` across metadata, indexed body and live-overlay retrieval. Original whole lexemes and normalized split terms have separate occurrence fields. Whole matching uses Rust Unicode lowercase; qualified metadata has an explicit weight-4 field when different from the bare name. Alias frequencies and lengths use a maximum, avoiding additive double counting. Existing name/path/signature weights, clipped metadata IDF and combined normalization remain unchanged.
- The native analyzer preserves original UTF-8 offsets/spelling, underscores, combining marks and non-ASCII text. Its documented delimiters are Unicode whitespace and ASCII punctuation except underscore. It does not claim language-specific identifier validity, canonical normalization, full case folding or confusable folding. Whole fields keep stopwords, so a retained single-word query can match a parameter such as `is`. Exact navigation and raw text contracts remain separate.
- Source representation v2 stores original whole occurrences; the manifest records analyzer revision and Unicode table identity. Legacy v1 generations remain readable, automatic reconciliation rebuilds incompatible facts, and identifier requests against unreconciled legacy facts use bounded live regeneration. Both adapters reject empty terms/occurrences, unsorted occurrences and lines outside the owning region before mutation; duplicate same-line occurrences remain valid frequencies.
- Tests cover lowercase whole names inside questions, qualified conjunctions, configuration keys, acronym/digit boundaries, combining marks, Unicode lowercase expansion, visually similar distinct strings, long names, original offsets, legacy reopen/live retrieval/automatic migration and rejection without deletion. Final full workspace: 186 Rust tests passed; strict workspace/all-target Clippy passed. Existing independent exhaustive split-score checks continue to pass.
- Controlled comparisons ran 276 evidence-v1 trials: identifier and split variants on 34 existing tasks plus the 12 newer tasks, each repeated three times. Every per-trial evidence score was unchanged, both between variants and between the split control and the preceding implementation. All runs had zero protocol errors and unchanged sibling sources. Response bytes changed in some trials; no byte-identical-output claim is made. These are exposed regression sets, not held-out task-success evidence. [Identifier comparison](results/native-implementation/identifiers/comparison.json), [newer suite](results/native-implementation/identifiers-fresh/comparison.json), [split control](results/native-implementation/identifiers-split/comparison.json).
- Split remains the default: targeted correctness tests demonstrate new whole-name coverage, but the external sets do not establish a relevance improvement. Separate analyzer-only/fields-only/formula ablations remain recommendation 14 work.
- Fixed 193-file warm profile: source facts grew from 3,800,722 to 6,912,082 bytes; reindex measured 479 ms versus the preceding 302 ms; exact lookup 10.5 ms and explore 12.7–14.5 ms. Both analyzer caches are currently built eagerly, so even split-only operation pays preparation/storage overhead. These single-run figures are not a peak-memory or large-index benchmark. [Profile](results/native-implementation/identifiers-repository-probe.json), [native probe](results/native-implementation/identifiers-native-probe.json), [provenance](results/native-implementation/identifiers-provenance.json). The measured source precedes the final semantic occurrence validation; that final change is covered by the full tests/lint above.
- Recommendation 13's identifier fields and normalization contract are implemented; positional phrase proof belongs to recommendation 15 and broader scale/default acceptance remains open. Recommendation 24 now has analyzer identity, with independent chunker/ranker identities still outstanding. No third-party components were added; commit/push remains pending the remaining ledger.

### Independent stored and executing representation identities

- Added a separate chunker revision to the manifest and a query-only ranker revision. Parser, graph schema, serialized source fields, analyzer/Unicode, chunker, inclusion fingerprint, publication format and result-wire identities now have distinct roles. Ranking changes do not trigger source/parser invalidation; there is no persisted query-result cache to migrate.
- Result context reports the actual indexed revisions separately from current runtime revisions. Missing historical context fields remain unknown when deserialized rather than claiming the current implementation. Current live results identify their runtime without claiming an indexed generation.
- Centralized source-representation compatibility checks across staleness and reconciliation. Found and fixed a same-hash reuse gap: a never-reconcile request now masks incompatible analyzer/Unicode/chunker facts and rebuilds bounded live evidence even when source bytes are unchanged. Exhausted live budgets cannot re-admit incompatible indexed facts.
- Migration regressions change analyzer, Unicode and chunker independently while preserving source hashes and recomputing manifest checksums. They verify visible old/current identities, live evidence without publication in never mode, automatic generation replacement, indexed evidence after migration, and subsequent no-op sync. Historical-result deserialization has a separate regression.
- Full workspace: 188 Rust tests passed; strict workspace/all-target Clippy passed after extracting the migration fixture helper; the final six source-lifecycle tests passed again after that test-only refactor. Evaluation Python suite: 25 passed (the first invocation omitted its required `PYTHONPATH=evaluation`; the corrected invocation passed). Required version metadata increased minimum viable output overhead: the old 1,000-byte success assumption now permits the documented `ResultBudget` error, while a 2,048-byte test requires actual items and verifies the exact byte/snippet caps. Required metadata is never stripped to force success.
- All 102 original-set and 36 newer-suite regression trials retained identical evidence scores, with zero protocol errors and unchanged sibling source fingerprints. [Original comparison](results/native-implementation/versions/comparison.json), [newer comparison](results/native-implementation/versions-fresh/comparison.json). These remain exposed retrieval regressions, not independent model task-success evidence.
- Native probes passed. The fixed 193-file warm profile measured 420 ms reindex and 9.3 ms exact lookup; source facts remain 6,912,082 bytes. Variation from the preceding one-run profile is not attributed to a performance optimization. [Profile](results/native-implementation/versions-repository-probe.json), [probe](results/native-implementation/versions-native-probe.json), [provenance](results/native-implementation/versions-provenance.json). Captured implementation precedes only the final test-helper refactor.
- Recommendations 13 and 24 are checked against their implemented contracts. Phrase positions, field/formula ablations, scope/occurrence modeling, structured Markdown, incremental maintenance and broader release gates remain separately open. No third-party components, commit or push were added in this increment.

### Native lexical binding facts and conservative call resolution

- Replaced per-reference parameter walking with a file-local syntax pass that records scopes, binding patterns, original call-expression spans and raw callee spelling. Scope/name indexes select visible bindings before workspace lookup; ordered binding lists use binary search for declaration position. This adds no third-party components.
- Rust and JS/TS calls now account for parameters, destructuring, closure bindings/captures, nested declarations and block extent. Rust `let`, loop, match, `if let`/`while let` and supported let-chain bindings have source-limited visibility. JS lexical declarations suppress outer lookup in their temporal dead zone; `var` binds at function/file scope, and catch/loop bindings retain their extent. Pattern keys, type names, constructors and default expressions are excluded from binding collection.
- A direct lexical declaration with a unique fact key binds explicitly. A direct immutable `const` function expression can bind syntactically, including self-reference in its body. Unknown local values, mutable callable aliases and ambiguous/missing declaration keys remain unresolved rather than falling back to an unrelated same-named function. Function/block-local declarations are excluded from out-of-scope/import/global-unique fallback. Qualified/module and type resolution remain separately bounded approximations.
- Raw extraction facts persist scope/binding ordinals, direct lexical keys and unresolved reasons. Call spelling is captured before import/receiver rewriting; other reference kinds do not claim exact original token occurrences. Parser revision 5 invalidates prior facts. This supplies foundations for recommendation 19 but does not yet add independent public occurrence records or restore every repeated edge site.
- New regression tables cover Rust, JavaScript and TypeScript binding/order patterns, Unicode/CRLF call spans, lexical targets vs imported bindings, out-of-scope nested functions, publication/reopen, and shadow insertion/removal compared with a clean rebuild. The full workspace passed 192 Rust tests with these changes; strict all-target Clippy passed after extracting a helper. The complete final gate including the header increment below passes 193 tests.
- The original native false-positive probe now returns `send` as unresolved with no target, where the preceding implementation incorrectly linked it to the unrelated function. [Probe before the header optimization](results/native-implementation/scopes-pre-header-native-probe.json).
- All 102 original-set and 36 newer-suite regression trials preserved every evidence score, with zero protocol errors and unchanged sibling sources. [Original comparison](results/native-implementation/scopes/comparison.json), [newer comparison](results/native-implementation/scopes-fresh/comparison.json). These exposed retrieval sets do not prove compiler-level resolution precision; the targeted binding fixtures establish the specific corrected contracts.
- An isolated fixed-corpus profile revealed the raw-fact cost: the 193-file manifest grew to 6,739,636 bytes, and resident exact/explore queries measured 20.3 / 23.2–24.9 ms because the freshness path repeatedly decoded it. [Pre-header profile](results/native-implementation/scopes-pre-header-repository-probe.json), [provenance](results/native-implementation/scopes-pre-header-provenance.json). This profile predates omitting unknown raw spellings on non-call facts and the header optimization. Concurrent first-run profiles are retained with a `scopes-concurrent-` prefix and are not used for latency comparisons.
- Recommendation 20 remains open for full namespace/package/receiver handling and public resolution classes. Recommendation 19 remains open for independent occurrence storage/API and aggregate-site preservation. No claim is made that all compiler semantics or dynamic assignment/dataflow are implemented.

### Generation-owned freshness headers without raw-fact reads

- Added `GraphStore::manifest_header` for freshness, status and result-context paths. Both native adapters omit extraction facts without cloning them; Grafeo caches the small header when opening a generation and replaces it only after successful publication. The full `manifest()` path remains available to reconciliation. No on-disk raw-fact split is claimed yet.
- Header state shares the graph's publication boundary and unavailable-state guard. Shared conformance preserves every header field while retaining raw facts for sync. Native tests prove header reads do not parse the raw manifest, verify publication/reopen replacement, and check header/manifest agreement throughout the existing failure matrix. Post-publication uncertainty rejects header reads too. The legacy migration fixture now compares its externally constructed graph/facts before reopening, then checks header coherence on the actual opened generation.
- Final validation ran sequentially: 193 Rust tests passed, followed by strict workspace/all-target Clippy. An earlier overlapping test/build run lost a rustdoc dependency artifact; the clean full rerun passed. A fixture-only legacy-header assertion initially assumed that bypassing publication updated an already-open header; the fixture was corrected to validate the opened-generation contract. No production cache behavior was weakened to satisfy it.
- All 102 original-set and 36 newer-suite follow-up trials preserved evidence scores, with zero protocol errors and unchanged sibling sources. [Original comparison](results/native-implementation/scope-header/comparison.json), [newer comparison](results/native-implementation/scope-header-fresh/comparison.json). Captured external source precedes only the final fixture correction and added benchmark measurements.
- Final isolated fixed 193-file profile: raw manifest 6,711,238 bytes; serialized header 45,906 bytes. Under the same executing implementation and generation, median full-manifest reads cost 8.292 ms versus 0.0109 ms for header reads. End-to-end resident exact lookup measured 3.11 ms and explore 5.65–7.43 ms; reindex was 467 ms. These are warm small-corpus measurements, not a large-repository latency or peak-memory claim. [Profile](results/native-implementation/scope-header-repository-probe.json), [native probe](results/native-implementation/scope-header-native-probe.json), [provenance](results/native-implementation/scope-header-provenance.json).
- Recommendation 23's query-header read path is implemented. Persisting raw facts separately, selective per-file loading/invalidation and avoiding full-generation rebuilding remain open. The remaining ledger, final release audit, commit and push are still pending.

### Independent reference occurrence storage foundation

- Added source-owned occurrence records alongside deduplicated adjacency. A single resolution pass emits both representations. Records retain raw-reference ordinals, optional original spans/spelling, coordinate precision, scope/binding ordinals, target and resolution class/reason. Repeated references are not collapsed in this representation; synthetic containment and cross-language matches do not invent raw source occurrences.
- Occurrence identities use explicit length-prefixed source fields and explicit numeric span encoding, excluding independently mutable target/display/reason fields. Removing a target preserves unchanged source identities and marks their bindings unresolved. File/hash/owner/range/identity validation runs before native-store mutation and on reopen; reopen also rejects missing resolved targets.
- Generation format 3 checksums the new `occurrences.json` artifact. Legacy formats remain readable; an independent occurrence representation revision forces reconciliation without invalidating body source compatibility. Both stores prepare compact lookup positions by owner, target, raw spelling and aggregate edge. They currently rebuild these indexes and scan occurrence bindings during application; this is not selective incremental maintenance.
- Added regressions for repeated same-line references, source identity under rebinding/deletion, foreign owners, duplicate facts and invalid byte ranges. Native persistence tests check nonempty occurrence state before/after failed publication, retry, reopen and corruption. Failure and subprocess-interruption matrices now include the occurrence persistence boundary.
- Full workspace passed 197 Rust tests. Strict workspace/all-target Clippy passed after replacing allocating format/appends with direct string formatting. No external evidence trials or occurrence storage-cost measurements were run in this increment, so the previous sibling-repository results are not claimed for this source revision.
- Recommendation 19 remains open: public bounded occurrence queries, aggregate-result counts/sites and end-to-end evaluation still need implementation. Resolution classes are persisted but not yet public result fields; recommendation 20 remains open too. No new third-party components were added. The remaining ledger, final release audit, commit and push are pending.

### Public aggregate occurrence counts and conditional owner correction

- Graph, traversal, impact and explore edge results now carry optional `occurrence_count`, populated from the native edge lookup after returned-edge truncation. Existing exact JSON-byte fitting includes its overhead. Missing legacy/synthetic facts omit the field; a count is not a compiler-completeness guarantee and does not inflate adjacency or impact counts.
- Added public regressions for three calls sharing one resolved edge, two unresolved calls sharing another, persistence/reopen and source replacement. The external evaluation then exposed an indexing failure on `nanus` conditional Rust functions: duplicate parser keys selected the last declaration as owner even for an earlier declaration's source span. Owner selection now verifies enclosure and selects a unique same-file/kind/qualified declaration by original coordinates; if no unique enclosing owner exists, the occurrence remains file-owned. Validation was retained. A focused cfg-alternative regression verifies separate call ownership/counts.
- The initial `occurrence-count` benchmark stopped before trials; its failure record is retained. A secondary sandbox denial of process-group cleanup obscured the initialization error. Direct subprocess reproduction isolated the real validation failure, and all three sibling hosts initialized successfully after the correction. Subsequent evaluation used approved process management outside the sandbox and temporary indexes, preserving sibling sources.
- Final workspace: 199 Rust tests passed; strict workspace/all-target Clippy passed. The 102 existing-set and 36 newer-suite evidence-v1 trials preserved every evidence score, had zero protocol errors, and verified unchanged sibling source fingerprints. [Existing comparison](results/native-implementation/occurrence-owner/comparison.json), [newer comparison](results/native-implementation/occurrence-owner-fresh/comparison.json). These are exposed regression suites, not held-out task-success evidence.
- Recommendation 19's aggregate counts are now public. Individual bounded occurrence queries, source-site delivery and their coverage/work contract remain open. Storage cost, full duplicate-declaration/containment modeling, the remaining recommendations and final commit/push remain pending.

### Bounded public occurrence queries

- Added `SearchService::occurrences` and CLI `search occurrences`, with target/owner lookup through declaration name or exact id and exact raw-spelling lookup for unresolved references. Source language/path and relationship-kind filters run before result admission. Results preserve full reference records, source hashes, original coordinate precision and explicit resolution classes/reasons. Owner lookup means directly owned references, not recursive graph traversal.
- The native occurrence indexes now supply borrowed file/record positions to query execution. An independent occurrence work budget charges entries before inspecting/filtering them (default 10,000; hard ceiling 100,000; zero permitted), observes cancellation/deadlines and reports examined entries and omissions. The result cap defaults to 50 and clamps at 500; input target size is bounded. Result bytes are checked both before and after library provenance and in the final CLI envelope. Packing drops whole records, preserving mandatory source/binding metadata for delivered evidence.
- Results distinguish files with occurrence metadata from successful extraction and carry the generation, indexed/executing revisions, freshness and source-verification status. They do not read snippets or claim that indexed coordinates describe unchecked live bytes. Legacy absence and adapter coordinate precision remain explicit; synthetic edges are not converted into fabricated occurrences.
- Public regressions cover distinct repeated UTF-8/CRLF call sites, target/owner/raw-name routes, lexical/import/unresolved resolution, unchanged-source identity after target deletion and reconciliation, stale indexed results, filtered-work accounting, cancellation, oversized queries, result caps and complete serialized-library/CLI byte limits. Full workspace: 204 Rust tests passed. Strict all-target Clippy passed after saturating counter increments and using clone assignment efficiently; the four API tests and CLI occurrence transport regression passed again after those lint fixes.
- All 102 existing-set and 36 newer-suite evidence-v1 trials retained identical evidence scores, zero protocol errors and unchanged sibling source fingerprints. [Existing comparison](results/native-implementation/occurrence-api/comparison.json), [newer comparison](results/native-implementation/occurrence-api-fresh/comparison.json). The existing-set build precedes only the three lint fixes; newer-suite provenance captures those fixes. These exposed retrieval regressions do not measure model task success or new occurrence-query relevance.
- Recommendation 19 is implemented: independent source-owned occurrence records, mutable target bindings, aggregate counts and bounded public access. Adaptive context assembly using those sites remains recommendation 12; broader binding/candidate/package semantics remain recommendations 20/21. Storage-cost profiling and the remaining ledger, final release audit, commit and push are pending.

### Context from repeated relationship occurrences

- Explore source selection now consults native occurrence positions for each returned relationship, rather than only its aggregate line. All distinct source lines can compete under the existing byte/interval limits; the previous silent four-edge-per-item cutoff is removed. An owner-to-returned-edge map avoids a repeated full-edge scan for each item. Legacy/synthetic edges retain only their existing coarse line anchor.
- Occurrence records are charged to the independent work budget before inspection, and their file hashes must match the already captured source. No additional source reads are introduced. Multiple calls on the same line share one candidate interval, and global source-line deduplication/exact byte admission remain in force. Context-disabled requests and requests with no returned edges perform no occurrence work.
- Final query statistics now include context occurrence examination and context-selection elapsed time. Work truncations are deduplicated when final accounting runs after selection, and the complete result is fitted again with those counters/notices present. Mandatory metadata is not stripped to accommodate extra source.
- A long-function regression places three calls to one target on widely separated lines. It verifies one aggregate edge with count three, all three delivered sites when budgets permit, unique delivered lines, a one-entry work cap with explicit omission, and zero occurrence work when context or returned edges are disabled.
- Full workspace: 205 Rust tests passed; strict workspace/all-target Clippy passed. Existing-set 102 and newer-suite 36 evidence-v1 trials retained every evidence score with zero protocol errors and unchanged sibling sources. Response sizes changed in 60 and 18 trials respectively; outputs are not byte-identical. [Existing comparison](results/native-implementation/occurrence-context/comparison.json), [newer comparison](results/native-implementation/occurrence-context-fresh/comparison.json). The targeted regression proves repeated-site coverage; the exposed external sets establish regression behavior, not answer success.
- Recommendation 12's repeated relationship-site assembly is implemented. Proximity-aware scoring and broader complete-region/citation/task-success acceptance remain tied to recommendations 11/15/30. Remaining ledger work and final commit/push are pending.

### Occurrence storage and successful-lookup profile

- Extended the native fixed-corpus probe to report occurrence artifact bytes, record count and resident occurrence lookup work/bytes. The initial miss-only query examples are retained in `occurrence-context-profile`; the follow-up selects the three most frequent indexed spellings deterministically, without using task labels, and is the successful-lookup measurement.
- On the frozen 193-file archive, 7,078 occurrence records use 3,617,503 bytes of JSON. Existing source units use 6,912,082 bytes, raw manifest 6,711,265 and graph 839,807. The occurrence artifact adds about 25% relative to those other three artifacts combined. This measures serialized composition, not resident/peak memory or a compression recommendation by itself.
- The successful-lookup run returned 50 records from each of the three common-name queries (`Vec`, `Some`, `Option`), examining exactly 51 records and reporting the result cap. Resident median lookup times were 3.24–3.31 ms, with 30.7–32.4 KB serialized results. Exact symbol lookup measured 3.00 ms, explore 5.56–7.74 ms and reindex 506 ms. These are warm small-corpus measurements; they do not establish large-repository throughput or a statistically significant comparison with earlier runs.
- [Successful-lookup profile](results/native-implementation/occurrence-context-profile-hits/repository-probe.json), [native checks](results/native-implementation/occurrence-context-profile-hits/native-probe.json), [provenance](results/native-implementation/occurrence-context-profile-hits/provenance.json). Raw facts, source units and occurrence JSON remain uncompressed and partly redundant; measured composition informs the still-open storage/incremental recommendations 23/27/28.

### Native Markdown heading and fence boundaries (release acceptance open)

- Added a native linear block-boundary scanner for top-level ATX headings and backtick/tilde fences. It preserves original UTF-8/CRLF offsets, ignores headings inside fences, requires compatible sufficiently long closers, rejects backticks in backtick info strings and keeps unmatched fences open through EOF. Source regions no longer cross these supported boundaries. Fenced blocks/fragments have an explicit `markdown_code_fence` evidence kind; long blocks retain bounded overlapping windows without claiming complete-block delivery.
- Chunker revision 2 independently invalidates old source partitions. Small fenced examples are kept intact as retrieval regions so the existing context packer can deliver opening/closing delimiters within its byte budget. Markdown/MDX remains an explicitly limited scanner: setext headings, frontmatter, nested list/blockquote containers, tables, breadcrumb context and MDX expression semantics are not yet implemented. Recommendations 9/25 remain open.
- Native tests cover fence width/type/indentation, embedded headings, invalid info strings, unmatched fences and exact reconstruction of original bytes. Public tests verify persisted/reopened fenced evidence, both delimiters in a small returned example, Unicode/CRLF spans and bounded fragments of a 202-line fence. Final full workspace: 209 Rust tests passed. Strict all-target Clippy passed after factoring region classification into a helper; the subsequent full test run used that final code.
- External evaluation found an unresolved release regression. All 36 newer-suite trials and 99 of 102 existing-suite trials preserve evidence scores; all 138 have zero protocol errors and unchanged sibling source fingerprints. In all three repeats of `nanus.grep.change`, one required region decreases from 20/68 to 11/68 lines while file recall and the other region remain unchanged. [Existing comparison](results/native-implementation/markdown-boundaries/comparison.json), [newer comparison](results/native-implementation/markdown-boundaries-fresh/comparison.json).
- Transcript comparison shows a ranking/context-allocation effect: `grep_outcome` moves from fourth to sixth among the same eight candidates; the three subsequent read requests are unchanged. [Candidate-order diagnostic](results/native-implementation/markdown-boundaries/ranking-diagnostic.json). The structural partition change alters body collection statistics and ordering; its exact scoring contribution has not yet been isolated. No task-specific weight adjustment was made. This regression must be resolved or explicitly gated before final release; passing syntax/integrity tests alone does not establish retrieval improvement.
- Broader Markdown structure, ranking/context regression investigation, remaining recommendations and final commit/push are pending.

### Isolated Markdown scoring regression diagnosis

- Added the dependency-neutral `body_partition_probe` research binary. It builds the current native source facts in memory, reconstructs only Markdown's previous 80-line windows using the same native analyzer, and compares the two representations. Its independent exhaustive scorer verifies every native result identity/order and score for both complete body indexes before reporting controlled combinations of IDF and average-length statistics. No production ranking or task-specific weight was changed.
- On the exposed `nanus.grep.change` query, structured Markdown changes nonempty region count from 10,143 to 10,392 and average region length from 44.9364 to 43.2726 (−3.70%). The implementation region's score moves from 21.3955 at rank 4 to 20.9794 at rank 6; the competing test moves from 21.2817 at rank 6 to 21.1540 at rank 4. The code regions themselves are unchanged.
- Holding only old IDF leaves the implementation at rank 6. Holding only old average length raises it to rank 5 but leaves it just below the competing test. Holding both old statistics restores its original score and rank 4. This isolates a joint collection-statistic effect rather than a lost Markdown/code span. The existing reciprocal-rank context value then amplifies a small score change into a larger source-allocation difference.
- [Full ablation](results/native-implementation/markdown-statistics/ablation.json), [diagnosis](results/native-implementation/markdown-statistics/diagnosis.json), [binary/production/source provenance](results/native-implementation/markdown-statistics/provenance.json). The sibling source fingerprint is unchanged. The probe compiled and its exhaustive/native checks passed. This single exposed query diagnoses the failure; it does not validate a replacement scorer or justify freezing corpus statistics.
- The release regression remains open. A general scoring/context-selection improvement needs controlled acceptance across query families under recommendations 11/12/14; it must preserve the new Markdown source-integrity behavior rather than tuning one known task. Remaining recommendations and commit/push are still pending.

### Damped context rank prior

- Replaced the context allocator's `1 / rank` multiplier with `1 / (60 + rank)`, reusing the existing retrieval fusion offset and fixed-point integer arithmetic. Source-role values, estimated byte costs, exact byte admission, candidate ranking and source integrity remain unchanged. Moving from rank 4 to 6 now changes this prior by about 3%, instead of 33%; this is a priority heuristic, not a probability or calibrated relevance score.
- Runtime ranker revision 2 identifies the query/context change without invalidating stored source facts. The full workspace passed 209 Rust tests and strict all-target Clippy passed. No dependencies or parser/source-partition changes were introduced in this increment.
- Controlled evidence-v1 comparisons against the Markdown-boundary implementation improve three existing-set tasks in all three repeats, with no new per-task evidence regression: `nanus.glob.debug` rises from 9/75 to 16/75 lines, `nanus.glob.change` from 12/75 to 31/75, and `nanus.grep.debug`'s second required region rises from 5/8 to 8/8, making both its required regions complete. The other existing tasks and all 36 newer-suite trials retain evidence scores. All 138 trials have zero protocol errors and unchanged sibling source fingerprints. [Existing comparison](results/native-implementation/damped-context/comparison.json), [newer comparison](results/native-implementation/damped-context-fresh/comparison.json).
- This generally beneficial change is retained, but it does **not** recover the earlier `nanus.grep.change` loss: that region remains 11/68 instead of the pre-Markdown 20/68. The release gate therefore remains open. Interval allocation still estimates whole-window costs before deduplicating already-delivered lines; true marginal context allocation and broader ranking/statistic ablations remain further work. These exposed retrieval outcomes do not establish model task success.
- Remaining recommendations, final acceptance and commit/push are pending.

### Marginal context byte costs and bounded recomputation

- Context allocation now estimates only undelivered lines, plus overhead for each remaining contiguous gap, and recomputes those costs after successful admission. Fully covered candidates leave the queue. The damped rank prior and role priorities remain fixed; this is marginal byte cost, not marginal semantic utility or an optimal packing claim. Exact serialized-byte admission remains authoritative.
- Added an independent context-window evaluation budget: 10,000 by default, clamped to 100,000, with zero allowed. Each evaluation checks cancellation/deadline before examining a bounded window of at most 80 lines. Exhaustion preserves primary evidence and reports `context_windows`; the final `context_windows_examined` counter includes repeated evaluations. Runtime ranker revision 3 identifies this change without rebuilding stored facts.
- A focused allocator test proves that completing a mostly delivered window can outrank a smaller wholly new window at equal value, and that completely delivered candidates disappear. A public integration test verifies zero-budget omission with primary evidence intact and complete short-function delivery under the default budget. Full workspace: 211 Rust tests passed; strict workspace/all-target Clippy and formatting passed. The lint-only follow-up makes the default work-limit type explicit in the test.
- All 102 existing-set and 36 newer-suite trials preserve evidence scores, with zero protocol errors and unchanged sibling source fingerprints. [Existing comparison](results/native-implementation/marginal-context/comparison.json), [newer comparison](results/native-implementation/marginal-context-fresh/comparison.json). These exposed suites establish regression behavior, not model task success. The earlier Markdown-induced `nanus.grep.change` loss remains 11/68 versus 20/68 and is still a release blocker.
- The frozen 193-file native profile completes successfully. Its three explore queries examine 64, 94 and 79 context windows, with resident medians of 5.41, 5.93 and 7.48 ms respectively. These are warm small-corpus observations, not an isolated statistically significant comparison or a worst-case scaling proof. [Profile](results/native-implementation/marginal-context-profile/repository-probe.json), [native probes](results/native-implementation/marginal-context-profile/native-probe.json), [provenance](results/native-implementation/marginal-context-profile/provenance.json).
- Candidate generation still uses primary/body/declaration/relationship windows; selecting around all matching positions, multiple regions per owner and broader relevance acceptance remain open. Remaining recommendations and final commit/push are pending.

### Adaptive fallback around retained body matches

- Body retrieval now retains all matching line positions in the selected region internally, alongside its primary densest-line anchor. Both indexed and live-overlay paths use the already computed positions; no additional source read, source analysis or persisted representation is introduced. Runtime ranker revision 4 identifies the context-policy change. Exact-navigation routes keep their existing behavior.
- Structural/body/relationship windows retain priority. Remaining space first admits five-line windows centered on other matches, clipped to the same verified region; a final tier tries the matching lines alone when their neighbors cannot fit. All tiers use the existing marginal byte costs, exact serialized admission, global source-version line deduplication and shared context work/interval/byte budgets. Single-line excerpts remain labeled partial evidence, not complete implementations.
- The new public regression constructs a function whose full region cannot fit in 8 KB, with three distant matches. All three matching lines are delivered for indexed and subsequently edited live source, with exact Unicode/CRLF line coordinates, verified hashes, bounded payload and no repeated lines. Full workspace: 212 Rust tests passed; strict workspace/all-target Clippy, formatting and diff whitespace checks passed.
- Three intermediate policies were rejected and their external results retained. Giving tiny windows equal priority to structural windows improved four tasks but regressed three, including loss of a complete required region ([comparison](results/native-implementation/match-context/comparison.json)). Deferring five-line windows preserved the external baseline and improved two tasks, but failed the new tight-budget test ([comparison](results/native-implementation/match-fallback/comparison.json)). Making structural windows all-or-nothing fixed that test but discarded useful partial source in two tasks ([comparison](results/native-implementation/match-atomic/comparison.json)). The final adaptive policy preserves structural packing and shrinks only fallback match windows. These are exposed development comparisons, not independent quality estimates.
- Final existing-set results improve `nanus.edit.change` from 4/65 to 10/65 lines and `nanus.context.debug`'s second region from 12/53 to 13/53, each in all three repeats; the other 96 trials retain evidence scores. All 36 newer-suite trials retain scores. All 138 trials have zero protocol errors and unchanged sibling source fingerprints. [Existing comparison](results/native-implementation/match-adaptive/comparison.json), [newer comparison](results/native-implementation/match-adaptive-fresh/comparison.json). Complete-region/task-success counts do not improve in these sets, and the earlier `nanus.grep.change` loss remains 11/68 versus 20/68 before Markdown partitioning. Final release acceptance remains open.
- Of the existing-set first search responses, 99 retain complete work metadata: the maximum is 475 window evaluations, with no observed context-work truncation. Three repeats of `whatsurvey.contact-policy.change` hit the evaluation harness's separate 16,384-byte response cap and lose their trailing metadata; their work counters are explicitly unknown, not zero. [Work observations](results/native-implementation/match-adaptive/context-work.json).
- The frozen 193-file native probe completes. Three explore queries examine 88, 160 and 105 windows, with resident median times 5.41, 5.85 and 7.40 ms. This measures warm small-corpus behavior, not a statistically significant latency improvement or large-scale bound. [Profile](results/native-implementation/match-adaptive-profile/repository-probe.json), [native probes](results/native-implementation/match-adaptive-profile/native-probe.json), [provenance](results/native-implementation/match-adaptive-profile/provenance.json).
- Recommendation 12 now carries matching positions into adaptive context selection. Distinct-term/proximity utility, merging adjacent excerpt fragments, multiple regions per owner and broader citation/task-success acceptance remain open. Remaining numbered recommendations and final commit/push are pending.

### Merge compatible adjacent source excerpts

- Added native adjacency merging during optional-source admission. Only excerpts in the same result item with the same role and source hash can merge; the joined interval must remain at most 80 lines. Primary snippets, gaps, distinct roles and distinct source versions remain separate. Existing global line deduplication already prevents overlap; joining neighbors now removes repeated excerpt metadata and frees interval slots.
- Admission accounts for the actual serialized size and retained interval count after merging, including negative byte deltas from removed metadata. A failed admission restores the prior excerpt vector. Even when the 64-interval cap is reached, a compatible extension can fit without consuming another slot. A conservative raw-byte check avoids materializing source that cannot fit even after removing all existing item metadata. Runtime ranker revision 5 identifies the policy change; storage/parser/source representations and dependencies are unchanged.
- The unit regression joins a bridge between out-of-order neighbors, verifies exact text and reduced serialization, and rejects role/hash/gap/over-80-line merges while accepting the exact 80-line boundary. A public regression delivers all 90 nearby call sites from one aggregate relationship, with fewer than 64 excerpts, no duplicate or altered lines, bounded individual intervals, and no default context-work exhaustion.
- Final full workspace: 214 Rust tests passed; strict workspace/all-target Clippy, formatting and diff whitespace checks passed. The final lint follow-up makes already bounded coordinate/loop additions explicitly saturating; the final workspace run includes that change. External/native profiles preceded that arithmetic-only cleanup and exercise the same bounded behavior.
- Existing-set coverage improves `nanus.read.debug`'s second region from 17/39 to 22/39 lines and `nanus.edit.change` from 10/65 to 12/65, each in all three repeats. The other 96 existing trials and all 36 newer-suite trials preserve evidence scores. All 138 trials have zero protocol errors and unchanged sibling source fingerprints. [Existing comparison](results/native-implementation/join-context/comparison.json), [newer comparison](results/native-implementation/join-context-fresh/comparison.json). Complete-region/task-success counts do not improve; the previously tracked Markdown-related loss remains unresolved.
- The frozen 193-file native profile completes, examining 88, 160 and 105 context windows in its three explore queries; resident medians are 5.58, 6.41 and 7.59 ms. These warm small-corpus observations do not establish a speedup or worst-case scale. [Profile](results/native-implementation/join-context-profile/repository-probe.json), [native probes](results/native-implementation/join-context-profile/native-probe.json), [provenance](results/native-implementation/join-context-profile/provenance.json).
- Recommendation 12 now includes compatible adjacent-interval merging. Distinct-term/proximity utility, multiple regions per owner, broader acceptance, remaining recommendations and final commit/push are still pending.

### Query-term membership and rejected context-weight experiments

- Native body matching now preserves a query-local 128-bit term-membership mask per matching line, replacing per-term cloned position sets and the final count-only representation. Repeated occurrences and whole/split aliases for the same query term set the same bit. Population count preserves the densest-line primary anchor; the retained fallback allocator uses the matching line positions with its previously accepted priorities. The masks are internal, not persisted or serialized, and do not encode within-line order or prove phrase matches. Runtime ranker revision 6 identifies this query representation change; source/parser/storage versions and dependencies are unchanged.
- Two scoring policies were tested and rejected. Multiplying fallback value by total distinct terms regressed `nanus.read.debug` from 22/39 to 17/39 required lines and `nanus.edit.change` from 12/65 to 10/65, in all three repeats, with no improvements ([comparison](results/native-implementation/term-context/comparison.json)). Recomputing a bonus only for query terms not yet delivered from the selected region retained the edit result but still regressed read coverage from 22/39 to 17/39, again with no improvements ([comparison](results/native-implementation/novel-context/comparison.json)). Both experiments have zero protocol errors and unchanged sibling sources. The exposed evidence does not justify shipping either bonus, and neither remains in production. These trials do not establish that every possible coverage/proximity model fails.
- Regression tests verify term identities across repeated spelling/case aliases, split versus whole-identifier analysis, CRLF line positions, all 128 term bits and explicit rejection beyond the mask bound. Final full workspace: 216 Rust tests passed; strict workspace/all-target Clippy, formatting and diff whitespace checks passed.
- After removing both bonuses, all 102 existing-set and 36 newer-suite trials retain the accepted adjacent-merge allocator's evidence scores, with zero protocol errors and unchanged sibling source fingerprints. [Existing comparison](results/native-implementation/term-masks/comparison.json), [newer comparison](results/native-implementation/term-masks-fresh/comparison.json). No quality or speed improvement is claimed for the retained representation alone.
- Term membership is now available for bounded positional/candidate analysis, but a beneficial term-coverage/proximity scoring policy remains unproven. The Markdown-related release regression, multiple regions per owner and broader acceptance are still open. Next work returns to unfinished execution-budget propagation and typed/positional query planning, rather than further weight tuning on these exposed tasks. Remaining numbered recommendations and final commit/push are pending.

### Cooperative cancellation for source walking and literal scans

- Fixed a public-service gap: `files` and `text` previously ignored the cancellation token and deadline supplied through `with_work_limits`. They now pass a request work budget into new native `search_files_with_work` / `search_text_with_work` entry points; existing core entry points remain compatible wrappers with default controls. Neither walk-served query requires or builds an index.
- Added a checked walker shared by file/text search and explore's body-candidate enumeration. It checks before/after iterator advancement and around final sorting; file matching checks each entry. Text search checks each source line and reads in at most 8 KiB chunks, checking between reads while preserving the existing per-file byte ceiling and one-byte oversize detection. Interrupted reads retry; ordinary read failures still contribute coverage loss; cancellation/deadline errors are not converted into successful partial evidence or read-error omissions.
- Deterministic tests cancel during enumeration and after the first read chunk, proving that the next read is not initiated and no partial success is returned. Public tests cover pre-cancelled/expired file and text queries on an unindexed root, normal uncancelled results, and no accidental indexing. A separate reader test verifies retry after interrupted I/O and exact adherence to its byte allowance. Final full workspace: 220 Rust tests passed; strict workspace/all-target Clippy, formatting and diff whitespace checks passed.
- All 102 existing-set and 36 newer-suite evidence-v1 trials preserve scores, with zero protocol errors and unchanged sibling source fingerprints. [Existing comparison](results/native-implementation/scan-cancellation/comparison.json), [newer comparison](results/native-implementation/scan-cancellation-fresh/comparison.json). The first comparison preceded an equivalent retry-branch lint cleanup; the newer comparison and final workspace tests use the final behavior.
- Native probes complete successfully. Dense literal samples at 60/120/240/480 KB measure 0.90/1.11/1.50/2.27 ms; the frozen 193-file explore medians are 5.61/5.88/7.44 ms. These are warm small-corpus observations, not a cancellation-latency guarantee or statistically significant performance comparison. [Native probes](results/native-implementation/scan-cancellation-profile/native-probe.json), [repository profile](results/native-implementation/scan-cancellation-profile/repository-probe.json), [provenance](results/native-implementation/scan-cancellation-profile/provenance.json).
- Recommendation 5 remains open for aggregate source-read/file-work accounting and broader freshness/reconciliation/indexing integration. Numeric graph budgets do not become source quotas in this increment. One iterator advance may do internal directory/ignore work, and in-flight filesystem calls, sorting, line transformations and matching are not preempted. The existing policy/result caps remain independent. No dependency or stored representation changes were introduced.
- Remaining recommendations, the Markdown-related acceptance regression and final commit/push are pending. Next: aggregate source-read accounting and the remaining query-work boundaries.

### Aggregate source work budgets (implementation under evaluation)

- Added request source-file attempts and consumed-byte quotas to literal scans and explore's shared live-source cache: defaults 10,000 attempts / 64 MiB; ceilings 200,000 / 1 GiB; zero permitted. Include filters precede literal admission. Open failures consume attempts; invalid/binary inputs and failed-read prefixes consume actual bytes. Cache hits consume no additional allowance. Freshness/reconciliation/index-maintenance reads remain outside this scope; recommendation 5 remains open.
- One aggregate overflow probe byte is permitted and reported in actual bytes consumed. Incomplete source files contribute neither text nor observed hashes. Completed earlier evidence survives exhaustion. Literal scans stop immediately after detecting overflow, avoiding an unnecessary subsequent admission attempt. Explore uses the same read budget across live body retrieval and snippet assembly, and returns explicit budget-exceeded source identities.
- Added four regressions covering zero/exact/overflow budgets, filtered admission, earlier successful files, invalid inputs, failed-read byte charging, shared-cache reuse and metadata-only operation. All 224 workspace tests passed; final seven public work tests passed after the final cancellation checkpoint and test-only lint annotations. Strict all-target Clippy, formatting and diff checks passed.
- Existing-set evaluation: 99/102 trials unchanged; all three repeats of `nanus.glob.change` lose r1 coverage from 31/75 to 28/75. Newer suite: all 36 trials unchanged. Both have zero protocol errors and unchanged sibling source fingerprints. These are exposed-label regression suites, not independent held-out task-success measurements. [Existing comparison](results/native-implementation/source-work/comparison.json), [newer comparison](results/native-implementation/source-work-fresh/comparison.json).
- The loss is not source-quota exhaustion: the query reads seven files / 529,606 bytes. Follow-up actions, candidate/posting/traversal counts and context-window evaluations are unchanged. The new result has a final serialized-byte truncation absent from the baseline. The allocator reserves the snippet omission notice but does not yet reserve all late work-statistic growth. The extra source counters expose this packing boundary. [Diagnostic](results/native-implementation/source-work/regression-diagnosis.json). This increment is not yet accepted for release; resolve or explicitly evaluate the packing regression before claiming completion. Native performance profiling remains pending.

### Reserve final context statistics before allocating optional evidence

- Fixed a late-packing boundary revealed by source-work counters. Occurrence discovery now updates its counter and attaches already-fired work notices before optional evidence allocation. Allocation reserves the maximum serialized integer widths of the still-changing elapsed-time and context-window counters. Thus their later growth cannot by itself evict an admitted interval. Newly fired work-limit notices still pass through final whole-result fitting; this is not a claim that every possible late notice is pre-reserved.
- Added a byte-boundary sweep using multibyte/quoted source. It finalizes counters and sets elapsed time to the maximum representable integer, checks complete serialized results against each cap, preserves primary source, and verifies optional source can still be admitted. The full workspace passed 225 tests. Final evidence tests, strict all-target Clippy, formatting and diff checks passed after attaching known notices, advancing runtime ranker revision to 7 and replacing a redundant Copy clone. Stored source/parser identities are unchanged.
- The original 102-trial comparison restores `nanus.glob.change` to 31/75 from the source-work experiment's 28/75. Against the pre-source-work `scan-cancellation` baseline, 93 trials are unchanged; three repeats each change `nanus.read.debug` r2 from 22/39 to 17/39, `nanus.edit.change` r1 from 12/65 to 10/65, and `nanus.grep.change` r1 from 11/68 to 21/68. These are context-selection changes under the same byte cap, not candidate recall or task-success gains. The improved grep coverage exceeds the old pre-Markdown 20/68 on this exposed query, but does not establish general Markdown relevance. [Comparison](results/native-implementation/context-stat-reserve/comparison.json).
- All 36 newer-suite trials preserve evidence scores. Both suites have zero protocol errors and unchanged sibling source fingerprints. The original comparison preceded the known-notice attachment and runtime-version increment; the newer suite uses those final semantics. The remaining lint cleanup only replaces clone with Copy. [Newer comparison](results/native-implementation/context-stat-reserve-fresh/comparison.json).
- Native probes complete successfully with source accounting enabled. Frozen 193-file warm explore observations are 6.18/6.44/9.37 ms. These small-corpus observations do not establish a speedup or tail-latency guarantee; tests were also active during this profile. The profile preceded the final known-notice/version-only changes. [Profile](results/native-implementation/context-stat-profile/repository-probe.json), [probes](results/native-implementation/context-stat-profile/native-probe.json), [provenance](results/native-implementation/context-stat-profile/provenance.json).
- Source-work and context-statistics correctness are now covered, but relevance acceptance remains open for the read/edit tradeoffs and the broader candidate/context design. Recommendation 5 still requires the remaining request-budget/payload boundaries. The other unchecked recommendations and final commit/push remain pending.

### Bound file/text collection, library payloads and CLI envelopes

- File/text scans now cap accumulated serialized hit bytes during collection and fit the complete compact JSON library result to 65,536 bytes. This bounds hit accumulation even when a caller constructs an oversized numeric limit directly. Execution also clamps count ceilings independently of builder methods; directly supplied zero retains its existing zero-result semantics. Final fitting removes a suffix, preserves retained hit order/source identity, and fails with `ResultBudget` when mandatory metadata cannot fit.
- Native suffix fitting counts a source identity only when its final remaining hit is removed. This avoids discarding every hit when provenance accounts for most excess bytes. The CLI reuses this accounting and separately fits the full compact JSON envelope, including query echoes. File/text JSON is now compact; its schema is unchanged. `stats.matches` counts collected text hits before final packing, not all corpus matches or only the delivered subset.
- Pattern/include/search-path fields reject more than 8,192 UTF-8 bytes before relevant compilation or filesystem access, without echoing oversized values into errors. A shared path check also protects callers of root resolution. Existing glob/literal and source-filter semantics are preserved. The 400-byte match-line prefix now has an explicit `match_line` notice; truncation notices are printed accurately instead of describing every source/byte omission as a match-count cap.
- New tests cover large file lists and dense text with Unicode/escaping, library and complete CLI byte ceilings, source hashes, raw oversized and zero count limits, exact/overflow Unicode query sizes, shared source identities, stable suffix removal, idempotence and required-metadata failure. An initial fixture incorrectly expected opening an index to create no directory; it now checks the live scans have no generation. The full workspace passed 229 tests before the final count-normalization/validation-order edits. Final four public payload tests and 13 CLI contract tests passed, including one subsequently added raw-count-ceiling regression (230 distinct workspace tests now). Strict workspace/all-target Clippy passed after formatting and exact-integer test cleanup; the added test passed its own lint check after expressing the same range inclusively. Formatting and diff checks passed.
- Native probes completed successfully. Dense literal samples at 60/120/240/480 KB measured 0.42/0.54/0.97/1.75 ms. Byte-bounded collection can stop earlier, so these are not equivalent-throughput speedup claims. The probe precedes the final raw-limit normalization and validation-order changes; ordinary builder-generated limits are already normalized. [Probes](results/native-implementation/scan-payload-profile/native-probe.json), [profile](results/native-implementation/scan-payload-profile/repository-probe.json), [provenance](results/native-implementation/scan-payload-profile/provenance.json).
- No explore-ranking changes were made, so the exposed ranking suites were not rerun for this scan/transport increment. Prior context-selection acceptance tradeoffs remain open. Recommendation 5 still requires the remaining request-walk/freshness/indexing work boundaries and broader metadata/report payload audit. All other unchecked requirements and final commit/push remain pending. No dependencies were added.

### Shared query directory-entry allowance

- Added `WorkLimits.walk_entries`, defaulting and clamping to the existing 1,000,000-entry ceiling, with zero permitted. File/text scans and explore's body-candidate walk apply the smaller of the remaining request allowance and the independent policy cap. Successive core walks using one budget consume the shared allowance; service calls start fresh budgets. Indexing still uses its original policy-controlled walker, and request limits do not alter the policy fingerprint.
- Accounting covers processed visible entries, including the root, directories and yielded errors. Ignore/exclusion internals are not counted; one unprocessed lookahead entry per walk distinguishes exact completion from overflow. The existing cooperative cancellation checks remain. The result reports `walk_entries` and incomplete enumeration; `require_complete` rejects the partial report. A stricter policy limit does not falsely consume the rest of the request allowance or report request exhaustion. Text coverage deduplicates the shared walk notice.
- Added public zero/exact/overflow/reset tests for files and text, a zero-budget explore coverage check, and successive-core-walk/policy-precedence tests. All 232 workspace tests passed. Final nine public work tests passed after strengthening policy precedence and replacing the equal numeric default/ceiling with the existing shared constant. Strict workspace/all-target Clippy, formatting and diff checks passed.
- Default enumeration behavior and output fields remain unchanged. No ranking or representation changes were introduced, and external relevance/performance runs were not repeated for this accounting change. The tests establish entry-budget behavior, not a hard filesystem latency bound.
- Recommendation 5 remains open for shared freshness/reconciliation/index-maintenance accounting and the remaining payload/report audit. Other unchecked recommendations, context-selection acceptance work, and final commit/push remain pending.

### Core freshness verification under a caller-owned work budget

- Added `stale::inspect_with_work`, sharing the caller's entry and source-read allowances. It requires complete enumeration before comparing manifest paths, checks cancellation during metadata comparison, and uses the chunked/accounted native reader for content fingerprints. Insufficient source allowance produces `IncompleteVerification`; it cannot return a partial check that a caller could mistake for current source. Ordinary read failures still mark the corresponding source changed, matching the existing verifier's conservative behavior.
- Added exact entry/file/byte boundary tests and independent exhausted-walk/file/byte cases. Metadata-only verification consumes no source reads, while content verification detects changed bytes after restoring the original size and mtime. All 81 core tests passed (79 unit plus two property tests); workspace compilation and strict all-target Clippy passed. Formatting and diff checks passed. No dependencies or representation changes.
- This is a core lifecycle building block, not completed service integration. Existing unbudgeted verification entry points preserve their behavior, and `SearchService` does not yet call the new path. Next: carry one request budget through service freshness, reconciliation and retrieval without resetting spent work or declaring partial verification fresh. Recommendation 5 and the broader implementation/acceptance/commit/push goal remain open.

### Carry freshness work into service retrieval

- Graph, occurrence, impact and explore service queries now allocate one work budget, use `inspect_with_work` for pre-reconciliation freshness and final generation context, and pass the consumed budget into retrieval. `QueryEngine::with_work_budget` preserves prior counters/remaining allowances for its first query, then resets normally if the engine is reused for another query. The existing normal engine constructors retain their reset behavior.
- Both freshness observations consume work. In strict content mode, verifying a one-file workspace twice costs two source attempts and twice its bytes; an explore snippet needs additional allowance unless already captured in its retrieval cache. This reports the actual repeated reads rather than resetting the counters. Incomplete freshness now fails the service query before retrieval instead of returning partially verified provenance. The prior zero-walk explore test was updated to this stronger boundary; file/text scans still return honest partial enumeration.
- Public tests cover exact combined limits, exhaustion during the second verification pass, retained symbol metadata with withheld snippets, successful snippets with sufficient remaining allowance, reset between service calls, and one-time inheritance on a reused core engine. All 236 workspace tests passed. Strict workspace/all-target Clippy, formatting and diff checks passed.
- All 36 newer-suite sibling-repository evidence trials are unchanged against `context-stat-reserve-fresh`, with zero protocol errors and unchanged source fingerprints. This is exposed-label regression evidence, not model task success or a new held-out result. [Comparison](results/native-implementation/shared-freshness-fresh/comparison.json), [provenance](results/native-implementation/shared-freshness-fresh/provenance.json). No scoring or stored representation changes were introduced.
- Automatic sync/reindex work, cold index construction, index opening and status reporting still sit outside this allowance. In particular, a successful automatic publication can precede a later verification-budget failure; this increment does not claim a whole-request index-maintenance bound. Next: pass the same budget into automatic maintenance and enforce it before publication. Recommendation 5 and all remaining implementation/acceptance/commit/push work remain open.

### Make reconciliation classification reads explicit

- Added a fallible hash-reader seam for change classification, preparing automatic maintenance to share request source allowances. The compatibility entry point remains available. A failed read now stays conservatively modified or added; it cannot masquerade as the SHA-256 of an empty file and create a false unchanged/rename decision.
- Added files are no longer read for rename detection when there are no unmatched removed files. Pending modified entries retain their walked references, removing the repeated linear search through all entries. Rename pairing remains deterministic and content-based.
- Regression tests cover zero reads when no rename is possible, unreadable previously empty content, and propagation of a cancelled rename hash read. All 84 core tests passed (82 unit and two property tests). No relevance scoring, stored representation or dependencies changed. This seam does not yet bound automatic maintenance: wiring its reads, extraction and walks into the shared budget, and checking before publication, remains next. The broader goal and final commit/push remain pending.

### Share request allowances with automatic index maintenance

- Automatic cold builds, metadata sync and strict-content rebuilds now pass the existing request `WorkBudget` through the projector. Walks consume the remaining entry allowance; classification hashes and extraction consume the remaining source attempts/bytes through the chunked reader. Repeated reads are charged independently. Explicit standalone index/sync operations retain their policy limits.
- Added `IncompleteMaintenance` for exhausted source allowances. Incomplete enumeration, read failure, source growth beyond policy, cancellation or deadline failure abort preparation. Checkpoints run between projection phases/files and immediately before publication or a metadata-only manifest commit. Parser completion is checked before using its result. A failed preparation never quarantines a budget failure or publishes a partial generation.
- Public regressions exercise cold builds, incremental hash-plus-extraction reads, strict verification followed by rebuild, exact shared allowances, and exhausted walks/files/bytes. Failed maintenance leaves the durable `CURRENT` bytes unchanged (or absent for a cold build). A core fixture cancels from inside extraction and verifies that both previous graph nodes and manifest remain intact.
- These are cooperative source/enumeration bounds, not preemptive parser, snapshot, memory or storage-publication bounds. Completed publication is not rolled back if a later freshness check or retrieval step fails. Status and remaining report/payload/phase limits still require audit. Recommendation 5 and the broader implementation/acceptance/commit/push goal remain open. No dependencies or stored representation revisions were added.
- Validation: all 243 workspace tests passed; the final parser-cancellation regression passed again after adding the immediate post-parser checkpoint. Strict workspace/all-target Clippy, formatting and diff checks passed. No ranking/scoring policy changed, so external evidence-ranking trials were not repeated for this maintenance-accounting increment.

### Bound status verification and cache exact graph counts

- Status now checks request cancellation/deadlines, uses the shared bounded freshness verifier, and fails explicitly on incomplete enumeration or content verification. It remains read-only and never triggers reconciliation. Its graph counts come from exact generation-owned summaries rather than cloning every node and edge on each call. Both adapters prepare summaries alongside their existing native indexes; the persistent adapter rebuilds them on reopen. Reading a summary does not consume graph-traversal quotas.
- The library status payload and compact CLI JSON envelope now fit the 64 KiB ceiling. Overflow omits a deterministic suffix of changed-path details and reports a `bytes` coverage notice. Observed changed totals, graph counts, policy and generation remain intact. Mandatory metadata overflow is an explicit `ResultBudget` error. Text status displays the truncation notice.
- Added public status tests for cancellation before index existence, expired deadlines, exact and insufficient walk/source limits, reset between calls and exact counts under zero graph-traversal quotas. Adapter conformance checks verify count changes after replacement and persistence after reopening; unresolved edges are included. A 500-file long-path fixture verifies library and CLI byte ceilings, exact totals and deterministic retained paths. Mandatory metadata retention has a core regression.
- No scoring or persisted representation changes, dependencies or held-out quality claims. The status work does not preempt index opening, parser calls, storage publication, snapshot allocation or coverage aggregation. Remaining report/input/phase boundaries and all other unchecked recommendations stay open; final commit/push is still pending.
- Validation: all 246 workspace tests passed. After the final saturating-arithmetic lint adjustments, all 19 adapter and CLI contract tests passed again. Strict workspace/all-target Clippy, formatting and diff checks passed. External ranking trials were not repeated because retrieval ranking and source evidence selection were unchanged.

### Bound sync reports before publication

- Added backward-readable optional `SyncCounts` totals, independent of the report's detail lists. `totals()` falls back to complete lists in older reports; `is_empty()` continues to account for classified paths even when their details are omitted. Recognized moves still overlap an added destination and removed source. Quarantine activity totals remain distinct from whole-generation quarantine coverage.
- Sync/reindex now assemble final source coverage and report metadata before publication. Coverage combines retained and prepared source facts without cloning all source records or rereading the new generation. Exact totals are captured before fitting, and the largest elapsed-time value reserves room for the post-publication timer. Required report metadata overflow fails before either graph publication or a metadata-only manifest commit.
- Reports and their compact CLI JSON envelopes fit the 64 KiB ceiling. Detail suffixes are removed in added/modified/removed/renamed/quarantined order, preserving warning details longest. A byte notice makes omission explicit; text output shows exact totals and the notice instead of claiming an empty detail list means up to date. Transport/write failures after successful publication remain post-publication failures, not rollback guarantees.
- Added a 500-file build with five binary quarantines to verify library/CLI fitting, exact totals and prepared-generation coverage. An oversized policy-metadata regression checks that both reindex and sync fail with the durable `CURRENT` unchanged. A core packing test covers every detail category, complete omission, idempotence, retained totals and older reports without `counts`.
- No ranking changes, dependency additions or persisted representation revisions. Report preparation remains bounded by the existing walk/file limits, not a new peak-memory guarantee. Remaining phase/input/transport audit work, other unchecked recommendations and final commit/push remain open.
- The report refactor also removes redundant manifest clones for batch ownership and metadata-only commits. During the transport audit, `--fail-if-stale` was found to derive its displayed count from status's potentially shortened path list. It now retains the exact observed total in text and JSON context, fits the complete stale JSON envelope, and keeps its three delivered path views consistent. The long-path status fixture verifies exit code 3, the exact total of 500, nonempty retained details and the final byte ceiling.
- Validation: all 249 workspace tests passed. After the manifest-move cleanup and stale-notice fix, all 20 public payload and CLI contract tests passed. Final strict workspace/all-target Clippy, formatting and diff checks passed. No external ranking trials were repeated because ranking and source evidence selection were unchanged.

### Remove discarded CLI freshness work and reject oversized graph fields early

- Removed the ordinary `--no-reconcile` CLI status preflight and obsolete renderer staleness arguments. Query context already supplied the authoritative freshness and source identity in every renderer. The separate status walk consumed work and could fail a scoped live scan because of unrelated subtree errors. The explicit fail-if-stale workspace gate remains.
- A new regression first reproduced the failure using a malformed ignore file in an unrelated sibling directory. After the change, scoped file/text scans return complete live evidence, while the explicitly requested workspace check still reports incomplete enumeration. All 16 CLI contract tests passed.
- Graph-service methods now check target/query text and path-filter byte lengths before freshness or automatic maintenance. Core navigation also checks targets independently, including both path endpoints before resolving either. The existing 8,192-byte ceiling is enforced on UTF-8 bytes and oversized contents are not copied into errors. Syntax, ambiguity and term-count validation are separate later checks; this increment does not claim a fully parsed request plan.
- Added all-entry-point rejection tests with multibyte inputs, a check that rejected service calls never create `CURRENT`, independent core tests, and exact-boundary acceptance. All 22 public payload/work tests passed. Strict workspace/all-target Clippy passed after both changes; formatting and diff checks passed. No dependencies or stored representation changes and no retrieval-scoring change.
- Recommendation 5 remains open during the remaining execution audit, including metadata records examined before candidate admission. Other unchecked recommendations and final commit/push remain pending.

### Bound metadata examination before filtering

- Added independent `metadata_entries` work limits (100,000 default, 1,000,000 ceiling, zero permitted). Exact-name lookup, case-folded exact seeding and path navigation charge records before filtering, preventing arbitrarily long rejected-record scans from bypassing candidate-admission quotas. The counter and truncation notice are exposed in query results. Prefix lookup retains its existing dictionary/posting budgets.
- Metadata allowance is inherited with a caller-owned budget for the first query, reset on normal query reuse, and shared across lexical phases; it is not divided for body retrieval, which does not consume it. Absorbed exhaustion notices report the enclosing query allowance. Identical bare/qualified name entries are deduplicated at cache construction, preserving lookup precedence while removing an iterator-internal duplicate scan.
- Regression fixtures cover zero/exact/overflow limits, rejected records without candidate admissions, all three affected routes, shared phase accounting and public service resets. Exact-ID and bounded ambiguity lookups remain constant-count lookups; this change does not treat them as collection scans.
- Validation: all 255 workspace tests passed, followed by the final 17 public work-contract tests after adding output-filter/boundary-reference coverage. Strict workspace/all-target Clippy passed before that final test-only addition. The new graph fixture proves traversal crosses excluded intermediate nodes while delivered nodes obey presentation filters, and capped results retain resolvable stable-ID boundary references.
- Both exposed-label evidence suites are unchanged: 36/36 newer-suite trials and 102/102 original-suite trials, with zero protocol errors and unchanged sibling source fingerprints. [Newer comparison](results/native-implementation/metadata-work-fresh/comparison.json), [original comparison](results/native-implementation/metadata-work/comparison.json). These establish evidence regression safety, not model task success or resolution of previous context-selection tradeoffs.


### Recommendation 5 requirement audit

- Request-owned allowances now cover visible walk entries, attempted source files/read bytes, dictionary/metadata/posting examinations, candidate admissions, graph nodes/adjacency entries, occurrences, context windows, delivered edges and serialized output. Freshness verification and automatic maintenance share the retrieval budget. Cancellation/deadlines are checked cooperatively at execution boundaries, not merely after producing output.
- Generation-owned `AdjacencyIndex::read` charges every examined incident entry before filtering or cloning. It returns only the budget-admitted prefix; the snapshot port does not materialize the full degree first. Truncation kinds and work counters identify incomplete traversal; candidate/impact counts are documented lower bounds when work is incomplete. Cached status counts remain exact for the indexed generation.
- Library query results and sync reports have byte fitting, with mandatory metadata overflow returning an error. CLI JSON separately fits its complete envelope. Stable-ID edge endpoints represent result-cap boundary nodes without requiring their full payload. Presentation filters compile once; body filters precede source admission. Graph filters restrict output while traversal can cross excluded intermediates, as covered by the new public fixture.
- Recommendation 5 is complete against those requirements. These are not hard real-time or peak-memory guarantees: an individual parser/store operation, index opening, generation construction and allocation are not preempted. Explicit standalone maintenance retains policy bounds. A completed publication is not rolled back after a later query failure. Those documented lifecycle/performance limits remain relevant to recommendations 23, 28 and 30; this audit does not mark them complete.

### Isolate native Markdown frontmatter

- Chunker revision 3 recognizes an exact unindented first-line `---` (YAML-style) or `+++` (TOML-style) marker. Matching delimiters close the block; YAML-style `...` also closes it. An unclosed block retains the remainder. This is an explicitly declared local dialect: no BOM, surrounding marker whitespace, preceding blank lines or YAML/TOML value interpretation is inferred.
- Authored frontmatter remains searchable as `markdown_frontmatter`, with raw UTF-8/CRLF coordinates and delimiters preserved. Heading-like metadata comments and fence markers are opaque inside it. Long blocks retain the existing 80-line/8-line-overlap fragmentation and source-unit caps, without crossing into following prose. The chunker revision invalidates old source partitions independently of parser/ranker revisions.
- Fixtures cover both delimiter families, YAML end markers, mismatched/missing closers, exact initial-marker requirements, multibyte CRLF, persistence/reopen/search, long-block coverage and unchanged prose boundaries. No dependencies, source text rewriting or ranking-weight changes. Setext, lists, tables, heading ancestry, links and structured fields remain open under recommendations 9/25; frontmatter recognition alone does not complete them.
- Validation: all 260 workspace tests passed. Strict workspace/all-target Clippy found one unchecked offset addition; it was changed to saturating addition and Clippy then passed. The counter sums disjoint slices of one string, so the valid-input result is unchanged. Formatting/diff checks passed; focused Markdown tests are repeated after that arithmetic change.
- Evidence regression runs before the equivalent arithmetic adjustment preserved 36/36 newer-suite and 102/102 original-suite trials, with zero protocol errors and unchanged sibling source fingerprints. [Newer comparison](results/native-implementation/frontmatter-fresh/comparison.json), [original comparison](results/native-implementation/frontmatter/comparison.json). Provenance records the exact evaluated production bytes. These exposed-label results show no measured regression; they do not establish a quality improvement or model task success.
- Final focused verification after the arithmetic change: all four core Markdown boundary tests and all four public Markdown integration tests passed. Remaining recommendations and acceptance gates, then commit/push, are still pending.

### Native Setext section boundaries

- Chunker revision 4 adds top-level Setext section boundaries at the beginning of the preceding contiguous prose paragraph, including multiline titles. Underlines accept a homogeneous nonempty `=` or `-` run with at most three leading spaces and trailing spaces/tabs. Blank lines end title eligibility. Raw title, underline, body, UTF-8 and CRLF bytes remain intact.
- Existing frontmatter/fences stay opaque. Indented code, list/quote/HTML/reference-like blocks conservatively suppress recognition until a blank line or recognized top-level boundary. This is a documented subset, not nested-container or full CommonMark conformance. No generated heading text is inserted into source evidence.
- New fixtures check multiline CRLF titles, consecutive sections, invalid underlines, blank-separated rules, code, lists, quotes and complete original-byte partitioning. A source-unit integration fixture verifies the matching body retains its title and excludes the next section. The chunker revision refreshes stale partitions; scoring weights and dependencies are unchanged. Recommendations 9/25 remain open for richer structure and structured fields.
- Both exposed-label evidence suites preserve every prior evidence result: 36/36 newer-suite and 102/102 original-suite trials, zero protocol errors, unchanged sibling source fingerprints. [Newer comparison](results/native-implementation/setext-fresh/comparison.json), [original comparison](results/native-implementation/setext/comparison.json). No task-success or universal section-chunking quality claim follows from these regressions.
- The initial source-unit fixture incorrectly queried an unsplit camel-case token in split-term postings. It now checks the actual `needle` term; production analyzer behavior was unchanged.
- All 203 core/public-library tests passed, including the source-unit integration fixture. Strict workspace/all-target Clippy required two syntax-only cleanups (a byte string and a collapsed conditional), then passed. The evidence runs precede those equivalent cleanups and retain exact build provenance. Formatting and diff checks pass; focused boundary/integration tests are repeated on the final syntax.
- Final focused verification passed: six core Markdown tests and five public Markdown integration tests. No processes remain running for this increment. Remaining implementation/evaluation gates and final commit/push are pending.

### Preserve heading ancestry and deliver parent context

- Source representation version 3 adds optional outer-to-inner Markdown heading references to stored source units. Candidate context retains those references internally until source selection. Each reference has a level, full heading span and title span in the same file/hash. ATX title spans exclude markers and surrounding whitespace while preserving inline markup; Setext title spans preserve authored title lines. Ancestry replaces equal/deeper levels, supports skipped levels, and has at most six entries. Code-fence fragments inherit it; frontmatter does not.
- Extraction maintains one small ancestry stack while visiting source boundaries rather than precomputing a separate hierarchy copy for every scanned region. Authored parent text is not duplicated into body postings, synthetic prefixes or source snippets. Optional vectors remain backward-readable from source versions 1/2. Native validation rejects invalid levels/order, foreign evidence kinds, file bounds and title containment before accepting stored facts.
- Ranker revision 8 offers parent spans to the existing evidence packer as `document_heading` excerpts after structural and matched source windows. Source hashes, shared source cache, marginal byte costs, global line deduplication and work/interval/output budgets still apply. Undelivered references do not consume mandatory response metadata; only selected excerpts count as delivered evidence. Long title delivery remains explicitly bounded.
- New tests cover title coordinates, empty/closing-marker ATX cases, multiline Unicode/CRLF Setext titles, invalid stored references, older records without ancestry, fragmentation, sibling/root replacement, publication/reopen and edited-source overlay context. Remaining Markdown lists/tables/links, full container handling and broader quality acceptance remain open.

- Rejected first context policy: serializing all ancestry references and letting parent excerpts compete with structural evidence changed three original-suite tasks across repeats: edit coverage 10/65 → 12/65, grep coverage 21/68 → 14/68, and glob coverage 31/75 → 12/75. The newer 36 trials were unchanged. [Original experiment](results/native-implementation/headings/comparison.json), [newer experiment](results/native-implementation/headings-fresh/comparison.json). These losses motivated separating candidate context from mandatory result metadata and admitting parent text after matched evidence; no query-specific exception was introduced.
- Added a byte-budget sweep comparing context selection with/without parent references. It requires identical primary and non-heading evidence at every tested cap while still admitting parent text when space remains. The first full workspace run passed 266 tests before this revised selection policy; final validation follows below.
- The revised policy restores all original evidence: 102/102 original-suite and 36/36 newer-suite trials match the pre-heading `setext` baselines, with zero protocol errors and unchanged sibling source fingerprints. [Original comparison](results/native-implementation/heading-context/comparison.json), [newer comparison](results/native-implementation/heading-context-fresh/comparison.json). This is regression evidence on exposed labels; the targeted Markdown fixture proves parent delivery, not general task-success improvement.
- Final revised implementation: all 207 core/public-library tests passed, including the tight-budget non-displacement sweep and persisted/live parent-context fixture. Strict workspace/all-target Clippy, formatting and diff checks passed. Evidence artifacts retain their exact production provenance; only test placement/fixture corrections followed the newer-suite build. All evaluation/test sessions are terminal. Remaining recommendations and final commit/push remain pending.

### Preserve fenced-block fields and fragment context

- Source representation version 4 adds optional native fence descriptors: full block, content excluding delimiter lines, trimmed authored info string, first ASCII-whitespace-delimited label, and closed/unclosed state. Coordinates reference original bytes; labels do not select a parser and values/escapes are not rewritten. Every long-block fragment retains the same descriptor. Version-4 fence units require valid descriptors; versions 1–3 remain readable without them.
- Metadata is computed once per visited fence and retained in internal candidate context, without copying language strings into body postings or mandatory result metadata. Ranker revision 9 may deliver the original opener/info line and actual closer as `document_fence` excerpts after matched evidence, under the existing hash, byte, work, interval and deduplication rules. No closer or omitted code is synthesized.
- Tests cover empty/closed/unclosed blocks, mismatched closers, Unicode/CRLF, info labels with extra attributes/ASCII whitespace, invalid stored coordinates/closure state, legacy records, long-fragment identity and label changes in never-reconcile live context. The tight-budget comparison now covers both heading and fence context. Markdown paragraphs/lists/tables/links and remaining implementation/evaluation gates are still open.
- All 36 newer-suite and 102 original-suite evidence trials match the prior heading-context baseline, with zero protocol errors and unchanged sibling source fingerprints. [Newer comparison](results/native-implementation/fence-context-fresh/comparison.json), [original comparison](results/native-implementation/fence-context/comparison.json). These are exposed-label regression results, not task-success or general relevance-gain claims.
- The initial unit fixture incorrectly expected vertical tab to delimit a Rust ASCII-whitespace token. The corrected fixture preserves that byte in the label and separately tests form feed as a delimiter; the declared contract now states the distinction. Production label parsing and benchmarked behavior did not change.
- Final validation: all 208 core/public-library tests passed, including metadata validation, long-block/live-label integration and the document-context non-displacement sweep. Strict workspace/all-target Clippy, formatting and diff checks passed. No dependency manifests or lockfile changes. All sessions for this increment are terminal; remaining recommendations, final acceptance and commit/push are pending.

### Native table row groups and header context

- Chunker revision 5 recognizes top-level pipe tables inside the native block scanner before Setext inference. Header/delimiter column counts must agree; optional edge pipes, escaped literal pipes and colon alignment markers are handled. Uneven raw data rows are retained until a blank line or recognized block boundary. Fences/frontmatter stay opaque; container/HTML/reference-like blocks retain conservative fallback. This is a declared GFM-inspired subset, not full nested renderer conformance.
- Source representation version 5 adds shared table/header/delimiter spans and column counts. Bounded row groups preserve original UTF-8/CRLF and inherited heading ancestry. Header text is not duplicated into every group's postings, and row cells are not padded/truncated. Publication/reopen validation checks containment, adjacency, single-line header/delimiter bounds, positive column counts and descriptor presence.
- Ranker revision 10 offers original header/delimiter lines as optional `document_table_header` context after matched evidence. Descriptors stay internal until context selection. Parser/source-unit tests cover escaped pipes, uneven rows, malformed delimiters, opaque blocks and Setext ambiguity; a long-table fixture checks fragment metadata, rejection of corrupted facts, publication/reopen and header delivery for a distant match.
- Both evidence suites preserve all prior results: 36/36 newer-suite and 102/102 original-suite trials, zero protocol errors and unchanged sibling source fingerprints. [Newer comparison](results/native-implementation/table-context-fresh/comparison.json), [original comparison](results/native-implementation/table-context/comparison.json). These exposed-label code-search regressions do not establish general table-search quality; the targeted long-table fixture establishes the implemented header-delivery contract.
- Final validation: all 212 core/public-library tests passed. Strict workspace/all-target Clippy, formatting and diff checks passed. No third-party dependencies were introduced. All sessions for this increment are terminal. Remaining Markdown structure, other recommendations, final acceptance and commit/push are pending.

### Measure durable generation churn before per-file storage

- Extended the disposable-copy sync probe to inspect committed generation descriptors, artifact sizes/hashes and canonical per-file record hashes, check graph equivalence again after durable reopen, and record no-op generation identity. Added `research/scripts/storage_review.py` to reproduce three independent trials against the frozen HEAD archive with exact implementation provenance. No production/dependency changes in this increment.
- On the 193-file frozen corpus, appending one comment to `crates/core/src/query.rs` parsed one file but republished 16 projections. All three incremental and reopened graphs matched clean rebuilds. The final instrumented trials took 481–503 ms (median 482 ms); no-op sync took 9 ms, parsed no files and preserved the generation. These small warm-local measurements are not large-corpus or concurrency results.
- Each new generation contained 19,354,585 logical artifact bytes. Manifest, occurrence and source files totaled 17,481,494 bytes, although only one of 193 records changed in each. Canonical changed record sizes were 328,163, 367,584 and 92,755 bytes respectively (788,502 combined). Canonical record bytes differ from artifact bytes because manifest formatting and map keys are excluded. Hash/size measurements do not measure physical disk writes or predict the full latency improvement.
- [Per-record summaries](results/native-implementation/storage-record-baseline/summary.json) and [provenance](results/native-implementation/storage-record-baseline/provenance.json) support native immutable per-file fact reuse as the next storage experiment. The initial artifact-only three trials are retained under `storage-baseline`; their harness provenance predates per-record instrumentation. `in_memory: true` controls initial opening, but nonempty store directories still publish durable generations; the probe verifies those actual artifacts rather than assuming memory-only execution.
- Next implementation must preserve atomic graph/fact/manifest publication, checksum verification, legacy reopen, failure recovery and current/previous generation reclamation. Reuse can reduce unchanged fact serialization without solving whole-graph reconstruction, cache rebuilding or broad dependency invalidation; recommendation 23 remains open. Full mutation-family clean-rebuild equivalence, large churn and peak-memory measurements remain required. Build, probe execution, formatting and diff checks passed; two intermediate harness compilation errors were corrected before the recorded run. Commit/push and the remaining recommendations are pending.

### Native per-file source storage experiment — correctness passes, performance gate open

- Added generation format 4 with a small version-1 source-record index and immutable SHA-256-named JSON records. Unchanged records reuse verified files through hard links, with synced-copy fallback. A generation-owned cached index prevents subsequent disk-index edits from redirecting reuse. New/changed records never overwrite shared inodes. Legacy whole-map source files and generation formats 1–3 remain readable.
- Source record hashes are transitively committed by the top-level index checksum. Reopening verifies every referenced record; publication hashes newly written bytes and verifies reused bytes before linking. The record directory, index and generation publication retain the existing sync/rename ordering. A redundant second verification of reused records during pointer preparation was removed; an in-memory set replaces per-record destination existence checks and deduplicates identical records.
- New engine fixtures cover per-file changes/removals, equal-record deduplication, actual inode sharing, survival after removing the previous directory, missing/corrupt records, invalid hash paths, old descriptor rejection, legacy-map migration and reuse after disk-index edits. Existing semantic-corruption and analyzer-upgrade fixtures explicitly write legacy maps to exercise compatibility. The first implementation passed all 275 workspace tests. After caching the opened index, 23 engine tests and six source-unit integration tests passed, followed by strict workspace/all-target Clippy. Final verification after removing redundant filesystem work follows below.
- Extended the probe to include source-record files in logical generation bytes, measure matching device/inode identities across generations, and compare canonical persisted source/occurrence records with a clean rebuild. All final trials preserve graph equality before/after reopen, source/occurrence fact equality, and no-op generation identity.
- The experiment is **not performance-accepted**. The same-harness whole-map control had sync times 503/487/494 ms (median 494 ms); the final per-file writer had 559/553/557 ms (median 557 ms, about 13% slower). Initial indexing also regressed: control 564/538/534 ms versus 1282/1294/1288 ms. No-op sync remained 9 ms in the final trials. This is a frozen 193-file warm-local benchmark, not a large-corpus/concurrency result.
- Current generations share 7,052,935 logical source-record bytes with their predecessors. Total logical generation size is 19,367,346 bytes versus 19,354,585 for whole maps; subtracting shared inode bytes leaves 12,314,411 newly represented bytes, excluding filesystem metadata/allocation behavior. These are not syscall-I/O measurements. Storage sharing succeeds, but many file operations and per-record syncs make the implementation slower at this scale.
- Artifacts preserve all attempts: `source-record-storage` (median 591 ms), `source-record-storage-single-verify` (569 ms), `source-record-storage-dedup` (559 ms), [whole-map control](results/native-implementation/source-record-storage-legacy-control/summary.json), and [final native run](results/native-implementation/source-record-storage-final/summary.json). Each has exact provenance. The control temporarily replaced only the source publication call with the existing whole-map writer and restored the production file in a `finally` block before the final native run; it is not a production option.
- Next action: test immutable packed records with per-file offsets/hashes, so initial publication syncs a bounded number of packs and a one-file edit writes a small delta pack while sharing previous packs. The measured per-file filesystem overhead provides evidence for this narrower native packing experiment under recommendations 23/28. It must validate slice bounds/checksums, avoid unbounded accumulation of dead record bytes, preserve atomic publication/reclamation/legacy reads, and outperform the whole-map control before acceptance. Whole manifest/occurrence rewrites, whole-graph reconstruction, cache rebuilding and precise invalidation still remain. Recommendation 23 and the overall goal remain open; commit/push is pending.
- Final state verification: all 23 engine tests passed after the filesystem reductions; strict workspace/all-target Clippy, workspace/harness formatting and diff checks passed. The final benchmark's production/harness hashes match the current files exactly. All processes for this increment are terminal. The measured latency/cold-index regressions remain explicit acceptance failures, not completed optimization claims.

### Replace individual source records with immutable packs

- Generation format 5 uses source-record index format 2. Each file retains its own record hash, pack hash, byte offset and length; packs target 8 MiB, with an oversized record occupying its own pack. The writer deduplicates identical records, serializes directly into a pack buffer and allocates a separate tail only when a record crosses a pack boundary. Whole-map and individual-record layouts remain readable and migrate on publication.
- Readers verify complete pack hashes, per-record hashes, checked nonempty ranges and non-overlap (exact duplicate references are allowed). Reuse verifies the complete pack against the index cached when opening/publishing. That equality preserves the already-validated record hashes, so reuse does not hash each record a second time. Cached references prevent later disk-index changes from redirecting reuse. Hard links share immutable packs; a tested synced-copy fallback preserves correctness when links fail.
- Packs with less than 75% live bytes are repacked. Multiple existing sub-MiB packs are combined, leaving at most two small packs per generation; other retained packs carry at least 75% live bytes. A 40-update fixture checks churn, deletions, reclamation, live-byte occupancy and small-pack counts. Further fixtures cover rollover, oversized records, pending/flushed deduplication, corrupt/missing packs, invalid/overlapping slices, incompatible layout versions, legacy migration, source-version changes without source-hash changes, and inode sharing.
- The store derives a touched-path set from batch upserts/removals. `complete_projection` clones existing source facts, and `apply_prepared` changes source facts only for those paths, so surviving untouched facts can skip deep equality checks. Touched facts still compare the complete representation. This proof is specific to source facts: occurrence rebinding can affect owners outside the directly changed files, so this hint must not be copied to occurrence storage without tracking those changes.
- The first packed implementation passed all 280 workspace tests. Final buffer/reuse/touched refinements passed all 28 engine tests and six public source-unit tests, followed by strict workspace/all-target Clippy. No parser, ranker or dependency changes were made. Final benchmarks verify graph equivalence before/after reopen, persisted source/occurrence equality against a clean rebuild, and unchanged no-op generations.

The final comparison uses separate prebuilt binaries, one warmup per binary/corpus,
and seven alternating pairs per corpus. The whole-map control is built in a
disposable source checkout; production sources are never patched by this driver.
Both layout variants and the restored production binary were verified. See
[paired results](results/native-implementation/source-pack-storage-paired/comparison.json),
[exact provenance](results/native-implementation/source-pack-storage-paired/provenance.json),
and [reproduction driver](scripts/storage_compare.py).

| Measurement | Frozen 193-file repository | Synthetic 512-file Markdown corpus |
| --- | ---: | ---: |
| Whole-map median sync | 484 ms | 1,152 ms |
| Packed median sync | 482 ms | 1,039 ms |
| Median paired sync change | −4 ms (5/7 faster; effectively parity) | −113 ms (7/7 faster; about 10%) |
| Median paired initial-index change | +23 ms | +146 ms |
| Shared pack bytes after one edit | 7,145,489 | 47,320,876 |
| Packed generation logical bytes | 19,481,378 | 47,908,838 |
| Logical bytes excluding shared inodes | 12,335,889 | 587,962 |
| Whole-map generation logical bytes | 19,354,585 | 47,725,360 |

- This retains a measured tradeoff: substantial generation sharing and faster large-source updates, with a modest initial-indexing cost (about 4% and 8% in these paired runs). It does not establish a general small-repository latency improvement. Shared packs may include bounded dead record bytes. Logical byte/inode measurements are not physical I/O or peak-memory measurements; the generated text corpus is not a code-search relevance or model-success evaluation.
- Earlier sequential attempts remain available under `source-pack-storage`, `source-pack-storage-buffer`, `source-pack-storage-reuse`, `source-pack-storage-touched` and their control/large counterparts. The paired experiment is the stronger timing comparison; it avoids treating changes between sequential runs as causal speedups. `storage_review.py` now supports repeat counts and reproducible generated corpora. No external repository or CodeGraph index was modified.
- Recommendations 23/28 remain open overall: source packs address one persistence cost. Manifest/occurrence whole-file replacement, whole-graph reconstruction, generation-wide index rebuilding, precise dependency invalidation, large mutation-family and concurrency/peak-memory acceptance still need work. The next storage work should reuse the packed primitive for other fact types while preserving their distinct invalidation rules. Final goal audit, commit and push are pending.

### Avoid duplicate source loading and manifest cloning

- Publication borrows the manifest already owned by its immutable write batch, eliminating a full extraction-cache clone. Its lifetime extends through preparation and persistence; no publication ordering changes.
- Generation selection decodes the source descriptor from the same bytes whose top-level checksum it verified, enforces layout/generation compatibility, and passes that owned descriptor into the source loader. The loader reads/verifies each referenced pack and deserializes its records once before returning the store. Legacy whole-map facts reuse the already decoded map. Replacing the descriptor during this handoff cannot redirect pack selection; pack corruption still fails open.
- All 29 engine tests and six public source-unit tests passed, including the authenticated-descriptor handoff, corrupt-pack rejection, legacy migration and existing failure-atomicity cases. Strict workspace/all-target Clippy passed after changing an implicit PathBuf clone to an explicit clone.
- The sync probe now reports `reopen_ms`, including graph opening, artifact verification, source loading, ownership validation and retrieval-index construction, but excluding subsequent equivalence checks/no-op synchronization. Three independent frozen-corpus trials had median reopen 276 ms, sync 486 ms and no-op 9 ms; the 512-file generated corpus had median reopen 782 ms, sync 1,141 ms and no-op 2 ms. All graph/fact/reopen/no-op checks passed. These are new timing baselines, not paired evidence of a latency gain from removing the duplicate work. See [frozen results](results/native-implementation/source-pack-open-once/summary.json) and [larger results](results/native-implementation/source-pack-open-once-large/summary.json), each with exact provenance.
- The paired driver now restores its production executable by replacing a copied temporary file, avoiding writes through a possible Cargo hardlink. No dependency or semantic representation changes were introduced. Whole manifest/occurrence writes and per-file index maintenance remain open.

### Build retrieval indexes only for the final prepared graph

- `apply_prepared` remains a private graph/fact mutation operation. Publication applies the complete old projection and then the requested batch using the same deletion/insertion order as before, but now builds metadata, body, occurrence, adjacency and count indexes only after both mutations. It does not merge the batches or change incoming-edge deletion semantics or outcome counts.
- Dependency check: mutation reads raw graph nodes, ID maps, source facts and occurrence facts; it never queries derived retrieval indexes. `refresh_indexes` enumerates the authoritative graph through `all_nodes`/`all_edges`. The isolated preparation handle is not returned to a reader; refresh finishes before either the transient-store swap or durable publication. Failure leaves the previously visible handle unchanged under the existing publication contract.
- The paired storage driver accepts `--control intermediate-indexes`, which builds a disposable control checkout with only an extra refresh after the old projection. This isolates the removed full index build while retaining identical packed storage, source-loader behavior and manifest borrowing in both binaries. Validation and measurement follow below.

- Final validation: all 282 workspace tests passed, followed by strict workspace/all-target Clippy, workspace/harness formatting and diff checks. No parser, ranker, wire/storage format or dependency changes were needed for the final-only index build.
- Seven alternating pairs per corpus, with one warmup per binary/corpus, isolate the redundant build. Every pair favored the final-only build. Frozen 193-file median sync fell from 480 to 376 ms; the median paired difference was −101 ms (about 21%). Generated 512-file median sync fell from 1,024 to 698 ms; the median paired difference was −325 ms (about 32%). Initial indexing does not build a populated old projection, so its small timing differences are not evidence of the same optimization. Reopen/no-op behavior and logical storage composition remain comparable.
- All 28 measured trials passed graph equivalence before/after reopen, canonical persisted source/occurrence equivalence and unchanged no-op generation checks. Production source hashes remained unchanged during capture, and the top-level production binary was verified restored. [Paired comparison](results/native-implementation/final-projection-indexes-paired/comparison.json), [exact control edit](results/native-implementation/final-projection-indexes-paired/control-edit.json), [provenance](results/native-implementation/final-projection-indexes-paired/provenance.json). These are warm local update measurements, not query latency, task success, physical I/O, concurrency or peak-memory evidence.
- This removes an unnecessary complete index build; the final build is still generation-wide. Recommendations 7/8/23 remain open for per-file graph/posting maintenance, precise invalidation and manifest/occurrence fact storage. Remaining requirements, final audit, commit and push are pending.

### Separate cold extraction facts from the persisted freshness header

- Generation format 6 commits a small `manifest.json` header and an `extractions.json` record index whenever a manifest is present. Raw per-file entries use the shared native pack writer under `extraction-records/`, with the same record/pack hashes, checked ranges, deduplication, 8 MiB target, 75% live-byte compaction, bounded small packs, link/copy fallback and generation reclamation as source facts. No new dependency or semantic representation version is required.
- Open authenticates and pins the header and record descriptor, verifies extraction pack bytes/ranges, and defers decoding raw JSON values until facts are requested. Hydration rechecks bytes, requires a present extraction and an exact match of all remaining per-file fingerprint fields. It rejects missing owners, malformed values, mismatched metadata, missing packs and corrupt bytes instead of silently supplying an empty cache. Legacy embedded manifests remain readable and migrate on publication.
- Reuse compares each current serialized entry's hash against its cached record reference. It does not assume that source-hash equality implies extraction equality. This still serializes all candidate extraction records to decide reuse, but avoids rewriting unchanged record bytes; selective loading and precise dirty tracking remain future work.
- Reconciliation classifies against `manifest_header()`. A true no-op reads no raw extraction facts, and full reindex uses only the old header because it parses every current file. Changed-file rebinding still hydrates the complete raw cache. Same-content timestamp/size refreshes hydrate and preserve facts before publishing updated fingerprints. A public integration fixture makes cold pack bytes unavailable after opening, verifies no-op generation identity, restores them, changes only mtime, and verifies cache preservation after publication/reopen.
- Publication/crash fixtures now include real extraction records and a failure point after extraction persistence but before header persistence. Additional fixtures cover same-source-hash fact changes, header size, immutable sharing and reclamation, legacy root/format-5 migration, incompatible generation formats, missing committed indexes, descriptor pinning, missing/corrupt packs, mismatched owner fingerprints and removal of cached facts.
- The sync probe now compares hydrated manifest entry records with a clean rebuild and compares reopened manifest entries as well as graph/source/occurrence state. Logical size/inode accounting includes extraction packs. The paired driver supports an embedded-manifest control built in a disposable checkout, with explicit generation-format and store-call edits recorded. A deterministic Rust corpus (64 chained declarations per file) exercises substantial raw facts rather than relying only on Markdown's mostly empty extraction values. Validation and measured tradeoffs follow below.

- Validation: the first storage-only workspace pass passed; strict lint then identified two functions over the line limit. Fact-owner validation was extracted and repeated uncommitted-artifact checks consolidated. The final workspace pass passed 287 tests, including the header-only no-op/metadata-refresh integration fixture, and strict workspace/all-target Clippy passed. One subsequently added format-5 migration test passed separately (288 distinct current tests in total). Workspace/harness formatting and diff checks pass. Production dependency manifests and lockfiles are unchanged.

The final layout comparison uses seven alternating pairs per corpus and separate
prebuilt binaries. Both variants use the new header-only no-op route; the control
stores embedded extraction facts in format-5 manifests. Its two exact source edits
are recorded alongside [results](results/native-implementation/manifest-packs-paired/comparison.json)
and [provenance](results/native-implementation/manifest-packs-paired/provenance.json).

| Measurement | Frozen 193-file repository | Generated Rust: 128 files × 64 functions |
| --- | ---: | ---: |
| Embedded / split manifest bytes | 6,711,265 / 58,922 | 20,227,294 / 37,086 |
| Embedded / split median sync | 374 / 402 ms | 648 / 694 ms |
| Median paired sync change | +28 ms (about 7.5%; 1/7 faster) | +42 ms (about 6.5%; 0/7 faster) |
| Embedded / split median reopen | 278 / 272 ms | 416 / 395 ms |
| Median paired reopen change | −7 ms | −23 ms |
| Median paired initial-index change | +4 ms | +2 ms |
| Embedded / split logical generation bytes | 19,481,378 / 16,789,811 | 32,786,737 / 23,116,912 |
| Embedded / split logical bytes excluding shared inodes | 12,335,889 / 6,052,815 | 28,630,952 / 8,547,431 |

- All 28 measured trials passed graph equivalence, hydrated manifest/source/occurrence record equivalence, reopened graph/manifest equivalence and unchanged no-op generation checks. Production hashes remained unchanged during capture and the restored production binary matches its captured hash. No-op medians were 1 ms on the frozen corpus and below the probe's 1 ms resolution on generated Rust, for both layouts. These timings do not isolate the header-only route's gain, since both arms use it.
- The split establishes small persisted headers, immutable raw-record sharing and modest reopen gains, but **does not pass an update-latency improvement gate**. Updates regress about 6–8% in these trials. It still serializes every candidate extraction record to establish safe reuse, reads complete raw caches for binding, and adds durable pack/index operations. The experiment does not separate those costs; phase measurements should guide the next optimization rather than guessing which dominates. Initial-index differences are small, and logical byte savings are not measured physical I/O or peak-memory savings.
- This storage increment remains under performance evaluation. Per-file fact loading, precise invalidation, whole occurrence persistence, graph reconstruction and generation-wide final index construction remain outstanding, along with the broader recommendation and release gates. Nothing is committed or pushed; no sibling repository/index was modified.

### Profile raw-fact storage and preserve verified record identity

- Added a reproducible diagnostic driver that copies sources into an instrumented checkout, records exact patches/binary/source hashes, captures phase events and ordinary correctness results, and restores the production executable by atomic replacement. Production source is never instrumented. Timer/logging overhead and nested `replace_*` subphases make these diagnostics unsuitable as an uninstrumented latency comparison.
- Three trials on each corpus identify repeated hashing as material. Median extraction load phases were 9.865 ms pack read/hash, 9.596 ms range/record hashes and 6.592 ms decode on the frozen repository; generated Rust took 28.738, 27.678 and 16.682 ms. Reuse decisions took 13.205/37.415 ms; retained-pack verification/linking took 10.311/29.612 ms. New-record serialization/flush and index commit took 4.872+7.594 / 4.686+6.443 ms. These phases have explicit boundaries; nested file-sync values must not be added again. See [phase medians](results/native-implementation/manifest-pack-phases/median-phases.json) and [instrumentation/provenance](results/native-implementation/manifest-pack-phases/provenance.json).
- Raw extraction descriptors now have a private `Verified` wrapper. Initial opening checks pack and record hashes before constructing it; publication constructs it from newly hashed records and already-verified retained references. Later hydration still checks the full pack hash, checked slice ranges and owner fingerprints. Equal complete pack bytes under an immutable descriptor preserve previously verified record hashes, eliminating one full pass over raw record bytes without retaining a duplicate byte cache or trusting source-hash equality.
- Unverified source-index loading and standalone sidecar loading retain full record-hash validation. New coverage rejects a forged record hash before creating the verified cache. Existing tests still reject corrupted packs after verification, preserve descriptor pinning and exercise legacy migration/failure atomicity. All 34 engine tests and seven public source-unit tests passed, followed by strict workspace/all-target Clippy. No storage, parser, analyzer or ranker version change is needed because bytes and semantics are unchanged.

- A second three-trial phase capture confirms the intended mechanism. Extraction range/record verification during cached hydration fell to 0.011 ms on the frozen repository and 0.006 ms on generated Rust; pack read/hash remained 9.960/28.833 ms and decoding 6.678/16.671 ms. Standalone validation reads still perform complete record hashing. [Updated phase medians](results/native-implementation/manifest-verified-phases/median-phases.json) retain exact instrumentation and provenance. All six diagnostic trials preserve graph/fact/reopen/no-op equivalence; production hashes and restored binary identity were checked.
- The uninstrumented seven-pair comparison still shows a remaining update penalty: frozen embedded/split median sync 373/387 ms, median paired change +13 ms (0/7 faster); generated Rust 651/667 ms, median paired change +23 ms (2/7 faster). Both paired penalties are about 3.5%, reduced from the preceding +28/+42 ms comparisons but not an update-latency win. Paired reopen changes remain −7/−23 ms; paired initial-index changes are +1/+4 ms. Storage composition and no-op timing are unchanged. [Latest paired comparison](results/native-implementation/manifest-verified-paired/comparison.json) includes all 28 passing equivalence trials and exact provenance.
- Reuse decisions still serialize/hash every extraction entry, while retained packs still require full-byte verification before linking. A possible next native improvement is immutable shared extraction values with generation-bound identity: unchanged values could prove equality without reserialization, while any mutation must lose that identity and fall back to full comparison. Such a change must preserve same-source-hash cache edits, arbitrary public manifest updates, legacy loading, concurrency bounds and memory behavior; source hash or an unverified dirty-path hint is insufficient. This is a proposed next step, not implemented behavior.
- Final engine/source-unit tests, strict Clippy, formatting, Python driver compilation and diff checks pass. No profiler hooks entered production source. All processes for this increment are terminal. The raw-fact storage performance gate, remaining recommendations, final audit, commit and push remain open.

### Coalesce unpublished generation-directory durability operations

- Split standalone sidecar writes from unpublished preparation. `prepare_manifest` and `prepare_dangling` still write, flush/sync and rename their artifact files; the generation owner syncs their containing directory after every artifact is ready. Public `save_manifest`/`save_dangling` retain their original standalone directory-sync behavior.
- The removed calls committed incomplete directory states that were never visible through CURRENT. All file syncs, separate source/extraction pack-directory syncs, the final generation-directory sync, its parent sync, the synced CURRENT temporary file/rename and post-publication store-root sync remain. A prepublication failure can leave only an unselected partial directory; previous-generation files are untouched. A post-publication directory-sync error still makes the handle unavailable until reopen.
- Added the `after_generation_sync` failure/process-interruption point after generation and parent-directory synchronization, before CURRENT publication. Existing fixtures carry extraction facts and compare complete state/retry behavior. All 34 engine tests and seven public source-unit tests pass, followed by strict workspace/all-target Clippy. These are process-interruption and injected-error checks plus the explicit I/O ordering argument, not physical power-loss experiments.
- The paired driver can restore the two standalone sidecar calls in a disposable control checkout (`--control repeated-directory-syncs`). Other layout comparisons now use the same coalesced directory policy in both arms. The phase profiler targets the prepared manifest writer and labels its timing `manifest_file_commit`, excluding the separately owned generation-directory barrier. No dependency, storage or semantic version changes are needed. Timing results follow below.

- Seven alternating pairs isolate the removed directory syncs. Frozen repeated/coalesced median sync is 386/381 ms, with median paired change −5 ms and 7/7 faster pairs. Generated Rust is 665/666 ms, median paired change −2 ms and 4/7 faster pairs: effectively parity at this noise level. Median paired initial-index changes are −10/−6 ms. Reopen, no-op and storage composition are unchanged. [Paired comparison](results/native-implementation/publication-barriers-paired/comparison.json) records all 28 successful graph/fact/reopen/no-op checks, exact control edits, unchanged production hashes and restored binary identity.
- The updated phase profiler was built and run successfully against a frozen-repository trial and a one-file generated Rust trial in `/tmp/publication-barriers-profile-smoke`; both passed the usual correctness checks. This is driver validation, not a separate performance claim. Formatting, Python compilation and diff checks pass. All processes for this increment are terminal. The remaining extraction-reuse penalty and broader recommendation/release gates are still open; nothing is committed or pushed.

The next substantial storage step should address raw-fact ownership rather than add
another hash shortcut. One implementation candidate is a native copy-on-write
shared-extraction wrapper: serialize exactly as the current `Extraction`, clone by
shared ownership, and make any mutable access detach from recorded weak identities.
The verified storage cache could retain only weak identities plus small fingerprint
metadata. Exact live identity and equal metadata would prove safe unchanged reuse;
unknown, replaced or mutated values would use serialized-hash comparison. Required
checks include unique/shared-owner mutation, same-source-hash edits, arbitrary public
manifest updates, independently decoded equal values, cache identity replacement,
legacy byte compatibility and memory retention. This is design work for the remaining
recommendation 23, not an implemented API or a completed performance gate.

### Shared extraction identity (implementation in validation)

`FileEntry.extraction` now uses native `SharedExtraction` (an `Arc<Extraction>`
with copy-on-write mutable access). Parsers still return owned `Extraction`;
Rust callers constructing a `FileEntry` convert with `.into()`. JSON bytes and
legacy deserialization are unchanged. Reconciliation shares unchanged facts,
rather than deep-cloning all symbol/reference vectors.

The generation's verified extraction descriptor retains weak identities and
small per-file fingerprints only. A matching live identity plus exact metadata
can skip serialization/hash comparison when selecting reusable records. Any
mutable access detaches the identity, including the single-strong-owner case;
independently decoded values and changed metadata fall back to byte comparison.
Retained pack integrity checks remain in place. Hydration replaces this optional
identity cache only after full record/fingerprint validation. Cache replacement
can lose a fast path but cannot authorize a stale value. No dependency added.

Validation and controlled latency measurements are pending for this increment;
no RSS or speedup claim is made yet.

Shared-identity validation: the workspace run passed 290 tests
(`/tmp/shared-extraction-tests.log`); the subsequently added cache-replacement,
metadata-only, and unique-owner regression passed in the final engine/types run
(`/tmp/shared-extraction-final-tests.log`). Strict workspace/all-target Clippy
passed (`/tmp/shared-extraction-clippy.log`), as did formatting and diff checks.
Controlled storage comparison and phase measurements remain the next gate.

### Shared extraction performance acceptance

Seven alternating pairs per corpus completed with production sources frozen:
[paired results](results/native-implementation/shared-extraction-paired/comparison.json),
[raw summaries](results/native-implementation/shared-extraction-paired/summary.json),
[provenance](results/native-implementation/shared-extraction-paired/provenance.json).
All 28 measured trials passed graph equality, hydrated extraction/source/occurrence
fact equality, reopen equality, and unchanged no-op generation checks.

| Corpus | Embedded / packed median update | Median paired packed delta | Embedded / packed median reopen | Median paired reopen delta |
|---|---:|---:|---:|---:|
| Frozen 193-file repository | 367 / 372 ms | +9 ms | 280 / 273 ms | −7 ms |
| 128 Rust files, 64 functions each | 646 / 620 ms | −25 ms | 418 / 391 ms | −27 ms |

The Rust update improved in all seven pairs. The frozen update was faster in
one pair, tied in one, and slower in five; it retains a small update penalty.
Initial publication median paired deltas were +6 ms and +9 ms respectively.
Both arms already have header-only no-op checks (1 ms / below millisecond timer
resolution); those are not new gains attributable to this layout comparison.

Logical generation bytes remain 19,481,378 → 16,789,811 on the frozen corpus and
32,786,737 → 23,116,912 on Rust. Unshared logical bytes are 12,335,889 → 6,052,815
and 28,630,952 → 8,547,431. These measure artifact composition and inode sharing,
not physical write traffic or resident memory. Both comparison arms include the
shared extraction representation, so this experiment isolates the storage
layout with that representation, not the total causal benefit of Arc sharing.

A separate three-repeat diagnostic profile for each corpus also passed all six
correctness trials, preserved production source hashes, and restored the
production executable. [Phase medians](results/native-implementation/shared-extraction-phases/median-phases.json)
show extraction reuse decisions at 1.210 / 0.329 ms (frozen / Rust), compared
with 12.908 / 37.198 ms in the earlier verified-index profile. Retained-pack
verification remains approximately 10.4 / 29.5 ms, and packs are still read and
hashed during hydration. These diagnostic runs are not an interleaved causal
latency comparison; nested timers must not be summed twice.

Retain the native packed extraction layout and shared identities: the measured
larger fact workload now improves update latency as well as reopen and storage,
with an explicitly accepted small-repository update tradeoff. This closes the
specific packed-extraction update regression investigation, not recommendation
23 overall. Selective fact loading, narrower invalidation/stable graph updates,
whole-occurrence persistence, and broader mutation/scale gates remain open.
No new dependencies, memory-speedup claim, or global performance claim.

### Complementary regions after owner selection (ranker 11)

Body scoring now retains a best-region owner ranking plus up to three additional
scored regions for each selected owner. Additional regions prefer uncovered
query terms, then body score and stable ordinal; without new terms their match
anchor must be outside retained region boundaries. They contribute no additional
owner score or result slot. Region-local AND/minimum-term semantics are unchanged.
The native term mask rejects more than 128 distinct terms rather than overflowing.

Context assembly receives each region's original bounds and Markdown labels.
It offers bounded matched windows/lines from the extra regions under existing
source-hash verification, work, byte, interval and deduplication rules. Primary
snippets and structural evidence keep their existing priority. Useful regions
omitted by the four-region bound are reported only for retained items with
source excerpts; unselected owners cannot consume payload with irrelevant notices.
This does not guarantee optimal semantic coverage or replace later phrase checks.

New regressions cover two matches 230 lines apart in both a Rust function and a
plain-text file, one result per owner, exact source lines/hashes, the four-region
bound, missing-term preference, region-local conjunction, oversized term input,
and omission reporting for retained versus unselected owners.

The initial implementation passed 293 workspace tests. Final notice placement
passed the core library and body/Markdown/payload suites plus strict workspace
all-target Clippy (`/tmp/complementary-selected-{core,integration,clippy}.log`).
The first external run retained required-file/complete-region totals but mixed
partial-region gains/losses; raw results are preserved under
`complementary-regions` and `complementary-regions-fresh`. Final notice-placement
results are being captured separately under `complementary-selected` and
`complementary-selected-fresh`; acceptance is pending that comparison.

Final retained-owner notice verification completed: 102 development trials and
36 newer-suite trials, zero protocol errors, unchanged sibling source snapshots,
and production source hashes verified after both runs.
[Development comparison](results/native-implementation/complementary-selected/comparison.json)
and [newer-suite comparison](results/native-implementation/complementary-selected-fresh/comparison.json)
retain every per-task change.

Required-file/complete-region totals remain 28/34 and 11/34 on development,
12/12 and 10/12 on the newer suite. Development mean region coverage changes
51.3373% → 51.2459%; mean response bytes 32,865.97 → 33,422.09. One nanus task
(temporary paths) gains three labeled lines; four nanus tasks lose one to three
lines (edit, context, glob, grep). All three repeats agree on the affected-task
coverage. The newer suite has identical evidence across all 36 trials, with
mean response bytes 37,198.25 → 37,602.58. These exposed-label follow-ups are
implementation checks, not new independent evidence or model task success.

The feature's quality gate remains OPEN. The synthetic long-owner failure is
fixed, but the current equal-priority allocation of primary and complementary
matched windows has a mixed aggregate result. Next: distinguish primary-region
context from complementary-region context during budget allocation, preserve
source-bound useful additional regions, and rerun identical-budget comparisons.
Do not mark recommendations 11/12 complete or characterize this as an overall
retrieval-quality improvement yet. Final post-notice validation includes 15
body tests plus Markdown/payload suites, core library tests and strict Clippy;
the earlier full workspace run covered 293 tests before the notice regression
was added.

### Rejected primary-region allocation experiment

Tested a strict order: existing structural evidence, primary-region matched
windows/lines, complementary-region matched windows/lines, optional document
labels. The temporary policy passed core/body/Markdown/payload tests, strict
Clippy, and a byte-budget sweep that preserved primary-region lines. That local
invariant did not predict external utility.

All 138 evidence trials completed without protocol errors or sibling source
changes. Development complete-file/complete-region totals stayed 28/34 and
11/34, but mean partial-region coverage fell from 51.2459% to 51.1250% relative
to `complementary-selected`; mean response bytes rose from 33,422.09 to
33,462.82. The three changed trials are all repeats of the previously improved
temporary-paths task: the gain disappeared. The newer suite's coverage remained
identical in all 36 trials. [Development comparison](results/native-implementation/primary-context/comparison.json),
[newer-suite comparison](results/native-implementation/primary-context-fresh/comparison.json).

Rejected the policy and its policy-specific test. Restored `evidence.rs` byte for
byte to the source hash recorded by the accepted-for-further-development
`complementary-selected` provenance. Its existing mixed quality gate remains
open. This experiment rules against rigid primary-region precedence; next work
should evaluate marginal query-term coverage/proximity per excerpt under the
same output budget, rather than impose region-origin priority. No new production
behavior is retained from this experiment, and no recommendation is marked done.

### Marginal query coverage in context allocation

Retained native query-term novelty per estimated added byte within the existing
role tiers. Each candidate keeps its rank-adjusted role value and multiplies it
by `1 + distinct_new_query_terms`; exact serialized admission is unchanged.
Already delivered primary and additional source lines determine which terms
are new for that owner, including delivery through another item sharing the
same path/hash. Mask unions are recomputed after successful admission, with
one charged context-window evaluation per retained source region. Repetition
loses its bonus; semantic equivalence and phrase proximity are not inferred.
No dependencies or source/index format changes; this is part of ranker 11.

Validation: 100 core tests, 15 body tests, Markdown/payload integration suites,
and strict workspace/all-target Clippy pass (`/tmp/marginal-{core,integration,clippy}.log`).
The added fixture proves preference for missing query evidence and ordinary
stable ordering once that evidence has been delivered. Existing hash/byte/work
budget fixtures continue to pass.

All 138 external trials passed without protocol errors or source modifications;
production hashes were verified before documentation edits.
[Development comparison](results/native-implementation/marginal-coverage/comparison.json),
[newer-suite comparison](results/native-implementation/marginal-coverage-fresh/comparison.json),
[exact experimental patch](results/native-implementation/marginal-coverage/experiment.patch).

Relative to complementary-selected, development required-file delivery remains
28/34 and complete-region delivery increases 11/34 → 12/34, with no previously
complete task lost. `nanus.read.debug` now delivers both required regions and its
labeled relationship. `nanus.temporary-paths.change` gains five labeled lines.
Four nanus tasks lose partial evidence: glob.debug (7 lines), glob.change
(16 lines), context.debug (7 lines in its second region), temporary-paths.debug
(3 lines). All changes reproduce in all three repeats. Mean per-task region
coverage changes 51.2459% → 51.0598%, so the complete-task gain must not be
reported as an aggregate partial-coverage gain. Mean response bytes increase
33,422.09 → 33,455.41. Other repositories' evidence is unchanged.

The newer 12-task suite retains identical evidence in all 36 trials: 12/12
required files, 10/12 complete regions, mean coverage 83.3333%. Mean response
bytes increase 37,602.58 → 37,679.42. These queries are already exposed; this is
not independent confirmation or an agent-success measurement. Median tool
measurements are about 10.28 → 10.32 ms on development and 11.61 → 11.16 ms on
the newer suite; non-interleaved task timings do not establish a causal speedup.

Retain the change for its complete-evidence gain with the explicit partial
coverage tradeoff. This resolves the decision on the two context-selection
experiments, not the full recommendations 11/12 or the broader release gate.
Independent multi-region queries, phrase/proximity policy, and the remaining
implementation ledger still require work before commit/push.

### Native positional verification kernel

Added `core::positional` as a bounded primitive over original UTF-8 source.
Ordered matching permits a declared total number of intervening whole tokens
(zero = adjacency); unordered matching requires the query multiset within an
inclusive token-span ceiling. Repeated terms consume distinct source positions,
stopwords remain positions, underscores stay inside identifiers, and split
aliases cannot create phrase paths. Punctuation separates whole lexemes, so
this is deliberately different from raw literal substring matching. Lowercase
comparison follows the existing analyzer contract, without NFC or full folding.

The verifier returns the earliest-ending witness, shortest among witnesses at
that endpoint, with original half-open byte offsets and token positions. Byte
and token limits produce `Limited`, never a false `Absent`. Cooperative checks
run at token boundaries and every 1024 source bytes (plus at most one UTF-8
scalar). It scans without materializing the whole source token stream. The
ordered prefix state keeps a dominating latest start; the unordered window
keeps multiplicities and expires positions. Query construction rejects empty
lexemes, over 128 query positions, impossible unordered windows, oversized query
bytes, and token-distance values over 4096. No dependency added.

An independent exhaustive interval oracle agrees in 82,000 generated cases
(3,280 sequences × five repeated/nonrepeated queries × five predicates).
Additional fixtures cover Unicode byte spans, CRLF, stopwords, identifier alias
exclusion, incomplete quotas, punctuation-only cancellation, inclusive distance
boundaries, and 128 repeated positions. This is not yet a CLI/search feature:
recommendation 15 remains open until integration and end-to-end verification.

Integration requirements: use whole-lexeme postings only as a necessary
candidate filter; retain duplicate query positions for verification; verify
hash-bound original region bytes BEFORE best-region owner selection/top-k;
never treat the four context regions as an exhaustive phrase candidate set.
Expose ordered versus unordered semantics explicitly, share query source/work
allowances, emit verified witness locations, and prevent metadata/navigation
fallback from weakening explicit phrase intent. Persisting extra positions or
shingles remains conditional on measured verification cost.

Kernel validation completed: five positional tests pass, including the 82,000
oracle comparisons (`/tmp/positional-tests-final.log`). Strict workspace/all-target
Clippy passed before the final boundary fixture; strict core/all-target Clippy
passed afterward (`/tmp/positional-clippy-final.log`,
`/tmp/positional-core-clippy-final.log`). Formatting and diff checks pass. No
end-to-end phrase search or overall workspace test claim is made for this new
module until the planned integration is complete.

### Verification before body owner grouping

Factored body score collection from owner grouping/top-k. Added
`BodyIndex::search_verified`: a source predicate checks every posting-admitted
region before selection, may supply a verified original-line anchor, reject a
candidate, or stop with an explicit incomplete-verification flag. A stop cannot
admit unchecked candidates. Verified anchors outside the candidate region are
rejected. Only accepted regions can become primary or complementary evidence.
The existing ordinary search path keeps the same score collection and ranking.
`PositionalQuery::candidate_terms` supplies distinct whole terms for necessary
filters while retaining duplicate/ordered requirements inside the verifier.

A regression combines real source-unit extraction, native body postings and the
positional kernel: the highest-ranked region has both terms but no phrase; a
lower-ranked region of the same owner contains the phrase at line 251. With
owner top-k one, the verified path retrieves the true witness. Additional tests
cover file masks before callbacks, explicit verification stop, unchecked-region
exclusion and invalid anchors. Validation: 107 core library tests, 15 body tests,
8 planner tests and strict workspace/all-target Clippy pass
(`/tmp/verified-body-{core-final,integration,clippy}.log`). Formatting/diff checks
pass. The CLI and service still have no phrase route; recommendation 15 is open.

Public phrase integration must distinguish logical source fields from storage
windows. For file-level phrase semantics, an all-terms intersection restricted
to an 80-line storage unit is NOT a sound necessary filter: a true phrase may
cross its boundary. Use a sound file-level candidate union/intersection and
verify captured full-field bytes (or a proven overlap strategy), preserve
multiple useful witnesses/owners under explicit quotas, and test boundary-
straddling and repeated matches. Do not silently narrow the contract to four
retained context regions or a first-witness-only file scan. The region verifier
hook supports bounded field predicates, but by itself does not solve these
public-route requirements.

### Streaming multiple witnesses and file-level positional candidates

`PositionalQuery::verify_all` now preserves overlapping witnesses in one source
scan. It returns the shortest witness for each matching end token, not every
combinatorial alignment. A witness cap (maximum 10,000) stops on the first
omitted witness; exactly filling it remains complete if no further witness is
found. Byte/token exhaustion preserves confirmed matches and reports incomplete
verification. Single-witness `verify` uses the same scanner and still stops at
its first witness. Neither route rescans the whole source for each match.

Added `BodyIndex::file_candidates`, using the native whole-identifier lane and
file-level distinct-term masks. Terms may occur in different storage units;
files are not cut by region/owner top-k. Filters and live-file masks precede file
admission. Postings/candidate work still uses the request budget; incomplete
posting enumeration is visible there. This is only a necessary filter: aliases,
word order, proximity and repeated query positions still need source verification.
Missing, stale or truncated facts require the service's explicit fallback and
coverage policy; the new primitive does not certify those facts complete.

An exhaustive endpoint oracle checks overlapping enumeration across 16,395
sequence/query/predicate combinations at four output caps (65,580 checks), in
addition to the earlier 82,000 first-witness checks. Tests verify retained
matches on token exhaustion and rejection of excessive output limits. A native
postings-plus-verifier fixture finds a valid ordered-window match across 202
source lines, rejects terms spread over separate files, respects file masks,
and reports zero-posting-budget exhaustion. Validation: 110 core tests, 15 body
tests, eight planner tests and strict workspace/all-target Clippy pass
(`/tmp/positional-files-{core,integration,clippy}.log`); formatting/diff checks
pass. No new dependencies.

Next: integrate the public typed phrase/proximity routes with file-level
candidates, source-version checks, shared positional allowances, witness-to-owner
mapping, and explicit CLI controls. Recommendation 15 remains incomplete until
those public contracts and end-to-end boundary/dirty-source tests are delivered.

### Request-owned positional allowances

Added independent positional byte/token/witness limits to `WorkLimits`, with
defaults 64 MiB / 1,000,000 tokens / 1,024 witnesses and hard ceilings 1 GiB /
10,000,000 / 10,000. `WorkBudget::verify_positions` passes remaining allowances
to the streaming verifier, accounts actual work, preserves proven witnesses,
and records the actual exhausted resource. The verifier now exposes a stop
reason instead of inferring exhaustion from counters (important when a UTF-8
scalar cannot fit the remaining byte allowance).

Lexical lanes reserve remaining positional allowances and merge usage back into
the request; they cannot reset witness capacity. Truncation notices use the
request's cap after lane merging. Added omitted-when-zero result statistics for
positional bytes, tokens and retained witnesses, populated by query finalization.
Existing ordinary search output has no additional zero fields. Source reads and
hash checks remain separately charged; positional scan bytes measure processing
of captured source, not additional filesystem reads.

Tests cover sharing across fields, exact-fill completeness, a first omitted
witness, proven absence when witness capacity is zero, multibyte byte exhaustion,
token exhaustion, hard ceiling clamps, zero limits, cancellation, fresh budgets,
and lane reservation/absorption. Initial validation passed 113 core tests and
body/work/payload integrations; the added lane regression and positional tests
passed afterward (`/tmp/position-budget-final.log`). Strict workspace/all-target
Clippy passes (`/tmp/position-budget-clippy-final.log`), with formatting and diff
checks clean. No dependency changes. Public phrase query routing and CLI exposure
are still pending; recommendation 15 is not complete.

### Public phrase and proximity routes

Added typed `ExploreMode::Phrase` and `ExploreMode::Near`, with `phrase_gap`
(default 0) and `near_window` (default 8), and explicit diagnostic routes.
The CLI exposes these through `--intent phrase|near`, `--phrase-gap`, and
`--near-window`. Invalid queries are rejected before service freshness work and
before CLI index opening. Discovery analyzer/channel/Boolean options cannot
broaden positional predicates. Runtime ranker revision 12 identifies the new
routing policy; source/index representations are unchanged.

`query_positional` uses native file-level postings as a necessary filter for
current, complete indexed facts, then verifies captured whole-file source before
owner grouping or top-k. Changed, missing, truncated, legacy, unchecked or
incompletely inspected sources use the explicit source-scan path. If freshness
or representation makes every eligible file require scanning, the route skips
posting work that cannot exclude a file. Shared source, positional, candidate,
metadata, graph and context budgets remain active. A verified witness may cross
any number of storage windows. Hash-matching owner identities are collected once
per file; only a full declaration span enclosing the entire witness qualifies.
The smallest enclosing declaration wins; changed source uses file evidence.
Owner lookup is bounded and truncation remains visible.

Witness byte endpoints become inclusive source-line spans with one monotone
source scan and a witness-bounded endpoint map. Overlaps and separate matching
owners survive. The first witness supplies source evidence; additional witnesses
supply context anchors without inventing per-line lexical term masks. Graph
coordinates are unchanged. Positional results rank deterministically by file and
source encounter order, with the existing optional file diversity. This is an
explicit predicate route, not an unmeasured proximity boost to conceptual ranking.

The initial seven integration tests passed: whole lexemes/stopwords/repetition,
no metadata fallback, exact unordered windows, a 202-line storage-crossing witness
inside one declaration, overlapping matches in separate owners, dirty-source
fallback, shared witness and zero-byte caps, zero context, Unicode original
coordinates, reopen/reindex equivalence and invalid-query validation order.
Additional tests cover unchecked core hosts, filtered exact-fill caps, and CLI
contracts. Full final validation is recorded below when complete. Recommendation
15 retains its separate measurement gate for persistent positions and conceptual
proximity features; no shingle index or new dependency was introduced.

Validation for this increment: the workspace suite passed 318 tests
(`/tmp/positional-route-workspace.log`), including the two new CLI contracts.
After adding the unchecked/incomplete-freshness prefilter bypass and its two
additional regressions, all nine positional integration tests passed
(`/tmp/positional-route-final-tests.log`). Strict workspace/all-target Clippy
passed after replacing a wildcard import and a truncating byte-count cast with
explicit imports and native bounded line counting
(`/tmp/positional-route-clippy-final.log`). Formatting and diff checks pass.
Cargo dependencies and lockfiles are unchanged. No commit or push yet; the
remaining recommendation and evaluation gates still apply.

### Positional routes on the three sibling repositories

Added a native research host and reproducible driver for six fixed predicates
per repository, with one warmup and five measured repetitions. All 90 measured
queries passed an independent batch-token/interval oracle: 2,860 delivered
primary source witnesses had valid original spans and matching source hashes,
and repeated outputs were deterministic. Thirteen untruncated workloads returned
all matching files. Five workloads explicitly hit result/output limits; their
missing files remain visible in the artifacts. Before/after production, driver,
binary, sibling source and sibling CodeGraph hashes are identical. Temporary
native stores leave all three original repositories untouched.

Warm medians ranged from 3.44 to 103.96 ms. The workload contains common syntax,
prose and absent predicates; it does not evaluate coding-task relevance or model
success. A simple uncompressed position/start/end payload estimate adds 1.29–1.48
source-byte equivalents before headers/dictionaries. This is not an implemented
codec or causal performance comparison. Keep streaming verification until a
measured verifier bottleneck and query-plus-maintenance comparison justify
persisted positions; conceptual proximity scoring remains a separate experiment.

[Results, limits and reproduction](results/native-implementation/positional-routes/README.md).
The independent oracle's hand-checked test passes; the host built release,
offline and locked with existing dependencies. Driver compilation and diff
checks pass. No production source changed during this increment or capture.

Recommendation 15's explicit phrase/proximity verification is now implemented:
typed predicates, file-level posting admission, whole-lexeme positional alignment,
raw byte evidence, source-version checks, bounded shared execution, CLI/API
contracts, exhaustive kernel oracles, integration boundaries and sibling workload
validation. Persisted positions and shingles remain conditional optimizations,
not hidden implementations. A conceptual proximity ranking feature remains in
the separate scoring/context ablations (14/12); this check does not claim those
recommendations or the complete project are finished.

### Separate IDF and field-normalization ablations; native opt-in BM25F

Ran a 2×3 metadata scoring factorial (clipped/positive IDF × combined length,
independent field saturation, BM25F), plus separate whole-analyzer,
qualified-field and combined-representation controls. Corpus, k1/b, boosts,
exact-name tier, global DF and deterministic ties are fixed. Missing fields count
as zero in corpus means; means are floored at one. The independent source-field
reconstruction passed 1,370,496 bit-exact baseline score checks, plus native
metadata top-50 ID/score comparisons. Captures preserve source-valid labels and
explicitly exclude all 26 drifted tasks.

Clipped-IDF BM25F improved development top-eight required-file presence from
11/20 to 13/20 with no losses. It was selected before validation, where it improved
8/14 to 10/14, also with no losses. Its top-50 coverage is unchanged. Positive IDF
alone lost a development task; independent field saturation lost tasks on both
splits. Historical held-out labels have already been exposed in earlier work,
so the validation is separate-family evidence, not a blinded/fresh evaluation.
These are candidate-file metrics, not source-region delivery or answer success.

Implemented `FieldNormalization::{Combined,Bm25f}` as a typed retrieval option,
with `--normalization combined|bm25f` in the CLI. Combined remains the default.
Native BM25F uses the existing split/whole field frequencies and four cached
field means: normalize per field, combine existing weights, then saturate once.
Clipped IDF stays fixed. No extra postings, source records or third-party
components. Both OR and shortest-list conjunction paths share the same scorer,
filters and work counters. Exact navigation and body/positional routes retain
their separate contracts. Runtime ranker revision is 13; no source representation
or index migration is required.

Native verification against both frozen factorial captures passed **2,740,992**
bit-exact legacy/BM25F score checks across four field representations. Production
metadata top-50 ordering matches the corresponding independent policy. All
candidate rankings equal the original captures; source, driver, binary, labels,
reference artifacts, sibling contents and CodeGraph state stayed unchanged
within each verification run. Compact reports avoid duplicating the raw ranks.

Validation: 116 core tests; 41 planner/body/work tests; 19 CLI contract tests.
A focused CLI contract passed again after switching the argument from a string
to a typed enum. Strict workspace/all-target Clippy passes
(`/tmp/normalization-clippy-pass.log`); formatting/diff checks pass. New tests
cover isolation from unrelated signature length, missing fields, whole/split
analysis, filtered conjunction equivalence, exact priority, body-route isolation,
and zero-posting reporting. Dependencies and lockfiles are unchanged.

[Factorial results and limitations](results/native-implementation/scoring-factorial-review.md),
[development native equivalence](results/native-implementation/scoring-native-dev/summary.json),
[validation native equivalence](results/native-implementation/scoring-native-validation/summary.json).
The ranking driver now accepts `--normalization` for a controlled full-context
comparison. That comparison, at equal candidate and serialized evidence budgets,
is the next gate; recommendation 14 and a default-policy change remain open.


### Integrated normalization gate: retain the existing default

Completed 552 controlled evidence trials: both normalization policies × automatic
and metadata-only routes × three repetitions on 34 established and 12 newer
source-valid tasks. Zero protocol errors; repeated evidence is deterministic.
Production/driver/host provenance is identical across arms and still matches
current source. Sibling source snapshots are stable; CodeGraph hashes also match
the pre-experiment native-equivalence capture.

Automatic-route evidence is identical across policies on all tasks. Established
metadata-only required-file presence improves 19/34→23/34 and mean region coverage
29.19%→31.30%, but complete-region answers remain 7/34. Newer metadata-only mean
region coverage improves 9.52%→11.28%, with complete-region answers still 1/12.
Its unchanged 8/12 file total hides one gain (`nanus.fresh-legacy-approval.debug`)
and one loss (`whatsurvey.fresh-media-flow.debug`); both have zero labeled-region
coverage under either policy. The actual partial-region gain is on
`whatsurvey.fresh-option-code-collision.debug`.

Decision: retain combined normalization as default; keep native BM25F opt-in.
The factorial, native equivalence and integrated evidence comparison complete
recommendation 14's evaluation without bundling positive IDF or pretending a
candidate-file improvement guarantees complete evidence. Historical/newer labels
are already exposed; no blinded, model-success, RSS or causal timing claim.
The remaining query-policy/body/context and performance recommendations stay open.

[Integrated results, individual tradeoffs and reproduction](results/native-implementation/normalization-context-review.md),
[machine-readable paired comparison](results/native-implementation/normalization-context-comparison.json).
No production code changed during this increment; prior strict lint and targeted
code validation remain applicable. No commit or push yet.


### Conditional adoption audit and corrected concurrency contract

Audited recommendations 16, 17, 26, 27, 28 and 29 against their actual conditional
wording, current code and retained measurements. Recorded decisions and concrete
reopening gates in [CONDITIONAL-DECISIONS.md](CONDITIONAL-DECISIONS.md). Regex and
neural retrieval remain explicitly deferred, satisfying 17/29's conditional
scope without claiming those features exist. Grams, pruning, compression and
segments/concurrency remain open for their specific economics, profiling and
lifecycle checks. Positional timings are not substituted for gram break-even;
packed fact reuse is not called a posting codec; old microbenchmarks are not
presented as current service phase profiles.

Added a literal contract regression for regex-looking punctuation, including an
invalid regex (`[`), in both case modes. All eight text-search tests pass
(`/tmp/conditional-literal-tests-final.log`). The first test placement was outside
its fixture module and failed to compile; it was moved into the proper module,
then the complete text-search test group passed.

Found and corrected the spec's statement that readers do not lock. Read queries
omit the writer file lock, but `Index::store_read` retains a shared in-process
guard through its callback and result materialization. `GraphStore: Send` does
not require `Sync`; the public `Index` does not promise arbitrary multithreaded
shared queries. A compile-fail API doctest passed, and its actual compiler output
specifically identifies the missing `Sync` implementation on `dyn GraphStore`
(`/tmp/concurrency-contract-doc.log`). Owned snapshots, cross-process reader
retention and concurrent workloads remain recommendation 28/30 work, not implied
guarantees of immutable publication.

No runtime algorithm or dependencies changed in this increment. Strict lint and
format/diff validation are recorded below after completion. No commit or push yet.

Final validation for the conditional audit: strict workspace/all-target Clippy
passes (`/tmp/conditional-audit-clippy.log`); formatting and diff checks pass.
The Cargo manifest/lockfile diff remains empty. The eight literal tests and
expected-failure thread-sharing doctest are the relevant behavioral checks;
no broader concurrency or performance completion claim is made.

### Reuse the impact root neighborhood; skip impossible connections

Explore now reads a function's incoming-call neighborhood once for both its
direct caller count and the first step of its transitive impact traversal.
The reuse is local to that traversal, carries the already charged prefix, and
does not reset edge/node allowances. Other neighborhoods still use bounded
adjacency reads. Direct callers remain observable when the node allowance
prevents expansion; total callers retain the existing lower-bound behavior.
Single-seed explore skips connection traversal because there is no pair of
seeds to connect. Ordinary graph traversal retains its existing path.

The public regressions cover exact-fill and exhausted edge limits, zero node
allowance, separate request budgets, self-calls, cycles and converging callers.
All 19 work integration tests pass (`/tmp/graph-impact-reuse-tests-final.log`).
The initial assertions incorrectly counted only matched call edges; adjacency
correctly charges every inspected incident edge, including containment and
opposite-direction calls. Corrected fixtures explicitly account for this:
the one-hop two-caller star examines three edges once; the cyclic four-function
fixture examines 15 incident entries and reports two direct/three total callers.

This increment avoids a generic uncharged adjacency cache, which could hide
repeated per-seed CPU work behind a single edge charge. Cross-seed shared
traversal, intent-specific graph selection and equal-budget graph-quality
ablations remain open under recommendation 22. No dependency changes; no
end-to-end latency or sibling quality claim for this increment. Full validation
is recorded after completion below. No commit or push yet.

Final validation: all 328 workspace tests pass, including documentation tests
(`/tmp/graph-impact-workspace.log`). Strict workspace/all-target Clippy passes
(`/tmp/graph-impact-clippy.log`); formatting and diff checks pass. Cargo manifest
and lockfile changes remain empty. Recommendation 22 and the overall delivery
remain incomplete.

### Shared connection components and explicit graph context

Factored connection-path selection into a native ordinal adjacency structure.
Its union/find component map is built alongside adjacency from the already
budgeted expanded graph. Seeds without a peer in their admitted component skip
BFS; other searches stop after reaching every seed peer in that component.
Sorted node ordinals and original edge order preserve the prior deterministic
shortest-path tie rule. Selected records retain their original edge direction.
Every BFS adjacency examined still consumes the request edge allowance; this
does not turn cached neighborhoods into unlimited per-seed traversal. Partial
input graphs retain the expansion's truncation notices.

An independent copy of the original NodeId-based BFS agrees on all 5,120 cases:
64 simple four-node graphs, 16 seed subsets, and hop limits zero through four,
with alternate input-edge order. Additional fixtures cover filtered nodes,
parallel relations, self-loops, dangling edges, cancellation and exact/exhausted
budgets. The component/early-stop fixture returns its sole connecting edge
with exactly two BFS edge examinations, while isolated seed components do no
BFS work. All 120 core tests pass (`/tmp/connection-components-core.log`).

Added `GraphContext::{Semantic,None,Calls,Imports,Types}` to typed retrieval
options and `--graph-context` to the CLI. Semantic preserves the existing
relationship set and caller impact; Calls restricts connections to call edges
and includes impact; Imports and Types select their relation families without
caller impact; None omits graph enrichment. Lexical seed ranking is independent
of this control. It is included in explained options and defaults correctly
when older serialized options omit it. Runtime ranker policy is now revision
14; persisted parser/source/occurrence formats are unchanged.

Public tests check lexical seed identity, bridge admission, omission of impact,
zero enrichment work, positive/negative relation admission for all seven tested
edge kinds, and direction preservation. All 20 CLI contract tests pass
(`/tmp/graph-context-cli.log`), including all five policy spellings and rejection
of an unknown mode. The ranking-review driver accepts `--graph-context` so the
next controlled sibling comparison can keep lexical/candidate/source budgets
fixed. Driver compilation passes. README and SPEC document the contract.

The exhaustive oracle establishes untruncated path equivalence, not identical
partial results at a tight work cap: saved work can legitimately admit more
evidence. Recommendation 22 remains open for its equal-budget sibling graph
quality comparison; no default relevance or latency improvement is claimed.
No new components or dependencies. Final integration and strict lint results
are recorded below when complete. No commit or push yet.

Final validation: 70 accuracy/body/planner/work integration tests pass
(`/tmp/graph-context-integration.log`), and all 20 CLI contract tests pass.
Strict workspace/all-target Clippy passes (`/tmp/graph-context-clippy-final.log`)
after adopting saturating counter arithmetic and renaming the path method to
match the workspace lint conventions. The three connection-kernel tests pass
again afterward (`/tmp/connection-components-final.log`). Formatting/diff checks
and driver compilation pass; dependencies and lockfiles remain unchanged.

### Equal-budget graph-context ablation

Completed 276 controlled trials: semantic versus no graph enrichment, three
repetitions on 34 established and 12 newer source-valid tasks. There are zero
protocol errors and all repeated evidence dictionaries are deterministic. All
four runs share identical production/driver/host provenance; production and
external sources are stable within and across runs, and the current production
hashes were verified after capture. Sibling CodeGraph directory files also match
their pre-capture checksums. No original source or index was modified.

Established required-file presence stays 28/34 and complete-region delivery
stays 12/34. Removing graph context changes mean coverage 51.06%→51.54%, with
one partial-evidence gain and four losses (all nanus). Newer results remain
12/12 files, 10/12 complete regions and 83.33% mean coverage, with identical
evidence dictionaries. The semantic arm also retains the preceding automatic
baseline's evidence on all 46 tasks. Keep Semantic as default with explicit
policy controls; the results do not establish a universal graph benefit or a
reason to disable graph context globally. Labels have been exposed, arms ran
sequentially, and there is no model-success or causal performance claim.

[Decision, individual effects and reproduction](results/native-implementation/graph-context-review.md),
[machine-readable paired comparison](results/native-implementation/graph-context-comparison.json).
The driver now records graph/normalization overrides and verifies production
hash stability during each capture. This closes the initial equal-budget graph
ablation gate. Recommendation 22 remains open for connection/impact-cone reuse
and broader intent-specific evidence selection. No commit or push yet.

### Native inverted-index completion audit

Audited recommendation 8 against current source, its independent score/heap/
intersection oracles, work-limit contracts and generation lifecycle. The native
ordered dictionary, sorted field-frequency postings, per-document lengths,
sparse OR, shortest-list AND/seek and deterministic bounded heap are implemented.
Both stores own the cache; snapshots borrow it. The prepared generation builds
its final index before publication, and reopen reconstructs from that same
generation. There is no separate persisted posting file with an independent
publication identity.

All 120 current core library tests pass (`/tmp/native-postings-core-audit.log`).
The engine publication/reopen/rename/deletion posting test also passes
(`/tmp/native-postings-lifecycle-audit.log`). The review's proposed port name was
an interface sketch; the existing borrowed MetadataIndex/typed-options/budget
boundary provides that separation. Recommendation 8 is marked complete for its
specified uncompressed baseline, without claiming WAND, bitset adaptation or a
posting codec.

[Requirement-by-requirement audit](NATIVE-INDEX-AUDIT.md).
Recommendation 7 remains open: both stores rebuild the complete metadata and
lexical cache after changes. Per-file posting/statistic maintenance must still
be implemented and compared against this correct full-rebuild baseline. This
audit changes no production code or dependencies. No commit or push yet.

### Incremental metadata posting and statistic maintenance

Added immutable native metadata updates to both adapters. Stable IDs plus exact
scoring-field equality identify reusable documents; a one-to-one claim prevents
duplicate constructor IDs from corrupting ordinal reuse. Only changed scoring
fields are analyzed. Unaffected term lists share their allocation; changed or
reordered lists are rebuilt from retained frequencies plus the analyzed delta.
Integer corpus totals subtract removed/changed documents and add new ones;
averages and DF use the resulting current population. Old generation contents
are unchanged. Exact maps, languages and compact ordering are reconstructed,
so this is not a claim of wholly sublinear sync work. Body/graph/fact maintenance
remains separately scoped.

All 337 final workspace tests pass (`/tmp/lexical-update-workspace-final.log`).
New tests compare complete postings, totals, means and scores with fresh builds,
prove allocation sharing, preserve prior generations, and exercise a 48-step
metadata mutation sequence under both analyzers and both normalization policies.
Workspace/all-target strict Clippy and the release probe's strict Clippy pass
(`/tmp/lexical-update-clippy-complete.log`,
`/tmp/metadata-update-probe-clippy-final.log`). Formatting/diff checks and the
updated storage driver's Python compilation pass. No dependency/format changes.

The final resident probe has 60 alternating-order pairs across six edit patterns
at 1,000 and 50,000 symbols; delta maintenance is faster in all pairs, with exact
candidate/score/order agreement and unchanged old-generation results. At 50,000
symbols, a signature edit has medians 360.42 ms full versus 89.13 ms delta; half
the signatures changing gives 358.56 versus 263.49 ms. These exclude graph/disk
work and do not measure RSS or the earlier non-shared representation's overhead.

An isolated publication control restores full metadata reconstruction in a
disposable checkout. Across seven pairs each, frozen-repository sync has median
paired delta −5 ms (6/7 faster), and 128×64-function synthetic Rust has −50 ms
(7/7 faster). All graph/fact/reopen/no-op checks pass and persisted generation
sizes agree. Production/probe hashes remained stable through capture and still
match afterward. Sibling repositories/indexes were not accessed by these probes.

[Full mechanism, results, limits and reproduction](results/native-implementation/metadata-deltas/README.md).
Recommendation 7 is complete for generation caching and metadata posting/statistic
updates. Global graph reconstruction, selective fact loading, body-index update
economics, memory composition and concurrency remain their own open requirements.
No commit or push yet.

### Controlled Markdown evidence comparison, including documentation

The new public-API `markdown_context.py` experiment builds the current source and
a disposable control differing only in Markdown structural partition selection.
Both use identical ranking, graph context, work limits and evidence-v1 response
budgets. All 348 trials (58 tasks × two arms × three repeats) completed without
protocol errors; evidence repeats are identical and production/binary/sibling/
CodeGraph hashes stayed stable through capture. No source labels were revised
from retrieval results and no third-party component was added.

The established 34 tasks retain 12 complete-evidence tasks in each arm, with one
complete gain and one loss. The newer 12 routing tasks regress from 10 complete
with fixed windows to 9 with structure. The 12 newly frozen README documentation
tasks improve from 2 complete with fixed windows to 8 with structure, and from
2 to 11 all-required-file hits. The README sample is deliberately narrow and
cannot establish general documentation or model-task success. Documentation also
has an individual loss (blogwright initial setup, 100% to 75% required lines).

Retain structural source representation; reject a claim of universal ranking gain.
The code losses are inspected: documentation displaces implementation from the
three follow-up reads. Ranking and context gates remain open, including separated
regions inside the same document. Recommendation 25's equal-budget comparison is
now measured; overall recommendation completion still awaits its final source/
dialect audit. Recommendations 9–12 and the release gate are not closed by this
mixed result. [Full results and limits](results/native-implementation/markdown-context-docs/README.md).

Added a public persistence regression for identical documentation at two version
paths. Both occurrences retain distinct file IDs and byte-correct evidence despite
sharing content hashes. Editing one version, synchronizing/reopening, then deleting
the other preserves the surviving version and removes only the deleted occurrence.
All 11 Markdown integration tests and strict Clippy for that target pass
(`/tmp/markdown-doc-versions.log`, `/tmp/markdown-doc-versions-clippy.log`).
The native Markdown-filtered core tests also pass (`/tmp/markdown-dialect-audit.log`).
The test uses distinct single-token markers: the initial camel-case markers shared
an analyzed term and correctly matched both versions under OR semantics; that test
assumption was corrected without changing production search.

### Markdown requirement audit completed

[Requirement-by-requirement audit](MARKDOWN-AUDIT.md) closes recommendation 25
for the explicitly declared native dialect, bounded authored metadata/context,
version/path identity and controlled fixed-window comparison. The mixed quality
results remain visible; recommendations 9–12 and the phase-2 release gate stay
open. Full CommonMark/MDX rendering is not silently claimed.

### Explicit task-prompt query policy

Added opt-in `QueryPolicy::Task` to library retrieval options and CLI
`--query-policy task`. Default `Verbatim` is unchanged. Auto/Terms remove only
standalone terminal sentences from the exact documented procedural vocabulary;
original input remains in the plan and omissions are recorded in source order.
A single bounded pass marks quoted prefixes so cleanup cannot consume instructions
inside unfinished quoted strings/code, and repeated suffixes do not cause repeated
full-prefix quote scans. Exact ID/name/prefix/path, phrase and near routes bypass
cleanup; raw text search is unaffected. The original input still governs inferred
navigation and automatic channel selection. Ranker policy identity is now 15;
source/parser/storage formats and dependency manifests are unchanged.

Core fixtures cover suffix boundaries, case preservation, Unicode, contractions,
quoted/unfinished inputs and route bypass. The public planner test verifies
opt-in behavior, AND semantics, both analyzers, original-query/omission provenance,
and protected exact/phrase modes. A CLI contract test verifies field propagation
and explain output. Quality comparisons are still required before considering a
default change; recommendation 10 remains open for its remaining typed-route audit
and evaluation. This is a deterministic declared policy, not inferred query intent
or arbitrary natural-language rewriting.

Validation: the workspace run passed 360 tests (`/tmp/query-policy-workspace.log`),
including 140 core tests, 12 planner integration tests and 21 CLI contracts. The
CLI option was made a compact typed enum after lint detected the extra String's
large-enum cost; its dedicated contract passed (`/tmp/query-policy-cli.log`). A
subsequent review extended quote protection to matching-length backtick runs;
both focused policy tests pass on that final implementation, including unfinished
multi-backtick and embedded-shorter-delimiter cases
(`/tmp/query-policy-quoted-tests.log`). Final workspace/all-target strict Clippy
passes (`/tmp/query-policy-clippy-final-ticks.log`), as do format/diff checks and
Python compilation/help for the driver. The full-suite count precedes that last
focused quote correction; it is not mislabeled as another final full run.
`ranking_review.py --query-policy verbatim|task` now supports controlled external
measurement and records the override; that quality experiment is still pending.

### Controlled task-prompt policy comparison

Completed 348 public-API trials: established 34 tasks, newer routing 12 and README
12, both policies, three repeats. Same binary/production hashes across all six
captures; all source/production checks pass, sibling CodeGraph files remain equal
to the preceding capture, zero protocol errors, repeat-stable evidence. On the
established set, task cleanup improves all-required-file coverage 28→30/34 and
complete evidence 12→14/34; mean region coverage rises 50.51%→56.76%. Eight tasks
gain mean region coverage and four lose it. Newer routing and README tasks have
identical evidence and response bytes, because their prompts contain none of the
removable suffixes. This is noninterference, not independent cleanup-quality gain.

Retain verbatim default and explicit task policy. Inspected losses include edit-tool
candidate displacement and competing read.rs regions. Two new complete tasks are
grep behavior and webhook signature verification; the latter gains its missing
evidence in initial explore despite unchanged follow-up reads. No model-success
claim, default change, suffix tuning or label revision follows from this experiment.
[Full comparison, losses, provenance checks and reproduction](results/native-implementation/task-policy-comparison/README.md).
Recommendation 10's procedural-policy experiment is complete; its remaining typed
route audit and the broader candidate/context release gates remain open.

### Query-planner requirement audit completed

[Requirement-by-requirement audit](QUERY-PLANNER-AUDIT.md) verifies typed exact,
path, raw-literal, ranked, positional and relationship forms, protected intent,
observable routing, Boolean requirements, shortest-posting prioritization and
the measured optional task policy. Recommendation 10 is complete. Literal and
graph operations keep their separate typed public methods; no new ambiguous
string query language was introduced. Candidate/context quality and fresh broad
evaluation remain open under 11/12/30, including documented regressions.

### Parser-owned documentation facts (recommendation 9, in progress)

Added native documentation-fact enrichment over the existing Rust and JS/TS syntax
trees, without dependencies or raw-text comment guessing. Facts retain original
spans, inner/outer style and conservative file-local declaration keys. Adjacent
comment groups share association lookups. Expression statements, ambiguous
multi-declarations/destructuring and duplicate declaration keys do not acquire an
arbitrary owner. The per-file 8,192-fact cap reports actual omission; raw body text
continues to be indexed. Merging orders/deduplicates facts and preserves cap loss.
Parser version is 6; source-unit/chunker/storage representations are unchanged.

This increment supplies persisted raw facts, not separate doc-comment retrieval
units. Recommendation 9 remains open for source partitioning, association in
retrieval evidence, public coverage reporting and controlled representation
comparison. The source-unit validator still enforces containment for ordinary
body owners; future integration must not weaken that invariant to attach comments
preceding declarations. No graph relationship is inferred from doc tags.

Integration constraints for the next increment:

- Keep lexical `owner` as physical containment. Add a separate documented-declaration
  association for preceding comments, validated against the same file/generation.
- Partition original comment spans into source units rather than copying comment
  terms into declaration metadata. This avoids counting authored text twice.
- Preserve raw bytes and bounded fragments for long comments; expose actual
  documentation-fact truncation independently of body-text coverage.
- Keep unparsed/live-overlay text searchable without claiming parser-derived
  documentation associations. Bump source/chunker policy only with integration.
- Evaluate candidate and delivered-region changes separately against fixed body
  windows; parser-fact persistence alone supplies no retrieval-quality result.

Validation of the parser-fact increment: the complete offline, locked workspace
suite passes (366 tests, zero failures/ignored), including the raw-fact pack/reopen
and incremental-versus-rebuild regression. Strict workspace/all-targets Clippy,
formatting and whitespace checks pass. Cargo manifests and lockfiles are unchanged.
These checks establish parser/persistence behavior, not a new retrieval-quality or
latency result; existing sibling-repository captures predate this increment.

### Documentation source retrieval integration (recommendation 9, in progress)

Parser facts now partition original source into dedicated documentation units.
Metadata holds the complete comment span and a separate documented-declaration
identity; lexical owners retain physical containment. Long comments use existing
bounded overlapping windows. No authored terms are copied into declaration fields.
Body candidate grouping and ranked evidence use the documented declaration where
available. Whole-file phrase/near verification retains its original-byte semantics
and associates only witnesses wholly inside a known comment. Live overlays make
no parser-owned association claims. Source representation is 8, chunker policy 9,
and ranker policy 16.

Publication validation covers target identity/path, inner containment/outer order,
fragment containment, shared-descriptor consistency and disjoint comment spans.
The independent public documentation-metadata coverage counter survives the same
source record lifecycle. The previous raw-facts-only limitation is superseded by
this integration; recommendation 9 remains open pending controlled representation
and delivered-context evaluation.

Adjacent same-association/style documentation comments separated only by whitespace
share a source region, preserving all-term queries across Rust line documentation.
Raw parser facts retain individual occurrences. Package association remains an
explicit outstanding dependency on recommendation 21; evaluation alone cannot
close every clause of recommendation 9.

Integration verification: the workspace run passes 368 tests. That run predates
the final adjacent-comment grouping and empty-source adjustment; the final two
core documentation tests and both public documentation integration tests pass
separately, including grouped all-term queries, phrase associations, live fallback,
reopen and source-record incremental/rebuild equality. Final strict workspace /
all-targets Clippy, formatting, Python driver compilation and whitespace checks
pass. No dependency manifests/lockfiles changed. Controlled code-region evaluation
uses an explicit two-edit disposable control: disable declaration boundaries and
comment boundaries/associations, retain Markdown and the current 80/8 windows.
This is a controlled structure comparison, not an exact replay of the historical
non-overlapping candidate-only prototype.

Controlled representation evidence is now captured in
[code-context](results/native-implementation/code-context/README.md) and
[documentation-context](results/native-implementation/documentation-context/README.md):
696 total trials, each comparison 58 tasks × two arms × three repeats. All runs
complete without errors; source, binary and CodeGraph-index stability pass; repeats
have identical evidence and bytes. The structured binary/results match across
captures. The code-structure arm loses established-task completion (12/34 versus
14/34) but gains newer-routing completion (10/12 versus 8/12). Isolated documentation
segmentation keeps established completion at 12/34 and improves routing 9/12→10/12,
while losing one README required-file hit. Per-task and initial/follow-up analyses
retain counterexamples rather than treating aggregate gains as universal quality.
The controlled representation gate has been exercised; package association and
candidate/context release gates remain open. No model task-success claim follows.

### Native package context (recommendations 9/21, in progress)

Standard Index extraction now decodes Cargo/package.json syntax through existing
library dependencies and a core registry port, retaining bounded native boundary
facts. Core selects the nearest compatible package, distinguishes virtual Cargo
workspaces, preserves manifest path/hash identity and reports ambiguous or
unavailable scope. Source records and ranked/positional evidence expose package
context; live replacements omit unverified associations. No dependency manifests
or lockfiles changed. Parser policy is 7 and source representation is 9.

Manifest presence is recorded before the walk's size filter and persisted in the
manifest header. Oversized manifests can block outer inheritance without being
read/indexed, and creation/removal invalidates freshness. Package changes currently
rebind all admitted files from cached extraction/source facts. Post-batch source
validation now includes cross-file manifest targets and rejects deletion/hash
changes that would leave retained package evidence inconsistent, before mutation
in either adapter. Ordinary lexical owners remain physically file-local.

This supplies source-unit package association. It does not complete recommendation
21: module/target trees, dependency and import resolution, workspace membership,
path aliases/reexports, receivers and embedded-framework coverage remain open.


Package-context verification now passes all 373 workspace tests, including the
three public package lifecycle tests, parser bounds, and both adapters' atomic
cross-file validation. Strict workspace/all-target Clippy and formatting pass.
The [source-unit audit](SOURCE-UNIT-AUDIT.md) closes recommendation 9 within its
declared contracts. Recommendation 21 remains open.

The [source-version-9 comparison](results/native-implementation/code-context-packages/README.md)
completed 348 trials with stable sources/binaries/indexes and repeat-identical
source evidence. Three groups vary by one response byte solely from elapsed-time
telemetry; normalized transcripts are identical. Structured/fixed completion is
12/14 of 34 established tasks, 10/8 of 12 routing tasks, and 8/8 of 12 README tasks.
Five structured tasks lose partial context relative to source version 8 while
retaining identical candidate headers and follow-up actions. These regressions
remain explicit work under 11/12/30; there is no universal quality or model-success
claim. No dependency manifests or lockfiles changed. Commit and push remain pending
completion of the remaining recommendations and final delivery gates.


### Package-budget isolation and residual source fragments (recommendation 12)

The [isolated package-metadata control](results/native-implementation/package-metadata-budget/README.md)
changes only returned package identity in a disposable build. All 348 trials
complete; evidence repeats and source/binary/index stability pass. Its control
reproduces the previous source-version-8 evidence for all 58 tasks, restoring all
five partial-context losses. This isolates metadata budget pressure without
changing ranking, source boundaries or package indexing.

A tested candidate retained identity and offered bounded residual fragments when
a complete structural interval could not fit. The initial fragment priority
failed the existing distant-match regression and was corrected. The corrected
candidate passed all 374 workspace tests and strict Clippy, including a new
synthetic test for bounded fragments, suffix matches and exact source provenance.

The [controlled fragment comparison](results/native-implementation/context-fragments/README.md)
then completed 348 trials: all delivered source-line sets and evidence scores
were identical between arms, while 44 of 58 tasks spent additional context-window
work. The candidate is therefore **withdrawn from production**, preserved as an
experimental patch with its evidence. Production was restored to ranker policy 16. No
package metadata is discarded; recommendation 12 remains open.

A public-API follow-up (compact JSON reconstruction, not literal wire capture)
on the five affected tasks shows package identities occupying 1,116–1,544 bytes per response, with total responses at 16,240–16,347 bytes
under the 16,384-byte cap. Small excerpts already consume the residual capacity;
extra splitting is not a useful remedy on these tasks. The next intervention is
shared package metadata with explicit references, preserving full identity and
provenance while avoiding repeated serialization. Its compatibility and byte-budget
contracts require implementation and verification before any quality claim.


### Shared result-local package identity (recommendation 12)

Repeated identities among selected seeds now share one `context.packages` entry.
Evidence has either its legacy inline `package` or a result-local `package_ref`;
`SourceEvidence::package_identity(context)` resolves either form and refuses
dangling/contradictory associations. Equality includes full manifest path, hash,
ecosystem and authored name. Single-use identities remain inline initially;
shared keys survive result trimming without renumbering, and unused entries are
removed by both library and CLI envelope budget fitting.

The table is built before candidate payload admission and source-context allocation,
so savings can fund source evidence rather than merely shrinking a completed
response. Persisted source facts remain unchanged. Wire contract is 4 and ranker
policy 18; ranker 17 belongs only to the withdrawn fragment experiment.

Public package tests pass for ranked and phrase retrieval at several byte limits,
with JSON round trips and resolvable identities. Strict workspace/all-target Clippy
passes. All 376 workspace tests pass (31 suites, no failures or ignored tests).
The [isolated sharing comparison](results/native-implementation/package-sharing/README.md)
completes 348 trials with stable sources/binaries/indexes and repeat-identical source
evidence/actions. It restores the previous coverage of `nanus.grep.change` r2
(70.83%→100%) and `whatsurvey.contact-policy.debug` r3 (20.97%→56.45%), without a
measured coverage loss elsewhere. Complete-task totals do not change; three
package-budget losses remain. The raw API differential covers 116 responses and
464 common nodes: 430 known identities and 34 absent associations agree, all
references resolve, and no orphan entries survive. Reconstructed package-field savings average 736.64 bytes/query
(57 improvements, one unchanged; no latency or model-success claim).

The same raw accounting locates primary-snippet overlap on three tasks: storage
envelope, invalid-date/rkey, and Markdown stacks. It occurs in both arms and is
unchanged by sharing. This supplies concrete remaining deduplication work under
recommendation 12.
No dependency manifests/lockfiles changed. Recommendation 12 and final delivery
remain open pending the remaining implementation and evidence gates.


### Primary source overlap removal (recommendation 12)

The source allocator now accounts for primary snippets globally by path, captured
hash and line. It preserves unique runs and all candidate metadata, while keeping
fully shared items eligible for distinct context. A separate retention step honors
ordinary payload eviction, preventing that eligibility from undoing byte-budget
choices. Split fragments share the existing interval cap, and rejected fragments
are not marked delivered. Ranker policy is 19; wire 4 and source 9 remain unchanged.

All 380 workspace tests pass (31 suites, no failures or ignored tests), as does
strict workspace/all-target Clippy. Four new core cases cover exact source union,
path/hash distinctions, fully shared primary eligibility, payload suppression and
interval-cap recovery. The public work-budget test verifies unique source lines
and retained candidates without changing source-read accounting.

The [controlled comparison](results/native-implementation/primary-dedup/README.md)
completes 348 stable trials. All 58 tasks retain their file/region coverage; repeats
have identical source evidence and actions, and the control reproduces the prior
package-sharing evidence. Raw validation covers 116 responses and 464 common nodes
with preserved package identities, resolvable references and no orphan entries.
Four duplicate coordinates across the three observed overlap tasks become zero.
No model-success or latency claim is made. Recommendation 12 remains open until
its remaining requirements are audited; existing package-budget losses remain.

### Share source captures across explore phases (recommendation 12)

The context audit found that the evidence cache did not cover preceding strict
freshness and automatic-maintenance reads. Public `explore` now enables a bounded
raw capture map in its request work budget before freshness, and transfers the same
budget into query execution. Freshness, maintenance and evidence share the reader.
Physical reads are charged once; text materialization retains its separate capacity.
Metadata drift fails closed, larger requests cannot mistake captured prefixes for
complete source, and binary/invalid UTF-8 captures remain usable for verification.
Other public modes retain their existing accounting. Ranker policy is 20 because
budget-limited explore responses can now deliver source previously displaced by
repeated verification reads. Wire/source/parser contracts are unchanged.

The [requirement audit](CONTEXT-SELECTION-AUDIT.md) distinguishes implemented
contracts from remaining proximity, initial materialization and model-success
measurement work. The 387-test workspace run passes (31 suites); final focused
validation after the last guards passes 152 core unit tests and 21 public work-budget
tests. Strict workspace/all-target and research-probe Clippy checks pass.

The [strict source-capture comparison](results/native-implementation/source-capture/README.md)
completes 54 trials over nine queries in the three sibling repositories. All returned
source lines/hashes validate, no coordinates duplicate, normalized results are
identical across arms, and repetitions/source/index/binary snapshots remain stable.
Source bytes read fall by 50.58–55.01%; file opens fall from 333–336 to 164 in nanus,
345–347 to 169 in blogwright, and 2,493–2,496 to 1,238 in whatsurvey. These are source
work counters, not device-I/O, latency, RSS or model-success claims. Capture retention
adds bounded request memory; its RSS cost remains to be measured under recommendation
27. No dependency manifests or lockfiles changed. Recommendation 12 and final delivery
remain open for the audit's remaining requirements.


### Admit source reads against necessary payload bounds (recommendation 12)

Initial item assembly now measures metadata before reading source. It skips reads
for metadata that already fails the ranked item allowance, and for primaries whose
minimum encoded growth cannot fit either that allowance or mandatory response
metadata with the retained item prefix. The response floor excludes optional data
and prior source text so overlap removal cannot invalidate an early rejection.
Cancellation is checked per candidate, and omitted reads report byte truncation.
Ranker policy is 21; wire/source/parser versions are unchanged.

The new regression gives two candidates a one-file source allowance: oversized
metadata on the first no longer starves a later fitting candidate. It also checks
the mandatory-response floor, exact final payload limits, absence of invented
source-work exhaustion, and successful source delivery when the cap is raised.
All 388 workspace tests pass (31 suites), as does strict workspace/all-target
Clippy. No dependency manifests or lockfiles changed.

The [controlled comparison](results/native-implementation/source-admission/README.md)
completes 348 stable trials with repeat-identical source lines, actions and evidence.
Delivered line sets, file recall and region coverage are unchanged on all 58 tasks;
the control reproduces the preceding primary-dedup oracle evidence. Raw validation
covers 116 responses and 464 common nodes, with unchanged selected candidates,
source-work counts and package associations; all references resolve without orphan
entries. Thus the corpus at 16 KiB establishes non-regression, not fewer reads or a
general speedup. The tight-budget regression demonstrates the saved-read behavior.
No RSS or model-success claim is made. The [context audit](CONTEXT-SELECTION-AUDIT.md)
records the admission argument and remaining recommendation 12 work.

### Bounded context proximity (recommendation 12, retained)

A native sliding-window calculation measures the shortest inclusive line span
covering all distinct, not-yet-delivered term bits in a source candidate. For at
least two new terms it contributes `floor(80 / width)` fixed-point units, alongside
`160 * (1 + new_term_count)` units of existing coverage utility. The role/rank prior
and marginal-byte denominator remain unchanged. At equal prior/cost an additional
new term always outranks the maximum proximity contribution. Repeated occurrences
can tighten the minimum span but cannot increase distinct coverage. After the term
bits are delivered, their proximity contribution disappears.

The calculation uses existing verified line masks, with at most 80 lines and 128
term bits per candidate. Single-line complete covers and two-line minimum covers
terminate early. No persisted postings/positions, source/parser/wire changes or
third-party components are introduced. Candidate ranker policy is 22. Exact token
phrase/proximity predicates retain their existing behavior.

The supplied research's distinction between unigram coverage and ordered/unordered
term dependencies motivates this separate local feature. Its fixed-point formula
is a native context-packing heuristic, not a reproduction of a trained document
ranking model. The weight is fixed before the corpus comparison, not fitted to its
labels. Tests pass against 65,536 exhaustive mask/spacing cases, high-bit/full masks,
coordinate translation, repetition and post-delivery masking. An allocator test
checks compactness, coverage precedence and restoration of deterministic ties.
The workspace passed 391 tests and strict all-target Clippy. The final core suite
passed 155 tests after the equivalent early-return optimization. The
[controlled comparison](results/native-implementation/context-proximity/README.md)
completed 348 stable trials: all 58 tasks retain their labelled evidence metrics,
and the control reproduces the previous source-admission capture. Repeats have
identical actions and delivered lines; paired actions are also identical. Four
full-protocol line sets differ, with every added/removed coordinate recorded.
The raw first-query probe validates 116 responses and 464 common node identities;
selected order and source work are unchanged, package references resolve, and no
orphan package entries remain. Context-window counts decrease from 34,630 to
34,553, but exclude the added proximity calculations and imply no CPU speedup.

Retained as ranker 22 to provide the oracle-tested bounded compactness preference.
The corpus shows no labelled-evidence gain or loss, and does not establish that
unlabelled substitutions improve answers. Model answer/patch success remains an
open independent gate under recommendation 30. No dependency changes.

### Share budget-admitted graph neighborhoods (recommendation 22, retained)

Ranker policy 23 adds a request-local incident-edge cache to `QueryEngine`. The
first read captures the adapter's budget-admitted, deterministically ordered
incident prefix before direction/kind filtering. Connection discovery and incoming
impact traversal can then select their own views of the same prefix. Both native
adapters already charge incident entries before filtering, so the first-read budget
boundary is preserved. Cache reuse does not re-charge snapshot adjacency reads;
connecting-path work retains its existing separate charges. Cancellation/deadline
checks still run on every cached selection. This is adjacency reuse, not memoization
of complete impact cones or a claim that cached filtering performs no CPU work.

Only nonempty neighborhoods are retained; every retained edge corresponds to a
charged entry and the number of cache keys cannot exceed retained entries. Global
work truncation remains visible for partial prefixes. Cache state is cleared at
query start, including engines created with inherited freshness work. The snapshot
is immutable for the request. No cross-request or cross-generation reuse occurs.

Core tests compare every subset of four edge kinds and all three directions with
the snapshot reference, including self-loops and unresolved edges. Partial-prefix,
empty-key retention, reset and cancellation cases pass. The full workspace passed
393 tests across 31 suites, including the new Memory/Grafeo public-query conformance
check. Strict workspace/all-target Clippy, formatting and whitespace checks pass.
Dependency manifests and lockfiles remain unchanged. The [paired capture](results/native-implementation/graph-neighborhoods/README.md)
completed 348 stable trials with identical delivered lines, labelled evidence and
actions. All 696 paired tool responses agree after documented generation/work/time
normalization, including three separately checked pre-existing truncated metadata
tails. The raw 116-response probe preserves all 464 common-node identities and all
captured items/truncations/other work counters. Graph-entry work decreases in 55 of
58 queries, with no increases: 41.99% for nanus, 24.35% for blogwright and 12.11% for
whatsurvey in aggregate. Retained as ranker 23; no latency/RSS/model-success claim.
The driver now automatically checks repeat determinism and saves paired evidence
and source-line deltas. Its checks reproduce the preceding proximity audit exactly. Recommendation 22
remains open for the broader traversal-economics and graph-quality requirements
listed in `GRAPH-REUSE-AUDIT.md`.

### Preserve API item metadata in the evaluation consumer (recommendations 22/30)

A graph-context ablation exposed a protocol omission: the evidence renderer kept
source lines and top-level edges/context but discarded per-item identity, impact,
retrieval provenance and source-evidence fields. It therefore charged graph impact
against the API byte allowance without showing it to the consumer. Historical
source-coverage results retain their original meaning, but cannot establish the
consumer benefit of those omitted fields or model task success.

The final renderer delivers compact native API JSON directly, preserving every
returned field without duplicating source text or expanding each line with path
prefixes. Candidate discovery reads item identities from the structured response;
evidence accounting reads only actual primary/excerpt lines, verifies source
coordinates/content, and checks a supplied source hash against current raw bytes.
Truncated JSON earns no candidate or source evidence. Legacy text/CodeGraph source
formats remain supported. Rendering neither reads source nor mutates API values.
All 28 evaluation tests pass, including metadata preservation, hash drift, source
gaps, no extra renderer reads, input immutability and exact/one-byte-short transport
budgets. No native API, ranker, dependency or storage change is introduced.

Two diagnostic captures are retained with explicit limitations: `graph-context`
used the historical metadata-omitting projection; `graph-context-metadata` added
per-item metadata but source-line expansion caused all 58 first-query top-level
metadata footers in each arm to be transport-truncated. Neither supports a default
policy decision about complete graph information. The final
[graph-context native-JSON capture](results/native-implementation/graph-context-native-json/README.md)
completed 348 stable trials without errors or any transport-truncated first-query
response (maximum 16,349 bytes). All selected nodes, node/source/retrieval metadata
and 464 common-node package associations match. Graph mode returns 36 edges and
215 impact summaries across 58 first queries. Complete-region and file counts are
unchanged; one task has lower partial coverage (1/6 versus 3/6), with no measured
labelled gains. Source-site coordinates are included for 27 of 36 returned edges,
a separate inclusion diagnostic rather than a precision or answer-success score.
Both explicit modes and the existing default are retained. Recommendation 22's
same-candidate/equal-budget comparison is now recorded; independent graph precision,
answer usefulness and traversal-economics requirements remain open.

### Native trigram adoption gate (recommendation 16)

Implemented a native research prototype with distinct byte grams, sorted postings,
reverse replacement/removal facts, conservative missing/dirty/new-file admission,
and raw verification. A disposable path-admission hook compares the unchanged
production scanner under identical matching and result-budget semantics. All 45
corpus hit lists agree; finite exhaustive and mutation/cap/Unicode tests pass.
No production index, dependency, API or policy-version change is introduced.

The [cumulative experiment](results/native-implementation/trigram-cumulative/README.md)
measures builds, logical forward/reverse bytes, median-file replacement/restore,
selectivity, source reads and repeated queries across all three sibling repositories.
Building an index plus 16 or 32 trusted-snapshot queries beats direct scanning on
the sampled long literal in each repository. Straightforward strict verification
loses at 1, 4, 16 and 32 queries. Its matched-file rereads are explicit; it is not a
lower bound on a future shared-capture design. Corpus and CodeGraph hashes stay
stable throughout measurement. The prototype and all exact instrumentation are
archived; no source or index writes occur in sibling repositories.

Conditional decision: retain direct live scanning. The data supports a future
trusted immutable-snapshot route, but does not justify weakening the current live
source contract or claiming an unmeasured strict speedup. Recommendation 16 closes
as a measured deferral, with explicit reopening conditions in the conditional
ledger. This does not advertise shipped trigram search. Broader implementation and
final delivery remain incomplete.

Validation follow-up: the new trigram tests and the existing scoring oracle test
pass. A range-index loop in the research scoring probe was replaced with an
equivalent zipped accumulation; its bitwise native-scorer comparison passes. Strict
Clippy now passes across all research-harness targets. Formatting, whitespace and
unchanged dependency-manifest checks pass.

### Current phase profile and score-pruning decision

- Added a disposable-build phase driver with buffered inclusive timers; production
  source and dependency manifests are unchanged. Across 103 queries, 618 measured
  calls preserve complete results after generation/elapsed normalization.
- The [profile](results/native-implementation/retrieval-phases/README.md) covers
  58 task prompts and 45 explicit broad Auto/Metadata/Body probes. Median posting
  evaluation consumes 4.19–10.08% of service time for tasks and 0.87–1.45% for broad
  probes. No sampled query is posting-dominated; observed instrumentation/noise
  and inclusive timing limits are recorded.
- Recommendation 26 closes as a measured conditional deferral, not a shipped WAND
  feature. Larger-workload profiling and exact bound/update proofs are reopening
  gates. Repeated freshness/context inspection and remaining seed preparation
  are stronger measured follow-up targets. All sibling/index/source/binary
  stability checks pass; remaining implementation and delivery gates stay open.

### One complete request freshness observation

- Phase profiling exposed repeated full inspections. `SearchService::prepare_context`
  now shares one complete observation between reconciliation and result context
  across graph, occurrences, impact and explore. A maintenance publication discards
  the prior observation and triggers a new inspection; nothing persists across
  requests. Independent source-evidence verification and work accounting remain.
- A public regression covers exact single-walk/single-read budgets, all four paths,
  repeated requests, restored-mtime edits, incomplete allowances and cancellation.
  The prior shared-budget regression now checks the new exact boundary.
- [618 paired release calls](results/native-implementation/request-inspection/README.md)
  preserve all results except generation/elapsed fields. All 103 query medians are
  faster; median paired task-query gains are 14.89%, 18.48% and 26.45% on nanus,
  blogwright and whatsurvey. These are warm adapter wall times, not cold/concurrent
  guarantees. The [certificate](REQUEST-INSPECTION-AUDIT.md) states lifecycle limits.
- Validation: all 394 workspace tests (31 suites), strict workspace/all-target lint
  and 28 Python evaluation tests pass. Dependencies and representation versions
  remain unchanged. Remaining numbered requirements and commit/push gates stay open.

### Qualified calls respect local callable shadows

- Reproduced a public-API false edge: a local function `Api` was bypassed for
  `Api.send()`, allowing it to resolve to an unrelated outer class method. The
  native scope pass now distinguishes namespace declarations from callable values
  and reports unknown local members as unresolved with explicit occurrence reason.
- JS and TS function/const-arrow cases pass alongside an unshadowed static-member
  control and Rust type/function namespace guard. Shadow insertion/removal and
  replacement after reopen produce the same occurrence records as clean rebuilds.
- Parser revision 8 invalidates previously cached binding facts. No dependency,
  wire, source or ranker changes. [Scope audit](SCOPE-RESOLUTION-AUDIT.md) preserves
  the exact contract and remaining namespace/receiver precision requirements.
- All 397 workspace tests (31 suites), strict workspace/all-target Clippy, formatting
  and diff checks pass. Recommendation 20 remains open; no repository-wide target
  precision or model-success claim is inferred from these fixtures.

### Direct static members follow the visible class

- Reproduced a second scope false target: a nested class's `Api.send()` resolved
  to outer `Api.send`. Native class/member lookup now selects the visible class
  binding and a unique directly declared static callable, preserving its exact
  extraction key and `ExplicitLexical` occurrence class.
- Missing, ambiguous, instance-only and accessor members cannot fall through to
  an outer class. Class initialization is guarded; method parameters/bodies have
  a deferred-execution exception, while unsupported initializer contexts remain
  explicit. Inheritance and arbitrary property dataflow are not guessed.
- A new regression also exposed dropped JavaScript class-field initializers. The
  adapter now accepts the grammar's `property` field alongside TypeScript's `name`,
  emitting the field and walking its source references. Parser revision 9 forces
  cached-fact refresh; wire/source/ranker versions and dependencies stay unchanged.
- All 12 scope tests pass, including JS/TS target identities and a five-state
  sync/reopen sequence whose full occurrence records equal clean rebuilds.
  Full validation: 400 workspace tests (31 suites), strict all-target Clippy,
  formatting and diff checks. [Captured evidence](results/native-implementation/class-static-bindings/checks.json).
- The [scope audit](SCOPE-RESOLUTION-AUDIT.md) keeps broader module/import/visibility
  and receiver requirements open. No repository-wide precision or task-success
  claim follows from these local syntactic fixtures.

### Bounded authored Cargo target metadata

- Source representation 10 adds explicit Cargo target tables, optional names/paths,
  feature gates and edition/discovery settings, preserving omissions and authored
  path bytes. Extraction uses the existing TOML decoder, with no new dependencies
  or filesystem reads. Virtual workspaces and Node manifests omit these facts.
- Target count/string/feature bounds apply within the existing manifest byte cap.
  Invalid target metadata becomes an entirely unavailable projection without
  discarding the valid package boundary. Independent persisted-fact validation
  rejects malformed/partial records; legacy absent target facts remain readable.
- Tests cover all target families, inherited edition, exact/over limits, invalid
  field types, NUL, real TOML adapters, legacy decoding and four edit states across
  sync/reopen versus clean rebuild. All 405 workspace tests across 31 suites pass.
  Final strict workspace/all-target Clippy, formatting and whitespace checks pass;
  post-test changes are style-only. [Evidence](results/native-implementation/cargo-targets/checks.json).
- Recommendation 21 remains open. This increment supplies authored facts for later
  target-owned module resolution; it does not discover targets, resolve workspace
  inheritance, choose active features or correct current Rust import edges. The
  [scope audit](SCOPE-RESOLUTION-AUDIT.md) records that boundary. No ranking,
  performance, graph-precision or model-success claim is made.

### Native Rust module-declaration directory contexts

- External `mod name;` declarations now resolve from native Cargo target roots and
  source-backed directory contexts. Supported roots include custom targets,
  edition/family-specific autodiscovery and build scripts. Source revision 11
  preserves explicit empty target families and authored build-script settings.
- Ordinary modules and `#[path]` modules propagate their different physical
  directory rules. A visited worklist allows at most two contexts per walked file,
  terminating cycles and diamonds. Every reached context must agree on the target;
  filename conflicts and context disagreement remain explicitly unresolved.
- Parser revision 11 tags raw module declarations independently of generic imports.
  Exact source spans select their symbol projections; missing projections cannot
  fall through to generic import guessing. Unsupported attributes, file-level inner
  path overrides and orphan contexts retain explicit reasons instead of false edges.
- Incremental updates rebind cached external-module consumers when Rust inputs or
  walked-file presence changes, including nonstandard `#[path]` extensions. Public
  tests compare complete occurrence records after mutation/reopen with clean builds.
  An initial overly broad all-Rust invalidation failed an existing selective-update
  test; the corrected implementation preserves unrelated files.
- An independent disposable offline Cargo oracle covers 25 manifest/layout cases;
  a Rust compiler fixture uses wrong-path compile-error decoys. Production invokes
  neither tool. No third-party components or dependencies were added.
- Final validation: 418 workspace tests across 32 suites, strict all-target Clippy,
  formatting, whitespace and matching source hashes. [Evidence and limitations](results/native-implementation/rust-modules/README.md).
- Recommendations 20/21/23 remain open: logical module identities, `use` aliases,
  reexports, visibility, qualified calls and complete dependency invalidation are
  not delivered by physical module-path resolution. No corpus-wide precision,
  latency, memory or answer-success improvement is claimed.

### Remove exponential Rust ancestor-key duplication

- A bounded source fixture reproduced exponential extraction-key growth: 184
  source bytes at nesting depth 12 generated 245,634 symbol-key bytes. Rust's
  qualifier prepended every ancestor key even though each already contained its
  ancestry. It now prepends only the immediate parent, yielding 4,438 bytes for
  that fixture; depth 16 produces 8,862 key bytes from 232 source bytes.
- Regression checks preserve parent links, call ownership, duplicate-key groups
  and distinct parent namespaces. Parser revision 12 refreshes cached facts;
  source/ranker versions and dependencies remain unchanged. This removes a
  concrete storage amplification, not all deep-nesting or allocation costs.
- [Reproduction and verification argument](results/native-implementation/rust-scope-keys/README.md).
  All 420 workspace tests across 32 suites pass; strict Clippy, formatting and
  whitespace checks pass. Broader recommendations remain open.

### Stop unqualified calls crossing Rust module boundaries

- Reproduced a public graph false edge: an unqualified `send()` in a child module
  targeted the parent's `send` without an import. An independent compiler probe
  rejects that source and accepts local or explicitly qualified variants.
- Native binding lookup checks the nearest module and stops with an explicit
  reason when no binding exists. The unresolved marker also blocks generic
  bare-name fallback. Parser revision 13 refreshes cached extraction facts;
  source/ranker versions and dependencies remain unchanged.
- All 14 scope integration tests pass, including add/remove-local-binding states
  whose complete occurrence records match clean rebuilds after reopen.
  [Evidence](results/native-implementation/rust-module-boundary/README.md).
- Recommendations 20/21 remain open: imported names, namespaces, logical module
  ownership and qualified cross-module paths still need native modeling. This
  fix prevents a false lexical edge; it does not claim complete import recall.

### Preserve authored Rust visibility and verify import-shadow precedence

- A regression showed all explicit Rust visibility modifiers were discarded:
  extraction queried a nonexistent grammar field. It now reads the named modifier
  node, handles basic and single-component restricted forms, and preserves exact
  modifier bytes for later module-aware restricted-path interpretation.
- Omitted/inherited visibility and arbitrary restriction paths are not guessed.
  Parser revision 14 refreshes cached facts; no dependency or source/ranker version
  changes were required. This supplies authored facts, not privacy enforcement.
- A separate public JS/TS alias-shadow regression passed without changing production:
  both adapters already perform lexical binding before import normalization. The
  test protects local-declaration precedence and the unshadowed imported target.
- [Evidence](results/native-implementation/rust-visibility/README.md). All 13 language
  unit tests pass, including whitespace/comments, struct fields and restricted
  modifier preservation. All 21 scope/module integration tests and strict
  workspace/all-target Clippy pass, as do formatting and whitespace checks.
  Recommendations 20/21 remain open for full module/import/visibility resolution.

### Native Rust use-tree syntax and binding facts

- Reproduced loss of grouped `self` and incorrect nested sibling paths in the
  previous comma-splitting import expander. An iterative syntax-tree traversal
  now preserves group prefixes, aliases, globs, absolute roots and trailing self.
- Optional `RustUseFact` metadata retains the local binding name, type-only self
  namespace, glob status and authored reexport visibility. Every emitted leaf has
  original source bytes/span and lexical scope. Unsupported syntax records an
  explicit unresolved argument and discards partial leaves; empty groups emit none.
- Parser revision 15 refreshes cached facts; legacy references remain readable.
  No third-party components or dependencies were added. Source/ranker versions
  remain unchanged. [Evidence](results/native-implementation/rust-use-trees/README.md).
- Final validation: 30 language/type unit tests, 22 module/scope integration tests,
  strict all-target Clippy, formatting and whitespace checks pass.
  Recommendations 20/21 remain open: this fixes
  extraction and preserves binding facts, but logical imports/reexports, export
  sets, visibility and module identity still require resolution work.

### Native anchored Rust module paths

- Added source-backed module member/interior traversal for `crate`, `self` and
  repeated leading `super`, using native Cargo roots instead of global names.
  Anchored import leaves now target source symbols; shared root contexts must
  agree. Basic module-item visibility is checked, with explicit unsupported or
  ambiguous reasons rather than speculative fallback.
- The worklist deduplicates before enqueueing and caps scope/root pairs at 65,536;
  paths cap components after the anchor at 256. Raw source spelling is retained
  through syntax-based call-path normalization. Parser policy is now 16.
- Cached external-module, anchored-path and structured-use consumers rebind on
  Rust/source-presence changes. Target creation/removal, duplicate declarations
  and visibility edits match complete clean-rebuild occurrences after reopen.
- Focused validation passed 164 core tests and 29 integration tests; five disposable
  compiler probes confirm the tested path/privacy rules. The parser-16 workspace run passed 433 tests across 32 suites,
  with final strict Clippy, formatting and whitespace checks passing. [Evidence](results/native-implementation/rust-anchored-paths/README.md).
- Recommendations 20/21/23 remain open for lexical aliases, reexports/globs,
  edition-sensitive unanchored paths, full namespace/visibility/receiver behavior
  and precise dependency invalidation. No corpus-wide precision claim is made.

### Native lexical Rust import aliases

- Registered structured named imports in their lexical scopes and connected
  selected function/module alias calls to native anchored module resolution.
  Calls retain raw spelling, binding identity and explicit-import provenance.
- Covered hoisting, local values/declarations, explicit type-only namespace
  coexistence, duplicate imports, block extent and module barriers. Unmodeled
  glob exports cannot select unrelated global names. Alias text expansion has
  per-target and per-file bounds with explicit unresolved reasons.
- Seven independent compiler cases pass their expected compile/fail outcomes.
  Public mutation tests compare complete occurrence records after alias edits
  and target removal/restoration with clean builds after reopen.
- Parser policy is 17; no dependency/source/ranker changes. Final focused checks pass: 18 language unit tests, 33 integration tests,
  strict all-target Clippy, formatting, whitespace and matching source hashes. [Evidence](results/native-implementation/rust-import-bindings/README.md).
- Recommendations 20/21/23 remain open for broader imports/namespaces, reexports,
  glob exports, receiver/type-use behavior, framework regions and invalidation.
