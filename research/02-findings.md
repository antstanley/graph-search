# Findings and implemented fixes

The accuracy failure has multiple layers. A better ranker cannot recover a missing definition; a correct definition cannot compensate for a call attached to its file; a graph traversal cannot repair an edge invented by stripping a receiver name. The most useful separation is **candidate coverage → reference extraction → binding → retrieval → graph assembly → freshness and output contracts**.

## Why the current ranker fails even with correct symbols

`score_against` takes the best match against any individual query term, adds 0.1 for each additional matching term, and saturates at 1.0. For `load pds secret`, the intended `loadPdsSecret` name scores 0.9 (three substring matches), while a generic symbol named `secret` scores 1.0 from one exact term. With four matching terms the intended name can reach 1.0, but many candidates tie and path/line ordering becomes the decision. This is a deterministic ranking failure, not a graph traversal failure.

The body fallback considers only the longest query term. That term can be a generic verb or a punctuated word, and the scan is limited to a prefix of walked files. A file seed also consumes one of the same eight slots as a precise symbol. FTS in this research changes both term analysis and document-frequency-aware ranking; the experiments do not independently isolate which component explains each gain. A useful next ablation is tokenization-only, BM25-only, then their combination.

## Confirmed defects repaired on this branch

### F01 — JavaScript/TypeScript calls belonged to files, not functions

**Impact: critical for graph accuracy.** `JsExtractor::scope` stores `(qualified_name, fact_key)`. `reference()` read the first element as a fact key. `edges_for_extraction()` could not find that key in `symbol_ids` and fell back to the file ID. Consequently every baseline call in blogwright (6,091) and whatsurvey (16,805) was file-owned. A call could be resolved yet be invisible to `callees(function)` and show a file as its caller.

**Fix:** use the actual scope key. Named functions, methods, and callable declarations now own their references. Genuine top-level calls and calls in anonymous callbacks without their own extracted symbol may still be file-owned; file ownership is not inherently an error.

**Evidence:** controlled `imported`/`entry` fixture, complete ownership counts, source-confirmed application edges, and `js_calls_belong_to_functions_and_nested_receivers_are_visited`.

### F02 — Calls inside receivers were skipped

Both Rust and JS call visitors walked argument expressions but not the callee/receiver subtree. `factory().run()` recorded the outer call but lost `factory()`. Longer call chains lost upstream construction and transformation steps.

**Fix:** visit the callee subtree as well as arguments. Preserve the outer call as dangling when its receiver type is unknown. Separate Rust and JS regression tests verify the inner call is present.

### F03 — Nested qualified names duplicated ancestors

Both extractors concatenated every already-qualified scope prefix. At depth three, a name could become `a::a::b::leaf` or `outer.outer.inner.leaf`.

**Fix:** extend only the immediately enclosing qualified name. Existing fact-key parent relationships remain intact. A nested Rust-module and JS-function test checks exact symbol lookup.

### F04 — Arbitrary qualified suffixes invented edges

The resolver progressively stripped `external::leaf` to `leaf`, then looked in a single-value global qualified-name map. A call into an unknown module could resolve to the first unrelated workspace function. This is a false positive, not just low recall.

**Fix:** do not discard an unverified module/receiver prefix. A qualified name must have a unique, kind-compatible whole-name match. Same-file resolution also retains duplicate candidates instead of silently taking the first stored row. Name candidate indexes keep this check from becoming a scan of all symbols per reference.

**Tradeoff:** some plausible but unsupported links now remain unresolved. The old unit test explicitly expecting unverified suffix resolution was changed to require a dangling result. A lower resolution percentage here is an honesty improvement; a real Rust module/binding resolver remains a separate capability.

### F05 — Rust `self.method()` lacked even lexical owner resolution

A Rust member call is spelled with a dot, while extracted methods use `Type::method`. Exact string matching could not connect `self.run_step()` in `AgentRunner::run_turn`.

**Fix:** simple `self.member` calls within a known lexical owner are emitted using that owner. This recovers ordinary inherent-method links without guessing arbitrary variable types. Cross-owner duplicate method names are tested. It is not general receiver inference or trait dispatch.

### F06 — Named JS import provenance did not reach call references

Import declarations recorded exported names, but calls retained local aliases without an import environment. Moreover, file-level `Imports` facts were treated as module specifiers before their `via_import` provenance could be considered. Imported symbols could dangle or accidentally resolve through a globally unique name elsewhere.

