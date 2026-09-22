# Graph-search accuracy investigation

**Implementation in progress (21 September 2026):** Native changes and outstanding gates are tracked in [IMPLEMENTATION.md](IMPLEMENTATION.md). The [documentation-inclusive Markdown comparison](results/native-implementation/markdown-context-docs/README.md) records both substantial documentation gains and code-evidence regressions; it does not establish universal quality improvement.

**Baseline review (19 September 2026):** [Native search improvements](09-native-search-review.md) applies the comprehensive search research to the current implementation, with 30 recommendations, release-mode native probes, 306 external evidence trials, and a body-retrieval ablation. That review itself changed no production code or dependencies; its [reproduction script](scripts/native_review.py) writes separate results.

Research branch: `research/search-accuracy`  
Baseline: committed `main` at `4dd6af6`  
Worktree: `/private/tmp/graph-search-research`

**Follow-up implemented:** production identifier-aware BM25 retrieval and selective sync using persisted raw facts. See [current results and tradeoffs](07-lexical-and-incremental.md): 90/90 exact, 89/90 split-name, 8/15 task-language hits, plus 60/60 new held-out split-name hits. The initial study below is preserved as historical evidence.

The search problem spans both **retrieval ranking** and **graph correctness**. This branch documents the investigation, repairs 26 concrete defects, and includes 25 new public-library regression tests. It retains the main checkout's uncommitted work separately.

The strongest controlled retrieval result is the gap between exact and split names: the original engine finds **90/90 exact-name targets but only 32/90 split-name targets** across nanus, blogwright, and whatsurvey. A research-only identifier-aware FTS5 prototype over the same extracted candidates finds **89/90 split-name targets**. These are sampled identifier-derived queries; the task-language challenge performs much worse and is reported separately.

The largest graph defect was JS/TS call attribution: all 6,091 baseline blogwright calls and 16,805 whatsurvey calls belonged to file nodes. Repairs cover scope keys, initializer ownership, receiver traversal, named imports, shadowing, ambiguity, filters, graph assembly, result budgets, and incremental rebinding. They improve graph correctness but do not turn the existing ranker into a semantic search system.

Read in this order:

1. [Methodology and limits](01-methodology.md): source provenance, 225 retrieval prompts, comparator fairness, and what the metrics mean.
2. [Findings and implemented fixes](02-findings.md): 26 repaired defects and remaining capability limits.
3. [Measured results](03-experiments.md): generated tables, graph evidence, timings, and rejected experiments.
4. [Improvement plan](04-improvement-plan.md): lexical retrieval, semantic bindings, incremental facts, budgets, and evaluation gates.
5. [Verification certificate](05-verification.md): evidence, regression paths, and material tradeoffs.
6. [Root checkout comparison](06-root-checkout-review.md): overlap with the pre-existing changes, additional ideas, and reproduced gaps.
7. [Implemented lexical retrieval and selective sync](07-lexical-and-incremental.md): current behavior, measurements, validation, and limits.

8. [Task-based evaluation suite](08-task-evaluation.md): source-backed real tasks, held-out families, engine adapters, agent protocol and blind grading.
9. [Native search improvements](09-native-search-review.md): current source findings, reproducible experiments, native architecture and phased acceptance gates.
10. [Independent evaluation](10-independent-evaluation.md) and [round 2](11-independent-evaluation-round-2.md): black-box trials of the shipped binary on `nanus`.
11. [Response to the independent evaluations](12-independent-evaluation-response.md): finding-by-finding evaluation, the four increments landed in response, the staged lazy-store plan, and the Criterion suite that validates progressive improvement.

## Reproduction

