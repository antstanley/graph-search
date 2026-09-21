# graph-search

**Status: v1 implemented (milestones M1–M5). The task-based [evaluation suite](evaluation/README.md) is available; controlled model trials for §16 remain pending. Accuracy research and targeted fixes are documented in [`research/README.md`](research/README.md).**

An experiment in a single, context-efficient search capability for code and
context: one surface for finding **files**, **text**, and **symbols and their
relationships**. It is a deliberate attempt to fold three things a coding agent
does constantly — glob, grep, and "what calls this / what breaks if I change
it" — into one query interface, backed by a local graph index.

It exists to answer one question before anything is changed in
[nanus](https://github.com/antstanley/nanus):

> Does a unified, graph-backed `search` reduce the number of round-trips and the
> tokens a coding agent spends finding code and context, without hurting the
> quality or honesty of its answers?

## The library is the product

The deliverable is an **in-process Rust library** — the `graph-search` crate —
that exposes an `Index` and a `SearchService` ([`SPEC.md`](SPEC.md) §4.7). That is
the shape a `nanus` tool would link, so it is what we build and what the
evaluation measures (shape 3, [`SPEC.md`](SPEC.md) §11.1). The `graph-search`
binary is a thin client over the library: just enough to drive the experiment
through `bash` before any integration.

## What it will do

- Index a workspace with **tree-sitter** for Rust, TypeScript/JavaScript, and
  HTML/CSS; store the resulting graph in an embedded **Grafeo** database.
- Answer `files` (glob), `text` (grep), and `graph` (definitions, references,
  callers, callees, impact, imports, paths) queries, plus a bounded combined
  `explore` query — through one library API, and one CLI command.
- Keep the index fresh **without a daemon**: an explicit `index`/`sync`, with a
  lazy, staleness-aware reconcile.
- Emit stable JSON from the CLI so an agent can parse it, and always report caps,
  truncation, and staleness rather than answering with a confident guess.
- Bound file/text library results and complete JSON envelopes to 64 KiB, with
  explicit omissions and source identities for retained text hits.
- Bound status and sync/index JSON reports while preserving exact totals; large
  path/detail lists report omissions, and required sync metadata is checked before publication.

## Non-goals (v1)

No daemon, no file watcher, no socket service, no embeddings/vector search, no
MCP server, and no change to `nanus` itself. See [`SPEC.md`](SPEC.md) §2.

## Layout

```
crates/
  types/         canonical value types (ids, nodes, edges, requests, results)
  core/          the domain, ports, projector/reconcile, query engine — pure
  langs/         tree-sitter extractors (adapter)
  engine/        the embedded Grafeo store (adapter)
  graph-search/  THE LIBRARY: Index + SearchService; wires the adapters
  cli/           a thin client over the library: the `graph-search` binary
```

The dependency edges point inward: `core` depends only on `types` and its own
ports; the engine and the parser are adapters wired by the library, never named
by `core`.

## Building

```
cargo build --release
cargo test --workspace               # unit, fixture, conformance, contract, property
target/release/graph-search --help
```

## Trying it

```
graph-search --root . index                     # full build (tree-sitter + Grafeo)
graph-search --root . status                    # counts, staleness
graph-search search files "crates/core/src/*.rs"
graph-search search text "WriteBatch" --include "*.rs"
graph-search search symbol "SearchService" --json
graph-search search callers "Index::sync" --json
graph-search search occurrences "Index::sync" --json
graph-search search occurrences "send" --by name --rel calls --json
graph-search search explore "how does reconcile classify a modified file"
graph-search search explore "SearchServ" --intent name-prefix --explain
graph-search search explore "cache invalidation" --all-terms --ranking body
graph-search search explore "cache invalidation" --ranking metadata --normalization bm25f
graph-search search explore "where gethttpresponse" --analysis identifiers
graph-search search explore "cache invalidation" --intent phrase
graph-search search explore "cache expired" --intent phrase --phrase-gap 3
graph-search search explore "expired cache" --intent near --near-window 8
```

`explore` uses indexed source regions for multiword discovery, with metadata
fallback when no body candidate matches. Single-token queries combine metadata
and body retrieval while preserving exact-name priority. Explicit name, ID, path
and whole-name prefix modes protect navigation intent. `--explain` reports the
executed routes and supporting channel ranks. Source excerpts are hash-verified
and packed into bounded, labeled intervals; read/work/output limits are reported.
After structural context, remaining space can show matching lines from up to four
regions of each selected function or document, with nearby context when it fits.
Extra regions favor missing query terms and distant matches without changing
which owners rank highest. Excerpt allocation rewards query terms not yet shown,
relative to added bytes. Omitted matching regions are reported.

Indexed evidence includes manifest-owned package context when known. Wire version
4 keeps single-use identities inline and shares repeated identities through
`context.packages` and `evidence.package_ref`. Keys are local to one result;
Rust callers can use `evidence.package_identity(&result.context)` to resolve either
form. Manifest path and hash distinguish same-named packages. Live replacements
omit unverified associations, and ambiguous/unavailable scope is reported.

Opt-in `--analysis identifiers` adds whole-identifier and qualified-name evidence
to conceptual retrieval. Split-term analysis remains the default. Metadata scoring also offers
`--normalization bm25f`: normalize each field's term frequency before combining
its weight and applying saturation. It preserves the existing clipped IDF,
exact-name priority, and budgets. Combined-length normalization remains the
default after the controlled evidence evaluation. This option does not change body
or positional scoring, or force automatic queries onto the metadata channel.
`--graph-context` controls enrichment independently of lexical ranking. The
default `semantic` connects calls, references, type uses, imports, implementations
and inheritance. `calls`, `imports` and `types` restrict connections to those
families; only `semantic` and `calls` include caller-impact summaries. `none`
returns lexical evidence without connections, bridge nodes or impact traversal.
Connections may traverse either direction, but returned edges keep their original
direction. `--explain` reports the chosen policy. This also supports graph/no-graph
comparisons at the same candidate and source budgets.
Explicit `--intent phrase` verifies ordered whole lexemes, with at most
`--phrase-gap` intervening tokens in total (default 0). `--intent near` permits
any order within `--near-window` tokens including the endpoints (default 8).
Both preserve repeated query positions and stopwords, use Unicode lowercase,
and treat punctuation as separators except underscores. They verify captured
source across storage windows; they do not fall back to name or metadata hits.
Discovery analyzer, channel, and Boolean switches do not alter these predicates.
Matches are grouped by the smallest enclosing indexed declaration when its
source hash agrees, otherwise by file. Results use deterministic file and source
order, with optional file diversity; work limits can make enumeration partial.

Rust and JS/TS call resolution checks lexical bindings before workspace names,
so local variables and parameters do not invent calls to unrelated functions.

The native research implementation and remaining acceptance gates are tracked in
[`research/IMPLEMENTATION.md`](research/IMPLEMENTATION.md).

After editing files, `graph-search sync` parses changed files and rebinds affected
callers using persisted extraction facts. Queries reconcile lazily and report
staleness (`SPEC.md` §6.5). Schema 2 rebuilds older indexes on the next reconcile.
See the [retrieval and incremental-sync results](research/07-lexical-and-incremental.md).

## Documentation

- [`SPEC.md`](SPEC.md) — the full specification: goals, architecture, the library
  API, domain model, indexing, query surface, output contract, CLI, safety,
  testing, and the evaluation plan.

## License

MIT (to match `nanus`), pending confirmation.
