# Methodology and scope

> Historical investigation / correctness-only revision. The subsequent implementation and current verification are in [07-lexical-and-incremental.md](07-lexical-and-incremental.md).

## Revisions and isolation

The production fixes are committed as `f158605`. The baseline is graph-search `4dd6af6`, the committed `main` revision at the start of this investigation. Work was performed in `/private/tmp/graph-search-research` on `research/search-accuracy`. The main checkout already contained uncommitted changes in query, reconcile, resolve, end-to-end tests, and an examples directory. Those changes were deliberately excluded from this baseline and were not edited. This matters when comparing these findings with experiments run from the main working directory.

The external repositories were read in place. The harness creates an in-memory **Grafeo** store using the production language registry, `Projector`, and `QueryEngine`. The harness uses the default `WalkPolicy` rather than loading per-repository graph-search configuration, and measures QueryEngine work without SearchService freshness checks. It does not create graph-search indexes in the external repositories or modify source in nanus, blogwright, or whatsurvey. CodeGraph queries use their existing indexes. Source revisions, dirty-tree status, per-source SHA-256 hashes, installed versions, and comparator freshness checks are recorded in `results/*-baseline.json` and `results/comparator-provenance.json`.

Both CodeGraph indexes were checked against on-disk source: all 247 indexed blogwright files and all 1,039 indexed whatsurvey files matched their recorded SHA-256 hashes at the time of the check. This verifies the indexed files, not that the comparator indexes every file it could support. CodeGraph was version 1.6.0. The GitHub default branch is not assumed to be the installed implementation.

## What was tested

1. **Production baseline tests:** the complete Cargo workspace suite before changes.
2. **Controlled semantic counterexamples:** tiny Rust, TypeScript, Svelte, Markdown, HTML, and CSS files isolating attribution, receiver calls, ambiguity, aliases, shadowing, filters, paths, bounds, and graph connections. The fixture is syntactic test data; it is intentionally not a compilable application.
3. **Discovery retrieval:** ten manually selected queries per repository, with an intended relevant file. Exact names, split identifiers, punctuation, and several task questions are represented. Nanus discovery deliberately concentrates on the agent loop that motivated this experiment.
4. **Expanded retrieval:** 30 deterministic production-function samples per repository, each queried by exact and split identifier: 180 queries total. Only globally unique function names containing at least two identifier tokens are sampled. Paths containing `test`, `spec`, `fixture`, or `example` are excluded. SHA-256 of the symbol ID orders the sample. This broadens coverage beyond the initial agent-loop examples but intentionally does not test ambiguous names.
5. **Task-language challenge:** five manually labelled questions per repository, expressed in task language rather than copied symbol names. These are difficult diagnostic examples, not a representative distribution of all agent tasks.
6. **Graph checks:** source-confirmed application call edges, extractor ownership counts across all three repositories, and 25 end-to-end regression tests covering the repaired contracts.
7. **Verification:** the full workspace suite, Clippy with warnings denied, formatting of changed Rust files, and diff whitespace checks.

The 225 retrieval prompts comprise 30 discovery, 180 expanded, and 15 task-language queries. They are evaluated under multiple arms. Additional symbol, refs, callers, callees, impact, deps, neighbors, and path requests in the harness and regression tests are separate from that count.

## Retrieval arms

| Arm | Candidate source | Ranking/context |
|---|---|---|
| Baseline | Production graph-search extraction at `4dd6af6` | Existing `explore`, default eight seeds |
| Patched | Same production pipeline with this branch's fixes | Corrected extraction/query contracts; existing multi-term scoring retained |
| FTS metadata | The baseline graph-search non-file nodes | Python SQLite FTS5, split identifiers; name/path/signature weights 8/2/1 |
| FTS body | Same nodes | Above plus source within the node span, weight 1 |
| CodeGraph raw FTS | Existing CodeGraph `nodes_fts` table | Controlled SQL `MATCH`/BM25 query, eight rows |
| CodeGraph query | Existing index | Installed CLI symbol search, eight results |
| CodeGraph explore | Existing index and current source | Installed full context pipeline, default file budget |

The FTS prototype is a **retrieval experiment**, not production code. It demonstrates that changing lexical tokenization and ranking while holding graph-search's extracted candidates constant can recover many failures. It does not prove that SQLite is the uniquely correct storage engine. No embeddings, learned reranker, semantic compiler integration, or replacement graph backend was installed.

The discovery comparison runs all relevant CodeGraph arms. The expanded sample uses raw CodeGraph FTS as the inexpensive lexical comparator; it does **not** stand in for the full CodeGraph explore pipeline. The task-language challenge compares full CodeGraph explore. Nanus has no CodeGraph index, and none was created. Missing CodeGraph arms for nanus mean **not run**, not zero accuracy.

## Metrics and fairness

- **File Hit@8:** whether the labelled file occurs in the first eight graph-search/FTS results. Discovery and task-language questions use this metric. Multiple hits from one file consume multiple slots. CodeGraph explore returns a differently sized set of file excerpts; its file-hit rate is reported separately, not as an identical eight-symbol budget.
- **Symbol Hit@8:** whether both the expected path and function name occur in the first eight results. Expanded exact/split queries use this stricter metric.
- **MRR@8:** reciprocal rank of the first labelled hit, zero if missing. This is not nDCG: there are no exhaustive graded relevance judgements.
- **Graph evidence:** presence of a specifically verified source-to-target edge, attribution of calls to file versus symbol nodes, and resolved/unresolved counts. Resolution rate is **not precision or recall**. A false resolved edge is worse than an honest dangling edge.
- **Latency:** harness wall time around a resident query, in a debug build, and index wall time. These are diagnostic single runs on a shared developer machine. FTS lookup timing excludes FTS construction and graph/snippet assembly. CodeGraph CLI timing includes process startup and full context work. These timings cannot establish a backend speed winner.
- **Output size:** serialized result bytes and CodeGraph output bytes are different output contracts. They are useful for context-cost inspection, not an end-to-end agent token benchmark.

The prototype splits camelCase, acronym boundaries, snake_case, and punctuation, lowercases terms, removes a small stop list, combines terms with OR, and applies BM25 field weights. Weights were not cross-validated. Prototype source-span bodies include code; they are not a comment-only semantic index. The metadata ablation restricts MATCH to metadata columns rather than merely assigning body a zero score (a zero weight alone still permits body matches).

## What this investigation does not claim

There is no exhaustive compiler-validated graph oracle for the three applications, no measured precision of every graph edge, no LLM-agent randomized trial, and no held-out training/tuning split. Source labels identify one intended useful file; alternative relevant files are not exhaustively graded. Exact/split prompts are derived from identifiers and therefore favor identifier-aware lexical search. The task-language challenge exposes why those strong results must not be generalized to semantic question answering.

No scalability conclusion should be drawn beyond these repository sizes. Read-only snapshots of the external code were used, not clean checkouts of their HEAD commits; recorded hashes are the exact source provenance. Unsupported languages, ignored files, generated files, unresolved dependencies, and ambiguous targets must be evaluated separately from ranking.