**Fix:** collect local-name → (specifier, exported-name) bindings for named imports; attach provenance after the file walk so source order does not matter. Resolve explicit provenance authoritatively, including named import facts. A failed explicit import does not fall through to an unrelated global definition. Default imports, namespace imports, re-export chains, and TS path aliases still need richer binding support.

### F07 — `.js` specifiers did not resolve to TypeScript source

The resolver tried `x.js.ts`, not `x.ts`, for a source import such as `./x.js`. Blogwright uses runtime-extension imports extensively.

**Fix:** add `.js → .ts/.tsx`, `.mjs → .mts`, and `.cjs → .cts` source candidates while preferring an existing exact file. Bare package specifiers are no longer treated as relative local paths. Package exports and tsconfig path mappings remain unsupported.

### F08 — Callable parameters incorrectly bound to same-named functions

`fn entry(leaf: fn()) { leaf(); }` could report a call to a workspace `fn leaf()`, despite the parameter shadowing it. The same problem occurs in JS/TS.

**Fix:** extract callable-parameter shadowing as a dynamic reference and preserve it as dangling. Explicit import binding does not override this marker. This is a deliberately narrow fix: destructuring, local variable flow, captured closures, and all language scope rules require a binding model, not more suffix heuristics.

### F09 — TSX was parsed with the TypeScript grammar

The TS extractor claimed `.tsx` but always selected `LANGUAGE_TYPESCRIPT`; the TSX helper was unused.

**Fix:** select the TSX grammar for `.tsx`. A JSX expression containing a call verifies extraction through the correct grammar.

### F10 — Symbol limits ran before path/language filters

The store selected the first N names, then `QueryEngine::symbol()` applied filters. With two `same` definitions, limit 1, and a filter for the second file, the result was empty although the requested definition existed.

**Fix:** filter the candidate set before limiting. Preserve store relevance ordering, report truncation, and count pre-limit candidates.

### F11 — `symbol` did not honor its advertised exact-ID input

Traversal target resolution recognized IDs, but symbol lookup always used a name search.

**Fix:** try ID lookup first, while still applying kind/path/language filters. The returned ID can now be round-tripped through `symbol`.

### F12 — Ambiguous graph targets silently selected one definition

`resolve_target()` requested just one name match. `callers("run")` or `impact("saveSurveyDraft")` could answer for an arbitrary first definition.

**Fix:** detect multiple matches and return an `Ambiguous` error directing the caller to use `symbol` and an exact ID. This intentionally changes behavior for ambiguous names. A filter currently restricts returned graph nodes; it is not a replacement for explicit target disambiguation.

### F13 — Graph filters were ignored or silently malformed

`refs`, `callers`, `callees`, `deps`, and `neighbors` accepted filters but did not consistently use them. Invalid graph globs quietly became no matches. Unknown CLI languages silently became no language filter.

**Fix:** validate graph globs, filter nodes before limiting, and filter edges to permitted endpoints (dangling edges can remain attached to permitted sources). Apply the shared assembly to graph modes. Reject unknown CLI languages. Traversals may pass through nodes outside the output filter; output is filtered afterward.

### F14 — Result limits were silent and direct struct fields bypassed ceilings

Several graph operations truncated nodes but returned no truncation notice. Public request fields and CLI assignments could bypass constructor clamps.

**Fix:** enforce graph result/hop ceilings at execution, report node truncation, and preserve candidate counts. Explore also clamps seed count. This bounds result size/depth, not the total amount of graph work; explicit visited-node/edge/time budgets are still recommended below.

### F15 — Identity paths were empty and path order was lost

A shortest path from a symbol to itself returned no node. Reconstructed paths were sorted by ID afterward, destroying traversal order.

**Fix:** return the endpoint for a zero-edge path; retain reconstruction order for ordinary paths. Edge direction remains stored explicitly even though path search is undirected.

### F16 — Explore body hits bypassed filters and indexing policy

The body-scan branch created file seeds without applying graph filters and used a new default walk policy rather than the index policy. A Markdown hit could appear in a Rust-only query, and explicitly excluded files could re-enter results.

**Fix:** apply path/language filters to body seeds; pass the index policy into `explore_with_policy`. The convenience `QueryEngine::explore` retains a documented default-policy wrapper for direct callers. File hits now carry their match line and a bounded snippet instead of a location with no evidence.

### F17 — Explore scan caps were invisible

Only the first 512 walked files and 8 MiB were scanned, but omitted coverage was not reported. The byte check happened before reading without checking the next file's size.

