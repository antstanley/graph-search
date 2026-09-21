# Graph context with faithful native API delivery

The final evaluation adapter delivers compact native API JSON. It preserves every
returned item, impact summary, edge, provenance field and source line, and does not
expand the response into a different text representation. Evidence accounting
reads actual snippet/excerpt text and checks source coordinates/content and supplied
hashes. This capture supersedes the two rendering diagnostics for graph-policy
interpretation; their historical source-coverage results remain separately recorded.

## Design and correctness

The semantic-graph arm and graph-disabled control differ only in the default
`GraphContext` value in a disposable control build. Lexical ranking, candidate
limits, source policy and all request budgets remain fixed: eight final seeds,
one graph hop, four tool calls, 16 KiB per response and 48 KiB cumulative context.
The capture uses the 34 established tasks plus 12 fresh-routing and 12 README tasks.
All 348 trials complete without errors. Production, executable, sibling-source and
sibling CodeGraph-index fingerprints remain stable. Repeats have identical evidence,
delivered lines and actions; paired actions agree too.

Every one of the 348 first-query responses is complete JSON, none is transport
truncated, and the largest is 16,349 bytes. Across the 58 repeat-zero pairs, selected
node order, full node metadata, retrieval provenance and source-evidence metadata
are identical. All 464 common-node package associations agree; references resolve,
no orphan package entries remain, and returned edge endpoints are present.

## Evidence results

| Suite | Tasks | Files found, both arms | Complete regions, both arms | Graph mean coverage | No-graph mean coverage |
|---|---:|---:|---:|---:|---:|
| Established | 34 | 28 | 12 | 48.974474% | 48.974474% |
| Fresh routing | 12 | 12 | 10 | 84.722222% | 87.500000% |
| Markdown/README | 12 | 10 | 8 | 77.083333% | 77.083333% |

One task changes labelled evidence: `nanus.fresh-runtime-tools.debug` delivers
1/6 of its required region with graph mode and 3/6 without it. That query returns
no connecting edges and five impact summaries; it demonstrates a metadata/source
budget tradeoff rather than improved region coverage from graph context. There
are no measured labelled-evidence gains. Full-protocol source-line sets differ on
50 tasks; all additions/removals are preserved in `protocol-line-changes.json`.

Graph mode delivers 36 directed edges and 215 impact summaries across the 58
first queries; graph-disabled mode delivers neither. All 36 edges have source-site
coordinates and 27 of those coordinates occur in the first response's delivered
source lines. This is a source-site inclusion diagnostic, not proof of semantic
edge correctness or model answer usefulness. Resolver precision is a separate
requirement. The graph arm records 9,563 graph-work entries versus zero in the
control; these are work counters, not latency/RSS measurements.

The existing `relationship_evidence_recall` metric requires complete source regions
supporting each labelled relationship. It does not grade returned graph edges or
caller summaries. This deterministic evidence protocol also does not produce model
answers. Consequently, unchanged complete-region counts do not establish that the
extra graph information is useless, and its presence does not establish a quality
gain. Default `Semantic` and explicit `None`/relation-specific modes are retained;
no universal graph-policy switch is made from this corpus. Independent answer and
relationship-correctness evaluation remain open.

## Reproduction and validation

```sh
python3 research/scripts/markdown_context.py --representation graph_context --output /tmp/graph-context-review --repeats 3 --suite research/fixtures/fresh-routing-2026-09-19 --suite research/fixtures/markdown-readme-2026-09-20
```

Exact intervention and fingerprints are in `build.json`. Freeze production and
drivers during capture; historical comparisons require matching sibling sources.
Sibling CodeGraph indexes are never rebuilt. `native-delivery-checks.json` and
`native-identity-checks.json` record the complete-response and identity checks.

All 28 Python evaluation tests pass, including exact-budget JSON delivery,
truncation without false evidence, hash drift, metadata retention, source gaps,
candidate extraction, no renderer source reads and input immutability. This change
adds no dependency and changes no native API, ranker or storage policy. The latest
unchanged native implementation passed 393 workspace tests and strict Clippy in the
preceding neighborhood-reuse increment.