The external repositories must be available under `~/code/`; blogwright and whatsurvey must already have CodeGraph indexes. The scripts read these repositories and existing indexes. They write their own temporary graph-search data under `/private/tmp` and results under this research directory.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 research/scripts/reproduce_followup.py
# Historical baseline/correctness-only/FTS comparison:
python3 research/scripts/reproduce.py
```

`reproduce_followup.py` measures current production retrieval and edits disposable copies for sync checks. Its held-out labels are frozen in `results/*-heldout-queries.json`. `reproduce.py` pins both historical arms (`4dd6af6` baseline, `f158605` correctness-only) in temporary archives so it cannot relabel current ranking as the earlier fix-only experiment. It requires cached Rust dependencies for `--offline`, Python with SQLite FTS5, and the installed CodeGraph CLI. Remove `--offline` deliberately if dependencies are not cached. Expect several minutes; these are debug builds and CodeGraph context queries perform additional work.

The harness is a standalone Cargo workspace so production manifests remain unchanged. `research/harness/target/` is ignored. Raw full graph dumps remain temporary; committed results retain query labels, locations, relationships, counts, timings, source hashes, and sanitized external-repository responses. External snippets/signatures are omitted from committed response records. Synthetic fixture output is retained in full.

`results/summary.json` and `03-experiments.md` are regenerated by `scripts/summarize.py`. Per-query files retain failures and misses, not just winning examples. The FTS prototypes live only in research scripts; this branch does not add an FTS database dependency to the product.

## Review cautions

The initial fix-only revision reprojected the entire tree on changed sync. Current production parses changed files and rebinds affected cached facts; dependency analysis and manifest I/O remain workspace-sized. File search uses the consistent scan path instead of the broken resident shortcut. Ambiguous names now return an error requiring an exact ID. Parser version 2 and schema version 2 force older projections to refresh. These are intentional behavior/performance tradeoffs, documented with the fixes.

Identifier-aware lexical retrieval and cached selective rebinding are now implemented. Remaining priorities include module/scope-aware binding, persistent lexical statistics if profiling warrants them, and explicit graph-work budgets. Do not choose a new storage backend or claim general semantic accuracy from this sample alone.


Conditional adoption decisions for grams, regex, pruning, compression, segments
and neural retrieval are tracked in [CONDITIONAL-DECISIONS.md](CONDITIONAL-DECISIONS.md).
Deferred features are distinguished from implemented behavior and from open
measurement/lifecycle gates.

The [native storage composition experiment](results/native-implementation/posting-compaction/README.md)
separates posting payload/capacity from process memory. It rejects unconditional
vector compaction after correctness and maintenance checks, while preserving the
candidate and its negative results. No codec or memory reduction is claimed.

The [graph-context comparison](results/native-implementation/graph-context-review.md)
records the shared-component traversal checks and 276 controlled semantic versus
no-enrichment trials. It retains the semantic default while exposing explicit
Calls/Imports/Types/None policies; per-task tradeoffs remain visible.

[Native index completion audit](NATIVE-INDEX-AUDIT.md) verifies recommendation 8
against current source and differential/lifecycle tests, while keeping per-file
cache maintenance distinct from index execution. The
[metadata delta follow-up](results/native-implementation/metadata-deltas/README.md)
now completes recommendation 7 with immutable posting/statistic updates, full
rebuild equivalence, and paired resident/publication measurements.

The native documentation integration and its controlled comparisons are recorded
in [implementation tracking](IMPLEMENTATION.md),
[code-structure evidence](results/native-implementation/code-context/README.md),
and [isolated documentation evidence](results/native-implementation/documentation-context/README.md).
Both comparisons retain per-task regressions, source provenance and fixed evidence
budgets; they do not measure model task success.


The [source-unit audit](SOURCE-UNIT-AUDIT.md) covers native bodies, documentation,
configuration, Markdown and manifest-owned package identity. The
[package-context comparison](results/native-implementation/code-context-packages/README.md)
refreshes the fixed-window experiment on source representation 9, retaining five
partial-context regressions and the remaining retrieval-quality gates.


The [package-budget isolation](results/native-implementation/package-metadata-budget/README.md)
identifies returned package metadata as the cause of five partial-context losses.
A [residual-fragment candidate](results/native-implementation/context-fragments/README.md)
was tested and withdrawn: identical delivered evidence with 15.20% more examined
context windows. The next implementation should share repeated identity records
without discarding their provenance.


[Shared package identities](results/native-implementation/package-sharing/README.md)
implement the next budget intervention without discarding provenance. The 348-trial
comparison restores two lost regions, and the 116-response differential preserves
all 464 common-node package associations (430 known, 34 absent). Three earlier coverage losses and three observed
primary-snippet overlap cases remain explicit follow-up work.

- [Primary snippet deduplication](results/native-implementation/primary-dedup/README.md): 348 controlled trials; four duplicate coordinates removed, unchanged task coverage, and 116 raw responses checked for preserved identities.

- [Context-selection requirement audit](CONTEXT-SELECTION-AUDIT.md) and [cross-phase source capture](results/native-implementation/source-capture/README.md): 54 strict-verification trials, identical returned evidence, 50.58–55.01% fewer source bytes read.

- [Source-read admission](results/native-implementation/source-admission/README.md): payload lower bounds preserve a tight source allowance for fitting candidates; 348 corpus trials retain identical delivered evidence.

- [Bounded context proximity](results/native-implementation/context-proximity/README.md): retained native line-span feature; 348 stable trials show unchanged labelled coverage, with all source-line substitutions recorded.

- [Graph neighborhood reuse audit](GRAPH-REUSE-AUDIT.md): request-local adjacency sharing, budget and direction invariants, and remaining graph-quality evaluation requirements.

- [Graph-context comparison with native API JSON](results/native-implementation/graph-context-native-json/README.md): 348 stable trials with complete metadata delivery, identical candidates, and the graph/source-budget tradeoff. The evaluation adapter now preserves all API fields and verifies structured source evidence.

- [Native trigram adoption experiment](results/native-implementation/trigram-cumulative/README.md): production-scanner equivalence and repeated-query economics; trusted snapshots benefit, while the measured strict pipeline does not justify changing live literal search.

The [current retrieval phase profile](results/native-implementation/retrieval-phases/README.md)
records 618 equivalent baseline/instrumented calls across task and broad-query
workloads. It closes the conditional score-pruning measurement gate as a deferral
and identifies repeated freshness/context preparation as a stronger next target.

The resulting [request-inspection optimization](REQUEST-INSPECTION-AUDIT.md) removes
a duplicate complete freshness walk while retaining post-reconciliation verification.
Its [paired release results](results/native-implementation/request-inspection/README.md)
preserve all 618 responses after generation/elapsed normalization and measure
14.89–26.45% median task-query improvements across the three repositories.

The [scope-resolution audit](SCOPE-RESOLUTION-AUDIT.md) records the reproduced and
fixed local-callable/outer-member false edge, its incremental provenance tests,
and the remaining namespace and receiver precision requirements.

The scope audit now also records native binding of direct static methods through
the visible local class, with initialization/accessor guards and JS field-source
coverage. Parser revision 9 and all 400 workspace tests are validated in the
[class-static capture](results/native-implementation/class-static-bindings/checks.json).

The [native ESM binding increment](results/native-implementation/js-module-bindings/README.md)
requires JS/TS imports to follow authored exports, preserves aliases and forwarding
identity, and rejects private or ambiguous targets. Its bounded parser/resolver,
Node oracle and incremental/reopen tests form a declared subset; package aliases,
framework regions and broader semantic quality gates remain open.

[Native package self-references and private imports](results/native-implementation/node-package-maps/README.md)
add bounded package.json facts and map-driven module lookup. A source-backed
whatsurvey fixture verifies two call sites and a mapping-removal counterfactual;
it is not a repository-wide quality comparison. The same investigation records
an outstanding custom in-tree store-exclusion bug before delivery.

[Configured store exclusion](results/native-implementation/store-exclusion/README.md) fixes custom in-tree stores entering source discovery, with migration and path-alias regression coverage.

[Native workspace dependency identity](results/native-implementation/node-workspaces/README.md) adds bounded pnpm/package.json membership, workspace protocol selection, invariant export targets and override guards, with a five-call whatsurvey counterfactual and independent runtime oracles.

Native scalar posting codec measurements and the decision to retain production
vectors: [experiment and limitations](results/native-implementation/posting-codec-measured/README.md).

Embedded JS/TS coordinate composition, the independently discovered destructuring
fix, and remaining framework integration:
[implementation and validation](results/native-implementation/embedded-script-coordinates/README.md).

Recommendation 27's conditional compression gate is closed with native vectors
retained: [requirement audit and accounting boundaries](results/native-implementation/storage-decision-audit/README.md).

- [Generation churn and retained-reader experiment](results/native-implementation/generation-churn/README.md): disposable native mutation, rebuild, retention and disk checks.

- [Generation lifecycle decision](results/native-implementation/generation-lifecycle-audit/README.md): recommendation 28 accepted with disk and own-process memory evidence and an explicit concurrency contract.

- [Native TypeScript configuration facts](results/native-implementation/typescript-config-facts/README.md): bounded JSONC projection, retained wildcard ordering, source persistence and compiler-oracle checks; resolver integration remains open.


- [Native TypeScript inheritance](results/native-implementation/typescript-inheritance/README.md):
  bounded merging over indexed config facts, option origins and dependency hashes;
  compiler comparisons and publication/rebuild parity. Project/alias integration remains open.

- [Native TypeScript alias dispatch](results/native-implementation/typescript-alias-dispatch/README.md):
  origin-aware paths/baseUrl precedence with authored substitutions retained for
  module-mode loading; project/loader integration remains open.

- [Native modern TypeScript file loading](results/native-implementation/typescript-file-loading/README.md):
  extension/suffix/index lookup, ESM distinctions and negative probes over published
  facts; selected-context compiler comparisons and mutation parity.

- [Recommendation 21 acceptance](RECOMMENDATION-21-AUDIT.md): clause-by-clause map
  of native packages/imports/framework regions, including the declared subset and
  the template/conditional-loader limits.
- [Native framework script regions](results/native-implementation/framework-regions/README.md):
  Svelte/Vue/Astro declared `<script>` (and Astro frontmatter) extraction through
  offset-translating JS/TS adapters, persisted region facts and coverage counters.
- [Native default TypeScript project selection](results/native-implementation/typescript-aliases/README.md):
  nearest `tsconfig`/`jsconfig`, bounded inheritance, `paths`/`baseUrl` before
  package maps, supported bundler/Node16 modes and conservative config invalidation.
- [Native Rust public reexports](results/native-implementation/rust-reexports/README.md):
  `pub use` export nodes, `as` aliases and bounded chains, with a private-`use`
  counterfactual.

- [Recommendation 30 acceptance](RECOMMENDATION-30-AUDIT.md): the release gate
  turns correctness, evidence, performance and resource objectives into a
  machine-readable decision, and refuses a full release while model task success
  is unmeasured.
- [Release gate decision](results/native-implementation/release-gate-v3/README.md):
  Criterion benchmarks (cold build, sync, lookups, explore routes, scans) with
  p50/p95/p99, the 34-task evidence metrics and the resident/index probe.
- [Gate calibration](results/native-implementation/release-gate-v1/README.md):
  the failed pre-calibration run and the measured `a0cbf7d` baseline used to
  re-freeze the accuracy thresholds.

- [Recommendation 12 acceptance](CONTEXT-SELECTION-AUDIT.md): every source
  selection clause is mapped to implementation and to the controlled experiments,
  with complete-region coverage, citations and response bytes now part of the
  release gate; model answer success stays an external objective.
