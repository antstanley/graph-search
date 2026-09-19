# Implementation verification

This is the implementing agent's source-level verification, not an independent
review. The task is to experiment with documentation, comments and bounded
bodies, isolate expansion and diversity, improve natural-language retrieval and
preserve exact-symbol retrieval. No model task-success claim is part of the
completion evidence.

## Resolution and data-flow checks

1. `Projector::extract_one` gets the language extractor's `Extraction`, enforces
   symbol count bounds, then calls `lexical::attach_body_terms`. Rust and JS/TS
   spans now resolve to zero-based source byte ranges; the Unicode/EOF regression
   would fail against the old one-based conversion.
2. `attach_body_terms` indexes only function/method facts and accepts only valid
   UTF-8 slice boundaries. It caps line and character traversal and stores a
   serialized token-count map. `populate_symbols` copies fact attributes to
   nodes; the manifest caches the same raw extraction. There is no query-time
   slice of a stale source offset and no raw body attribute in search responses
   (`SymbolHit::of` does not copy node attributes).
3. Parser version 3 invalidates the old projection. `stale::check` detects the
   mismatch even when source size/mtime is unchanged; default
   `SearchService::freshness` calls `Index::sync`, whose projector rebuilds when
   global parser versions differ. The regression checks staleness, rebuild and
   new cached term presence. Explicit no-reconcile mode deliberately opts out.
4. `QueryEngine::seed` uses metadata and body indexes from the same snapshot.
   Exact and complete split-name matches are explicit priority sets. Separate
   bare-name targets are considered only when no complete split-name target
   exists. The regression includes functions for every individual component of
   `load_pds_secret` plus callers repeating that name, and requires the complete
   function to win for both exact and split queries.
5. `diverse_seeds` uses raw relevance, file counts, and stable path/line/id ties.
   Exact/named lanes do not depend on body frequency or diversity. Synthetic
   ranking tests verify another-file promotion and exact priority. Existing
   two-hop connection tests verify that explicit multi-symbol queries still
   include their intermediate path.
6. `explore` assembles bounded snippets and graph edges as before. `fit_explore`
   updates approximation counts without erasing the body bound note. Existing
   byte-budget and snippet-cap regressions continue to pass.

## Sufficiency and empirical checks

- Five new public-library/parser integration tests cover body-only retrieval,
  reopen/edit freshness, filters, source bounds, cache version migration,
  exact/split spelling, same-line siblings and byte offsets. One core unit test
  covers diversity and exact priority.
- `cargo test --workspace`: 101 tests pass. Strict all-target Clippy and workspace
  formatting pass. The existing 24 taskbench tests and three new prototype tests
  pass. Build/test source hashes and log summaries are in `results/verification.json`.
- The content experiment includes metadata alone and all predeclared weights for
  each separate field and the combined fields. Expansion and diversity have
  individual arms before a combined arm. Failed pilot and production checks are
  retained. Selection comes from development file recall/MRR, not confirmation.
- `results/frozen-public-core.json` records all 354 requests per engine and full
  candidate ranks. `summarize.py` verifies every exact query retains or improves
  its target rank, checks expected request counts, and rejects query errors.
- Separate public `Index` task runs exercise the public API, temporary persistent
  stores, freshness, output budgets and follow-up reads. Both run all 60 tasks.
  Their manifests validate source snapshots before/after and preserve executable
  hashes. The core, task and profile runs must agree on source snapshots.
- `results/public-index-profile.json` measures debug latency and live temporary
  store size sequentially. Its executable hashes must equal those used in the
  public task comparison. This distinguishes accuracy preservation from latency
  and storage costs.

## Regressions and limits that remain

File recall, symbol-span overlap, complete delivered evidence and agent success
are different claims. The report retains every task-level change and calls out
lost evidence-ready cases. Larger body vocabularies increase per-query index
construction and persisted attribute size. Body bounds can miss late matches;
documentation/leading-comment fields and expansion are not enabled by default.
The graph's existing approximate binding and graph-work budget limits remain.

The published confirmation prompts have been seen during earlier work; later
compatibility fixes are regression confirmation, not a fresh hidden evaluation.
Default promotion is justified by the predeclared file-recall objective and
exact-accuracy gate, not by an assertion that every task or latency metric wins.
