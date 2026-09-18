# graph-search — Specification

| | |
|---|---|
| **Status** | Draft (M0) |
| **Owner** | Ant Stanley |
| **Scope** | Repository-wide |
| **Depends on** | Tree-sitter (parsing), Grafeo (embedded graph store) |
| **Consumed by** | A human at a shell; the `nanus` agent, via `bash` |

---

## 0. Purpose and context

`graph-search` is an experiment. It asks whether a coding agent is better served
by **one** context-retrieval capability than by several narrow ones, and whether
a local graph index of the workspace earns its keep in round-trips and tokens.

Today `nanus` offers `glob` (find files) and `grep` (find text) as separate
tools, plus `read` for content and `bash` for anything else. An agent looking
for "where is this defined, who calls it, what breaks if I change it" must
reconstruct that structure by hand: several greps, several reads, and a
call-graph assembled in the model's head. This spec proposes to replace that
crawl with a single query interface that can answer file, text, and graph
questions from one place — and to measure whether it actually helps **before**
touching `nanus`.

The design borrows from two references:

- **[llm-wiki-graph](../llm-wiki-graph)** — the architectural template: truth is
  files, every index is a *projection* reconciled toward them, retrieval is a
  neutral tool API with capability negotiation, and the engine sits behind a port
  so it can be swapped. Its domain is markdown/prose; this repo's is source code,
  so its **projector is replaced** while its shape is reused.