**Fix:** emit separate scan truncations, check the next file against remaining scan bytes, report files scanned, and retain the full pre-limit candidate count. Ranking is still biased by this bounded scan's prefix; an indexed lexical retrieval arm is the proposed replacement.

### F18 — Explicit symbol punctuation caused misses

Queries such as `run_turn()` searched for the parentheses as part of the term; very short symbol names were also discarded.

**Fix:** normalize boundary punctuation and allow short names for a single symbol-shaped query. Broad multi-term stop-word/punctuation normalization was experimentally rejected after a task-language regression. Multi-term scoring remains the baseline heuristic pending a proper ranker.

### F19 — Explore expanded paths but discarded bridge nodes

The engine expanded to `hops`, then retained only edges directly between original seeds. `entry → middle → leaf` could not explain the relationship between entry and leaf at two hops.

**Fix:** select bounded shortest connections between seeds over semantic relationships and include the intermediate source nodes. Containment is excluded from this connection stage to avoid treating unrelated functions in one file as a meaningful call path. Bridges are subject to result and byte caps. The separate general `path` mode retains its general relationship semantics.

### F20 — Explore caller summaries counted edges as callers

`total_callers` used the number of expanded edges, which overcounts converging/cyclic paths. Direct counts also need symbol identity, not path multiplicity.

**Fix:** count distinct caller nodes, excluding the seed. This remains an approximate static caller count, not evidence that all runtime callers were found.

### F21 — Impact was a discovery tree, not the reported cone

The BFS skipped edges whose source had already been visited, dropping converging relationships. Its ranking comment promised ring order, but the code ranked an 8-bit degree then sorted the final list by ID. Truncation was not reported and the CLI discarded impact truncations.

**Fix:** retain all inspected cone edges, track first-arrival distance, count distinct nodes per filtered ring, rank by depth then full-width degree and stable location, cap top nodes, filter displayed edges, and render truncations. A diamond-shaped call graph tests the missing-edge case.

### F22 — Explore's byte budget excluded most of the payload

Only individual item serialization was counted; edges and metadata were omitted, and the CLI pretty-printed a larger envelope. Removing items could leave edges referring to absent items.

**Fix:** enforce the complete serialized library result budget and then the CLI JSON envelope budget, recount approximation after trimming, and remove dangling display connections. Budgets too small for metadata return a clear error. Compact JSON avoids spending the budget on indentation. Snippets also obey the ten-line hard ceiling. `max_bytes=0` uses the normal default. Global graph-work budgets remain a separate concern.

### F23 — Incremental sync lost or failed to rebind incoming references

Grafeo replaces a changed file's nodes, deleting incident edges. The projector re-resolved only changed source files, so unchanged callers lost their edges. Adding a definition did not bind unchanged dangling callers. The pre-apply symbol/file tables also retained removed targets.

**Fix:** until raw facts and reverse dependencies are persisted, any content/topology-changing sync reprojects the walked tree. A true no-op still skips reparsing. Build lookup tables only from the post-change file set and exclude removed symbols. Modification, addition, and deletion regressions verify rebinding.

**Tradeoff:** changed sync is O(project) extraction rather than O(changed files). The report's modified list reflects actual reprojection; `unchanged` is zero when everything is reprojected. This is a correctness-first interim implementation, not the desired final incremental architecture.

### F24 — Freshness did not invalidate old parser versions

Staleness compared entries against the manifest's own old versions rather than the running parser/schema version. Lazy queries could continue using obsolete projections after an extractor upgrade.

**Fix:** compare both manifest and entries with current versions. `PARSER_VERSION` is bumped to 2 for these extraction/resolution changes. A missing index now builds on the default before-query policy; explicit/no-reconcile policies still require an index.

### F25 — File/text service semantics differed by residency and path

The service resolved `query.path`, then the lower-level file/text search resolved it again. The resident file-index shortcut returned a different path basis, ignored per-query expansion of hidden/ignore flags, did not recheck on-disk changes, and lost truncation notices. CLI files did not copy its hidden/ignore flags into `FilesQuery`. A malformed text include glob silently became no filter.

**Fix:** resolve subdirectories once by calling the walker from the workspace root; serve file queries through the same walk path consistently; wire flags and reject malformed includes. This trades away the resident file shortcut until a safe invalidation/policy-aware cache exists. Graph storage is still used for graph operations.

### F26 — Calls in local initializers still bypassed the enclosing function

A source-level application check showed that fixing the scope key alone was insufficient: `const form = await requireSurvey(id)` inside `saveSurveyDraft` produced a call owned by `saveSurveyDraft.form`. Consequently function-level callers/callees still omitted that relationship. Destructured declarations also pushed each binding onto the same scope stack, incorrectly nesting sibling bindings.

