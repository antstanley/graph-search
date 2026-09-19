# Implemented lexical retrieval and selective sync

Production implementation: `3978833`.

This follow-up implements the promising ideas identified in the root-checkout review. It supersedes the earlier statement that production multi-term ranking is unchanged and that every changed sync reparses the whole tree. The original baseline, correctness-only, FTS, and CodeGraph results remain historical records.

## Production retrieval

`core::lexical` splits camelCase, acronym boundaries, snake_case, paths, and punctuation into lowercase tokens. It deduplicates query terms and removes a small function-word list for multi-word questions. Short literal symbol names remain usable. An exact bare/qualified-name lane always takes priority, including names that are themselves stopwords.

Non-exact symbols use BM25 over name/path/signature fields, weighted 8/2/1. Document frequency and document length affect the score; there is no flat symbol bonus or saturating multi-term bonus. A monotone mapping keeps this lexical lane below exact matches without changing its ordering. Zero token matches mean zero relevance. Filters precede result truncation. Bounded literal body scanning remains a lower-priority fallback, followed by the existing graph/context assembly and byte limits.

The implementation is native Rust over the current graph snapshot, with no SQLite dependency or second persisted index. It adopts the successful experiment's metadata tokenization/field-weighting approach rather than migrating storage. The [SQLite FTS5 BM25 documentation](https://www.sqlite.org/fts5.html#the_bm25_function) describes the 1.2/0.75 constants and column weighting used as the reference. This implementation uses positive scores, separate exact-name priority, Unicode-aware identifier splitting, and existing graph context assembly; it is not claimed to be a byte-for-byte SQLite clone.

Building lexical statistics still scans all indexed symbols on each query. This avoids independent index freshness bugs but is not a scalable persistent inverted index. Qualified names receive exact-match priority; non-exact metadata scoring uses bare name, path, and signature, matching the experiment. Prefix matching, indexed bodies/docs, learned synonym retrieval, diversity ranking, and semantic receiver inference remain follow-up work.

## Retrieval measurements

Same frozen 225 prompts, default first eight returned items, same external source hashes. Expanded queries require both the labelled function name and path; discovery/task-language queries require the labelled file.

| Repository | Exact symbol | Split symbol | Discovery file | Task-language file | New held-out split symbols |
|---|---|---|---|---|---|
| nanus | 30/30 | 30/30 | 10/10 | 3/5 | 20/20 |
| blogwright | 30/30 | 29/30 | 10/10 | 3/5 | 20/20 |

Compared with the correctness-only production revision, expanded exact hits remain **90/90**, split-name hits rise **32/90 → 89/90**, discovery file hits rise **24/30 → 29/30**, and task-language file hits rise **3/15 → 8/15**. The remaining expanded miss is blogwright's `make packages` → `makePackages`; the corresponding FTS metadata prototype also misses this query. The unsupported Svelte discovery target remains outside symbol extraction coverage.

A new sample excludes every function in the original expanded sample and selects 20 globally unique production functions per repository by SHA-256 of `heldout-v1:<symbol-id>`. Labels are frozen before queries run. It finds **60/60** split-name targets. No ranker tuning followed inspection of these outcomes. This is an identifier-derived held-out sample, not held-out natural-language or whole-graph accuracy. We have not run a new agent trial, separately attributed gains to signature matching versus tokenization/IDF, or measured compiler-level semantic precision.

Per-query evidence: [frozen-set results](results/lexical-followup.json), [held-out results](results/lexical-heldout.json), and `results/*-heldout-queries.json`.

## Selective parsing and rebinding

Schema 2 persists raw `SymbolFact`/`ReferenceFact` records alongside each file's content fingerprint. Those shared value types now live in `types::extraction`; the old `core::extraction` path re-exports them. Facts include raw spelling, source ownership, import provenance, dynamic-reference markers, declaration spans, and attributes. They contain signatures, not source bodies.

On changed sync:

1. Parse only added/modified/renamed source files.
2. Collect old and new symbol names/qualified names for changed or removed files. Build reverse raw-name consumers, including unresolved references. This covers both newly available targets and previously unique names becoming ambiguous.
3. Compare import specifier resolution against old/new file sets and changed target files. This covers missing imports, deletions, and extension precedence changes.
4. Expand existing incoming-edge closure because replacing any file subtree deletes incident edges. Rebuild affected unchanged projections from cached facts, preserving their source ownership and re-running binding.
5. Conservatively rebind HTML/CSS files on every graph-changing sync because cross-language matching emits bidirectional edges and file links.
6. Commit graph changes, then the new manifest. Missing fact caches fall back conservatively; old schema versions rebuild. No-op sync avoids graph application and sidecar rewriting; same-content timestamp changes refresh metadata only.

The reverse dependency maps are built per changed sync, not maintained as a separate persistent index. Fact caching saves parsing and unrelated subtree writes, but analysis still reads the workspace graph and manifest. Incoming closure may be broad. `SyncReport.modified` includes cached projections that were rebound; `unchanged` excludes them.

