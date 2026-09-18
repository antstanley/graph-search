# graph-search

**Status: M0 — specification only. No behaviour implemented yet.**

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

## Why an experiment, and not a tool in nanus yet

`nanus` ships exactly seven tools, and `glob` and `grep` are two of them. The
hypothesis is that a single `search` tool — serving file, text, and graph
queries — is easier for a model to pick correctly and cheaper in context than a
menu of narrow tools. That hypothesis is cheap to test in a standalone binary
the agent can invoke through `bash`, and expensive to test by changing the
harness first. So this repo is built to be driven **by hand and by `nanus`
through the shell**, and integration is a decision made *after* the numbers come
in, not before.

## What it will do

- Index a workspace with **tree-sitter** for Rust, TypeScript/JavaScript, and
  HTML/CSS; store the resulting graph in an embedded **Grafeo** database.
- Answer `files` (glob), `text` (grep), and `graph` (definitions, references,
  callers, callees, impact, imports, paths) queries, plus a bounded combined
  `explore` query.
- Keep the index fresh **without a daemon**: an explicit `index`/`sync`, with a
  lazy, staleness-aware reconcile.
- Emit stable JSON so an agent can parse the result, and always report caps,
  truncation, and staleness rather than answering with a confident guess.

## Non-goals (v1)

No daemon, no file watcher, no embeddings/vector search, no MCP server, and no
change to `nanus` itself. See [`SPEC.md`](SPEC.md) §2.

## Layout

```
crates/
  types/    canonical value types (ids, nodes, edges, requests, results)
  core/     the domain, ports, projector/reconcile, and query engine — pure
  langs/    tree-sitter extractors (adapter)
  engine/   the embedded Grafeo store (adapter)
  cli/      the `graph-search` binary (composition root)
```

The dependency edges point inward: `core` depends only on `types` and its own
ports; the engine and the parser are adapters selected in `cli`.

## Building

```
cargo build            # the workspace builds at M0 with stub crates
cargo run -p graph-search-cli -- --help
```

The real command surface is specified in [`SPEC.md`](SPEC.md) §10 and lands
across milestones M1–M5 (§17).

## Documentation

- [`SPEC.md`](SPEC.md) — the full specification: goals, architecture, domain
  model, indexing, query surface, output contract, CLI, safety, testing, and the
  evaluation plan.

## License

MIT (to match `nanus`), pending confirmation.
