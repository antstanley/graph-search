# Recommendation 10: native query-planning audit

The public typed queries and explicit explore options together implement the
requested query forms. A second string query language or wrapper enum is not
needed to obtain that behavior. Literal and graph relationship operations remain
separate named public methods; explore's multi-stage discovery reports its plan.

| Requirement | Implementation and evidence |
|---|---|
| Separate exact ID/name forms | `ExploreMode::ExactId/ExactName`, `SymbolQuery`; direct ID/bare/qualified maps. `explicit_navigation_does_not_broaden_or_spend_posting_work` checks protected routes and zero posting work |
| Path/glob navigation | `FilesQuery`, `ExploreMode::PathGlob`, compiled anchored filters; explicit path misses do not become conceptual queries |
| Raw literal text | `TextQuery` and `SearchService::text`, native line scanner; punctuation and even invalid regex syntax remain literal. Multiline queries are explicitly rejected, not silently changed |
| Ranked terms | `ExploreMode::Terms`, Auto and independently selectable metadata/body/fusion policies; default multiword body-first with metadata fallback only when empty |
| Analyzed phrase | Explicit Phrase and Near, whole-lexeme verification, distinct adjacency/window contracts, original-source witnesses and bounded work |
| Graph relationships | Typed `TraversalQuery`, `RefQuery`, `DepsQuery`, `NeighborsQuery`, `PathQuery`; named service routes. `GraphContext` chooses enrichment relations independently of lexical candidates |
| Preserve input and show routes | Queries are borrowed without mutation. CLI envelopes name the operation and original input; optional `RetrievalPlan` records original query, effective options, final terms, attempted explore routes and procedural omissions |
| Avoid every query running every stage | Exact/path/prefix return protected navigation; phrase/near dispatch before lexical discovery; explicit channel selection and graph-none skip unrelated work. Planner integration tests assert actual route lists |
| Configurable inferred exact fast path | `exact_fast_path` defaults false. The controlled planner fixture verifies early exact return when enabled and discovery fallback on a filtered exact miss. Enabling it deliberately forgoes other discovery candidates; no general quality superiority is claimed |
| Preserve meaningful literals in task prompts | Optional task cleanup removes only documented standalone terminal instructions, with quote/matching-backtick protection. No arbitrary rewriting or extraction of punctuation-sensitive literals; raw matching uses TextQuery |
| Explicit AND/OR/minimum coverage | `TermMatch::{Any,All,AtLeast}`; minimum requirements survive independent channel choice. Incomplete postings never claim unobserved conjunctive coverage |
| Rarity rather than longest-token selection | Body posting lists are processed shortest-first; AND uses shortest-list intersection/seek. No longest-string body fallback remains. Oversized queries are rejected rather than silently dropping required terms |
| Explain omissions/expansion | Final analyzed terms and original query expose normalization/identifier expansion; exact removed procedural sentences are recorded in source order. Work caps have explicit truncation records; no automatic query-term truncation is performed |

Source anchors: `types/query.rs`, `types/retrieval.rs`, `core/query.rs`,
`core/query_positional.rs`, `core/query_policy.rs`, `core/body.rs`,
`core/metadata.rs`, `core/text_search.rs`, `graph-search/service.rs`, and CLI
argument/rendering modules. Unit/public/CLI coverage includes 12 planner tests,
phrase/near source verification tests, literal scanner tests and 21 CLI contracts
in the 360-test workspace run. The subsequent matching-backtick correction passed
focused policy tests and final workspace/all-target strict Clippy.

The task policy was also compared with verbatim input in 348 equal-budget public
API trials. It gains two complete-evidence tasks on the exposed established set
but loses partial evidence on four tasks. Two newer suites are unchanged because
they contain no matching suffix. The decision is to retain verbatim default and
explicit task cleanup; this is an evaluated policy, not a claim that all prompts
benefit or that model task success improved. See
[the full comparison](results/native-implementation/task-policy-comparison/README.md).

Recommendation 10 is complete for its typed routing, intent protection, observable
term policy and initial evaluation. This does not close the separate candidate/
context acceptance gates (11/12), fresh broad evaluation (30), or measured scale
requirements. Graph precision and package/framework semantics remain 20/21; the
planner cannot turn an unresolved or unsupported graph fact into a correct edge.