- **[codegraph](https://github.com/colbymchenry/codegraph)** — evidence for the
  extraction model (tree-sitter, symbols + edges, per-language fallback) and for
  the product thesis that **one** strong retrieval tool outperforms a menu.

The intended end state is a `nanus` `search` tool replacing `glob` and `grep`.
The path to it runs through this binary being invoked by `bash`, so the
capability can be evaluated in situ with no harness change and a one-line
rollback.

---

## 1. Goals

1. **One query surface.** A single command for locating files, text, symbols,
   and relationships. Modes, not tools.
2. **Context efficiency.** Results are bounded, structured, line-anchored and
   ranked. Bodies are fetched by `read`, not dumped by search. Every cap and
   every truncation is reported.
3. **A fresh index without a daemon.** An explicit `index` / `sync`, plus a
   lazy, staleness-aware reconcile. No watcher, no resident process.
4. **Honest answers.** Static analysis is approximate; unresolved edges,
   staleness, and truncation are stated, never hidden. (This is the `nanus`
   house style: `grep` says when it hit its cap; `edit` refuses an ambiguous
   match.)
5. **Shell-parity for `nanus`.** `files` and `text` reproduce `nanus`'s `glob`
   and `grep` semantics exactly, so the experiment is apples-to-apples and the
   eventual swap is behaviour-preserving where it should be.
6. **Machine-readable output.** A stable JSON envelope an agent can parse from
   `bash`.
7. **Language coverage (first cut).** Rust, TypeScript, JavaScript (incl. JSX/
   TSX), HTML, and CSS.

## 2. Non-goals (v1)

- **No daemon, no file watcher.** Deferred; `index`/`sync` plus lazy reconcile
  is the v1 freshness story. (Rationale in §11.)
- **No embeddings or vector search.** Grafeo's vector/BM25 features are not
  enabled in v1; `text` remains literal-substring, matching `nanus` `grep`.
- **No MCP server.** CLI-first. An MCP surface is a later, optional adapter.
- **No `nanus` integration and no tool removal.** `glob` and `grep` stay exactly
  as they are until the evaluation (§16) says otherwise.
- **No cross-language semantic resolution** beyond the explicit, documented
  approximation in §7.4.
- **No general-purpose query language exposed to callers.** GQL/Cypher stay
  inside the engine adapter; callers use the modes in §8.
- **Not a knowledge base.** It does not author prose, and it does not index
  markdown *content* as a knowledge graph. (That is `llm-wiki-graph`'s job.)

---

## 3. Consumers and interfaces

| Consumer | Interface | Notes |
|---|---|---|
| A person | `graph-search <command>` at a shell | Human-readable output by default. |
| The `nanus` agent | `bash` running `graph-search search … --json` | JSON on stdout; diagnostics on stderr. |

The CLI is the only interface in v1. It is deliberately usable both ways: legible
enough to run by hand, strict enough to script. The `--json` payload is the
contract `nanus` depends on; the text rendering is for humans and is not a
contract.

---

## 4. Architecture

### 4.1 Crates and the dependency rule

Dependencies point inward. `core` knows nothing about tree-sitter or Grafeo; the
engine and the parser are adapters selected in the composition root.

```
                         ┌────────────────────────────┐
                         │        graph-search-cli     │  composition root:
                         │   init · index · sync ·     │  the `graph-search`
                         │   search · status           │  binary; wires adapters
                         └───────┬─────────────┬───────┘
                                 │             │
                ┌────────────────▼──┐      ┌───▼─────────────────┐
                │  graph-search-    │      │  graph-search-       │
                │  engine           │      │  langs               │
                │  (Grafeo adapter) │      │  (tree-sitter        │
                │  impl GraphStore  │      │   extractors)        │
                └────────┬──────────┘      └──────────┬───────────┘
                         │                            │
                         └───────────┬────────────────┘
                                     ▼
                        ┌────────────────────────────┐
                        │      graph-search-core      │  domain · ports ·
                        │  projector · reconcile ·    │  query engine — pure,
                        │  query engine               │  no engine/parser
                        └─────────────┬──────────────┘
                                      ▼
                        ┌────────────────────────────┐
                        │      graph-search-types     │  leaf value types
                        └────────────────────────────┘
```

| Crate | Owns | May depend on |
|---|---|---|
| `graph-search-types` | Ids, node/edge records, requests, results, the JSON contract types. | *(nothing in-tree)* |
| `graph-search-core` | Domain model; port traits; the projector (parse→graph); the reconcile (diff/apply); the query engine; the `files`/`text` walker. | `types` only |
| `graph-search-langs` | Tree-sitter grammars and per-language extraction queries; impl of core's `LanguageExtractor`. | `core`, `types`, tree-sitter |
| `graph-search-engine` | The embedded Grafeo store; impl of core's `GraphStore`. | `core`, `types`, `grafeo` |
| `graph-search-cli` | Argument parsing, config resolution, adapter construction, command dispatch, rendering. | all of the above |

A dependency from `core` onto an adapter or a vendor crate is a **defect**; the
gate should make it impossible to merge (§15).

### 4.2 Ports

Defined in `core`, implemented by adapters, vendor-free in their signatures.

```rust
/// The projected store the projector writes and the query engine reads.
pub trait GraphStore {
    fn apply(&mut self, batch: WriteBatch) -> Result<ApplyOutcome, Error>;
    fn snapshot(&self) -> Result<Box<dyn GraphSnapshot>, Error>;
    fn manifest(&self) -> Result<Option<Manifest>, Error>;
    fn commit_manifest(&mut self, manifest: Manifest) -> Result<(), Error>;
}

/// A point-in-time read view.
pub trait GraphSnapshot {
    fn node_by_id(&self, id: &NodeId) -> Result<Option<Node>, Error>;
    fn find_by_name(&self, name: &str, kinds: &[NodeKind], k: usize) -> Result<Vec<Scored<Node>>, Error>;
    fn edges_from(&self, id: &NodeId, kinds: &[EdgeKind], dir: Direction) -> Result<Vec<Edge>, Error>;
    fn expand(&self, seeds: &[NodeId], hops: u8, kinds: &[EdgeKind], dir: Direction) -> Result<Subgraph, Error>;
    /// Files whose path matches the glob, from the *indexed* set.
    fn files_matching(&self, glob: &str, k: usize) -> Result<Vec<Node>, Error>;
}

/// One language's extraction into nodes and edges.
pub trait LanguageExtractor {
    fn language(&self) -> Language;
    fn supports(&self, path: &Path) -> bool;
    fn extract(&self, file: &SourceFile<'_>) -> Result<Extraction, ParseError>;
}
```

Design notes:

- **Read/write split at the type level** (from `llm-wiki-graph`): `apply` is the
  only mutator; every read goes through a snapshot. A snapshot taken before an
  apply never sees that batch.
- **No vectors in v1.** `NodeUpsert` carries no embedding, unlike the wiki
  design. If vector search lands later it is an additive port method, not a
  change to `Node`.
- **The `files`/`text` modes do not need the store at all.** They are served by a
  filesystem walker in `core` (§8.1, §8.2). The store exists for `graph` and
  `explore`. This keeps `glob`/`grep` semantics exact (no tokenizer, no BM25
  mismatch) and means the binary is useful even with no index built.

### 4.3 The engine: Grafeo

`graph-search-engine` wraps the embedded **Grafeo** database (Apache-2.0,
pure-Rust; `GrafeoDB::new_in_memory()` / `GrafeoDB::open(path)`; GQL and a direct
API). The store lives at `<root>/.graph-search/index/` by default and is
git-ignored.

**Feature selection is a milestone-zero decision.** Grafeo's `embedded` profile
is `lpg + gql + ai(vector+text+hybrid) + algos + parallel + regex + grafeo-file +
arrow-export`. v1 needs graph + persistence and *not* `ai`; the engine crate
should enable the narrowest set that supports LPG storage, traversal, and
on-disk persistence (candidate: `--no-default-features --features gql,grafeo-file`,
confirmed against Grafeo's feature table during M2). Keeping `ai` off removes the
HNSW index, BM25, CDC and the embedding question entirely.

Two properties to preserve:

- **No Grafeo type crosses the port.** GQL strings and `NodeId`/`Value` types are
  confined to `engine`, exactly as `nanus` keeps `std::io::Error` out of its
  domain.
- **The engine is replaceable.** If an in-process adjacency map proves
  sufficient, `engine` is swapped without a change in `core` or the CLI.

### 4.4 The parsers: tree-sitter

`graph-search-langs` holds the grammars and one extraction query set per
language. Grammars are C under Rust bindings; `unsafe` inside a dependency is
acceptable — this workspace's `forbid(unsafe_code)` applies to *its own* crates.

Planned crates and the versions observed at writing (pin exactly in M3):

| Language | Crate | Observed |
|---|---|---|
| core | `tree-sitter` | 0.27 |
| Rust | `tree-sitter-rust` | 0.24 |
| TypeScript/TSX | `tree-sitter-typescript` | 0.23 |
| JavaScript/JSX | `tree-sitter-javascript` | 0.25 |
| HTML | `tree-sitter-html` | 0.23 |
| CSS | `tree-sitter-css` | 0.25 |

Grammar/abi compatibility with the core crate is verified in M3 (a language that
does not link is dropped, not debugged indefinitely). Extraction is **never**
allowed to fail a command: a file the parser cannot handle is *quarantined* (no
nodes, a recorded reason), not an erroring search (§6.4).

### 4.5 Where each concern lives

| Concern | Home |
|---|---|
| Walk + exclude policy | `core` (`ignore::WalkBuilder`) |
| Extension → language map | `core` (a table), extractors in `langs` |
| Frontmatter? none — source has none | — |
| Parse → nodes/edges | `langs` |
| Resolve references → edges | `core` |
| Diff vs manifest | `core` |
| Apply to store | `core` driving the `GraphStore` port |
| Store | `engine` |
| `files`/`text` matching | `core` (walk) |
| `graph`/`explore` assembly | `core` (read via snapshot) |
| Rendering (text/JSON) | `cli` |

### 4.6 Sourcing: paths, symbols, and bodies are three different things

The speed of a query depends on which of three kinds of data it needs, and only
two of them are in the graph:

| Data | In the index? | Serves |
|---|---|---|
| **Paths** — the set of files | yes, if the projector records *every walked path* (not only files that parse) | `files` (glob) |
| **Symbols and edges** — declarations and their call/import/reference edges | yes | `graph`, `explore` |
| **Bodies** — the raw text of each file | **no** | `text` (grep) |

The symbol graph stores a signature and a line range per symbol, never file
contents. So **`text` cannot be answered from the graph**: grep needs the bytes.
Making it index-backed would require either a separate content store with a
substring index (trigram/n-gram), or Grafeo's BM25 text index — which is
*tokenized, ranked* search and would change `grep`'s semantics (no exact
substring, no exact line/column). Those are options (§19), not the v1 design.

Two consequences shape §8:

- **Residency is the prerequisite for the speed win.** "The index is in memory"
  is only true of a process that stays up across queries. A one-shot CLI pays the
  store open on every invocation, so an index-backed `files` can be *slower* than
  a walk — and the walk is fresh by construction.
- **Staleness is a correctness risk, not only a speed one.** An index-backed
  `files` that misses a file created after the last index is a confident false
  negative — the exact failure this project exists to avoid. Any index-sourced
  answer must carry a verified-fresh guarantee or a staleness notice.

So v1 sources `files` and `text` from a **walk** and uses the index only for
`graph`/`explore`. §11's resident mode is where `files` becomes index-first;
`text` stays a scan (or gains a dedicated content index), because the symbol
graph never holds bodies.

---

## 5. Domain model

One workspace is one graph. Nodes are **files and symbols**; edges are **lexical
containment and semantic relationships**. Nothing is authored — the entire graph
is derived, so correctness is a question of convergence to the working tree
(§6.5), not of a stored source of truth.

### 5.1 Node kinds

| Kind | Source language | Notes |
|---|---|---|
| `file` | all | one per **walked** file, whether or not it parses (a file with no extractor still gets a `file` node, `language = unknown`), so a glob can be answered from the index. Carries language, size, line count, hash. |
| `module` | Rust (`mod`), TS/JS (module) | a named module; may or may not correspond to a file. |
| `function` | Rust, TS/JS | free function. |
| `method` | Rust, TS/JS | associated function / class or impl method. |
| `struct` | Rust | |
| `enum` | Rust, TS | |
| `trait` | Rust | |
| `impl` | Rust | an `impl` block, the parent of its methods. |
| `type_alias` | Rust, TS | `type` / `type alias`. |
| `const` / `static` | Rust, TS/JS | |
| `macro` | Rust | `macro_rules!`. |
| `field` | Rust, TS | struct/class member. |
| `variant` | Rust | enum variant. |
| `class` | TS/JS | |
| `interface` | TS | |
| `variable` | TS/JS | top-level `let/var` (and `const` where not a function). |
| `export` | TS/JS | an exported binding. |
| `element` | HTML | an element with an `id` and/or notable attributes. |
| `css_rule` | CSS | a selector block. |
| `css_at_rule` | CSS | `@media`, `@keyframes`, `@import`, … |
| `css_custom_property` | CSS | `--var`. |

The vocabulary is **closed** and versioned; adding a kind is a schema change
(§9.1), not an incidental one.

### 5.2 Edge kinds

| Kind | From → To | Meaning |
|---|---|---|
| `contains` | file/module/impl/class → child symbol | lexical nesting (also gives "symbols in this file"). |
| `imports` | file → file / module / symbol | `use`, `mod`, `import`, `require`, `from … import`. |
| `exports` | file/module → symbol | TS/JS export, Rust `pub use`. |
| `calls` | function/method → function/method | call site (resolved or dangling). |
| `references` | any symbol → any symbol | a name use that is not a call/import (types, consts, fields). |
| `extends` | class/trait → class/trait | inheritance. |
| `implements` | class/impl → interface/trait | implementation. |
| `type_uses` | symbol → symbol | a type position (parameter, return, field). |
| `declares` | css_rule → symbol? | *(CSS/HTML cross-edges, see below)* |
| `links_to` | HTML element → file | `href`/`src` resolving to a workspace file. |
| `loads_stylesheet` | HTML element → file | `<link rel="stylesheet">`. |
| `uses_class` | HTML element → css selector | `class="…"` matched to a `css_rule`. |
| `selects` | css_rule → HTML element | selector matched to an element `id`/`class`. |

Every edge carries `resolved: bool`. An unresolved reference is kept with the
**name it referred to** and marked dangling — never dropped, never an error
(§7.4).

### 5.3 Identity

Identity is stable under unrelated edits and **opaque-looking but reproducible**
(a hash of stable inputs), so an index can be diffed across runs without a
database migration.

| Entity | Id |
|---|---|
| File | `file:<workspace-relative-path>` |
| Symbol | `sym:<path>#<kind>:<qualified_name>[@<disambiguator>]` |
| Edge | `(<from-id>) -[<kind>]-> (<to-id-or-name>)` |

- `qualified_name` is the lexical path (`Parser::parse`, `mod.rs::Token::new`).
- `disambiguator` is the 1-based source line, used only when a file has two
  same-kind same-name symbols (e.g. two `impl` blocks) — the smallest thing that
  keeps ids unique without making them change when a file grows above them.
- File identity follows the **path**; a rename is detected by content hash in the
  reconcile (§6.3) and is reported as a rename, not a delete + add.

### 5.4 Properties

| Node | Properties |
|---|---|
| `file` | `path`, `language`, `bytes`, `lines`, `content_hash`, `parser_version` |
| symbol | `path`, `kind`, `name`, `qualified_name`, `signature` (one line, truncated), `start_line`, `end_line`, `start_byte`, `end_byte`, `visibility`, `async`, `parent` (id, optional) |
| `element` | `tag`, `id`, `classes`, `path`, `start_line`, `end_line` |
| `css_rule` | `selector`, `path`, `start_line`, `end_line` |

`signature` is a *single truncated line* (the declaration's first line, capped at
`MAX_SIGNATURE_CHARS`). Full bodies are never stored in the graph; `read` fetches
them.

---

## 6. Indexing and reconcile

### 6.1 Walk and exclusion

`core` walks the root with `ignore::WalkBuilder` and the following policy, which
**differs deliberately from `nanus`'s current walker** (which disables ignore
files and descends `target/`):

- Respect `.gitignore`, `.ignore`, `.git/info/exclude`, and global git excludes.
- Always skip: `.git/`, `.graph-search/`, `target/`, `node_modules/`, `dist/`,
  `build/`, `out/`, `.venv/`, `venv/`, `vendor/`, and any configured extras.
- Skip hidden entries unless configured otherwise.
- Skip files larger than `MAX_FILE_BYTES` (1 MiB default) and files whose
  extension has no extractor.

The exclusion policy is a **named, configurable value** (`config.toml`, §12), not
scattered calls, because an index that eats `target/` is worse than useless.

> **Behavioural note.** `files`/`text` used by `nanus` as `glob`/`grep` today do
> **not** honour `.gitignore`. This spec makes the new `files`/`text` modes honour
> it, for consistency with the index and because descending `target/` is almost
> never intended. This is a *deliberate divergence from `nanus` parity* and is
> flagged in §19 as a decision to confirm during evaluation.

### 6.2 Parse

Every walked file gets a `file` node (§5.1) — including files with no extractor,
so the index can answer `files` globs completely. A file whose language has an
extractor is additionally read and parsed: its bytes are handed to the extractor
as a `SourceFile`, which emits nodes, `contains` edges, and candidate
(`unresolved`) references. `core` then:

1. Resolves references against the workspace index (§7.4).
2. Adds `calls`/`references`/`imports`/… edges with `resolved` flags.
3. Bounds the per-file extraction: `MAX_NODES_PER_FILE`, `MAX_EDGES_PER_FILE`;
   exceeding either quarantines the file rather than truncating silently.

### 6.3 Manifest and diff

A manifest is the store's self-fingerprint, one entry per file:

```
{ path, size, mtime_ns, content_hash, parser_version, schema_version }
```

Reconcile classifies every path against the stored manifest into exactly one
bucket:

| Class | Test | Effect |
|---|---|---|
| `added` | not in manifest | parse, insert |
| `modified` | hash differs | re-parse, replace subtree |
| `removed` | in manifest, gone from tree | delete nodes + incident edges |
| `renamed` | hash matches a removed entry | update path, keep nodes' identity where possible |
| `unchanged` | hash equal | skip (the O(1) no-op path) |
| `quarantined` | parse failed | record; do not fail the run |
| `unchanged-global` | `parser_version`/`schema_version` changed | re-parse all |

### 6.4 Apply

The delta is applied as one atomic `WriteBatch`. **The manifest is committed
last**, after the apply succeeds: an interrupted run leaves the old manifest, so
the next run recomputes the same delta from content hashes. Correctness comes
from convergence, not from transactional safety.

### 6.5 Freshness

v1 has no watcher. Freshness is provided by four mechanisms, in increasing cost:

1. **`index`** — parse everything, build from scratch (verify/repair).
2. **`sync`** — reconcile against the manifest; the normal way to keep current.
3. **Lazy reconcile on search.** `search graph|explore` performs a cheap
   `(size, mtime)` scan (no hashing) against the manifest. If it differs, the
   index is **stale**: the result carries `"stale": true` and a `stale` block
   naming the changed paths, and — unless `--no-reconcile` — the command
   reconciles first. `--fail-if-stale` turns staleness into exit code 3.
4. **Staleness after mutation.** When the agent edits a file and searches in the
   same turn, mechanism 3 catches it at the next query. (With `nanus` driving via
   `bash`, the `edit`/`write` happen through the shell or the tool, so the only
   robust trigger is the next `search`.)

`sync` is therefore the *correct* thing to run after a burst of edits, and the
lazy check makes forgetting it safe rather than silently wrong.

### 6.6 Concurrent writers

`index`/`sync` take an OS advisory lock on `<store>/index.lock`. A second writer
is refused with a sentence naming the holder — the same posture as `nanus`'s
session claim. Readers do not lock; a reader during an apply sees either the old
or the new revision (the engine's snapshot isolation), never a half-applied
batch.

---

## 7. Language extraction and resolution

Each extractor is a tree-sitter query set plus a small amount of Rust that turns
captures into `Node` and `Edge` records. Extraction is declarative where it can
be and tested per language with fixtures (§15).

### 7.1 Rust

| Node | Capture |
|---|---|
| `function` | `function_item` (free) |
| `method` | `function_item` inside an `impl_item` |
| `struct` / `enum` / `trait` / `type_alias` / `const` / `static` / `macro` | matching items |
| `impl` | `impl_item` (parent of its methods) |
| `field` | `field_declaration` |
| `variant` | `enum_variant` |
| `module` | `mod_item` |

| Edge | Source |
|---|---|
| `contains` | item nesting (`file`→item, `impl`→method, `struct`→field) |
| `imports` | `use_declaration` (also `mod x;` → `x.rs`/`x/mod.rs`) |
| `calls` | `call_expression` → callee path |
| `type_uses` | parameter/return/field types |
| `implements` | `impl Trait for Type` |
| `references` | path expressions not already a call/type |

### 7.2 TypeScript / JavaScript (incl. JSX/TSX)

| Node | Capture |
|---|---|
| `function` / `method` / `class` / `interface` / `enum` / `type_alias` / `const` / `variable` / `field` | declarations |
| `export` | `export_statement` bindings |

| Edge | Source |
|---|---|
| `contains` | class → method, module → top-level |
| `imports` / `exports` | `import`/`export … from`, `require()` |
| `calls` | `call_expression` |
| `extends` / `implements` | `class … extends/implements …` |
| `type_uses` | annotations, generics, return types |
| `references` | identifier uses |

### 7.3 HTML / CSS

Neither has "symbols" in the program sense, but both carry relationships an
agent asks about ("what links this page to that stylesheet", "where is
`.btn-primary` used").

| Node | Capture |
|---|---|
| `element` | elements with an `id` or a `class` attribute (not every tag — a per-file element cap applies) |
| `css_rule` | qualified rule (selector + block) |
| `css_at_rule` | `@media`, `@keyframes`, `@import`, `@supports` |
| `css_custom_property` | `--foo` declarations |

| Edge | Source |
|---|---|
| `links_to` | `href`/`src` that resolves to a workspace file |
| `loads_stylesheet` | `<link rel="stylesheet" href=…>` |
| `uses_class` | element `class="…"` → `css_rule` whose selector names that class |
| `selects` | `css_rule` selector → element `id`/class |
| `imports` | CSS `@import` |

HTML/CSS cross-edges are **matched, not resolved**: a class name used in HTML is
linked to a CSS rule only when the selector and the class string match exactly.
False negatives are expected and reported in the result's `stats` (e.g.
`"unmatched_classes": 12`).

### 7.4 Resolution rules, and the honesty contract

Static resolution is a best effort; the spec commits to doing it the *same way
every time* and to **saying what was not resolved**.

1. **Same file, same name** — resolve to the local definition.
2. **Imports** — a reference to an imported binding resolves to the importing
   file's corresponding export, if found.
3. **Qualified paths** — `a::b::c` / `A.b.c` resolve along modules/classes when
   the first segment resolves.
4. **Global unique name** — a bare name that matches exactly one workspace symbol
   of a compatible kind resolves to it.
5. **Everything else is dangling** — recorded with `resolved: false` and the
   *referenced name*, counted in `stats.unresolved`.

Macros, conditional compilation, dynamic dispatch, re-exports, generated code,
and any language without an enabled extractor are all sources of **false
negatives**. The tool never presents the graph as exhaustive. Every `graph` and
`explore` result carries:

```
"approximation": {
  "resolved": 812,
  "unresolved": 204,
  "note": "static approximation; dynamic/macro/generated edges may be missing"
}
```

---

## 8. Query surface

One `search` command with a mode, plus `status`. Sourcing follows §4.6: `files`
and `text` are served by a filesystem walk in v1 (reproducing `nanus`'s
`glob`/`grep` semantics exactly, and fresh by construction), while `graph` modes
and `explore` are served from the index. `files` becomes index-first once a
resident store exists (§11); `text` never reads the symbol graph, because the
graph does not hold bodies.

### 8.1 `files` (glob)

```
graph-search search files <pattern> [--path DIR] [--limit N] [--hidden]
```

- Anchored to the search root; `*` does **not** cross `/` (`literal_separator`),
  so `*.rs` is top-level and `**/*.rs` is any depth — identical to `nanus`
  `glob`.
- Default `limit` 100, ceiling 1000.
- Empty result renders `No files found.`
- Truncation notice: `(more than {limit} matches; narrow the pattern to see the rest)`.

### 8.2 `text` (grep)

```
graph-search search text <literal> [--path DIR] [--include GLOB] [--limit N]
                                 [--ignore-case] [--hidden]
```

- **Literal substring, not a regex** — identical to `nanus` `grep`. `include`
  takes exactly one positive glob; a comma list or a leading `!` is rejected with
  the same guidance `nanus` gives.
- Default `limit` 250, ceiling 2000. Lines are capped at 400 characters with `…`.
- Matches are grouped by file, `path:` header then `  <line>: <text>`.
- Empty result renders `No matches found.`
- A capped search that matched nothing renders:
  `No matches in the files reached: the search stopped at the {limit}-match cap before it finished, so matches may exist beyond it.`
- Truncation notice: `(stopped at {limit} matches; narrow the pattern or the include filter)`.
- **Sourced by scanning the files** (mmap + a SIMD substring search), not by the
  index; §4.6. A `--ranked` mode backed by Grafeo's BM25 text index is a possible
  later addition with explicitly different (tokenized, ranked) semantics — it
  would not be a drop-in for `grep`.

These strings are reproduced verbatim so that the evaluation compares the same
answers and so that any later swap is behaviour-preserving.

### 8.3 `graph` — symbols and relationships

All graph modes accept `--lang`, `--path GLOB`, `--limit`, and `--json`.

| Mode | Answers | Key args |
|---|---|---|
| `symbol <name>` | where is `<name>` defined (all kinds) | `--kind` |
| `refs <name\|id>` | every reference to it | `--limit` |
| `callers <name\|id>` | direct (or N-hop) callers | `--depth 1` |
| `callees <name\|id>` | what it calls | `--depth 1` |
| `impact <name\|id>` | transitive callers/references — the blast radius | `--depth 2` |
| `deps <path\|id>` | imports / imported-by | `--direction in\|out\|both` |
| `neighbors <id>` | adjacent nodes along chosen edge kinds | `--rel`, `--hops` |
| `path <from> <to>` | shortest path between two symbols/files | `--max-hops` |

Semantics:

- `<name>` matches `name` or `qualified_name`, case-sensitive by default; a
  fuzzy/prefix match is a later option, not a default.
- `<id>` is exact.
- `impact --depth N` reports **counts by depth and by kind**, plus the top
  nodes; it does not dump the whole cone. A model asking "what breaks" wants the
  shape, not 400 lines.
- Hops are clamped to `MAX_HOPS_CEILING` (4); over-ceiling is clamped, not an
  error (matching `llm-wiki-graph`'s capability-clamp posture).
- Every result lists nodes then edges; edges carry `resolved`.

### 8.4 `explore` — the one-call retrieval

```
graph-search search explore <query> [--k 8] [--hops 1] [--context-lines 2]
                                    [--max-bytes N] [--lang L]
```

This is the context-efficient entry point and the closest analogue to
`codegraph`'s single tool. Given free-text terms:

1. **Seed** — rank files and symbols by (a) name match on query terms, (b) path
   match, (c) a bounded literal scan of candidate files. (No embedding in v1.)
2. **Assemble** — for the top `k` seeds: the definition location, its
   `signature`, and a **bounded snippet** of `context_lines` around the
   definition.
3. **Connect** — the edges among the returned nodes, up to `hops`.
4. **Summarise impact** — for function/method seeds, a one-line blast-radius
   count.
5. **Bound** — total output capped by `--max-bytes`; anything dropped is
   reported in `truncations`.

`explore` is where the context-efficiency hypothesis lives: the goal is one call
that returns *enough to act on* and no more. It must never return whole bodies by
default — that is the failure mode `codegraph` itself documents (fewer tokens
processed, more tokens resident).

### 8.5 `status`

Reports whether an index exists, its store path, schema and parser versions,
node/edge counts by kind, per-language file counts, last-index time, and
staleness (count of changed paths, computed cheaply). Exit 0 either way.

---

## 9. Output contract

### 9.1 The JSON envelope

`--json` (or `--format json`) writes exactly one JSON document to stdout:

```json
{
  "schema_version": 1,
  "command": "search.text",
  "root": "/abs/workspace",
  "query": { "pattern": "fn main", "include": "*.rs", "limit": 250 },
  "stale": false,
  "results": [ /* command-specific */ ],
  "edges": [],
  "truncations": [],
  "approximation": null,
  "stats": { "files_scanned": 42, "matches": 3, "elapsed_ms": 12 }
}
```

- `schema_version` is bumped on any change to a result/edge field; the CLI refuses
  nothing but the agent should check it.
- `command` is the dotted path (`search.files`, `search.graph.callers`).
- `query` echoes normalized arguments — the answer is reproducible from it.
- `stale` is `true` when the index was behind the tree at answer time; the
  `stale_paths` array is included when it is.
- `approximation` is present for `graph`/`explore` (§7.4).
- `truncations` is empty when nothing was dropped.

Result shapes:

| Command | Result item |
|---|---|
| `files` | `{ "path", "language" }` |
| `text` | `{ "path", "line", "text" }` |
| `symbol`/`graph` | `{ "id", "name", "qualified_name", "kind", "path", "start_line", "end_line", "signature" }` |
| `explore` | a `{ "node", "snippet": {"start_line", "lines":[…]}, "impact": {…} }` |
| edge | `{ "from", "kind", "to", "to_name", "path", "line", "resolved" }` |

### 9.2 Truncation

Nothing is dropped silently. Every cap that fired appends:

```json
{ "kind": "results", "cap": 250, "dropped": true, "message": "stopped at 250 matches; narrow the pattern or the include filter" }
```

`kind` ∈ `results | match_line | signature | snippet | files | bytes`. The text
rendering prints the same messages.

### 9.3 Snippets and the context budget

- `--context-lines` defaults to 2 for `explore`, 0 for `graph`; ceiling 10.
- `MAX_TOTAL_BYTES` (64 KiB default) caps the whole payload; when reached, the
  result is cut and a `bytes` truncation is reported.
- `signature` is one line capped at `MAX_SIGNATURE_CHARS` (200).

### 9.4 Determinism

Results are sorted by `score` desc (where scored), then `path` asc, then
`start_line` asc; edges by `from`, then `kind`, then `to`. Two runs over an
unchanged tree produce byte-identical JSON except `elapsed_ms` — which is why
golden fixtures (§15) are viable.

---

## 10. CLI reference

### 10.1 Commands

```
graph-search init                    # create .graph-search/ (config + store dir)
graph-search index [--force]        # full build
graph-search sync                   # incremental reconcile
graph-search search files   <pattern> [flags]
graph-search search text    <literal> [flags]
graph-search search symbol  <name>    [flags]
graph-search search refs    <name|id> [flags]
graph-search search callers <name|id> [flags]
graph-search search callees <name|id> [flags]
graph-search search impact  <name|id> [flags]
graph-search search deps    <path|id> [flags]
graph-search search neighbors <id>    [flags]
graph-search search path    <from> <to> [flags]
graph-search search explore <query>   [flags]
graph-search status
```

Global flags: `--root DIR` (default cwd), `--store DIR` (default
`<root>/.graph-search/index`), `--format text|json`, `--json` (shorthand),
`--no-ignore`, `--hidden`, `--quiet`, `-v`.

### 10.2 Exit codes

| Code | Meaning |
|---|---|
| 0 | Success. An empty result is a success, not an error. |
| 1 | Operational failure (store unreadable/corrupt, I/O failure). |
| 2 | Usage error (bad arguments, rejected `include`, unknown mode). |
| 3 | `--fail-if-stale` and the index was stale. |
| 4 | `--no-reconcile` and no usable index exists. |

### 10.3 How `nanus` calls it (the experiment interface)

```sh
# files (glob)
graph-search search files '**/*.rs' --limit 50 --json

# text (grep)
graph-search search text 'ToolRegistry' --include '*.rs' --json

# where is it defined
graph-search search symbol 'SearchQuery' --json

# who calls it / what breaks
graph-search search callers 'ToolRegistry::execute' --json
graph-search search impact  'SearchQuery' --depth 2 --json

# one-call context retrieval
graph-search search explore 'how does a tool call get dispatched and approved' \
    --k 8 --context-lines 2 --json
```

The system prompt line that would accompany it during the experiment is short and
explicit: *"To find code or context, prefer `graph-search search …`; run
`graph-search sync` after editing many files."*

---

## 11. Concurrency and lifecycle

**v1 is one-shot.** Each invocation opens the store, does its work, and exits.
This is deliberately the least machinery that can answer the question: no
daemon, no watcher, no resident state to keep coherent.

Consequences and their mitigations:

- **Open cost per call.** If opening Grafeo per invocation is measurably slow on
  a real repo, the fix is a resident `serve` mode — the store on a dedicated
  thread, reached over a channel — not a watcher. That design is recorded here
  and deferred to M6+: a `std::thread` owns the store; commands send requests and
  await a `oneshot`; the boundary is a channel, so callers stay `!Send`-friendly.
  (This matters for the eventual `nanus` integration: a dedicated engine thread
  does **not** require making the kernel's futures `Send`.) In a resident
  process, `files` moves to index-first (§4.6): the whole path set is a few
  megabytes and a glob is an in-memory match. `text` does not — it needs bodies,
  which the graph does not hold — so it stays a scan, or gains a content index.
- **Single writer.** `index`/`sync` hold `<store>/index.lock`; a second writer is
  refused by name. Readers never lock.
- **Atomicity.** One `apply` per reconcile, manifest last (§6.4).

---

## 12. Configuration

Optional `<root>/.graph-search/config.toml`:

```toml
# Directories and paths never walked. Defaults are shown; user values replace
# the defaults only when `replace_defaults = true`.
excludes = ["target", "node_modules", "dist", "build", "vendor", ".venv"]
replace_defaults = false
include_hidden = false

# Languages enabled for extraction. Disabling one leaves its files walkable but
# un-indexed.
languages = ["rust", "typescript", "javascript", "html", "css"]

# Extra extension -> language bindings (e.g. ".mjs" = "javascript").
[extensions]
".mjs" = "javascript"

store = ".graph-search/index"
max_file_bytes = 1048576
```

Precedence: CLI flag > config file > built-in default. No secrets, no network
settings.

---

## 13. Safety and trust

- **The workspace is untrusted input.** Every file is parsed by a C library; a
  malformed file quarantines, never crashes, and the extractor bounds its output
  (§14). A parse failure must not be able to fail a search.
- **The index is derived and disposable.** It can always be rebuilt from the
  tree; deleting `.graph-search/` is always safe.
- **No automatic loading of untrusted index state.** The store path is either the
  default under the root or an explicit `--store`; the tool does not discover and
  open an index from an arbitrary file supplied by the repository.
- **No code execution.** The tool never runs, compiles, or imports anything it
  finds; it only reads and parses.
- **No network.** Nothing in the default feature set reaches out (Grafeo's `ai`/
  `embed` features, which could, are off).
- **Symlinks** are not followed out of the root.
- **Caps everywhere.** Every loop is bounded by a named constant (§14); a
  pathological tree degrades by reporting caps, not by hanging.

---

## 14. Limits and performance

Named constants (the values are defaults, overridable in `config.toml` where
noted by †):

| Constant | Value | Bounds |
|---|---|---|
| `MAX_FILE_BYTES` † | 1 048 576 | file considered for parsing/search |
| `MAX_FILES` | 200 000 | files walked |
| `MAX_NODES_PER_FILE` | 50 000 | nodes from one file (else quarantine) |
| `MAX_EDGES_PER_FILE` | 200 000 | edges from one file (else quarantine) |
| `MAX_RESULTS` (files) | 100 / ceil 1 000 | `files` |
| `MAX_RESULTS` (text) | 250 / ceil 2 000 | `text` |
| `MAX_RESULTS` (graph/explore) | 50 / ceil 500 | `graph`, `explore` |
| `MAX_MATCH_LINE` | 400 chars | one echoed match line |
| `MAX_SIGNATURE_CHARS` | 200 chars | one stored signature |
| `MAX_SNIPPET_LINES` | 10 (default 2) | `explore`/`graph` snippet |
| `MAX_TOTAL_BYTES` | 65 536 | whole JSON payload |
| `MAX_HOPS_CEILING` | 4 | traversal depth |
| `PARSER_VERSION`, `SCHEMA_VERSION` | constants | manifest invalidation |

Performance targets (to be measured, not asserted):

- `status`/`files`/`text` on this repo: < 100 ms.
- Full `index` of a ~200-file repo: < 2 s; of a ~5 000-file repo: < 60 s.
- Incremental `sync` after one edit: < 200 ms.
- `symbol`/`callers`/`impact` query: < 100 ms p95.
- `explore`: < 250 ms p95, bounded payload.

---

## 15. Testing and quality gate

The gate mirrors `nanus`: `cargo fmt --check`, `cargo clippy -- -D warnings`,
`cargo nextest run`, doctests, and a release build. Lints are workspace-wide
(`forbid(unsafe_code)` on our crates, `missing_docs`, pedantic clippy).

Test layers:

1. **Extraction fixtures.** Per language, a small file per construct under
   `crates/langs/tests/fixtures/<lang>/`, asserting the exact nodes and edges
   (and their lines) extracted. This is where grammar quirks are pinned.
2. **Resolution tests.** Same-file, imported, qualified, global-unique, and
   dangling cases, each asserted to be *classified* correctly, not merely found.
3. **Reconcile tests.** One per diff class (added/modified/removed/renamed/
   unchanged/quarantined) and the crash case (manifest not committed → same
   delta recomputed).
4. **Store conformance.** A suite run against **both** the in-memory fake and the
   Grafeo adapter — the `llm-wiki-graph` pattern, so the port is the contract and
   the engine is swappable.
5. **Query golden tests.** Byte-identical JSON for each mode over a fixture repo
   (fixed root path in the fixture to keep ids stable), excluding `elapsed_ms`.
6. **Contract tests.** `include` rejection messages, cap notices, empty-result
   strings, exit codes.
7. **Property tests.** Glob anchoring, path normalisation, id stability across
   irrelevant edits, truncation never silent.
8. **Determinism test.** Two runs over an unchanged tree produce identical JSON.

No test reaches the network; no test depends on wall-clock time or filesystem
ordering.

---

## 16. Evaluation plan (the point of the repo)

The binary exists to be measured. The evaluation is designed before the
implementation so the implementation is aimed at it.

### 16.1 The question

> With graph-search available through `bash`, does `nanus` spend fewer tool calls
> and fewer tokens reaching correct answers about code and context — without
> regressing accuracy or bloating its context window?

### 16.2 Arms

| Arm | Tools available |
|---|---|
| **Baseline** | `nanus` as it is: `glob`, `grep`, `read`, `edit`, `write`, `bash`. |
| **Treatment** | The same, plus the system-prompt line pointing at `graph-search search …` and `graph-search sync`. |

Everything else is held constant: same model, same effort, same sandbox/approval
state, same turn budget.

### 16.3 Task suite

A fixed set of questions over at least three repositories — this repo, `nanus`,
and a third-party repo per language family (one Rust, one TypeScript). Questions
are chosen to require *discovery*, not recall, e.g.:

- "Where is `X` defined and what calls it?"
- "What would break if the signature of `Y` changed?"
- "Find the code that handles `Z` and summarise how it flows."
- "Which files import `W`?"
- "Find every place `V` is referenced outside tests."

Each task has a **rubric** (must-mention symbols/files, and the correct
structural answer) so correctness is scored, not eyeballed.

### 16.4 Metrics

| Metric | Source |
|---|---|
| Tool calls to answer | transcript fold (`StepStart`/`ToolCall` events) |
| Total tokens (prompt + completion) | session usage totals |
| Steps, wall time | session events |
| Answer correctness | rubric score per task |
| Context left resident at end | transcript tokens |
| Failure modes | stale answers used, truncated results mistaken for complete, tool-call churn |

### 16.5 Success and kill criteria

**Success (any, with no accuracy regression):**
- ≥ 25 % fewer tool calls, or ≥ 20 % fewer tokens, on discovery-heavy tasks; and
- no decrease in rubric score; and
- `explore`/`graph` answers are not mistaken for exhaustive when they are not.

**Kill / rethink:**
- Accuracy regresses (stale or truncated results cause wrong answers).
- Token use rises because results are dumped into context.
- The model does not reach for it, or mis-selects modes.
- Index maintenance costs more than it saves (sync latency, open cost).

The evaluation is a report in this repo (`docs/evaluation-YYYY-MM-DD.md`), not a
vibe. It is the input to the decision in §18.

---

## 17. Milestones

| # | Milestone | Deliverable | Notes |
|---|---|---|---|
| **M0** | Repo + spec | This document; workspace skeleton that builds. | **Current.** |
| **M1** | Types, ports, CLI shell | `graph-search-types`; `core` ports + walker; `files`/`text` modes fully working; CLI with `--json`; contract tests. | No engine, no parser. Ships `glob`/`grep` parity on its own. |
| **M2** | Engine + index/sync | Grafeo adapter; `init`/`index`/`sync`/`status`; manifest + reconcile; store conformance suite. | Feature set for Grafeo pinned here. |
| **M3** | Rust extractor + graph modes | `langs` Rust extractor; `symbol`/`refs`/`callers`/`callees`/`impact`/`deps`; staleness-aware lazy reconcile. | The core value; one language proven end-to-end. |
| **M4** | TS/JS + HTML/CSS | Remaining extractors; `neighbors`/`path`; cross-edges for HTML/CSS. | Languages can land behind features. |
| **M5** | `explore` + eval harness | Combined retrieval; the task suite and the measuring harness. | The context-efficiency bet is tested here. |
| **M6** | Evaluate → decide | `docs/evaluation-*.md`; a go/no-go on §18. | Includes the resident-`serve` decision if open cost warrants it. |

Each milestone is independently useful: M1 alone is a `glob`/`grep` replacement;
M3 alone is a code-graph tool.

---

## 18. Future `nanus` integration (only if §16 says so)

If the evaluation is positive, the integration is small and reversible:

1. **Port.** Add a `GraphPort`/`SearchPort` to `nanus-ports` mirroring §4.2's
   read surface, plus a `graph_key()`. Vendor-free, as every nanus port is.
2. **Adapter.** Move `graph-search-engine` + `graph-search-langs` behind a
   `nanus-adapter-graph` crate, **feature-gated**, so a minimal `nanus` build
   stays dependency-light and `search` degrades to `files`/`text`.
3. **Tool.** Replace `glob` and `grep` with one `search` tool (`ToolAccess::Read`)
   whose modes are §8.1–§8.4. The tool count goes 7 → 6.
4. **The CLI stays.** `nanus` may keep invoking `graph-search` via `bash` even
   after the tool exists; the tool and the binary share `core`.
5. **Removal criteria.** `glob`/`grep` are removed only when the tool's modes
   reproduce their semantics (§8.1–§8.2) and the evaluation shows no regression.

The `graph-search` repo remains the place where extraction and engine choices are
developed; `nanus` depends on the crates, not the other way round.

---

## 19. Open questions

1. **`.gitignore` in `files`/`text`.** §6.1 diverges from `nanus` `glob`/`grep`
   (which descend `target/`). Confirm the divergence, or keep `--no-ignore` as
   the default for those modes.
2. **Grafeo feature set.** Exactly which features are needed for LPG +
   persistence + traversal without pulling `ai`/`arrow-export`. Pin in M2.
3. **Store granularity.** Is a symbol-level graph necessary, or do file-level
   nodes with symbol *properties* suffice for the queries that matter? Cheaper if
   the latter.
4. **HTML/CSS value.** Do these languages pay for their weight in the evaluation,
   or are they a distraction from Rust/TS?
5. **`explore` seeding without embeddings.** How good is literal + name ranking?
   If it underperforms, is a *local* embedding model worth the dependency?
6. **One-shot vs resident.** Does per-invocation open cost matter at real repo
   sizes? (Decides the `serve` mode.)
7. **`impact` semantics.** Counts + top-N, or a full cone? The former is bounded
   and probably right; confirm against real use.
8. **`path` between symbols.** Is it actually used, or is it a feature nobody
   asks for?
9. **Binary/build footprint.** Tree-sitter + Grafeo build times and binary size —
   acceptable for an experiment, and for a feature-gated nanus adapter?
10. **The one-tool hypothesis itself.** `explore` may be the real answer and the
    discrete `graph` modes may be unnecessary. The evaluation decides whether the
    tool has three modes or one.
11. **A content index for `text`.** Is a trigram/n-gram index (or an in-memory
    body cache) worth it over a mmap + SIMD scan? And is a Grafeo BM25 `--ranked`
    mode worth offering beside exact `grep`, given the semantic difference?
12. **`files` index-first crossover.** At what repo size and invocation pattern
    does an index-backed glob actually beat a walk? This is measured, not
    assumed, and it gates the resident-mode decision.

---

## Appendix A — Node and edge vocabulary (summary)

**Nodes:** `file`, `module`, `function`, `method`, `struct`, `enum`, `trait`,
`impl`, `type_alias`, `const`, `static`, `macro`, `field`, `variant`, `class`,
`interface`, `variable`, `export`, `element`, `css_rule`, `css_at_rule`,
`css_custom_property`.

**Edges:** `contains`, `imports`, `exports`, `calls`, `references`, `extends`,
`implements`, `type_uses`, `links_to`, `loads_stylesheet`, `uses_class`,
`selects`.

## Appendix B — Example JSON (illustrative)

`graph-search search callers 'ToolRegistry::execute' --json`:

```json
{
  "schema_version": 1,
  "command": "search.graph.callers",
  "root": "/Users/ant/code/nanus",
  "query": { "target": "ToolRegistry::execute", "depth": 1, "limit": 50 },
  "stale": false,
  "results": [
    {
      "id": "sym:crates/nanus-bundle/src/agent_loop.rs#method:AgentRunner::run_tools",
      "name": "run_tools",
      "qualified_name": "AgentRunner::run_tools",
      "kind": "method",
      "path": "crates/nanus-bundle/src/agent_loop.rs",
      "start_line": 721,
      "end_line": 796,
      "signature": "async fn run_tools(&self, session: &mut Session, calls: &[ToolCall], …)"
    }
  ],
  "edges": [
    {
      "from": "sym:crates/nanus-bundle/src/agent_loop.rs#method:AgentRunner::run_tools",
      "kind": "calls",
      "to": "sym:crates/nanus-domain/src/tool.rs#method:ToolRegistry::execute",
      "to_name": "ToolRegistry::execute",
      "path": "crates/nanus-bundle/src/agent_loop.rs",
      "line": 761,
      "resolved": true
    }
  ],
  "truncations": [],
  "approximation": { "resolved": 1, "unresolved": 0, "note": "static approximation; dynamic/macro/generated edges may be missing" },
  "stats": { "files_scanned": 0, "candidates": 1, "elapsed_ms": 7 }
}
```

## Appendix C — References

- `llm-wiki-graph` — the architecture template: markdown truth, projection/
  reconcile, neutral read-only tool API, capability negotiation, engine behind a
  port. Source: `../llm-wiki-graph`.
- `codegraph` — tree-sitter extraction model, per-language fallback, the
  single-tool thesis, and the residual-context caution. Source:
  <https://github.com/colbymchenry/codegraph>.
- Tree-sitter — <https://tree-sitter.github.io/tree-sitter/>.
- Grafeo — <https://github.com/GrafeoDB/grafeo> (Apache-2.0).
- `nanus` — the target harness; its `glob`/`grep` semantics and tool contract live
  in `../nanus/crates/nanus-bundle/src/tools/` and
  `../nanus/crates/nanus-domain/src/tool.rs`.