During verification, the in-memory reference store was found to delete an upsert's nodes immediately before inserting its edges. Replacing a later target file could then delete an earlier file's newly inserted edge. It now removes all replaced subtrees before insertion, matching the Grafeo adapter. The shared conformance suite now applies its two-file fixture twice and verifies the restored relationship.

## Incremental experiments

The probe walks each original repository with the default policy, copies exactly that file set to a temporary directory, and appends a harmless comment to one selected source file in the copy. Ignore/hidden filtering is disabled only for that already-selected copy so moving it cannot alter membership; equality of copied and source-selected paths is asserted. The external originals are never edited or reindexed.

A complete clean reindex of the edited copy is compared against sync: serialized node records and edge records must match (edge duplicates normalized, order ignored). **All three comparisons pass**, and each edit invokes the parser only once. Edit targets are `context.rs` in nanus-domain, `secret.ts` in blogwright/pds, and `crypto.ts` in whatsurvey/backend/settings. Exact paths and modified-file lists are retained in each result.

| Repository | Walked files | Parsed: full → sync | Replaced projections | Sync ms | Clean reindex ms | No-op ms | Manifest bytes |
|---|---|---|---|---|---|---|---|
| nanus | 185 | 130 → 1 | 89 | 1887 | 2019 | 132 | 5,787,490 |
| blogwright | 337 | 230 → 1 | 27 | 2033 | 2733 | 141 | 5,432,845 |

These are single debug-build measurements on a shared machine, not reliable speedup estimates. The drop in parser invocations is established; storage, dependency scans, and manifest serialization still dominate much of elapsed time. Persisting facts increases manifest size to roughly 5–14 MB on these corpora. No-op still reads that manifest and walks the tree. A separately indexed fact/dependency store or manifest header is a potential next optimization after profiling, not a claim made by this change.

## Verification and compatibility

- **95 workspace tests pass**, including four new integration tests and the identifier-tokenization unit test. Strict workspace/all-targets Clippy passes; formatting and diff checks pass.
- Existing 25 accuracy regressions remain passing.
- Counting-extractor tests prove only the changed file is parsed, even after closing and reopening the persistent store. Additions resolve dangling calls and introduce ambiguity; deletions remove ambiguity; renames update IDs; no-op does not parse.
- Differential tests compare the complete incremental graph with a clean rebuild for those operations, JS import extension precedence, HTML/CSS links and classes, old schemas, and missing caches.
- Retrieval regressions verify short/exact names, camel/acronym splitting, signature-only matches, deduplicated terms, and empty results for unrelated queries. Existing filter, snippet, output-budget, and graph tests remain active.
- CLI smoke checks pass: full JSON envelope budget, invalid-language rejection, and hidden-file forwarding.
- Root checkout modifications are untouched. External source hashes still match the initial study.

Schema version is now **2** (parser remains 2). The existing shared schema constant also appears in CLI envelopes, so consumers must accept schema 2. Old manifests deserialize with absent extraction facts and are rebuilt before normal reconciled queries. Read-only/no-reconcile behavior continues to report staleness. Storage atomicity/crash recovery beyond the existing commit-last contract is not newly certified.

Evidence: [workspace tests](results/followup-tests.log), [Clippy](results/followup-clippy.log), [CLI smoke](results/cli-smoke.json), `results/*-incremental-followup.json`, [source stability](results/source-stability.json).

## Reproduction

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 research/scripts/reproduce_followup.py
```

The follow-up script builds the current harness, runs frozen and held-out retrieval, runs sync probes on disposable copies, and audits source stability. Original historical comparisons use `reproduce.py`, now pinned to `4dd6af6` and `f158605` so current ranking cannot overwrite the meaning of the original correctness-only arm. The held-out labels are committed; deriving a new sample requires baseline graph dumps from the historical workflow.

## Correctness checkpoints

**Resolution:** cached raw facts retain the same `from_key`, import source, dynamic marker, and symbols used by fresh extraction. Both paths converge through `populate_symbols`, then the same resolver and cross-language matcher. Name dependencies cover every name candidate used by the current conservative resolver; import resolution compares both file sets. No unverified receiver fallback was introduced.

**Sufficiency:** resolved-edge closure alone is insufficient; raw unresolved-name consumers and import candidate changes are explicitly included. Rebinding a file can delete its own incoming edges, so transitive closure is retained. Cold-cache, missing-cache, and schema migration cases are tested.

**Regression paths:** exact lookup, public graph queries, filters, output caps, persistent reopen, schema envelopes, and memory/Grafeo conformance run through their unchanged public entry points. Frozen and held-out retrieval labels plus full-graph differential checks supplement small fixtures. This is self-verification, not an independent agent review.

VERDICT: LIKELY_CORRECT
CONFIDENCE: medium
SUMMARY: Lexical retrieval improves the measured query sets, and selective sync preserves clean-rebuild equivalence while parsing only changed files; semantic completeness and large-scale latency remain bounded by the documented limits.