**Fix:** attribute JS/TS calls to the nearest callable scope, while preserving declaration ownership for non-call references. Emit destructured bindings as siblings. The initializer regression checks both the call and exact sibling qualified names. This additional defect was discovered by checking real source relationships, after the first aggregate runs; final measurements include its repair.

## Remaining capability limits and research risks

These are not presented as fixed by the above repairs:

- **Lexical ranking:** substring scoring, a flat additional-term bonus, and path-order ties are weak for split identifiers and task questions. Scores saturate, repeated/common terms can dominate, and sibling test/helper symbols consume the small context budget. There is no IDF, learned relevance, or calibrated confidence.
- **Rust module/binding model:** crate/workspace roots, nested modules, `use` aliases, glob imports, trait implementations, generic receivers, method dispatch, macro expansion, and conditional compilation are not comprehensively resolved. The current `src/`-based file-candidate logic is especially weak for multi-crate nanus.
- **JavaScript module/binding model:** default and namespace imports, barrels/re-exports, package exports, tsconfig aliases, destructuring, higher-order functions, and framework conventions need a real binding layer. Named relative imports are the repaired subset.
- **Language coverage:** Svelte and Astro components, Vue, MDX, and other unsupported extensions are walkable file content but do not yield a full symbol graph. `copyWebhook` demonstrates a symbol-recall ceiling in whatsurvey. FTS over current symbols cannot recover a symbol that was never extracted.
- **References are not complete:** the extractor does not emit every value reference, callback registration, JSX component use, or runtime dispatch edge. `refs` is references *represented by the graph*, not a compiler reference search.
- **Callsite identity:** edge identity collapses some repeated source/target relationships. A relationship graph and an occurrence index should be distinct if every callsite must be returned.
- **Partial parses:** tree-sitter may return a tree containing error nodes. Root-error coverage is not surfaced as a rich per-file completeness signal. Zero quarantined files is not proof that extraction understood every construct.
- **Work budgets:** depth and output ceilings do not bound visited fanout, repeated dangling-edge scans, source bytes used by all phases, or elapsed time. Cancellation and graph-work budgets belong in the query/store ports.
- **Confidence/provenance:** a boolean resolved flag cannot distinguish an explicit import from a unique-name heuristic. Inbound search cannot count unknown unresolved references to the target, so zero unresolved in an inbound answer is not full certainty.
- **Freshness contract:** metadata-based freshness can miss same-size edits with preserved mtime. Library result types also do not expose a complete freshness/coverage envelope like the CLI. The repaired parser-version check does not remove these limitations.
- **Context usefulness:** the default snippet is small, often mostly a declaration; a relevant location is not equivalent to enough evidence to modify code. Longer complete-symbol snippets need a budgeted policy.
- **Storage durability:** this investigation tests normal apply/reopen and sync behavior, not fault-injected transactional atomicity of the Grafeo adapter or crash recovery. Do not infer durability guarantees from retrieval tests.

The improvement plan turns these limits into separately measurable work rather than broadening unverified name heuristics.

## Code navigation

| Area | Implementation | Regression evidence |
|---|---|---|
| JS/TS scope, imports, initializer calls | [js_common.rs](../crates/langs/src/js_common.rs) | JS calls, aliases, shadowing, initializer/sibling tests |
| Rust scope and receiver calls | [rust.rs](../crates/langs/src/rust.rs) | Rust receiver, self owner, qualified-name tests |
| Resolution candidates and provenance | [resolve.rs](../crates/core/src/resolve.rs) | Foreign qualified call, import alias, ambiguity tests |
| Query filters, paths, impact, explore | [query.rs](../crates/core/src/query.rs) | Filter, limit, identity path, diamond, bridge, byte tests |
| Reprojection and freshness | [reconcile.rs](../crates/core/src/reconcile.rs), [stale.rs](../crates/core/src/stale.rs) | Target edit/add/delete and existing no-op/rename tests |
| Public service behavior | [service.rs](../crates/graph-search/src/service.rs) | First query, subdirectory, resident file-change tests |
| CLI validation and output | [main.rs](../crates/cli/src/main.rs), [render.rs](../crates/cli/src/render.rs) | [CLI smoke results](results/cli-smoke.json) and contract suite |

The new public-library regressions are in [accuracy.rs](../crates/graph-search/tests/accuracy.rs). Research scripts and fixture labels are linked from the [research index](README.md).
