# Fixed candidate-pool channel combination protocol

Frozen before capturing scores. Use the current native metadata and body indexes
over read-only sibling sources, building only an in-memory temporary graph. Include
source-valid established tasks and the frozen fresh-routing and Markdown/README
suites. Validate labels and record exclusions; no hash-only label repair.

For each unchanged task prompt, capture top 50 entities per native lane with split
analysis, OR terms, combined metadata normalization, and no graph or source packing.
Retain native ranks, scores, identities and body region coordinates/hashes. Metadata
scores are the native bounded BM25/(1+BM25) values, not recovered raw BM25; body
scores are native body BM25. Reject captures that exhaust candidate/posting work.

Compare body only, metadata only, equal-weight RRF (k=60), and two query-local score
normalizations at three fixed body weights (0.25, 0.5, 0.75): score/max and min-max.
Normalize non-exact candidates in each retained lane separately. A nonempty flat
min-max lane maps to one; empty/missing lanes contribute zero. Exact metadata hits
remain a separate first tier in all policies. Do not tune weights after observing
labels. Deduplicate by native entity identity; keep representation and file
diversity fixed (no file cap). Break combined ties by path then identity.

Report all-required-file presence at eight and fifty entities, mean required-file
recall at eight, per-task rankings, and aggregates by repository and task kind.
These are candidate-file metrics, not complete-region delivery, semantic target
precision, answer correctness, or production latency. Published IDs/paths/scores
contain no source bodies. Broad discovery prompts dominate these suites; exact
navigation correctness is covered separately by native tests, not inferred here.

This diagnostic decides whether normalized combinations warrant a subsequent
end-to-end candidate. Promotion requires a gain over body-only without losses by
repository/task kind, then separate source-budget and query-class validation.
The diagnostic alone cannot change the production default. Preserve observed
regressions and do not conflate the top-50 union with new independent candidates.

Hash production, probe, driver, protocol, labels, binary, sibling sources and each
sibling CodeGraph file before capture. Verify all remain unchanged after capture.
Do not rebuild or write external indexes. No timing claim is made from this run.
