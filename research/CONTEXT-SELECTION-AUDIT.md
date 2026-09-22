# Source-context selection audit

Scope: recommendation 12 in `09-native-search-review.md`. Candidate ranking and
file diversity are separately tracked by recommendation 11; model answer/patch
success is separately tracked by recommendation 30. This audit does not close any
of those requirements merely because source correctness tests pass.

## Requirements and current evidence

| Requirement | Implementation / evidence | Status |
|---|---|---|
| Carry matched locations from lexical retrieval | `analyzer::matching_lines`, `body` match regions, `SourceEvidence::match_line`, and request-local `MatchContext`; source hash and original region spans accompany those locations | Implemented at line granularity; not a persisted byte-position posting index |
| Prefer query coverage | `evidence::refresh_costs` uses distinct term masks and recomputes marginal new-term coverage after delivery; focused coverage/repetition regression | Implemented |
| Score proximity | Ranker 22 adds a bounded shortest-line-cover feature over distinct undelivered term masks; exhaustive and allocator tests pass | Retained; 348 paired trials show unchanged labelled coverage, not a quality gain |
| Declaration identity plus matching body | Primary match anchor plus declaration/header and body candidates; full declarations up to the interval line cap, labeled separate intervals for larger bodies | Implemented, subject to declared byte/work caps |
| Relevant connecting call sites and endpoints | `query::connect` retains bridge nodes; `evidence::extend_prepared` obtains version-matched occurrence positions for returned edges and allocates reference intervals alongside endpoint declaration/body context | Implemented under returned-edge, occurrence, interval and payload caps |
| Marginal value per byte | Dynamic coverage/value-to-estimated-byte ordering; exact serialized-byte admission; final-statistics and omission-notice reservation | Implemented; not a global optimal packing claim |
| Merge overlap and avoid duplicate lines | Global primary accounting by path/hash/line, coverage accounting during allocation, contiguous same-role excerpt coalescing | Implemented; primary-dedup experiment removes all four observed duplicate coordinates |
| Preserve metadata/edge explanation | Base result and graph explanation are fitted before optional context; shared package identities retain full provenance with result-local references | Implemented; final payload fitting can explicitly remove relationships/items |
| Read each source once per request | New raw captures in the explore request's `WorkBudget` join freshness, automatic maintenance and evidence; local `SourceCache` still controls text materialization | Implemented in the current worktree; validation recorded in the implementation ledger |
| Stop materialization when remaining space cannot admit useful content | Initial assembly rejects impossible item metadata before reading source and checks minimum primary growth against both the item budget and mandatory response metadata; optional interval admission checks remaining bytes | Implemented with conservative necessary bounds; unknown source-line sizes still require reading |
| Measure complete regions, supported citations, redundant bytes and answer success independently | The release gate (`research/scripts/release_gate.py`) records complete-region delivery, mean region coverage and response bytes on the 34 source-valid tasks under frozen thresholds; citations are source-hash verified by the protocol; duplicate coordinates were driven to zero by the primary-dedup experiment and the protocol's delivered-line set cannot double-count one coordinate. Model answer/patch success is a separate gate objective recorded as `not_measured`. | Implemented; answer success remains an external objective |

## Request capture invariants

The public `explore` call enables raw captures before freshness. Its `WorkBudget`
is transferred into the query engine, so automatic maintenance and evidence use
the same capture map. Other public modes retain their existing behavior. The cache
is destroyed with that request; it is neither a generation cache nor a substitute
for the next request's content verification.

Each admitted path is opened at most once through the shared reader. Raw bytes are
retained before UTF-8/binary classification so strict verification and extraction
can agree even for excluded text. Physical open/byte counters are charged on the
first read only. Text materialization separately charges its own capacity for
captures obtained during freshness. Denied opens do not allocate capture records;
retention is bounded by admitted opens and the shared source allowance (plus vector
allocation overhead). Cancellation and deadline checks apply to cache hits too.

Size/mtime drift on reuse causes an incomplete-verification error. A prefix captured
under a smaller limit cannot satisfy a later larger read. A complete capture can
serve a smaller overflow probe without claiming that the probe is the whole file.
Captures are observations, not an atomic snapshot of the live filesystem: a writer
that restores metadata can evade metadata drift detection during the request.
Returned source hashes still describe the actual captured bytes, and a new strict
request hashes the source again. This limitation must remain explicit.

## Existing measured tradeoffs

Shared package identity recovered two of five partial region losses introduced by
package context. Primary deduplication removed four repeated coordinates without
changing complete-region coverage. Three package-budget losses remain. The prior
fragment-bisection experiment was withdrawn because it delivered no additional
source while increasing context-window work. These observations do not justify
restoring that experiment or discarding package provenance.

See `results/native-implementation/package-sharing/README.md`,
`results/native-implementation/primary-dedup/README.md`, and
`results/native-implementation/context-fragments/README.md` for controlled data,
limits and reproduction. Proximity scoring is retained with its neutral corpus
result documented in `results/native-implementation/context-proximity/README.md`.

## Acceptance

Recommendation 12 is accepted for every requirement that can be established with
source evidence: matched offsets, term coverage, bounded proximity, declaration
identity plus matching body, relationship sites, complete small functions and
labelled intervals for larger ones, marginal-value-per-byte packing, overlap and
duplicate-line removal, metadata/edge budget reservation, one source read per
request, and admission/rejection before materialization. Complete-region
coverage, citations and response bytes are now thresholds in the release gate
decision rather than ad-hoc measurements.

The one requirement this audit cannot establish is model answer/patch success:
there is no model driver or blind reviewer in this environment, so the gate
records `model_task_success: not_measured` and returns `conditional_pass` instead
of a release. That is the intended division of labour between recommendation 12
(source selection) and recommendation 30 (release decision); closing 12 does not
assert answer success.


## Initial source admission certificate

Ranker 21 measures metadata before final source assembly. If an item's metadata
already exceeds the existing item-admission allowance, it cannot be rescued by
removing its snippet, so its source need not be read. This preserves the existing
ranked admission rule while retaining read allowance for later candidates.

For a metadata-admitted item, every useful primary requires at least one line, a
positive line number and the full SHA-256 string. The serialized growth from `None`
to that smallest possible snippet is a lower bound on any primary. Another lower
bound contains mandatory response fields, the admitted prefix's item metadata and
JSON separators. Its temporary context excludes source identities and package
tables; edges and nonzero statistics are excluded too. Those optional/growing
fields cannot invalidate the lower-bound argument. The staleness list is normalized
exactly as in final assembly before its size contributes to the bound.

Final fitting strips source before dropping items from the tail. Therefore, if a
primary plus its necessary metadata prefix exceeds the cap, that primary cannot
survive fitting even after duplicate source and optional relationships are removed.
Prior primary/excerpt bytes are not included in this metadata floor. This avoids
rejecting source solely because overlaps may later shrink. Exact serialized fitting
still decides actual admission after reading, including escaping and real line
lengths. Skipped reads receive a byte-limit notice, not a fictitious source-read
budget failure. Cancellation is checked before every candidate's assembly.

These guards apply to final source materialization. Freshness verification and
candidate-generation reads retain their own correctness and work-budget contracts.
