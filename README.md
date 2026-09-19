# graph-search

**Status: v1 implemented (milestones M1–M5). The evaluation of §16 is pending. Accuracy research and targeted fixes are documented in [`research/README.md`](research/README.md).**

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
graph-search search explore "how does reconcile classify a modified file"
```

After editing files, `graph-search sync` reconciles incrementally; queries
reconcile lazily and report staleness either way (`SPEC.md` §6.5).

## Documentation

- [`SPEC.md`](SPEC.md) — the full specification: goals, architecture, the library
  API, domain model, indexing, query surface, output contract, CLI, safety,
  testing, and the evaluation plan.

## License

MIT (to match `nanus`), pending confirmation.
