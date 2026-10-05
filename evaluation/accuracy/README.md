# Oracle-scored accuracy benchmark

Measures how *accurate* graph-search's answers are on real repositories,
scored against independent oracles rather than hand-written labels, so it
scales to any number of Python, TypeScript/JavaScript, Rust and OKF v0.2
repositories. It complements the task suite in [`../`](../README.md), which
is hand-labelled and limited to three repositories.

It records no timings. Criterion measures time, not correctness, so it has no
role here; use the Criterion benches in `crates/graph-search/benches/` for
latency (for example `GRAPH_SEARCH_BENCH_REPO=<root> cargo bench -p
graph-search --bench evaluation -- repo`).

## How it works

1. `host/` is a JSONL server over the public library: one resident `Index` per
   repository, indexed into a disposable store under `/tmp`. Repositories are
   never written.
2. The host lists the walked files. Each language's oracle reads exactly
   those files, so walk policy is held constant and only extraction, binding
   and ranking are measured.
3. `run.py` samples queries with a fixed seed (`<repo>:<lang>:1729`), issues
   them in-process and scores the answers. `report.py` pools the results.

| Oracle | Built on | Independent of graph-search's |
|---|---|---|
| `oracles/python_oracle.py` | CPython `ast` | tree-sitter-python |
| `oracles/ts_oracle.cjs` | TypeScript compiler parser (`typescript` 6.x, syntactic only) | tree-sitter-typescript/javascript |
| `oracles/rust-oracle` | `syn` 2 with span locations | tree-sitter-rust |
| `oracles/okf_oracle.py` | a CommonMark-subset reader | tree-sitter-okf |

## Tasks and metrics

**Comparable definitions.** Python: module- and class-level functions,
methods and classes. TS/JS: function declarations, classes, class methods and
accessors, function-valued class properties and `const` bindings, interfaces,
type aliases, enums. Rust: functions, methods, structs, enums, unions, traits,
type aliases, consts, statics, `macro_rules!`, and associated consts/types.
Items nested in function bodies are recorded (they count for name uniqueness
and as legitimate for precision) but not sampled. A graph-search node matches
a definition when path and name agree and the oracle's name line falls inside
the node's span.

| Task | Query | Score |
|---|---|---|
| Extraction | `neighbors file:<path> --rel contains --hops 4` on up to 60 files per language | recall of oracle definitions (falling back to `symbol` when the 64 KiB result cap truncates a listing; a definition found only that way in an untruncated listing is a *containment gap*); precision of graph-search definition nodes over untruncated listings |
| Symbol | `symbol <name>` (limit 500) for up to 300 definitions | found anywhere; rank 1 overall and for names unique in the repository |
| Callers | `callers <id>` (depth 1) for up to 150 targets | call-site recall and caller precision, broken down by call shape |
| Binding | `callees <enclosing method>` for `self.f()` / `this.f()` / `Self::f()` inside a method of X, where X defines f and f's name is defined more than once in the repository (up to 150 pairs) | bound to X's f (recall); correct among bound (precision) |
| Explore | first sentence of the definition's doc comment, its own name removed, `k = 8` | definition hit@1/3/8 and MRR; file hit@8 |

Queries are not sampled from `.d.ts` files (ambient declarations, mostly
generated and often duplicated per package); their definitions still count for
name uniqueness and as legitimate for precision. Byte-identical files are
treated as the same file when scoring.

**Caller targets** are functions/methods whose name is defined exactly once in
the repository, is *distinctive* (≥ 6 characters with an underscore or a
camel-case hump), and is not a builtin/standard-library method name. Every
oracle call site of that name is then taken to target it, except a bare
`f()` for a method and `self.f()` for a free function. A site is recalled when
some returned caller lies in the same file, spans the call line, and is one of
the site's enclosing named functions (or is the file, for module-level code).
This is a name-based oracle: it cannot see aliased imports or dynamic dispatch,
so it slightly *understates* recall where graph-search binds an alias, and
treats every distinctive same-name call as a true site. Because targets are
uniquely named, caller precision here can only catch callers with no matching
call at all; the **binding** task is the precision test on ambiguous names.
Callers graph-search finds in files no oracle reads (Svelte/Vue/Astro scripts,
HTML) are left out of precision and counted separately. A corpus made of many
near-identical copies of one project defeats name-based oracles (no name is
unique); report such a repository with `report.py --exclude-from-pool`.

**Explore** queries are doc-comment text, which graph-search also indexes, so
this measures retrieval from a natural description with lexical overlap; it
is not a paraphrase benchmark.

**OKF** (opt-in per repository, `--okf`): concept lookup by `title`, section
lookup by heading text (path and line must match), `links_to` and `cites`
pairs (collected from each target's incoming edges via `neighbors`/`deps`),
and explore by frontmatter `description`.

## Run

```sh
CARGO_TARGET_DIR=/tmp/accuracy-host-target cargo build --release --manifest-path evaluation/accuracy/host/Cargo.toml
CARGO_TARGET_DIR=/tmp/rust-oracle-target cargo build --release --manifest-path evaluation/accuracy/oracles/rust-oracle/Cargo.toml
# repos.json: {"name": "/absolute/root", ...}
python3 evaluation/accuracy/run.py --repos repos.json --out evaluation/accuracy/results/<date>
python3 evaluation/accuracy/report.py evaluation/accuracy/results/<date> [--exclude-from-pool REPO ...]
```

If a file makes indexing abort, `--exclude-aborting` excludes directories
named like its parent and retries (graph-search excludes match directory names,
not paths); `--exclude-dir REPO:NAME` pre-seeds exclusions found earlier. Both
are recorded in the results and stated in the report.

`diagnose_rust_callers.py <results>` classifies missed Rust caller sites (macro
argument / receiver shape, inside or outside `#[cfg(test)]` modules).

`TYPESCRIPT_PATH` selects the `typescript` package the TS oracle loads.
Each repository writes `<repo>.json` (metrics) and `<repo>.failures.json`
(every miss, with the query and top answers) for auditing.
