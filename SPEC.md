# graph-search — Specification

| | |
|---|---|
| **Status** | Draft (M0) |
| **Owner** | Ant Stanley |
| **Scope** | Repository-wide |
| **Depends on** | Tree-sitter (parsing), Grafeo (embedded graph store) |
| **Delivery shape** | An in-process Rust library (shape 3, §11.1); the CLI is a thin client over it |
| **Consumed by** | An in-process host (a future `nanus` tool; the evaluation harness); and, via the CLI, a person and the `nanus` agent through `bash` |

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
The product is an **in-process library** (§3, §4.7, §11.1) — the thing such a
tool would link — and the CLI is a thin client over it. The path there runs
through that CLI being invoked by `bash`, so the capability can be evaluated in
situ with no harness change and a one-line rollback.

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
   TSX), Python, HTML, CSS, and the declared script regions of Svelte, Vue and
   Astro single-file components.

## 2. Non-goals (v1)

- **No daemon, no file watcher, no socket service.** v1 is an in-process library
  plus a thin CLI; `index`/`sync` and a lazy reconcile are the freshness story.
  A socket service (`serve`) is §11.1 shape 2, deferred and optional.
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
  general markdown *content* as a knowledge graph. (That is `llm-wiki-graph`'s
  job.) The one exception is an Open Knowledge Format bundle (§7.6), whose
  concepts, links and citations are structured data by design.

---

## 3. Consumers and interfaces

The **product is a library**: a `SearchService` handle that owns a workspace's
index and answers queries in-process. Every other surface is a thin client over
it, so the behaviour measured through any of them is the behaviour `nanus` would
get if it linked the library directly.

| Consumer | Interface | Notes |
|---|---|---|
| **An in-process host** — the reference shape; the evaluation harness; a future `nanus` adapter | the `graph-search` library: `Index::open(root)` → `SearchService` (§4.7) | No IPC, no per-call startup. Shape 3 in §11.1. |
| A person | `graph-search <command>` at a shell | A thin client over the library; human-readable by default. |
| The `nanus` agent, initially | `bash` running `graph-search search … --json` | JSON on stdout; diagnostics on stderr. |

The CLI is a *client*, not the implementation: it parses arguments, opens the
library, and renders. The `--json` payload is the contract `nanus` parses from
`bash`; the text rendering is for humans and is not a contract. When `nanus`
links the library directly, the typed API (§4.7) replaces the JSON contract and
nothing above `core` changes.

---

## 4. Architecture

### 4.1 Crates and the dependency rule

Dependencies point inward. `core` knows nothing about tree-sitter or Grafeo. The
engine and the parser are adapters wired by the **library** (`crates/graph-search`)
— the product — and consumed in-process by the CLI and by any host that links it.

```
   hosts (in-process, shape 3)          clients
   ┌───────────────┐  ┌────────────┐    ┌──────────────────────────┐
   │ nanus adapter │  │ eval       │    │ graph-search-cli (thin)  │  the binary;
   │ (future)      │  │ harness    │    │ parse · render · dispatch│  wires nothing
   └───────┬───────┘  └─────┬──────┘    └────────────┬─────────────┘
           └────────┬───────┴────────────────────────┘
                    ▼
        ┌──────────────────────────────────┐
        │        graph-search (LIBRARY)     │  Index::open → SearchService
        │  options · adapter wiring · the   │  the product; no IPC, no socket
        │  in-process service API (§4.7)    │
        └────────┬───────────────────┬──────┘
                 ▼                   ▼
     ┌───────────────────┐   ┌──────────────────────┐
     │ graph-search-     │   │ graph-search-langs    │
     │ engine (Grafeo)   │   │ (tree-sitter          │
     │ impl GraphStore   │   │  extractors)          │
     └─────────┬─────────┘   └──────────┬───────────┘
               └───────────┬────────────┘
                           ▼
             ┌────────────────────────────────┐
             │        graph-search-core        │  domain · ports ·
             │  projector · reconcile · query  │  pure; no engine/parser
             └───────────────┬────────────────┘
                             ▼
             ┌────────────────────────────────┐
             │        graph-search-types       │  leaf value types
             └────────────────────────────────┘
```

| Crate | Owns | May depend on |
|---|---|---|
| `graph-search-types` | Ids, node/edge records, requests, results, the JSON contract types. | *(nothing in-tree)* |
| `graph-search-core` | Domain model; port traits; the projector (parse→graph); the reconcile (diff/apply); the query engine; the `files`/`text` walker. | `types` only |
| `graph-search-langs` | Tree-sitter grammars and per-language extraction queries; impl of core's `LanguageExtractor`. | `core`, `types`, tree-sitter |
| `graph-search-engine` | The embedded Grafeo store; impl of core's `GraphStore`. | `core`, `types`, `grafeo` |
| `graph-search` | **The library (the product).** The public API (`Index` → `SearchService`), options, and the wiring of concrete adapters behind the core ports. What a host links in-process. | adapters + `core`, `types` |
| `graph-search-cli` | A thin client: argument parsing, dispatch into the library, rendering. It wires nothing itself. | `graph-search` |

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
    fn metadata(&self) -> &MetadataIndex;
    fn edges_bounded(&self, id: &NodeId, kinds: &[EdgeKind], dir: Direction, budget: &mut WorkBudget) -> Result<Vec<Edge>, Error>;
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

**Decision — the fastest accurate route.** For the two operations either route
can serve, the walk wins on *both* axes in the architecture we have:

- **Speed.** A one-shot process pays the store open and a freshness check on
  every invocation; the walk pays neither. A warm `mmap` + SIMD substring scan
  reads at gigabytes per second, and the OS page cache is already holding the
  bytes, so a content cache would save syscalls, not bytes. Index-first `files`
  only wins in a **resident** process, where the path set is a few megabytes and
  a glob is an in-memory match.
- **Accuracy.** The walk is fresh by construction. An index-sourced answer is
  correct only while the index is current, so index-first ties on accuracy only
  once freshness is *maintained* — a watcher (the daemon we deferred) or a
  per-query scan that costs about what the walk it replaces costs.

`text` is body-bound and gains little either way. And for *end-to-end agent
retrieval* the graph is the real lever regardless: `explore`/`impact` replace a
chain of greps and reads with one call, and that does not depend on how
`files`/`text` are sourced.

**In the target (library) shape this flips for `files`.** Shape 3 holds the
`Index` open in-process (§4.7), so the path set *is* resident and the store open
is paid once, not per query. The rule the library applies is therefore: **`files`
is index-first whenever the index is resident and fresh in this process;
otherwise it walks.** A one-shot client that opens cold walks unless it is told
otherwise; a host that keeps the library alive gets the fast path. `text` stays a
scan regardless, because the bytes must still be examined.

### 4.7 The library API (the product surface)

The library exposes one index handle and one query service. Everything the CLI
does is a call through this API — which is also the API a future `nanus` adapter
would call, so the evaluation measures the real thing.

```rust
/// An opened workspace index.
pub struct Index { /* store · manifest · options */ }

/// How to open it.
pub struct OpenOptions {
    pub root: PathBuf,
    pub store: Option<PathBuf>,   // default <root>/.graph-search/index
    pub excludes: Vec<String>,
    pub languages: Vec<Language>,
    pub reconcile: Reconcile,     // Never | BeforeQuery | Explicit
    pub read_only: bool,          // refuse to build or mutate
}

impl Index {
    pub fn open(options: OpenOptions) -> Result<Index, Error>;
    pub fn search(&self) -> SearchService<'_>;
    pub fn status(&self) -> IndexStatus;
    pub fn sync(&self) -> Result<SyncReport, Error>;
    pub fn reindex(&self) -> Result<SyncReport, Error>;
}

/// The read API. Every method is bounded (§14) and reports truncation.
impl SearchService<'_> {
    pub fn files(&self, q: FilesQuery) -> Result<FilesResult, Error>;
    pub fn text(&self, q: TextQuery) -> Result<TextResult, Error>;
    pub fn symbol(&self, q: SymbolQuery) -> Result<GraphResult, Error>;
    pub fn references(&self, q: RefQuery) -> Result<GraphResult, Error>;
    pub fn callers(&self, q: TraversalQuery) -> Result<GraphResult, Error>;
    pub fn callees(&self, q: TraversalQuery) -> Result<GraphResult, Error>;
    pub fn impact(&self, q: TraversalQuery) -> Result<ImpactResult, Error>;
    pub fn deps(&self, q: DepsQuery) -> Result<GraphResult, Error>;
    pub fn neighbors(&self, q: NeighborsQuery) -> Result<GraphResult, Error>;
    pub fn path(&self, q: PathQuery) -> Result<GraphResult, Error>;
    pub fn explore(&self, q: ExploreQuery) -> Result<ExploreResult, Error>;
    pub fn status(&self) -> IndexStatus;
}
```

Properties the API holds:

- **Transport-free.** The `graph-search-types` records are the request/result
  vocabulary; nothing here names a socket, a process, or JSON. The CLI's `--json`
  output is a serialisation of these same results (§9).
- **Bounded and honest.** Every method enforces the §14 caps and returns a
  `truncations` list and a `stale` flag. There is no unbounded method.
- **Residency decides sourcing, not the caller.** `files` is index-first when the
  `Index` is held open and fresh, and walks otherwise (§4.6); `text` always
  scans. The caller does not choose.
- **Read/write split.** `sync`/`reindex` mutate; every `SearchService` method
  reads a snapshot.

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
| `concept` | OKF | one per non-reserved bundle document, named by its `title`. |
| `section` | OKF | a heading section, nested under its concept or parent section. |

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
| `links_to` | HTML element → file; OKF concept/section → concept/file | `href`/`src` or an OKF cross-link resolving to a workspace file. |
| `loads_stylesheet` | HTML element → file | `<link rel="stylesheet">`. |
| `uses_class` | HTML element → css selector | `class="…"` matched to a `css_rule`. |
| `selects` | css_rule → HTML element | selector matched to an element `id`/`class`. |
| `cites` | OKF concept/section → concept/file | a `sources[].resource`, cited by the concept and by each section holding its `[^id]` footnote. |

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
- Skip files larger than `MAX_FILE_BYTES` (1 MiB default). Unknown extensions
  remain searchable text and receive file nodes; disabled languages receive no
  parser facts.

The exclusion policy is a **named, configurable value** (`config.toml`, §12), not
scattered calls, because an index that eats `target/` is worse than useless.

> **Behavioural note.** `files`/`text` used by `nanus` as `glob`/`grep` today do
> **not** honour `.gitignore`. This spec makes the new `files`/`text` modes honour
> it, for consistency with the index and because descending `target/` is almost
> never intended. This is a *deliberate divergence from `nanus` parity* and is
> flagged in §19 as a decision to confirm during evaluation.

Enumeration is deterministic by path before applying the file cap (200,000)
and entry cap (1,000,000, including directories). `walk_report` returns partial
entries plus coverage; `walk` requires complete enumeration. Files, text, and
explore may serve partial evidence and report the boundary. Sync and reindex
require complete enumeration before preparing any removals: an incomplete walk
or a subsequent source read error preserves the previous index. Bounded reads
also reject files that grow past the size ceiling after enumeration.

Result context, status, and sync reports carry `coverage`: policy choices and a
fingerprint of the complete policy; optional `enumeration_complete`; counters
for admitted, oversized, unsupported, disabled-language, unreadable, binary,
invalid-UTF-8 and quarantined files; and work truncations. Zero counters are
omitted. An absent enumeration flag means no walk was performed. Counts cover
entries visible within the inclusion policy, not all ignored descendants.
Changing the policy fingerprint invalidates extraction caches, including when
languages are disabled or re-enabled. Body search applies path and language
filters before spending its file and byte quotas.

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

Binary (NUL in the first 8 KiB) and invalid UTF-8 source is quarantined before
parsing, with an explicit coverage status when encountered by live search.
A source read error aborts reconciliation rather than projecting an empty file.

Reconciliation also records native source retrieval facts through `source-units.json`.
It partitions valid UTF-8 using declaration byte boundaries, chooses the smallest
containing declaration, and creates windows of at most 80 lines with eight-line
overlap inside long regions. Regions carry original half-open byte bounds,
one-based line occurrences for normalized split terms and original whole lexemes,
and the full-file source hash. Source representation version 2 adds the whole
field; version 3 adds Markdown heading references; version 4 adds fenced-block fields;
version 5 adds table descriptors; version 6 adds paragraph/list/opaque block descriptors;
version 7 adds authored link descriptors and partial-link metadata flags.
Versions 1–6 remain readable; automatic reconciliation upgrades it, while
identifier-aware requests against an unreconciled legacy index regenerate old
facts through the bounded live overlay. Occurrence vectors must be nonempty,
sorted and within their region; repeated occurrences on one line are valid.
The manifest records analyzer revision 2, the Rust toolchain's Unicode table
version and chunker revision 8 independently of parser, serialized source fields,
storage and result-wire versions. A mismatch invalidates the representation
during reconciliation. A never-reconcile query masks incompatible body facts
and regenerates them within live-overlay budgets even when file hashes match.
Unregenerated incompatible facts remain masked.

Chunker revision 6 shields top-level delimiter-terminated HTML blocks from
Markdown heading, fence and table recognition. Comments, processing instructions,
declarations, CDATA and raw pre/script/style/textarea blocks retain their original
searchable bytes, including blank lines and unclosed blocks through EOF. Closing
delimiter lines remain inside the block; subsequent Markdown resumes normally.
This implements boundary shielding for CommonMark HTML block types 1–5, not an
HTML renderer or complete container parser. Generic HTML blocks, nested Markdown
containers and inline link structure still require further work.

Result context exposes `indexed_versions` from the actual stored manifest and
`runtime_versions` from the executing library. Unknown legacy revisions are zero;
historical results without these fields deserialize as unknown, never as current.
Runtime ranker revision 13 identifies scoring, routing and context-selection code;
ranking-only changes do not invalidate persisted parser/source facts. A retrieval
cache key must additionally include generation, original query, selected options,
filters and effective limits. No cross-request result cache is currently present.
Source-inclusion identity remains the policy fingerprint; publication format is
recorded in `CURRENT`, and result-wire revision remains independently versioned.
Required version metadata participates in byte fitting, so a very small requested
budget may produce `ResultBudget` instead of an empty successful result.
Unknown-language, configuration and Markdown text remain eligible; parser
quarantine does not discard otherwise readable source text. Binary/invalid UTF-8
has no source facts. The per-file region cap is 8,192, with explicit truncation.

Chunker revision 8 partitions Markdown (`md`, `mdx`, `markdown`) at top-level
ATX/Setext headings and fenced code blocks. The native scanner recognizes backtick or
tilde runs of at least three characters, up to three leading spaces, matching
closers at least as long as the opener, and unclosed fences through EOF.
Backtick info strings containing a backtick do not open a fence. Headings and
shorter/mismatched fence runs inside a block are code. Original UTF-8 and CRLF
offsets are retained. Fenced regions have kind `markdown_code_fence`; large
blocks still use bounded overlapping fragments and do not claim complete-block
delivery. Small blocks remain intact as retrieval regions, allowing context
assembly to retain both delimiters when the response budget permits.

An exact, unindented first line `---` or `+++` opens frontmatter. YAML-style
`---` closes with `---` or `...`; TOML-style `+++` closes with `+++`. The closing
line is included. Missing closers retain the remaining bytes as frontmatter.
Markers with surrounding whitespace, a BOM, or preceding blank lines are not
recognized by this declared dialect. Frontmatter is opaque to heading/fence
recognition and has kind `markdown_frontmatter`; its authored terms remain
searchable. Long metadata blocks use the same bounded overlapping windows as
other source regions. Values are not parsed or rewritten. This extension is a
local Markdown dialect choice, not a CommonMark frontmatter claim.

Setext headings begin at the start of a contiguous top-level prose paragraph,
including multiline titles. An underline contains only `=` or only `-`, with
up to three leading spaces and optional trailing spaces/tabs. Blank lines break
title eligibility. Code fences and frontmatter remain opaque. Recognition is
conservatively suppressed through indented code, list/quote/HTML/reference-like
blocks until a blank line or a recognized top-level boundary; the delimiter-terminated
HTML blocks described above remain opaque until their closer or EOF. Nested container
semantics are not inferred. Original title and underline bytes remain authored
source, with no generated heading prefix.

Headings and paragraphs are separate authored units. A blank line ends a prose
paragraph; its trailing blank lines stay attached. Leading blank runs remain
plain source regions. Paragraphs have kind `markdown_paragraph` and reference
heading ancestry without copying titles into their analyzed body terms.

Flat top-level bullet (`-`, `+`, `*`) and ordered (one to nine digits followed by
`.` or `)`) list items have kind `markdown_list_item`. Markers permit up to three
leading spaces and require following whitespace or end of line. Setext underline
recognition takes precedence over an empty `-` item when a title is pending;
thematic-break lines are not list markers. Continuation text stays in its item;
blank-separated indented continuations stay with it. The next peer marker,
unindented structural boundary, or unindented text after a blank line ends it.
Nested headings, fences, lists and other unsupported content remain searchable
inside the item and set `contains_unsupported`; they cannot change document-level
heading ancestry. Tabs and unusually deep marker padding are also flagged.

Paragraph, list and opaque units carry a `block` descriptor with the entire raw
block span, optional raw list-marker span, and `contains_unsupported`. Long blocks
use the existing 80-line/eight-line-overlap windows sharing that descriptor.
Opaque HTML, quote, indented-code and reference-like blocks have kind
`markdown_opaque`. Generic unsupported blocks are conservatively blank-delimited;
the recognized delimiter-terminated HTML blocks retain their explicit closer rule.
Metadata validation rejects foreign kinds, missing required descriptors, invalid
marker coordinates and out-of-file block bounds. Scanning and refinement retain
only a bounded prefix plus an omitted-region sentinel at the source-unit limit;
extraction reports truncation rather than silently claiming complete coverage.

Eligible heading, paragraph, flat-list and table regions also retain authored
inline-link, image and URI-autolink fields. Each `MarkdownLink` records its complete
syntax, label, destination, optional title and syntax kind as original spans.
Link fields are metadata beside the original analyzed text, not duplicated body
terms or synthesized graph edges. Destinations are not decoded, normalized,
resolved against the filesystem or fetched. Overlapping windows can carry the
same descriptor; validation requires consistent fields for the same source span.

The initial link dialect supports single-line `[label](destination)` and
`![label](destination)` with an optional single- or double-quoted title, empty
destinations, angle-wrapped destinations, preserved backslash escapes and up to
16 nested destination parentheses. Labels containing nested brackets or code
spans are not interpreted as outer links. URI autolinks require a 2–32-character
ASCII scheme and no ASCII whitespace/control characters in the URI. Ordinary code
spans, raw HTML tags/attributes and opaque source blocks suppress link metadata;
code-span state spans the original block before windows are formed. Unmatched
code/tag openers conservatively suppress further link recognition in that block.
Reference links, email autolinks, multiline links and parenthesized titles are
outside this declared initial dialect. Their bytes remain searchable.

Link extraction retains at most 256 descriptors per original block and 4,096
distinct links per file. Per-block byte inspections are bounded by eight times
block length plus 32, with an absolute ceiling of 1,048,576. `links_truncated`
marks fragments of blocks where either scan work or eligible-link count exceeded
the allowance. The generation's `source_link_truncated_files` coverage counter
reports these files independently of omitted source regions; raw body indexing
continues. Legacy units lacking link fields decode with empty metadata.

This boundary scanner does not recursively model blockquote/list containers or
MDX expressions.
It is not a full Markdown renderer or CommonMark parser. Those richer document
structures remain implementation work, rather than being inferred from isolated
heading-like lines inside known fences.

Stored source units carry optional `headings` ancestry,
ordered outer-to-inner with strictly increasing levels (at most six). Every
heading records its full original span and separate title span in the same file
and source hash; no heading text is prepended to body terms or excerpts. ATX title
spans omit opening/optional closing markers and surrounding whitespace; inline
markup remains authored text. Setext title spans preserve all title lines and
exclude the underline. Equal/shallower headings replace the corresponding
ancestry suffix. Code-fence fragments retain their parent headings; frontmatter
has none. Older records deserialize with absent ancestry. Publication/reopen
validation rejects invalid levels, ordering, file bounds and title containment.

Ranker revision 10 retains document context in internal candidate context and offers
verified parent headings as `document_heading` excerpts after structural and
matched source windows. Undelivered heading references do not consume serialized
result space. These remain optional: source-hash checks, marginal byte costs, line
deduplication, context work, interval limits and the final payload cap all apply.
Only selected snippets/excerpts contain delivered source; stored heading
references are not returned as mandatory response metadata. Long headings are clipped
by the existing interval limit and do not imply complete-title delivery.

Fenced fragments store a shared `fence` descriptor: original block span,
content span excluding delimiter lines, trimmed info-string span, optional first
ASCII-whitespace-delimited label span, and whether a compatible closer exists.
Label splitting follows Rust ASCII-whitespace rules (vertical tab stays authored
label text). The label is authored text, not a verified language/parser selection; attributes,
escapes and indentation inside the body are not rewritten. Empty and unclosed
blocks have explicit coordinates. Version-4 fence facts require valid metadata;
older source records may omit it. All fields are checked for ordering, source
bounds and fragment containment on publication/reopen.

The context planner can deliver the original opener/info line and existing
closer as separate `document_fence` excerpts, after matched evidence. It never
synthesizes a closing delimiter, concatenates omitted code or claims a fragment
is the full block. Descriptors remain internal candidate context; unused fields
do not consume response bytes. The same source/hash, work, interval, byte and
line-deduplication rules used for heading context apply.

The native top-level pipe-table subset recognizes a header immediately followed
by a delimiter/alignment row with the same cell count. Delimiter cells contain
one or more hyphens with optional edge colons; optional edge pipes and escaped
literal pipes are handled without rewriting authored cells. At least one
unescaped pipe is required across the header/delimiter pair. Uneven data rows,
including one-cell rows without pipes, remain verbatim until a blank line or a
recognized block boundary. Container/HTML/reference-like blocks retain the
scanner's conservative fallback; full nested GFM conformance is not claimed.

Tables have kind `markdown_table`. Their bounded overlapping row groups retain
a shared `table` descriptor containing the full table span, original header and
delimiter spans, and column count. Header text is not copied into each group's
postings. Missing/excess cells are neither padded nor dropped. New table facts
require valid descriptors; heading ancestry is retained when present. The context planner
may deliver the original header/delimiter as a `document_table_header` excerpt
after matched evidence; unused table metadata stays out of the response budget.
This is a source-fidelity contract, not an assertion that structural partitioning
improves every query. [GFM table syntax](https://github.github.com/gfm/#tables-extension-)
informs the declared subset.

These facts are replaced and removed with their file projection, preserved when
unchanged parser facts are merely rebound, and covered by generation checksums.
Both stores validate source ownership before mutation: an owner must exist in the
file projection, be a declaration in the same file, and enclose the entire
region in bytes and lines. Persistent reopen checks the same invariant against
the selected graph generation, independently of artifact checksums.
Descriptor format 1 remains readable; a missing/old manifest `source_version`
causes automatic reconciliation to rebuild using source representation revision
1. The facts are persisted separately from the hot manifest and never duplicate
the original source blob. Each generation prepares native source-region postings
from these facts. Queries enumerate all matching term lists; they no longer
scan unchanged source files to discover body matches.

Coverage reports include indexed-source file/region counts and files whose
region cap fired. Zero-valued work/statistic counters are omitted on the wire
and deserialize as zero, preserving output space for evidence and provenance.

### 6.3 Manifest and diff

A manifest is the store's self-fingerprint, one entry per file:

```
{ path, size, mtime_ns, content_hash, parser_version, schema_version, extraction }
```

Query freshness, result context and status use `GraphStore::manifest_header`.
Both native adapters omit extraction facts without cloning them; the persistent
adapter loads this small header once when opening a generation and replaces it
only with successful publication. Header reads check the same unavailable-state
guard as graph reads. Full raw extraction remains available through `manifest()`
for compatibility and inspection. Generation format 6 and later keep only the header
in `manifest.json`; extraction entries live in independently hashed native packs.
Their first reader verifies their bytes without deserializing their values. A true
no-op sync uses only the header. Native changed-sync uses persisted dependencies to select the
unchanged files requiring rebinding and requests just their raw facts. Changed files
are parsed normally. Unchanged ECMAScript module surfaces come from compact dependency
records, so module resolution does not require all parser payloads. Adapters without
a dependency index retain the full-manifest compatibility path. Full reindex needs
only the previous header because it extracts every current file. Timestamp-only
refreshes preserve all caches and decode only records whose packed fingerprint changes.

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

Schema 2 persists raw symbol/reference facts in `extraction` (no source bodies).
Changed files are parsed. For Rust/JavaScript/TypeScript, consumer repair is seeded
by changes to binding-relevant symbol values and normalized authored module surfaces,
not merely by source hashes. Source spans, documentation, signatures and async flags
do not affect the current resolver's cross-file kind/name target selection; the
edited file still receives fresh source facts, nodes, occurrences and outgoing edges.
IDs, names, kind, parentage, visibility and semantic attributes remain exact, and
import/export names, targets, type-only flags and completeness remain authoritative.
Missing facts, quarantine or graph/manifest fingerprint disagreement retain
conservative repair. Reverse name dependencies include unresolved names and new
ambiguities, and imports are checked against old/new file sets. Affected unchanged
files are rebound from cached facts. Both stores preserve nodes with
unchanged ID, path and kind during upserts, replace source-owned edges, and retain
untouched sources' edges to surviving endpoints. Explicit removals still delete
incident edges, including when the same batch recreates an ID. Edge ownership is
its explicit source path, falling back to the pre-update source node's path.
Incoming-edge closure remains conservative pending narrower binding-dependency
invalidation; HTML/CSS cross-matching is conservatively rebound on every change. `SyncReport.modified` includes rebound files, while
`unchanged` excludes them. A no-op skips graph application; same-content metadata
changes refresh only the manifest. Missing caches fall back conservatively, and
older schemas trigger a rebuild. Selected raw-fact decoding is proportional to affected
files, but the compact dependency walk, graph snapshots and generation preparation
still include workspace-sized work; this is not constant-time incremental indexing.


Reconciliation reports carry optional `counts` with exact added, modified, removed,
renamed and quarantined totals; `unchanged` remains its own exact counter. Readers
of older reports may derive totals from their complete detail lists. Rename totals
overlap the added destination and removed source, preserving the existing class
semantics. Quarantine totals count work in this reconciliation; coverage reports
all quarantined files in the resulting generation.

Reports fit the 64 KiB compact-JSON ceiling before publication, including final
coverage and space for the largest elapsed-time counter. When needed, detail
suffixes are dropped in added, modified, removed, renamed, then quarantined order,
with an explicit `bytes` coverage notice; exact totals survive even if no detail
fits. Required metadata overflow fails before publication or a metadata-only
manifest commit. Source coverage is calculated over retained and prepared facts
without cloning all source facts or rereading the published generation. The CLI
also fits its final JSON envelope and prints totals with any detail omission.
Transport/write failures after successful publication do not roll it back.
The `--fail-if-stale --no-reconcile` notice also fits its complete JSON envelope;
its exact changed total is retained in `context.staleness.changed` even when
path details are omitted, and its text total does not use the retained-list length.

### 6.4 Apply

Reconciliation calls `GraphStore::publish` with one `WriteBatch`. The Grafeo
adapter prepares a complete replacement graph in isolation, saves it with the
manifest, dangling references, native source facts and reference occurrences under `generations/<id>/`, then publishes a
small `CURRENT` descriptor by atomic rename. The descriptor records storage
format 9 and BLAKE3 fingerprints of every committed top-level artifact. A missing
or corrupt committed artifact is an error, never a silently empty index. Generations
written before format 8 carry SHA-256 fingerprints and fail verification; they are
rebuilt, not migrated (deleting the store directory is always a safe rebuild).

Opening a generation is proportional to its header, not its size. Open validates
the descriptor's completeness, rejects uncommitted artifacts, takes the reader lease,
and verifies and parses only `manifest.json` and `summary.json`. Every other
artifact (the graph, dangling references, source and occurrence facts, extraction
and dependency indexes, the edge occurrence table) is verified against its CURRENT
fingerprint by its first reader, before any of its bytes are used; the lease keeps
those immutable paths alive for as long as the handle may read them. Facts are
validated against the graph when they load, exactly as on an eager open, and
retrieval indexes (metadata, source-region postings, occurrence positions,
adjacency) are built on first use. A damaged artifact therefore fails the first
query that touches it, loudly, and a failed load is not cached. `status` and the
result context of every query read only the header, the summary and the tree, so
they never load the graph or its facts.

Format 9 adds two derived artifacts. `summary.json` holds the generation's exact
node, edge and file counts and its source coverage counters, so status and result
coverage need no facts. `edge-occurrences.bin` holds, for every relationship with
at least one source occurrence, the BLAKE3 digest of its edge id and its occurrence
count (`GSE1`, then 32-byte digest and little-endian `u32` entries in strictly
ascending digest order); relationship queries report `occurrence_count` from it by
binary search without loading occurrence facts. Both are computed by the writer
from the final prepared state. Older generations lack them and fall back to
computing counts at open and to the occurrence index.

All content hashes, both file fingerprints in the manifest and artifact, pack and
record fingerprints, are lowercase hex BLAKE3 (256-bit). BLAKE3 is fast through
portable SIMD with runtime dispatch (NEON on AArch64, SSE4.1/AVX2/AVX-512 on x86)
rather than dedicated SHA instructions, which some deployment CPUs lack. Each query
verifies the committed bytes it reads, so hash throughput is query latency.

Preparation applies the old projection and the update before constructing derived
retrieval indexes. Intermediate mutation reads only the graph, ID maps and owned
facts; it does not query those indexes. The final indexes are built before
persistence or exposure of the replacement store. Publication borrows the batch's
manifest instead of cloning its raw extraction cache.

Source facts use a version-3 per-file index in `source-units.json`. Each entry
contains a pack fingerprint, record fingerprint, byte offset and nonzero length.
Each immutable pack under `source-records/` is one zstd frame (level 3, with its
content size) of concatenated records; offsets and lengths address the inflated
bytes, and the pack fingerprint covers the compressed file, so corruption is
detected before inflation. Inflation is bounded by the declared content size and
a 1 GiB ceiling. Records never straddle packs. New packs target 8 MiB of record
bytes; a single larger record gets its own pack. This is a buffering target, not
a maximum record size.

Version-3 records use each fact type's native encoding. Source facts use `GSR1`
(`crates/engine/src/record_codec.rs`): a per-record sorted, front-coded dictionary
of terms, identifiers and owners; posting maps as ascending dictionary-id deltas
with delta-coded line lists in LEB128 varints; the source hash as 32 raw bytes;
and JSON only for rare Markdown, documentation and package fields. Decoding
rejects truncation, trailing bytes, out-of-range ids, unsorted dictionaries and
counts larger than the remaining record. Extraction records stay JSON inside the
same compressed packs. A dictionary per record, not per generation, keeps records
independently hashed, reusable across generations and selectively readable.
The index transitively commits pack and record bytes. Open checks pack hashes,
record hashes, checked slice bounds and non-overlap; identical references may
share an exact range. Hash names accept only 64 lowercase hexadecimal characters.
The source descriptor is read when source facts are first needed and verified
against the fingerprint CURRENT held at open, so a replaced descriptor fails
verification rather than redirecting the load. Each referenced pack is then loaded
and verified once before any source fact is exposed.
Version-1 and version-2 indexes (uncompressed JSON records) remain decodable by the
pack reader, but publication never reuses their records; a version-3 index requires
generation format 8. Source representation
and chunker revisions are independent of this layout.

Publication uses its cached, validated record index for reuse. Surviving source
facts outside the batch upsert/removal paths are unchanged by construction;
touched facts compare the complete representation with the previous generation. It verifies each retained pack once,
then shares it by hard link or synced-copy fallback. New/changed records become
small delta packs; publication never overwrites a shared inode. Packs below 75%
live bytes are repacked from current facts, and multiple existing packs smaller
than 1 MiB are combined. At most two sub-MiB packs survive a publication; other
retained packs carry at least 75% live bytes. Identical serialized records are
deduplicated. Repacking and removal preserve original per-file facts, and deleting
older generations unlinks their references without deleting current packs.
The pack directory is synced before committing its index and CURRENT.

Generation format 7 additionally commits the dependency index (since format 8,
`dependencies.json.zst`: one zstd frame of its JSON) whenever it commits a
manifest. This compact version-1 index contains per-file header identities, raw
non-dynamic reference names (including unresolved references), defined/exported
names, authored module surfaces and specifiers, binding-surface fingerprints,
selected module candidates and incoming file relationships. It is authenticated
by CURRENT; its first reader checks header/cache availability and reproducible
reverse maps before admitting it. Legacy generations or incoherent direct adapter batches have
no dependency index and use conservative repair. Explicit JSON null denotes that
fallback in new generations. Dependency publication occurs before CURRENT changes.

Native reconciliation uses the cached dependency index to select consumer repair.
Stable Rust/JS/TS binding surfaces do not seed repair merely because source display
coordinates or bodies changed. Missing extraction caches, package boundaries,
Rust context changes and HTML/CSS retain conservative invalidation. Presence changes
reconsider authored module choices; reverse raw names include previously unresolved
references; graph and selected-module dependencies participate in transitive closure.
The writer reuses per-file dependency records for explicitly retained extractions,
recomputes changed records, and reconstructs reverse maps against the complete final
graph/file set. Reconciliation still snapshots all nodes and edges, but does not
hydrate raw facts for unchanged, unaffected files when native dependency records exist.

Generation format 6 and later commit `extractions.json` whenever they commit a manifest.
This version-2 record index uses the same native pack codec under `extraction-records/`.
Each record is a complete per-file manifest entry with a present extraction value;
header entries carry no extraction values. Reopen authenticates the header and index,
rejects unknown record owners, and verifies pack/record hashes and checked ranges
when extraction facts are first needed. It defers raw JSON value decoding until
facts are requested. A private verified-index
wrapper can be constructed only by initial pack/record verification or the writer.
Hydration rechecks each full pack hash and range bounds. With that pinned descriptor,
identical pack bytes preserve the already-verified record hashes, so hydration does
not hash every record again. Standalone sidecar reads still verify record hashes.
Hydration requires extraction presence and an exact match of the remaining entry fingerprint,
and combines records with the pinned header. Malformed fact values or mismatched
fingerprints fail hydration rather than silently becoming empty caches.

`GraphStore::extraction_facts(paths)` returns only requested cached facts; missing
records and unknown paths are omitted, while present empty extractions remain present.
MemoryStore selects shared facts directly. Grafeo uses the pinned verified descriptor
to look up requested paths, groups by pack and exact byte slice, inflates each
pack holding a requested record (one bounded zstd frame; a frame has no random
access), and verifies each selected record hash before decoding. Duplicate slices
share one data read. Unselected records are not decoded and unrelated packs are not
opened. This authenticates returned records, not unselected bytes that may have
changed after open; full hydration still verifies whole packs. Selected records must
match the complete per-file header fingerprint. Its identity cache stores only weak
references and merges selected paths without evicting other cached identities.
Empty requests perform no fact I/O; unavailable handles still refuse reads. Legacy
embedded manifests use the full-read compatibility path for nonempty requests.
Generation leases keep lazy reads pinned across later publications. Native changed-sync
uses this API for affected unchanged files only.

`GraphStore::publish_retaining` takes a separate `FactRetention` request with the
observed generation, exact old header and retained path set. Retained owners cannot
be upserted or removed, must exist in both manifests, and must have identical entry
fingerprints except for mtime. Representation and policy identities must agree.
Stale requests, conflicting ownership and missing caches fail before mutation.
Ordinary publication still treats absent extraction values as cache removal.

Grafeo retains verified descriptors for untouched records. Timestamp-only changes
load and rewrite just those records with updated fingerprints. Compaction copies
verified record slices without typed JSON decoding; malformed cold values remain
errors when explicitly requested, rather than being silently converted to empty
facts. New/materialized records use shared-identity or serialized-hash equality to
prove reuse; source hashes alone do not prove equal facts. MemoryStore preserves
shared payloads and reuses compact dependency records. Compatibility adapters may
materialize retained facts before ordinary publication. Legacy embedded manifests
migrate on publication. Header, extraction index, packs and graph become visible
through the same CURRENT swap; generation leases protect retained readers.

This layout reduces unchanged source-fact serialization and storage duplication.
Earlier pack-layout measurements are recorded in `research/IMPLEMENTATION.md`;
they predate selective reconciliation and must not be treated as measurements of
its end-to-end latency or memory. The preceding individual-record experiment had
a measured latency regression. Graph reconstruction,
whole occurrence publication, header replacement and generation-local retrieval-index rebuilding
still occur. This is not per-file incremental graph/posting maintenance.

Preparation failures preserve the previous generation. Every artifact file is
synced before its rename. During generation preparation, manifest and dangling
writers defer their parent-directory sync to the generation owner; standalone
sidecar writes retain their own directory sync. Pack subdirectories are still
synced separately. After all artifacts are ready, the generation directory and
its parent are synced before CURRENT is replaced; the store root is synced after
that replacement. No unpublished intermediate state needs a separate directory
commit. File contents and containing directories are synced before durable acknowledgement. If syncing
the directory fails after the pointer rename, publication is uncertain: the
handle refuses reads and writes until reopen. Reopen follows the complete
published descriptor; it does not combine artifacts from different generations.
Orphan prepared directories are invisible. Reclamation retains the current and
previous generation plus every generation leased by a live store handle. A
reader pins the existing mandatory dangling-reference sidecar
(`dangling.jsonl.zst`: one zstd frame of JSON lines since format 8) with a shared OS
file lock before validating/loading its generation; no reader-side file creation
or write permission is required. Publication pins the newly prepared store before
changing CURRENT. The lease lasts through lazy extraction reads and closes after
the store's graph/fact fields. Reclamation takes a nonblocking exclusive lock on
the same file before removing an older directory. Contention or lock/open errors
skip removal; unfinished directories missing the mandatory sidecar cannot have
an admitted reader and remain reclaimable. Cleanup retries on a later publication,
not immediately when a reader exits. Failed cleanup may leave additional files.
Retaining arbitrarily many distinct reader generations therefore retains their
disk data; there is no unconditional two-generation disk bound. OS handle closure,
including process exit without destructors, releases the lease.

If a selected generation is retired before a reader acquires its lease, opening
retries only after observing that CURRENT changed. Stable corruption/locking
errors remain errors. Eight unsuccessful selections under repeated publication
return a retryable opening failure rather than looping indefinitely. This protocol
requires participating binaries that honor the lease; it does not coordinate
with older writers or arbitrary external deletion of generation files.
Process-interruption tests exercise publication boundaries; these are not a
simulation of storage-device power loss.

The lower-level `apply` API publishes graph changes while retaining the prior
manifest for compatibility; `commit_manifest` separately publishes metadata.
Callers requiring coherence use `publish`, as reconciliation does. The initial
implementation copies the full graph per publication, using additional memory
and temporary disk space. Compact deltas remain a measured optimization.

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

Every library search result now includes `context`: the selected graph generation
(if applicable), verification method, reconciliation policy, observed staleness,
and source fingerprints. These values describe the generation read under the
query's store lock. A later `status()` call is not used to certify the result.
`files` and `text` report `live`; direct domain queries without a host freshness
check report `unchecked`.

`OpenOptions.verification` defaults to `Verification::Metadata`. This compares
inclusion, size, mtime, and parser versions; it can miss same-size edits with
restored mtimes. `Verification::Content` (CLI `--verify-content`) additionally
hashes source files. With automatic reconciliation, observed content drift forces
a full rebuild so metadata shortcuts cannot reuse stale parser facts. This is
an explicit, more expensive mode; checks occur per file, not as an atomic
snapshot of the live filesystem. `never` and `explicit` policies report drift
without reconciling.

### 6.6 Concurrent writers

`index`/`sync` take an OS advisory lock on `<store>/index.lock`. A second writer
is refused with a sentence naming the holder — the same posture as `nanus`'s
session claim. Read-only queries do not take that **writer file lock**. Within
one `Index`, however, `store_read` holds a shared `RwLock` guard for its complete
callback, including snapshot use and result materialization; maintenance holds
an exclusive guard. The current `GraphStore` trait requires `Send`, not `Sync`.
Consequently `Index` does not provide a `Sync` contract for arbitrary concurrent
shared queries across threads. An immutable persisted generation is a publication
boundary, not a promise of lock-free concurrent query execution.

Opening the persistent adapter verifies one published generation. An existing
handle retains its opened state; it is not an automatically refreshing view of
other processes' writes. Publication failure/durability behavior follows §6.4.
Multiple independent handles/processes can retain their opened generations
through later publications using the leases in §6.4. Tests cover lazy facts,
multiple readers, normal/crash exit and selection/reclamation races. This does
not make one `Index` shareable across concurrent threads. Independent handles
own resident graph/index state; sharing a persisted generation or hard-linked
packs does not share those allocations. Holding arbitrary reader handles or
distinct histories therefore has no global memory/disk cap. Own-process RSS and
disk-retention experiments are recorded in `research/results/native-implementation/`
under `generation-memory` and `generation-churn`; sampled RSS is not a peak or
physical-memory guarantee. An `Arc`-owned snapshot
that outlives its store guard or automatically refreshed readers still requires
a separate lifecycle contract; the current borrowed-snapshot API provides neither.

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

1. **Lexical call bindings** — Rust and JS/TS scopes choose the nearest visible
   binding before workspace lookup. A direct declaration with a unique fact key
   resolves to that declaration. A local value or parameter with an unknown
   callable target remains unresolved; it cannot fall through to a same-named
   function elsewhere. Failed explicit lexical targets also remain unresolved.
2. **Imports** — explicit import provenance uses the supported module resolver
   and a unique compatible target. Failure cannot fall through to workspace names.
3. **Same file, same name** — require one visible, compatible definition.
   Function/block-local declarations cannot escape their lexical source extent.
4. **Qualified paths** — require the complete qualified name, without discarding
   an unknown receiver or module prefix to match a suffix.
5. **Global unique name** — a bare name that matches exactly one non-local
   workspace symbol of a compatible kind is a heuristic target.
6. **Everything else is dangling** — recorded with `resolved: false` and the
   *referenced name*, counted in `stats.unresolved`.

Parser revision 5 persists file-local scopes and binding patterns in the raw
extraction cache. Scope construction walks syntax once; per-scope name lists
support declaration-order lookup. Parameters, destructuring, closures, Rust
`let`/loop/match/conditional bindings, JS block bindings, function-scoped `var`
and catch bindings participate. Pattern keys, constructors, types and default
expressions are not mistaken for bound identifiers. Rust locals become visible
after their declaration; JS lexical bindings suppress outer lookup even before
initialization. Direct immutable `const` function expressions have syntactic
targets, including self-reference from their body. Mutable aliases and arbitrary
value flow are not inferred.

Call facts retain original expression spans and raw callee spelling before
import/receiver rewriting, plus scope/binding ordinals, explicit lexical target
keys or an unresolved reason. Other reference kinds may have only an enclosing
syntax range; no exact token-occurrence claim is made for them. These raw facts
survive unchanged-file rebinding; graph adjacency still aggregates repeated
relationships. Individual occurrence results expose the indexed binding class
and unresolved reason. This call-binding subset is not a full compiler
scope/type system: package visibility, reexports, dynamic mutation, receiver
types and unsupported grammar constructs retain their existing approximation.

Reference occurrence representation 1 additionally persists each raw reference
in a checksummed `occurrences.json.zst` artifact (one zstd frame of JSON) owned by
its source file and hash.
Occurrence identity encodes the original file, hash, owner, relationship kind,
name/spelling, raw-reference ordinal, line and optional byte/line span. It excludes
the selected target and binding reason, so rebinding unchanged source preserves
the evidence identity. Resolution classes distinguish explicit lexical/import
selection, same-file/qualified/unique-name heuristics and unresolved references.
Deleting a target clears that binding without deleting the source occurrence.
Synthetic containment and cross-language matches do not claim raw occurrence
facts. File completeness means extraction succeeded, not compiler completeness.
Native generation-local indexes retain lookup positions by target, owner, raw
name and aggregate edge; adjacency continues to deduplicate relationships.
Returned graph edges include `occurrence_count` when indexed raw references
support the relationship. This is a source-occurrence count, not a traversal
degree or compiler-completeness claim. Legacy and synthetic edges omit the
field rather than reporting a fabricated zero. Counts are attached only after
the returned-edge cap; exact serialized-byte admission includes the new field.
`SearchService::occurrences` / `search occurrences <target>` returns individual
records. `--by target` (default) resolves a declaration name/id and reads its
incoming occurrence index; `--by owner` selects references directly owned by
that declaration or file, without recursive traversal. `--by name` matches
original reference spelling exactly, including unresolved references. Where an
adapter does not preserve raw spelling, this route uses its parser reference
name. Ambiguous declaration names require an exact id. `--rel`, `--lang` and
`--path` filter relationship kind and source files before result admission.

The default record cap is 50, hard ceiling 500; lookup strings contain 1–8,192
bytes. `WorkLimits::occurrences` independently allows 10,000 examined entries
by default, hard ceiling 100,000, with zero allowed. Every entry is charged
before kind/path/language filtering; cancellation/deadline checks are cooperative.
Results report `occurrences_examined` and fired work/result caps. Returned records
retain full identity, source hash, coordinates, spelling and binding evidence;
64-KiB library and final JSON-envelope limits drop whole records. Required
metadata is never removed to manufacture a fitting response.

Occurrence results carry indexed/executing revisions, generation, freshness and
source verification. These are indexed coordinates, not claims about unchecked
live bytes. `indexed_files` counts files with occurrence metadata and
`extracted_files` counts successful reference extraction; neither claims full
language/compiler coverage. Legacy facts are explicitly absent. Occurrence
queries do not read source snippets or infer occurrences from synthetic edges.
Occurrence revision mismatches require reconciliation independently of source
body compatibility. Legacy generations without occurrence facts remain readable.

Macros, conditional compilation, dynamic dispatch, re-exports, generated code,
and any language without an enabled extractor cause missing relationships;
workspace-name heuristics can also produce false positives. The tool never
presents the graph as exhaustive. Every `graph` and
`explore` result carries:

```
"approximation": {
  "resolved": 812,
  "unresolved": 204,
  "note": "static approximation; dynamic/macro/generated edges may be missing"
}
```

### 7.5 Python

`.py`, `.pyi` and `.pyw` files. Python binds names at run time, so the adapter
models what the syntax states and leaves plain identifier uses and local bindings
to the generic resolution rules (§7.4).

| Node | Capture |
|---|---|
| `class` | `class_definition` |
| `function` | `function_definition` outside a class body (incl. nested and `async def`) |
| `method` | `function_definition` directly in a class body |
| `field` | `name = …` / `name: T = …` in a class body |
| `variable` | `name = …` / `name: T = …` at module level |
| `type_alias` | `type X = …` / `type X[T] = …` (PEP 695; the name is `X`) |

Only a single-identifier assignment target declares a symbol; tuple unpacking,
subscripts, attributes (`self.x = …`) and function-local assignments do not. A
`def` or `class` inside a function body is lexically local to that function
(`lexical_local`): it is visible only within it, never to same-file,
qualified or unique-name lookup elsewhere.

| Edge | Source |
|---|---|
| `contains` | file → top level, class → method/field, function → nested `def` |
| `imports` | `import a.b` (one edge per name); `from m import x` emits the module edge `m` and one binding `x` via `m` |
| `calls` | `call` → callee spelling; `self.m()` in a method becomes `Class.m`; `mod.f()` where `import a.b as mod` (or `import mod`) binds `mod` becomes `f` via module `a.b` |
| `extends` | `class C(Base, pkg.Base, Generic[T])` positional bases, a subscripted base naming the class it parameterizes (keyword arguments such as `metaclass=` are not bases) |
| `type_uses` | parameter, return, annotated-assignment, type-alias and PEP 695 bound annotations, including names in string forward references; excluding builtin scalar/container names, `typing` special forms and generic aliases, in-scope PEP 695 type parameters, `Literal[…]` arguments and `Annotated[…]` metadata |

A Python call binds to a `class` (calling it constructs an instance), and a bare
call name never binds to a `method`, which is only reachable through a
receiver.

Module specifiers resolve against the known file set, never `sys.path` or
installed packages. `a.b` tries `a/b/__init__.py`, `a/b/__init__.pyi`, `a/b.py`
and `a/b.pyi` from the workspace root: as in Python's path finder, a regular
package wins over a same-named module. A leading dot is the importing file's
directory; each further dot walks one directory up, and walking past the root has
no target. `from m import x` (and a module-qualified call) binds only a top-level
symbol `x` of `m`, never a method, class field or nested `def` sharing the name;
when `m` rebinds `x` (`@overload` stubs, conditional `def`s) the last binding in
the file wins. When `from m import x` finds no symbol `x` in `m`, `m.x` is tried
as a submodule (`from . import views`). An `import` inside a function or class body
is owned by that symbol but still resolves as a module, never by name to an
unrelated workspace symbol. Unresolved imports are dangling with their
reason, as in every other language. Package context comes from the nearest
`pyproject.toml` (§ Manifest-owned package context); `__init__.py` does not create
a package boundary.

### 7.6 Open Knowledge Format

[OKF](https://github.com/GoogleCloudPlatform/open-knowledge-format) v0.2
bundles, parsed with `tree-sitter-okf` (markdown body plus a native OKF-YAML
frontmatter tree). OKF makes `index.md` optional in every directory and has no
required root marker, so bundle membership is a declared path approximation: a
`.md` file the extension table does not claim is an `okf` document when its own
directory or an ancestor holds an admitted `index.md`. Every other `.md` file
stays unknown-language prose, chunked as Markdown as before. Binding `.md` to
`okf` in `[extensions]` claims every Markdown file instead. Because membership is
not a per-file diff input, `sync` rebuilds the index when the set of
`index.md` directories changes. With `okf` disabled no membership is decided,
so no file is walked as `okf` and no such rebuild happens. Language filters and
live search use this walked language, not the extension table.

The rule is deliberately path-only, and it over-claims in one known way. The
outermost `index.md` is the bundle root. A documentation site with its own
`docs/index.md` therefore absorbs a bundle at `docs/kb/`: its `/`-links
resolve from `docs/`, and a root `index.md` makes every `.md` in the
workspace an OKF document. An `okf_version` key in a root `index.md` (OKF §12)
is not consulted.

| Node | Capture |
|---|---|
| `concept` | the document, unless it is a reserved `index.md` or `log.md`; named by frontmatter `title`, else the file stem; signature `type: description`; `okf_type`, `okf_status`, `okf_tags`, `okf_description`, `okf_resource`, `okf_stale_after`, `okf_trust` (OKF §5.3: `unverified` without a mapping entry) and `concept_stem` attributes |
| `section` | each ATX/setext heading `section`, qualified `Concept > Heading > Subheading`, named by the heading's reader-visible text: link and image text without destinations, no emphasis or code delimiters, escapes and character references decoded, footnote markers, raw HTML and an ATX closing `#` run dropped |

| Edge | Source |
|---|---|
| `contains` | file → concept → section → nested section |
| `links_to` | inline links and images, and full, collapsed and shortcut reference links and images (through their definitions), from the innermost section (else the concept, else the file); a link inside a footnote definition belongs to the first section citing that footnote |
| `cites` | concept → every `sources[].resource` that names a path; section → the resource of each `[^id]` footnote naming such a `sources[].id` |

Frontmatter is a block or flow mapping. Scalars are strings: double-quoted
escapes (YAML 1.2 §5.7) are decoded and folded lines joined. An unterminated
frontmatter block leaves the grammar no body, so such a document has its
concept but no sections or links. A `sources[].resource` with whitespace, or
with neither a `/` nor a file extension, is a scope descriptor (OKF §5.1),
not a path, and is not cited.

A URI with a scheme, a scheme-relative `//host` path, and a fragment- or
query-only destination are not bundle paths and produce no reference; links
inside code are not links. Destination backslash escapes and character
references are decoded as `CommonMark` does. A destination beginning
with `/` is bundle-relative, the bundle root being the outermost ancestor
holding an `index.md`; any other destination is relative to the document. A
path-valued field (`sources[].resource`) also tries the bundle root, as
producers commonly spell those paths from the root without the `/`. Query and
fragment are dropped and `%XX` escapes decoded; a directory names its
`index.md` and an extensionless path names a concept id (`x` → `x.md`). The
target is the document's concept when it has one, else its file node. A path
naming no walked file is dangling with `okf_link_target_missing` (OKF §6.1:
broken links are not malformed). Each link or citation path is a dependency
specifier: a document is rebound when a file it may select appears, vanishes
or changes (a retitled concept), never on unrelated edits. Tracking resolves
paths with the bundle-root fallback for every kind, a superset of what a
`links_to` path can select. A heading repeated under the same parent keeps one
qualified name; its fact key gains `@line` (and `#n` while that is still
taken), so containment stays exact.

`neighbors` is the query for OKF relationships. `deps` reads only edges
incident to the file node. For an OKF document those are its incoming links to
a concept-less target such as an `index.md`, plus `links_to`/`cites` made
outside any concept. Links and citations made by a concept or section, and
links into a concept, belong to those symbols.

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
- Pattern and optional path fields are each limited to 8,192 UTF-8 bytes before
  glob compilation or filesystem access. Oversized fields return `InvalidQuery`.
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
- Matching is line-oriented: empty literals and literals containing CR or LF are rejected.
  `--ignore-case` uses Unicode lowercase (not full case folding); returned lines
  always retain their original source spelling. One matching line produces one hit.
- Default `limit` 250, ceiling 2000. Lines are capped at 400 bytes at a UTF-8 boundary with `…`.
- Literal, include and optional path fields are each limited to 8,192 UTF-8 bytes.
  Oversized fields return `InvalidQuery` before compilation or filesystem access.
  Shortened lines explicitly report `match_line`.
- Matches are grouped by file, `path:` header then `  <line>: <text>`.
- Empty result renders `No matches found.`
- A capped search that matched nothing renders:
  `No matches in the files reached; evidence is incomplete.` followed by the
  actual truncation notices. Byte/source limits are not described as match limits.
- Truncation notice: `(stopped at {limit} matches; narrow the pattern or the include filter)`.
- **Sourced by scanning the files** (one forward line pass and a reusable literal finder), not by the
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
  shape, not 400 lines. The traversal cone is the same reference vocabulary as
  `refs`: `Calls`, `References` and `TypeUses`. Including `TypeUses` means a
  struct, enum or trait reports the code that names it, so "what breaks if I
  change this type" is answerable rather than empty.
- Hops are clamped to `MAX_HOPS_CEILING` (4); over-ceiling is clamped, not an
  error (matching `llm-wiki-graph`'s capability-clamp posture).
- Every result lists nodes then edges; edges carry `resolved`.

### 8.4 `explore` — the one-call retrieval

```
graph-search search explore <query> [--k 8] [--hops 1] [--context-lines 2]
                                    [--detail compact|full]
                                    [--max-bytes N] [--lang L]
                                    [--intent auto|exact-name|exact-id|name-prefix|path|terms|phrase|near]
                                    [--phrase-gap 0] [--near-window 8]
                                    [--ranking auto|fusion|metadata|body]
                                    [--normalization combined|bm25f]
                                    [--graph-context semantic|none|calls|imports|types]
                                    [--analysis split|identifiers]
                                    [--all-terms | --min-terms N] [--per-file N]
                                    [--tests defer|neutral]
                                    [--exact-fast-path] [--explain]
```

This is the context-efficient entry point and the closest analogue to
`codegraph`'s single tool. Given free-text terms:

`retrieval.graph_context` controls relation enrichment independently of seed
ranking. Its backward-compatible default `semantic` admits Calls, References,
TypeUses, Imports, Implements and Extends, excluding Contains. `calls` admits
Calls; `imports` admits Imports; `types` admits TypeUses, Implements and Extends.
Only `semantic` and `calls` calculate caller-impact summaries. `none` omits
connection expansion, bridge-node admission and impact summaries. Navigation or
owner lookup may still read graph nodes; disabling enrichment does not promise
that all candidate routes avoid graph storage. These policies are exposed by
`--graph-context` and included in explained options.

Connections use the selected relations bidirectionally and retain original edge
direction in their output. The bounded expanded graph is shared across seeds.
A native union/find component map skips seeds with no reachable seed peer;
each deterministic shortest-path BFS stops once it has reached every peer in
its component. Every examined BFS adjacency still consumes the shared edge
allowance. A truncated expanded graph remains partial and carries its notices;
component membership does not certify completeness beyond that admitted graph.

1. **Seed** — explicit name/ID/path/prefix intent runs only its navigation route.
   `name-prefix` means a case-sensitive prefix of the complete bare or qualified
   name, preserving Unicode spelling and punctuation. It is neither a split-token
   prefix, a glob nor an arbitrary substring. Empty prefixes are rejected; misses
   never trigger fuzzy correction or ranked fallback. Bare dictionary entries
   precede qualified entries, and duplicate entities are admitted once.
   Explicit `phrase` verifies ordered whole lexemes with total unmatched positions
   at most `phrase_gap` (default 0); `near` verifies the query multiset in an
   inclusive `near_window` token span (default 8). Distances are at most 4096;
   an unordered window must fit all 1..128 query positions. Empty/punctuation-only
   queries are invalid. Validation precedes service freshness and CLI index open.
   Both retain stopwords and repetitions, use the whole-lexeme Unicode contract
   below, and override discovery analyzer/channel/Boolean policies. There is no
   metadata/navigation fallback. Native file-level postings are only a necessary
   filter: verification scans captured source across region boundaries. Missing,
   changed, unchecked, old or truncated facts require a source scan. Confirmed
   witnesses retain original byte spans, source hashes and inclusive line spans.
   A hash-matching declaration must enclose the whole witness; the smallest
   enclosing declaration wins, otherwise evidence belongs to the file. Multiple
   matching owners and overlapping witnesses survive before top-k grouping.
   One shortest witness per matching end token is enumerated, not all possible
   alignments. Owners rank by deterministic file/source encounter order; ordinary
   body relevance scores do not claim to rank these verified predicates.
   Positional byte/token/witness allowances are shared across files, and quota
   stops retain proven witnesses with explicit truncation. Source-read, graph,
   candidate, context and payload budgets still apply. Zero context suppresses
   excerpts without suppressing verified source evidence.
   In ranked metadata retrieval, exact bare/qualified names take priority. Split camelCase,
   acronym, snake_case, and path tokens and rank metadata with BM25 (name/path/
   signature weights 8/2/1). Deduplicate query terms and remove sentence function
   words. Require a real token match; scores are relevance, not confidence.
   Split analysis remains the default. Opt-in `identifiers` keeps whole query
   lexemes, matching folded whole-name evidence as well as split postings, and
   adds a qualified-name metadata field with weight 4 when it differs from the
   bare name. Whole and split frequencies/lengths are retained separately; each
   field uses their maximum rather than summing overlapping aliases. Metadata
   defaults to the existing combined-length normalization and clipped Robertson
   IDF. Opt-in `normalization: bm25f` normalizes each field frequency by
   `0.25 + 0.75 * field_length / average_field_length`, combines weights 8/2/1/4,
   then saturates once with k1=1.2. IDF remains the global symbol-document DF,
   clipped to 1e-6. Per-field averages use all symbol documents, missing fields
   contribute zero, and averages are floored at 1. Whole/split aliases use their
   per-field maxima for both frequency and length. This is a query-time policy
   over the same generation-owned postings; no additional persisted terms or
   reindex are required. Exact-name priority, conjunction/minimum coverage,
   filters and work limits are unchanged. Body and positional routes ignore this
   metadata normalization choice. Whole fields retain stopwords, and single-word queries do
   too; sentence stopwords are removed only from multiword queries.
   Whole lexemes split on Unicode whitespace and ASCII punctuation except `_`.
   Original spelling and UTF-8 offsets are preserved; matching uses Rust Unicode
   lowercase, without stemming, canonical normalization, full case folding or
   confusable folding. Non-ASCII punctuation remains inside a whole lexeme; the
   split field still supplies its legacy terms. Line occurrences establish match
   locations, not positional phrase proof. Exact navigation and raw text search
   preserve their separate spelling and punctuation contracts.
   Native source-region postings supply an independent body lane. Lexical
   statistics and native postings are built with the graph generation and reused
   by its snapshots. Exact bare/qualified tables and cached file languages avoid
   per-query node scans. The ordered term dictionary maps to sorted document
   ordinals and weighted frequencies; sparse queries enumerate matching postings.
   Metadata publication reuses documents only when stable identity and analyzed
   name/path/signature/qualified-name fields agree. It analyzes changed fields,
   updates affected posting lists, remaps moved ordinals and shares untouched
   immutable lists. Corpus length totals change by subtracting removed/changed
   documents and adding the analyzed delta; scoring uses the new population,
   averages and list lengths. Exact maps, file languages and compact ordering
   are reconstructed for the new view. Preparation never mutates the old cache.
   Reopen builds a fresh resident index; no posting codec or schema migration is
   introduced. These updates do not remove global graph/body/fact maintenance.
   Path filters compile once per request and run before candidate admission.
   Source facts and body postings are native; no external text index or embedding model is used.
   Metadata selection uses a deterministic bounded heap of compact ordinals;
   full nodes are cloned only for winners. The metadata pool is at least 64 and
   otherwise four times the requested seed count, capped at 500; it is never
   smaller than the clamped final seed count. Selection scores every admitted
   candidate and is not WAND or score pruning. Candidate counts retain the
   pre-selection total. Name/path/signature term counts and field lengths remain
   separate in postings for field-aware ranker evaluation; cached weighted term
   frequencies retain the existing score computation.
   Body retrieval uses positive-IDF BM25 (k1=1.2, b=0.75) over source regions,
   with all query terms eligible. Selective posting lists run first. Each owner
   or unowned file contributes its best region before body top-k selection, so
   overlapping windows cannot fill the pool. After owner top-k, retain up to three
   complementary regions per owner: prefer previously uncovered query terms,
   then BM25 score and stable region ordinal; a region without new terms must
   have its matching anchor outside already retained region boundaries. These
   regions contribute source context only, never additional owner scores or
   result slots. Matching regions omitted by this bound receive a candidates
   truncation notice. AND/minimum coverage still applies within each region;
   terms in separate regions do not fabricate an AND match. Additional matched
   windows retain their own source boundaries and Markdown heading/fence/table
   labels, share the selected source hash, and compete under the existing exact
   context byte/work/interval budgets. Automatic ranking treats multiword
   queries as conceptual discovery: run body retrieval first, then metadata only
   if the body pool is empty. Single-token queries retain exact-priority fusion.
   This is a documented lexical heuristic, not semantic intent recognition.
   Explicit `metadata`, `body`, and `fusion` strategies remain selectable.
   Fusion combines ranks with reciprocal rank (constant 60) and an exact-name
   priority tier. File diversity is disabled by default after controlled tests;
   a positive `per_file` makes a soft first pass, with exact-name exemptions,
   then fills remaining slots from deferred entities. Before that pass,
   test-owned hits (§ Test-owned symbols) follow every other hit unless the
   query names them; `--tests neutral` ranks them like any other.

   `RetrievalOptions` preserves the original query separately from mode, ranking,
   minimum term coverage and diversification. `Any` is OR discovery; `All` and
   `AtLeast(n)` require observed distinct terms in one metadata document or source
   region. Coverage counts are collected in the same budgeted posting pass;
   unexamined terms cannot satisfy a conjunction. AND (including a minimum equal
   to the distinct term count) uses the shortest posting list as its driver,
   with monotonic galloping/binary seeks through the remaining lists. Every
   inspected posting, including seek probes, consumes work. Only fully verified
   intersections receive candidate admission and scores; missing terms prove an
   empty lexical intersection without scanning other lists. Scoring retains each
   channel's original addition order and corpus statistics. Smaller minimum-term
   thresholds retain bounded union accumulation with coverage filtering.
   Explicit name lookup is case-sensitive; explicit IDs and anchored path globs
   do not broaden on a miss. Automatic mode recognizes stable ID prefixes. The
   optional exact-name fast path stops only when a name survives filtering.
   Explore rejects input exceeding 8,192 bytes or 128 distinct analyzed terms;
   it never silently drops terms to make an AND query cheaper.

   `explain` adds the original query, effective structured options, analyzed terms,
   actual retrieval routes, and per-seed metadata/body ranks. Diagnostics participate
   in the normal result byte limit and are omitted by default. The CLI exposes
   these controls directly; its JSON envelope retains the library plan.

   Known changed paths mask indexed body facts. A bounded request-local overlay
   reads at most 512 eligible files and 8 MiB, builds replacement facts, and uses
   the same bytes for snippets. Unchanged indexed files need no body scan.
   Direct core queries without freshness context may verify a bounded prefix;
   unobserved facts retain indexed provenance. Complete enumeration masks removed
   paths; partial enumeration never implies deletion. Live and indexed body
   pools merge by rank, since their BM25 corpus statistics differ.

2. **Assemble** — for the top `k` seeds: the definition location and signature,
   plus a bounded snippet. Body hits carry separate `evidence` coordinates, kind,
   owner, source hash, live/indexed origin and matched line. The excerpt centers
   on the line with the most distinct query terms (earliest on ties); declaration
   coordinates are unchanged. Other hits use the definition location. Snippets
   require captured bytes matching the evidence hash. The primary excerpt stays
   bounded to ten lines. Additional labeled `excerpts` use the remaining byte
   budget: complete declarations up to 80 lines, matched body regions, declaration
   headers for larger symbols, and original occurrence sites for returned
   relationships. Repeated references sharing one adjacency edge remain eligible
   independently. A native owner lookup avoids repeatedly scanning every edge
   for each item; occurrence entries consume the independent query work budget
   before their records are inspected. Repeated sites on the same line share one
   candidate window. Legacy/synthetic edges without raw facts retain their coarse
   indexed-line anchor. All added lines come from the same captured source; hash mismatches
   suppress both primary and additional excerpts.

   Matching line positions from each selected body region are retained internally
   for both indexed and live retrieval. Each line carries a query-local 128-bit
   term-membership mask under the existing 128-term bound. Repeated occurrences
   and whole/split aliases for one query term share its bit; population count
   preserves the primary anchor's distinct-term density. These masks are not
   persisted, do not encode within-line word order and do not boost fallback
   priorities by total or previously unseen term counts. After structural and relationship windows
   have been considered, five-line windows centered on these matches compete for
   remaining space, clipped to the verified region. If these do not fit, a final
   tier tries the matching lines alone. These labeled excerpts do not claim to
   deliver the complete region. This fallback does not scan other regions
   belonging to the same owner or reanalyze source; all tiers share the same
   byte, interval and context-window work limits. Primary snippets remain
   independent of this optional-window admission.

   Additional interval candidates compete by rank-adjusted value per estimated
   new source byte, with exact serialized-byte admission. Response bytes are
   measured after occurrence-discovery counters and already-fired notices are
   attached. Allocation reserves full integer widths for the final elapsed-time
   and context-window counters, so their growth cannot evict admitted evidence.
   New work-limit notices remain subject to the final whole-result byte check.
   Estimated costs are
   recomputed after successful admission, excluding already delivered lines and
   charging overhead for each remaining contiguous gap. Fully covered windows
   leave the candidate set. Within each existing role tier, value is multiplied
   by one plus the number of distinct query terms the candidate adds beyond
   source lines already delivered for that owner. This term mask is recomputed
   after successful admission, so repetition loses its novelty bonus. The
   source-region mask passes consume the context-window work budget too.
   This is a lexical coverage heuristic, not marginal semantic information,
   phrase verification, or a guarantee that every labeled region improves.
   Each window-cost evaluation consumes an independent `context_windows` work
   budget (10,000 default, 100,000 ceiling, zero allowed), checking cancellation
   and deadlines before inspecting at most 80 lines. Exhaustion reports an
   explicit `context_windows` truncation; `context_windows_examined` includes
   repeated evaluations. Primary evidence remains reserved. Context uses a damped
   reciprocal-rank prior proportional to `1 / (60 + rank)` (one-based rank),
   matching the retrieval fusion offset. Source-role priorities remain separate;
   this reduces the context-value penalty from small reorders without treating
   scores as confidence probabilities. This is a deterministic
   heuristic, not an optimal knapsack solver. Lines already delivered by any
   primary/additional excerpt from the same source version are excluded from new
   intervals. Adjacent additional excerpts within one result item are merged
   when their source hashes and roles agree and their combined length is at most
   80 lines. Gaps, different roles, different source versions and primary snippets
   remain separate. Admission recomputes actual serialized size and interval count
   after merging, including any saved overhead; failed admission restores the
   previous excerpts. At most 64 additional intervals are retained, and interval/byte
   omissions are explicit. Occurrence work omissions are reported separately,
   and final work counters include source-context selection. Required metadata
   and primary snippets are reserved first. Final packing removes optional
   intervals, then edges, then primary
   snippets before discarding symbol metadata. An oversized source line cannot
   by itself erase its symbol or stop consideration of later candidates; excerpt
   omission reports the effective byte cap without altering source text.
   `context_lines=0` disables all source excerpts; nonzero values control the
   primary excerpt radius while bounded implementation expansion uses remaining
   space. Execution clamps the radius to half the ten-line primary ceiling so
   oversized public/decoded requests cannot move the excerpt away from its anchor.
   `detail` selects how much of that evidence reaches the payload. `compact` (the
   CLI default) returns the item shape in §9.1 — node, one primary snippet and
   impact — and computes matched-body facts internally to anchor the snippet
   without publishing them. `full` adds the labelled `excerpts` and matched-body
   `evidence` fields. The library's `ExploreQuery::new` keeps `full` by default;
   a host that pays per byte selects `compact`. Compact exists because the
   evidence fields roughly doubled the per-item payload and pushed `explore`
   above the `text` search it replaces.
3. **Connect** — the edges among the returned nodes, up to `hops`.
4. **Summarise impact** — for function/method seeds, a one-line blast-radius
   count.
5. **Bound** — total output capped by `--max-bytes`; anything dropped is
   reported in `truncations`.

`explore` is where the context-efficiency hypothesis lives: the goal is one call
that returns *enough to act on* within explicit budgets. A complete small
implementation can be cheaper and more useful than repeated partial reads;
large implementations receive selected intervals rather than unbounded bodies.

### 8.5 `status`

Reports whether an index exists, its store path, schema and parser versions,
node/edge counts by kind, per-language file counts, last-index time, and
staleness (observed changed-path count under the selected verification mode).
A completed inspection exits 0 whether or not an index exists.

Status counts are exact summaries cached with each graph generation, including
unresolved edges; reading them does not clone or traverse the graph. Status honors
request cancellation/deadlines and uses the same bounded freshness verifier as
search, without automatically reconciling. Incomplete verification fails explicitly.
Both the library status result and compact CLI JSON envelope fit the 64 KiB ceiling.
When changed-path details overflow, a deterministic suffix is omitted with a
`bytes` coverage notice; the observed changed total, graph counts, generation and
policy remain intact. Required metadata that cannot fit returns `ResultBudget`.

---

## 9. Output contract

### 9.1 The JSON envelope

`--json` (or `--format json`) writes exactly one JSON document to stdout:

```json
{
  "schema_version": 4,
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

- `schema_version` is the independent `RESULT_SCHEMA_VERSION` (currently 4),
  distinct from the stored projection schema and generation descriptor format; the CLI refuses
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
| `explore` | a `{ "node", "snippet": {"source_hash", "start_line", "lines":[…]}, "impact": {…} }` |
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
- File/text scans account for serialized hit bytes while collecting results,
  then fit the complete library result, including source identities, counters
  and notices. Final fitting removes a suffix and removes a source identity only
  with its last hit. `stats.matches` retains the number of text hits collected
  before final packing; it is not an exhaustive corpus total. Result count
  ceilings apply inside execution even to directly constructed/deserialized
  queries. The CLI separately fits the complete compact JSON envelope, including
  echoed query fields, to the same 64 KiB hard cap. Required metadata that cannot
  fit yields `ResultBudget`; it is not silently discarded.
- `signature` is one line capped at `MAX_SIGNATURE_CHARS` (200).
- Indexed snippets are emitted only when the observed file hash matches its
  indexed fingerprint. On mismatch, the snippet is absent and
  `context.sources[path].verification` is `mismatch`; unavailable and budget-limited
  reads are distinguished. A live body hit carries its own source hash and may
  still show live evidence while the indexed fingerprint is stale.
- A request-local source cache shares the bytes used for body matching and
  snippets. Reads are capped by the walk policy per file and 8 MiB per request
  (with one extra byte used to detect overflow). Coverage counts distinct cached
  paths withheld by read budgets and reports `source_bytes` or `source_file_bytes`
  truncations even when those paths never become selected results.
  Snippet lines preserve source spelling without inserting ellipses; each snippet
  carries the BLAKE3 of the original file bytes. The complete serialized result,
  including provenance, must fit the requested output budget.
- Byte coordinates are zero-based half-open UTF-8 ranges; display lines are
  one-based. Parser revision 4 invalidates older Rust and JS/TS offsets.

Graph and impact packing enforces 64 KiB on the compact serialized library
result, including provenance, at both the core and service boundaries. It removes
tail edges before ranked nodes, preserves candidate/depth counts, and recomputes
approximation counts from delivered edges. If required metadata alone cannot fit,
the query fails with `ResultBudget`; metadata is not silently stripped. The
graph/impact CLI serializes compact JSON and applies a second check after adding
its transport envelope. It trims tail edges before ranked nodes, preserves impact
depth totals and required provenance, and refreshes delivered-edge counts/source
identities. Required metadata overflow returns `ResultBudget` without partial
stdout. Payload packing is independent of traversal-work limits.

Graph navigation targets, both shortest-path endpoints, explore query text and
path-filter fields have an 8,192 UTF-8 byte ceiling. The service checks these
lengths before freshness or automatic maintenance, and core navigation independently
checks targets before copying IDs or reporting lookup failures. Oversized-input
errors describe the field and limit without echoing its contents. This is byte
validation; later syntax, resolution and analyzed-term validation retain their
own contracts.

Ordinary CLI `--no-reconcile` searches use the query result's provenance directly;
they do not run a separate status inspection whose result would be discarded.
Scoped live file/text scans therefore do not depend on enumeration in unrelated
subtrees. The explicit `--fail-if-stale --no-reconcile` workspace check remains.

Graph queries share `WorkLimits` across adjacency reads, graph expansion, path
search, connection search and impact summaries. Defaults admit 10,000 distinct
node identities and examine 50,000 adjacency entries; hard ceilings are 100,000
and 500,000. Filtered adjacency entries consume work too. Exhausted caps return
partial results with `graph_nodes`/`graph_edges` truncations; candidate and impact
counts are then lower bounds. `stats.graph_nodes_visited` and
`stats.graph_edges_examined` describe the work charged to these budgets.
Graph path/language filters restrict output, not traversal. Expansion can cross
an excluded intermediate node to discover an included node at a later hop, and
that intermediate work still consumes its budget. Delivered edges require their
known endpoints to satisfy the output filters. A resolved edge may refer by stable
ID to an endpoint omitted by the result-count cap: this is a compact, navigable
boundary reference, not an unresolved relationship. Delivered-edge and byte caps
still apply. Presentation filters are not an authorization boundary.

A separate `WorkLimits.returned_edges` allowance caps delivered relationships
at 1,000 by default (10,000 hard ceiling); zero suppresses edge output. Every graph,
impact and explore query applies it before final serialization, with a
`returned_edges` truncation only when a relationship was omitted. Traversal work
and impact depth totals retain their independently measured values; approximation
counts describe the edges actually delivered.

Metadata and body retrieval share limits on candidate admissions (10,000 by default,
100,000 hard ceiling) and examined postings (200,000 by default, 1,000,000 hard
ceiling). `candidates` and `postings` truncations report incomplete retrieval;
`stats.retrieval_candidates_admitted` and `stats.lexical_postings_examined` expose
charged work. Fusion gives metadata at most half the remaining lexical budget
(rounded up), reserving capacity for body retrieval. Metadata-only execution uses
the full remaining budget; automatic body-first discovery skips metadata unless
its body pool is empty. The live overlay receives
at most half what remains; indexed body search uses the remainder. Unused capacity
is retained. Lane caps report incomplete retrieval even if another lane leaves
capacity unused. Exact candidates reserve admission before lexical accumulation.
Filtered postings still consume examination work, while filters precede candidate
allocation. Corpus statistics describe the complete indexed symbol set, not only
the admitted query prefix. The scorer retains its existing weights, combined
length normalization and clipped IDF; formula changes are evaluated separately.

Unscored metadata examination has an independent `WorkLimits.metadata_entries`
allowance: 100,000 by default, clamped to 1,000,000, with zero permitted. Exact-name
lookup, case-folded exact seeding and path navigation charge each record before
kind/path/language filtering. Rejected records consume examination work without
consuming candidate admission. `metadata_entries` truncations and
`stats.metadata_entries_examined` expose incomplete examination. Successive
retrieval phases share the remaining allowance; lexical lanes do not divide this
allowance because body postings use their own examination budget. Prefix expansion
continues to use its independent dictionary and posting limits. Identical bare
and qualified name entries are stored once, so duplicate filtering cannot hide
an uncharged second scan. Direct exact-ID lookup and bounded ambiguity lookup
remain dictionary/property lookups rather than metadata scans.

Explicit name-prefix lookup expands at most 256 dictionary entries by default,
with a hard ceiling of 4,096. `dictionary_entries` truncation and
`stats.dictionary_entries_examined` expose incomplete expansion. Bare/qualified
entries are charged separately, including repeated spellings; posting entries
are charged before filtering, and candidates after filtering/deduplication.

A reused query engine resets its accounting at each public query entry point.

`WorkLimits.walk_entries` bounds visible entries processed by file/text queries
and explore's body-candidate enumeration, defaulting and clamping to 1,000,000.
Zero is permitted. The allowance is shared across successive walks using the
same work budget; each walk also honors the independent policy ceiling. The
root, visible directories, files and yielded entry errors consume this allowance.
Entries suppressed internally by ignore/exclusion processing are not counted.
One unprocessed lookahead entry per walk distinguishes exact completion from
overflow. Coverage reports processed entries and `enumeration_complete=false`
with `walk_entries` on exhaustion. Such a report fails `require_complete` and
cannot safely infer deletions. Query limits do not change the stored inclusion
policy or its fingerprint. Service freshness walks consume this same allowance.
Automatic index maintenance and status freshness checks share this allowance.

The core `stale::inspect_with_work` entry point supports bounded freshness checks
using an existing `WorkBudget`. It requires complete enumeration before comparing
the manifest and charges content reads through the shared chunked reader. An
insufficient source-file or byte allowance returns `IncompleteVerification`,
not a successful fresh observation. Metadata-only checks read no source blobs.
The service takes one complete request-scoped observation for both the
reconciliation decision and returned generation context. If automatic maintenance
publishes a generation, it inspects again afterward; the pre-maintenance context
is discarded. Observations never persist across requests. Both passes, when
needed, consume the same request allowance; actual verification reads are charged.
This is an observed source boundary, not an atomic filesystem snapshot; indexed
snippets still independently verify the bytes they deliver. Retrieval receives
the already-spent budget through `QueryEngine::with_work_budget`, so it cannot
reset source or enumeration allowances. Its first query inherits the supplied
work; later queries on that core engine reset normally. An incomplete freshness
check fails the service call rather than returning partially verified provenance.

Hosts can use `index.search().with_work_limits(limits)` with a cloneable
`CancellationToken` and optional monotonic deadline. Checkpoints return explicit
cancellation/deadline errors. Already-cancelled or expired requests fail before
freshness reconciliation. File and literal-text service queries use the same
cancellation/deadline settings without requiring an index. Their walker checks
before and after each iterator advance and around final sorting; file matching
checks each entry, and text search checks between 8 KiB read chunks and source
lines. Explore's body-candidate enumeration also uses the checked walker.
Cancellation returns an error, not a successful partial response. Existing
policy enumeration/per-file read caps and result limits remain separate from
numeric graph work limits.

Literal scans and explore's live source cache share request source quotas:
`source_files` defaults to 10,000 open attempts (ceiling 200,000), and
`source_bytes` defaults to 64 MiB (ceiling 1 GiB). Both permit zero. Filters run
before literal-file admission. Failed opens consume attempts; binary, invalid
UTF-8 and failed-read prefixes consume actual bytes. Cache hits cost no new
source work. Existing cache and per-file ceilings still apply independently.
Readers may consume one additional byte to detect aggregate overflow; this byte
is included in `stats.source_bytes_read`. `stats.source_files_attempted` counts
admitted open attempts. An incomplete file supplies no text or observed hash;
previously completed evidence remains usable. Exhaustion reports `source_files`
or `source_bytes`. Exact EOF at the byte ceiling succeeds unless another source
is needed. These counters include service freshness and automatic index-maintenance reads.
Change-detection hashing and extraction are separate reads and are both charged.
Maintenance source exhaustion returns `IncompleteMaintenance`, and incomplete
walks fail before inferring deletions. Cancellation checkpoints precede publication
and metadata-only commits. Failed maintenance preserves the prior generation.
A completed maintenance publication is not rolled back if subsequent freshness
verification or retrieval exhausts the remaining request allowance.

These controls are cooperative, not a hard wall-clock bound. One iterator advance
may perform internal directory/ignore processing, and an in-flight filesystem
operation, sort, line transformation or matcher call is not preempted. Index
opening, snapshot materialization, an individual parser call and atomic publication
are not preempted; broader phase and memory limits remain necessary.

Both adapters prepare a native adjacency index with each generation. Edges are
stored once in stable order; node adjacency contains integer positions. Bounded
reads stop before cloning omitted entries, without materializing or sorting a
whole high-degree neighborhood. This adds generation preparation/memory cost;
query work does not rebuild the adjacency index. Core query expansion uses one
visited set, so cycles cannot repeatedly expand an already visited node.

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
- **Atomicity.** One coherent `publish` per reconcile (§6.4).

### 11.1 Delivery shapes — the in-process library is the target

There is one API (`SearchService`, §4.7) and three ways to reach it. **Shape 3 is
the reference and the thing we build**: it is what a linked-in `nanus` tool would
be, so it is what the evaluation measures.

1. **One-shot CLI** — each command is its own process that opens the library,
   answers, and exits. This is how `nanus` drives the experiment through `bash`
   before any integration; it is a worse proxy for integrated behaviour only in
   its per-call startup cost.
2. **Persistent socket service** (optional, later) — `graph-search serve` owns a
   workspace and answers clients over a local socket, to share one warm index
   between processes. The same pattern `nanus` uses for its own link, and **not**
   required by the target.
3. **In-process library** — **the reference and the target.** A host links
   `graph-search` and holds an `Index` open: no IPC, no per-call startup, indexes
   resident. The evaluation harness and a future `nanus` adapter both use this,
   and it is built from M1.

The order is deliberate: shape 3 is the target; shapes 1 and 2 are conveniences
around it.

What residency (shape 3) changes:

| Operation | One-shot client (shape 1) | In-process library (shape 3) |
|---|---|---|
| `files` | walk (fresh, ~10–50 ms) | **index-first** — path set resident, in-memory glob |
| `text` | mmap + SIMD scan | scan of resident/mmap bodies; a **trigram index** becomes affordable at scale |
| `graph`/`explore` | store open per call | no open; lower latency; room for a richer single call |

So residency flips `files` to fast **and** accurate — the one place it changes the
answer — and lowers the latency of every graph query, which is what makes a
richer one-call `explore` viable. `text` remains body-bound; residency saves
syscalls and enables an incremental substring index, but the bytes still have to
be examined.

A resident host must meet five requirements:

- **It owns freshness.** A long-lived index drifts. The host reconciles on a
  debounced watcher (`notify`) or before a query, and reports staleness either
  way. A host that answers from a drifting index is **less** accurate than the
  one-shot walk — the failure mode to design against, and the freshness machinery
  §2 deferred.
- **Lifecycle.** Shape 3 is held by the host (a session owns it). Shape 2, if it
  is built, adds one service per workspace root, claimed by a lock file; a `0600`
  socket; an idle timeout; and a version handshake.
- **Fallback.** A one-shot client with no cold index walks for `files`/`text`,
  and for graph modes either reconciles locally or returns a clear "not indexed"
  answer. The capability is never unusable because an index is absent.
- **Single writer and the explicit borrowed-snapshot/locking contract** (§6.6).
- **A memory bound.** The path set is cheap; bodies are not. Prefer `mmap` over a
  heap copy, and cap any body/trigram cache.

Accuracy under a resident shape is therefore *unchanged from the one-shot walk
if, and only if, freshness is enforced*; it is worse if the index is allowed to
drift.

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
| `MAX_MATCH_LINE` | 400 UTF-8 bytes before ellipsis | one echoed match line |
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

| Arm | How the capability is reached |
|---|---|
| **Baseline** | `nanus` as it is: `glob`, `grep`, `read`, `edit`, `write`, `bash`. |
| **Treatment A — CLI** | The same, plus a system-prompt line pointing at `graph-search search …` / `sync`: the one-shot client (shape 1), reached through `bash`. |
| **Treatment B — in-process** | A harness that links the `graph-search` library (shape 3), holds the `Index` open across a turn, and exposes it to the agent as a pre-warmed handle. This is the arm that models the integrated tool. |

Everything else is held constant: same model, same effort, same sandbox/approval
state, same turn budget. The gap between A and B *is* the cost of not being
in-process — the number that decides whether linking the library into `nanus` is
worth it.

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

### 16.6 Pre-registered prediction (priors, before building)

Recorded *before* M1 so the measurement has something to falsify. These are
predictions, not results. Sources: (a) **codegraph's** published benchmark — 62%
fewer tokens and 44% lower cost on average across seven repos, 57–78% fewer on
discovery-heavy questions, and ~80% **more** residual context; and (b) `nanus`'s
own limits, below.

Anchors from `nanus`, used by the token model:

| Fact | Value |
|---|---|
| `read` default window | 2 000 lines (ceiling 20 000, ≤ 4 MiB) |
| `grep` default / ceiling | 250 matches / 2 000; line cap 400 UTF-8 bytes before ellipsis |
| `glob` default / ceiling | 100 / 1 000 |
| Context budget | 64 000 estimated tokens (chars ÷ 4), drop-oldest |
| Step budget | 512 |

The multiplier that matters: `nanus` replays the whole log on every step, so a
tool result added at step *s* is re-sent on every later step. A turn's token cost
is ≈ Σ (prompt size per step), which grows with **both** the size and the number
of results. Replacing a chain of `grep`/`read` results with one bounded result
cuts both, and the step reduction compounds it.

Per-archetype prediction (a mixed suite ≈ 20% trivial, 15% discovery, 25%
definition-location, 25% structural, 15% broad):

| Archetype | Baseline calls / steps / tokens | Treatment calls / steps / tokens | Δ tokens |
|---|---|---|---|
| Trivial literal find | 2 / 1–2 / 5–12k | 1 / 1 / 5–10k | 0 to −20% |
| File discovery (glob) | 1 / 1 / 0.5–3k | 1 / 1 / same | ~0% |
| Locate a definition | 3–4 / 2–3 / 8–20k | 1 / 1 / 1–2k | −70 to −85% |
| Structural (callers / impact / flow) | 4–12 / 4–10 / 25–120k | 1–3 / 1–2 / 3–8k | −70 to −90% |
| Broad exploration | 8–20 / 6–15 / 40–200k | 1–2 + reads / 3–5 / 10–30k | −60 to −85% |

**Headline prediction:**

- **Tool calls:** −40% to −65% on discovery-heavy tasks; **0%** on trivial and
  file-discovery tasks.
- **Tokens:** **−30% to −55%** weighted across a mixed suite; −60% to −85% on
  discovery-heavy tasks; ~0% on trivial ones.
- **Wall time:** −25% to −55% on discovery tasks (steps × model latency dominate).
- **Cost:** tracks tokens, −30% to −55%.
- **Accuracy:** neutral-to-positive *if adoption is high*; the risks are
  static-graph false negatives (dynamic dispatch, macros, HTML/CSS class matching)
  and stale indexes. A confident-wrong answer is a fail whatever the token number.
- **Residual context:** predicted **lower** than baseline here, unlike codegraph,
  because `explore` is bounded (≤ 64 KiB, small snippets) and `nanus` elides at
  64k. If it comes out higher, the payload discipline (§8.4, §9.3) failed.

Two confounds to control:

1. **Adoption.** The benefit is zero if the model does not reach for the tool.
   Record the fraction of discovery tasks where it did (target: > 50%).
2. **Cold index.** A first query on an unbuilt index pays the build (~2 s per 200
   files). Warm the index before a timed run; measure build/sync separately.

Falsification thresholds (the go/no-go for §18):

- **Proceed** if a mixed-suite run shows **≥ 25% fewer tokens or ≥ 35% fewer tool
  calls** with **no rubric regression**.
- **Strong** if **≥ 50% fewer tokens *and* ≥ 50% fewer calls**.
- **Kill / rethink** if rubric scores regress, or tokens rise, or stale/truncated
  answers are mistaken for complete ones.

The measurement protocol and a results table to fill in live in
[`docs/benchmark-plan.md`](docs/benchmark-plan.md).

---

## 17. Milestones

| # | Milestone | Deliverable | Notes |
|---|---|---|---|
| **M0** | Repo + spec | This document; a workspace skeleton — including the `graph-search` library crate — that builds. | **Current.** |
| **M1** | **Library + `files`/`text`** | `types`; `core` ports + walker; the **`SearchService` API** (§4.7); the `graph-search` library wired for `files`/`text`/`status`; a thin CLI with `--json`; contract tests. | Ships `glob`/`grep` parity **as a library**. No engine, no parser. |
| **M2** | Engine + index/sync | Grafeo adapter; `Index::open` + `reindex`/`sync`; manifest + reconcile; the **resident handle** (index-first `files`); store conformance suite. | Grafeo's feature set is pinned here. |
| **M3** | Rust extractor + graph modes | `langs` Rust extractor; `symbol`/`refs`/`callers`/`callees`/`impact`/`deps`; staleness-aware lazy reconcile. | One language proven end-to-end; the core value. |
| **M4** | TS/JS + HTML/CSS | Remaining extractors; `neighbors`/`path`; HTML/CSS cross-edges. | Languages can land behind features. |
| **M5** | `explore` + eval harness | Combined retrieval; the task suite; the in-process harness (shape 3) and the CLI arm (shape 1). | The context-efficiency bet is measured here. |
| **M6** | Evaluate → decide | `docs/evaluation-*.md`; a go/no-go on §18. | Optional here and only if the numbers ask: a socket `serve` (shape 2) and a trigram `text` index. |

Each milestone is independently useful: M1 alone is a `glob`/`grep` library; M3
alone is a code-graph library. The **library API exists from M1**, so every later
milestone is reachable in-process without rework.

---

## 18. Future `nanus` integration (only if §16 says so)

The library is already shaped for this: `SearchService` (§4.7) is what a `nanus`
adapter would call, so integration is a link plus a port, not a rewrite.

1. **Adapter.** A `nanus-adapter-graph` crate depends on the `graph-search`
   library, **feature-gated**, so a minimal `nanus` build stays dependency-light
   and `search` degrades to `files`/`text`.
2. **Port.** Add a `SearchPort` (or `GraphPort`) to `nanus-ports` whose methods
   mirror `SearchService` (§4.7); the adapter implements it. Vendor-free, as
   every nanus port is.
3. **Tool.** Replace `glob` and `grep` with one `search` tool (`ToolAccess::Read`)
   whose modes are §8.1–§8.4. The tool count goes 7 → 6.
4. **Lifetime.** The tool holds one `Index` for the session — a resident
   in-process handle — so `files` is index-first from the first call and no
   per-call startup is paid (shape 3).
5. **The CLI stays** as an independent client and as the `bash` fallback.
6. **Removal criteria.** `glob`/`grep` are removed only when the tool's modes
   reproduce their semantics (§8.1–§8.2) and the evaluation shows no regression.

The `graph-search` repo remains where extraction and engine choices are
developed; `nanus` depends on the library, not the other way round.

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
6. **Resident in-process vs one-shot for measurement.** The target is resident
   (shape 3) and the CLI is one-shot. How much of the measured difference is real
   retrieval value rather than per-call startup? (This is Treatment A vs B, §16.2.)
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
12. **`files` index-first crossover.** With the library resident, when does an
    index-backed glob beat a walk? Measured, not assumed; it also decides whether
    the one-shot CLI should ever prefer the index.

---

## Appendix A — Node and edge vocabulary (summary)

**Nodes:** `file`, `module`, `function`, `method`, `struct`, `enum`, `trait`,
`impl`, `type_alias`, `const`, `static`, `macro`, `field`, `variant`, `class`,
`interface`, `variable`, `export`, `element`, `css_rule`, `css_at_rule`,
`css_custom_property`, `concept`, `section`.

**Edges:** `contains`, `imports`, `exports`, `calls`, `references`, `extends`,
`implements`, `type_uses`, `links_to`, `loads_stylesheet`, `uses_class`,
`selects`, `cites`.

## Appendix B — Example JSON (illustrative)

`graph-search search callers 'ToolRegistry::execute' --json`:

```json
{
  "schema_version": 4,
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

### Explicit task-prompt query policy

Ranker revision 15 adds `RetrievalOptions.query_policy` (`verbatim`, the default,
or opt-in `task`; CLI `explore --query-policy task`). For `Auto` and `Terms`, task
policy removes repeated terminal procedural sentences from this exact vocabulary,
with ASCII case-insensitive spelling and a final period:

- `Cite the relevant source and explain the execution path.`
- `Distinguish observed behavior from assumptions.`
- `Propose regression tests; do not edit the repository.`
- `Do not edit the repository.`
- `Do not modify files.`
- `Include file paths and line numbers.`

A suffix must follow whitespace after a sentence-ending `.`, `?`, `!`, or a
newline, and nonempty content must remain. No paraphrase inference, arbitrary
stop-sentence removal or internal whitespace normalization occurs. A single pass
tracks single/double quotes, matching-length backtick runs and backslash escapes; apostrophes inside
words do not open quotes. An unfinished quote/code span protects the remaining
input. This is conservative query protection, not a Markdown parser. Quoted
literals are not extracted, decoded or rewritten; raw punctuation-sensitive
matching remains the separate literal text route.

Only ranked query terms change. Original input still governs inferred navigation
and automatic channel selection; explicit navigation, phrase and near modes
bypass cleanup. The original query is retained in `RetrievalPlan.query` and
`omitted_boilerplate` records removed sentences in source order when explanation
is requested. Input byte limits apply before processing, and remaining term
limits retain their existing error behavior. No corpus-rarity term truncation or
silent weakening of AND/minimum coverage is introduced. Verbatim remains default
pending a controlled quality comparison of this optional policy.

### Parser-owned documentation comment facts

Parser revision 6 retains `Extraction.doc_comments` alongside symbols and
references. Rust `///` (excluding `////`), `//!`, `/**` (excluding `/***` and
`/**/`) and `/*!` are recognized only on syntax-tree comment nodes. JS/TS retain
`/**` blocks under the same exclusions. Authored markers, UTF-8 and line endings
remain in the original spans; strings and template literals are not scanned for
comment-like substrings. No JSDoc tags, Markdown content or Rust doc attributes
are interpreted by this initial fact extractor.

Rust inner documentation refers to the nearest enclosing extracted declaration,
or the file when no such declaration exists. Outer documentation considers only
the next syntactic neighbor after comments and Rust outer attributes. A direct
declaration or unambiguous export/variable declaration wrapper can supply an
`owner_key`. Intervening statements, multiple sibling declarations, destructuring
with multiple same-span symbols, and duplicate declaration keys remain unowned.
`inner` distinguishes inner documentation from unassociated outer comments.
Associations are file-local raw fact keys, not new graph edges.

At most 8,192 documentation occurrences are retained per file. An actual extra
eligible comment sets `doc_comments_truncated`; exact fill is complete. Merge
orders facts by original byte span, deduplicates equal records and applies the
same cap, preserving prior truncation. Legacy extractions decode with empty
comment facts and false truncation. Shared extraction packs publish these fields
with the existing source fingerprint and generation identity. Source-unit
segmentation currently continues to index authored comments as ordinary code
text; this raw-fact addition does not change the body ranking representation.

### Documentation source regions and retrieval associations

Source representation 8 / chunker policy 9 partition parser-recognized comment
spans into `DocumentationComment` units. Original UTF-8 byte offsets are retained;
retrieval display lines are normalized to the last covered line rather than the
parser's potentially following-line end point. Long comments use the existing
80-line / 8-line-overlap windows. Comments are partitioned out of ordinary body
regions, not copied into declaration term fields.

Each fragment carries the complete comment span, inner/outer style and optional
`documented_symbol`. `owner` continues to mean physical lexical containment. A
preceding outer comment can document a declaration without being owned by it;
an inner comment may have both relationships. Associations resolve only within
the same projected file. Stored validation rejects missing/foreign targets,
impossible containment/order, inconsistent shared descriptors, overlapping
comment descriptors and out-of-file spans.

Ranker policy 16 groups body candidates by documented declaration when present,
otherwise by lexical owner/file. Ranked evidence retains both relationships.
Phrase/near verification still scans original file bytes across storage boundaries;
a witness wholly inside a known comment can carry that comment's association.
A witness spanning comments or unrelated code does not gain such an association.
Live unparsed overlays carry no parser-owned documentation metadata.

`SourceFileUnits.documentation_truncated` and coverage counter
`source_documentation_truncated_files` report omitted or rejected comment
metadata independently of source-unit truncation. Invalid input spans are omitted
from documentation metadata while ordinary body text remains eligible. Existing
source-unit limits can separately truncate body coverage. Old serialized facts
and evidence default the new metadata to absent; representation drift triggers
normal source-index refresh.

Adjacent recognized documentation comments with the same inner/outer style and
same documented declaration are grouped when only whitespace separates them.
This preserves region-local all-term matching across Rust `///` lines. Raw parser
facts remain individual comments. An intervening ordinary comment or statement,
a different association, or a different style prevents grouping.

### Manifest-owned package context

Parser policy 7 and source representation 9 add native package-boundary context.
The standard library registry reuses the existing JSON/TOML decoders through an
optional `LanguageRegistry::package_manifest` port; core names no syntax decoder.
Recognized files are exactly `Cargo.toml`, `package.json` (with
`pnpm-workspace.yaml` workspace roots) and `pyproject.toml`. The native subset
retains the authored package name, ecosystem and package/workspace/unavailable
role. A Cargo `[workspace]` without `[package]` is not itself a package. A Node
manifest can define an unnamed boundary. A `pyproject.toml` takes its name from
PEP 621 `[project].name`, else `[tool.poetry].name`; with neither table it is an
unnamed boundary. This is package-context discovery, not
validation of every package-manager option or dependency/import resolution.

Manifest syntax input is capped at 262,144 bytes and retained names at 512 bytes.
Invalid syntax, unsupported name values or limits produce fixed unavailable
metadata diagnostics while admitted original text remains searchable. Known
manifest paths without available decoder/source facts are also barriers. No
install, configuration execution or lookup outside the walked inclusion policy
occurs. Ignored manifests are outside that policy.

The walker records observed manifest paths before the source-size filter. The
index manifest/header retains this boundary set; additions/removals participate
in freshness checks even when a manifest body is oversized and unindexed. A
nearest observed-but-unavailable boundary blocks outer inheritance. Partial walks
cannot infer boundary removals. Rust selects Cargo boundaries; JS/TS selects Node
boundaries; Python selects `pyproject.toml` boundaries. Other text, HTML and CSS
select the nearest unique boundary across all families; a tie is explicit
incomplete scope. Virtual Cargo workspaces stop
package inheritance without inventing a package.

`SourceFileUnits.package` and resolved indexed source-evidence identities retain manifest path,
manifest hash, ecosystem and optional authored name. Different manifest paths are
distinct even when names match. Units inherit package/file/language metadata from
the source file rather than duplicating ancestor bodies. Source/evidence
`package_scope_incomplete` and coverage `package_scope_incomplete_files` report
observed ambiguous/unavailable scope. Live unparsed replacement evidence omits
package association; indexed associations describe their selected generation.

Package metadata is validated against the complete post-batch file universe,
including a manifest's hash and declared role/name. Retained source facts cannot
refer to a removed or changed manifest identity. Both adapters reject incoherent
batches before visible mutation; lexical declaration-owner validation remains
file-local. The projector currently rebinds admitted files from cached facts when
any manifest changes or the observed boundary set changes. Narrow dependency
invalidation remains future work. Old serialized fields default to absent, while
policy/version drift triggers normal rebuilding.


### Shared package identity in results

Wire contract 4 and ranker policy 18 share repeated package identities before
source-context allocation. Source/parser representation versions do not change.
The withdrawn residual-fragment experiment used ranker 17; it is not production.

`SourceEvidence.package` remains an optional inline identity for single-use
packages and historical results. Repeated identities among selected seeds are
stored once in `ResultContext.packages`; evidence instead carries `package_ref`,
a result-local key such as `p0`. Full manifest path, content hash, ecosystem and
authored name determine equality. Same names never imply the same package.
Keys are assigned deterministically from sorted full identities, not source
traversal order, and have no meaning outside their containing result.

`SourceEvidence::package_identity(context)` resolves either form. It returns
`None` for absent, dangling or contradictory inline/reference associations.
Library-produced evidence never contains both forms. Empty tables and absent
references are omitted; older inline evidence and contexts without a package
table deserialize with default empty fields. Clients consuming wire 4 must
resolve references through the accompanying context rather than treating a
missing inline field as an absent package.

Both library and CLI budget fitting prune unreferenced table entries after item
removal. They retain the original keys for surviving references, even when only
one member of a formerly shared group survives. Full identity bytes count toward
the response cap once; reference bytes count on each evidence item. Table entries
are allocated only for selected indexed evidence. Live unparsed replacements
remain unassociated, and package-scope incompleteness remains independent.


### Primary source overlap accounting

Ranker policy 19 assigns a `(path, source hash, line)` to the first selected item
that carries it. Later overlapping primary snippets retain only their uncovered
contiguous runs: the first remains primary, and further runs use labeled source
excerpts. Paths and versions never merge, and candidate identity, rank, retrieval
facts and package associations are unchanged. A fully covered primary can be absent
without removing its selected item.

Pre-deduplication source eligibility is retained internally so such an item can
still contribute distinct declaration, matched-region or relationship context.
Eligibility removed by ordinary payload fitting stays removed; it is not mistaken
for shared text. Existing excerpts participate in coverage and interval accounting,
so the allocator cannot reintroduce duplicate primary lines or exceed the shared
interval allowance. Omitted fragments have an explicit snippet-limit notice and
are not marked as delivered. Exact final byte fitting still applies to the complete
library result and CLI envelope. No wire or persisted-source revision changes.

### Explore source captures across phases

Public explore requests retain bounded raw source captures across freshness,
automatic maintenance and evidence assembly. The request's work budget owns the
captures and charges physical file opens/bytes only on first capture. The evidence
cache separately accounts for bytes it materializes, including bytes captured by
freshness. Cache records cannot exceed admitted open attempts, and retained raw
bytes cannot exceed the source allowance; vectors and decoded text add bounded
allocation overhead. This is ranker policy 20; wire 4 and source representation 9
are unchanged.

Reusing a capture checks size/mtime drift and fails incomplete verification on a
change. A truncated capture cannot satisfy a larger later read. Cancellation and
deadlines apply to cached reads. Missing, binary and invalid-encoding inputs are
not silently promoted to valid text. Captures expire after the request; the next
strict request verifies source afresh. This does not promise an atomic live-tree
snapshot or detection of a concurrent writer that restores source metadata.


### Initial source admission

Ranker policy 21 checks selected-item metadata before final source reads. Oversized
metadata is omitted under the existing ranked item cap without spending source-read
allowance. Metadata-admitted items are also checked against the serialized growth
of a minimum useful primary and a lower bound on mandatory response metadata plus
the retained item prefix. Optional edges, source identities, package tables and
prior primary text are excluded from that response floor. This makes the test
conservative in the presence of deduplication and final trimming.

A skipped primary is reported as byte truncation and does not become eligible for
later context allocation. Known cached source identities can still be reported;
unread source is not labeled verified. Final fitting continues to use exact encoded
bytes. Candidate-generation and required freshness reads precede these guards.
Wire 4 and source representation 9 remain unchanged.

### Line proximity in source-window utility

Ranker policy 22 adds bounded proximity to source-context utility. Let `n` be the
number of distinct query terms in a candidate that have not already been delivered
for that item, and `w` the shortest inclusive line span covering those terms within
the candidate. Coverage contributes `160 * (1 + n)` integer units. If `n >= 2`,
proximity contributes `floor(80 / w)` additional units; otherwise it contributes
zero. The existing role/rank value multiplies this sum, which competes by marginal
estimated bytes. Exact encoded bytes still govern admission.

The proximity contribution is at most half one coverage unit, so an extra distinct
term wins at equal cost and role/rank value. Already delivered term bits are excluded
before span calculation. Repetition changes placement evidence, not term count.
The calculation uses bounded original line coordinates and does not alter lexical
candidate ranks, explicit phrase predicates, source boundaries or result fields.
The controlled quality evaluation is recorded in the implementation ledger.

### Request-local graph neighborhood reuse

A query may reuse the incident adjacency prefix already admitted by its shared
work budget. Relation and direction selection operate on that same prefix without
another snapshot adjacency charge. A partial prefix retains the request's graph
work truncation; reuse never asserts that omitted edges are absent. Cancellation
and deadlines apply to cache hits. Retention is bounded by charged adjacency entries,
empty neighborhoods need no retained key, and every new query clears the cache.
Path-search work remains charged separately. Ranker policy 23 identifies this
budget-sensitive change. Cache selection and cloning still consume CPU; adjacency
counter savings alone are not latency or memory measurements.

### Callable roots of qualified calls

A local callable declaration must not let a dotted member call inherit a member
from an unrelated outer namespace of the same name. Such unknown members retain
their original occurrence and become unresolved with
`lexical_member_target_unknown`. Known class/struct/enum/module declarations retain
the supported qualification route; this does not infer arbitrary function-object
properties. Rust `Type::member` remains distinct from value-level function names.
Parser revision 8 invalidates cached facts from the prior declaration bypass.

### Local JS/TS static class members

Direct dotted calls rooted in a visible file-local class bind only to a unique
static callable extracted under that class's key. The selected method retains
its lexical extraction identity and occurrence resolution class. A missing,
ambiguous, instance-only or accessor member cannot inherit a target from an outer
same-named class. This does not infer inheritance or arbitrary property dataflow.

Before class initialization, calls remain unresolved. Method parameter/body spans
receive a deferred-execution exception; computed names, heritage and fields do
not. Unmodeled initialization contexts are reported explicitly. The JS adapter
also recognizes the grammar's `property` field alongside TS's `name`, preserving
field-initializer references. Parser revision 9 invalidates prior extraction facts.

### Authored Cargo target facts

Source representation 10 adds optional `PackageManifest.cargo_targets`. Valid Cargo
package boundaries retain explicit `[lib]`, `[[bin]]`, `[[example]]`, `[[test]]`
and `[[bench]]` tables, optional authored name/path, `required-features`, package
edition (including explicit `edition.workspace = true`) and explicit automatic
search switches. Omitted settings stay unspecified. Paths retain their authored
bytes; extraction does not read them, normalize them or expand them outside the
workspace. Other manifest families and virtual workspaces have no target facts.

The existing 256 KiB manifest cap applies before decoding. Target projection has
additional bounds: 256 explicit targets, 64 features per target, 512 bytes per
name/feature, 4096 bytes per path and 32 bytes per explicit edition. Empty strings,
NUL and wrong field types make the entire target projection unavailable, with a
fixed diagnostic and no partial data. The independently valid package boundary
remains available. Persisted validation enforces these bounds, at most one library,
mutually exclusive explicit/inherited edition and absence of partial unavailable
metadata. Legacy missing target metadata remains readable; source-version drift
requests refreshed facts under the existing policy reconciliation contract.

These are raw authored facts, not Cargo semantic validation. No target discovery,
default edition or target-path inference, inherited-edition resolution, active
feature selection, build-script execution or module ownership follows from this
increment. In particular, Rust import resolution still needs a target-owned module
tree; merely retaining a custom `[lib].path` does not fix existing import edges.

### Native Rust module-declaration paths

Parser policy 11 separates `mod name;` declarations from generic import spellings
using an explicit raw-fact tag, exact declaration spans and inline/external module
attributes. Missing or ambiguous module projections cannot fall through to generic
import guessing. Resolution
uses the post-update Cargo manifest facts and walked paths. It recognizes explicit
and conventional library/binary/example/test/bench roots and build-script roots;
no Cargo command, script execution, dependency installation or extra source read
occurs in production. Source representation 11 additionally retains explicit empty
target arrays and the optional build-script setting, because omissions differ from
those authored values. Old omitted fields remain readable; policy drift refreshes
facts through the existing reconciliation mechanism.

Root discovery is bounded by the walked files and manifest limits. Edition-2015
automatic discovery defaults are per target family; explicit switches override
those defaults. Empty arrays count as declared families. An unresolved inherited
edition is used only where both supported edition rules agree. Conflicting names,
unavailable manifests or unsupported root settings keep affected context unknown.
The implementation does not validate every Cargo option or select active features.
See the [Cargo target contract](https://doc.rust-lang.org/cargo/reference/cargo-targets.html)
and [build-script setting](https://doc.rust-lang.org/cargo/reference/manifest.html#the-build-field).

A native worklist starts at known roots. Following a plain module declaration
adds the target file's stem directory (or the physical parent for `mod.rs`);
following `#[path]` adds its physical parent. Inline ancestors extend or override
the current directory according to the [Rust module rules](https://doc.rust-lang.org/reference/items/modules.html).
Ordinary unescaped strings and raw strings are supported. Escaped strings,
file-level inner path attributes, `cfg_attr`, unhandled attributes and
block-local modules remain explicitly unresolved. Attribute uncertainty propagates
through inline ancestors. Conditional `cfg` declarations are potential source
relationships, not proof of an active build configuration.

Each source file has at most two directory contexts. A visited worklist prevents
inclusion cycles from growing traversal; it does not certify compiler-valid crates.
A declaration resolves only when every reached context agrees on its target.
Different targets, or success in only some contexts, remain ambiguous. Explicit
physical-directory overrides can agree even where default child paths disagree.
Both `child.rs` and `child/mod.rs` present is ambiguity, never first-file wins.
Unreachable files have unknown module context rather than a filename-based guess.
Without Cargo, only conventional `lib.rs`/`main.rs` files seed root traversal.

Paths normalize only inside the workspace and resolve solely against walked paths;
absolute paths, backslash paths and workspace escapes are not followed. Inline
ancestry is capped at 256 declarations. The worklist has at most twice as many
states as walked files; each external declaration is evaluated at most once per
source file directory context. Unchanged source fact caches supply the syntax.

Rust edits and changes to the walked file set conservatively rebind cached external
module declarations, including previously unresolved
consumers and newly created conflicting files. No extra parser work is needed for
unchanged cached sources. Narrow module dependency invalidation remains work under
recommendation 23. This increment resolves module declarations and their physical directory contexts.
Anchored `use` paths, imported bindings, reexports, logical crate/module membership,
visibility and qualified cross-module calls still require the logical module tree
under 20/21. The old
generic Rust specifier helper is not evidence of completion of those requirements.

### Rust scope-key representation (parser revision 12)

Rust extraction keys prepend the immediate parent key once; that key already
contains the full ancestor chain. They must not prepend every ancestor's full
key again. Qualified display names, duplicate declaration groups, parent links
and reference ownership retain their existing semantics. Deep keys change, so
parser revision 12 invalidates cached extraction facts. This removes exponential
ancestor-prefix duplication; it does not claim linear total key storage or a
bound on parser nesting.

### Rust module lexical boundaries (parser revision 13)

Unqualified call binding lookup checks the current Rust module, then stops rather
than inheriting parent-module items. When no native binding is known, it records
`rust_module_binding_unknown` and suppresses bare-name fallback. Local declarations
remain eligible. Qualified path handling is unchanged. Imports, preludes and
complete logical module resolution remain separate requirements; this boundary
must not be treated as proof that an unresolved source call is invalid Rust.

### Authored Rust visibility (parser revision 14)

Rust extraction reads the grammar's `visibility_modifier` child. Explicit `pub`,
`pub(crate)`, `pub(super)` and `pub(self)` populate the existing visibility enum;
single-component `pub(in ...)` equivalents use the same syntax path kinds.
Original modifier bytes are retained in `rust_visibility_modifier`. Arbitrary
restricted paths and malformed modifiers are not promoted to public. Omitted
visibility remains unspecified in these authored facts; effective/inherited
visibility and access enforcement require the logical module model.

### Structured Rust use-tree leaves (parser revision 15)

Rust import extraction traverses syntax-tree groups instead of comma-splitting
source. Each leaf stores a normalized target path plus optional `RustUseFact`
metadata: local alias/name, glob status, type-only `self` import status and exact
authored reexport visibility. Original leaf spelling/span and lexical scope remain
attached to the reference. Grouped/trailing `self`, absolute roots, aliases and
nested siblings preserve their paths. `as _` introduces no named local binding.
Unsupported/error syntax produces one explicitly unresolved fact for the argument,
without exposing partial guessed leaves. Empty groups introduce no binding.

Legacy references omit `rust_use`; parser revision 15 refreshes raw facts. This
representation does not itself resolve imports, glob exports, module identities
or visibility. Those remain separate native resolution responsibilities.

### Native anchored Rust paths (parser policy 16)

The resolver traverses source-backed module member/interior links for `crate`,
`self` and repeated leading `super` paths. Root context comes from native Cargo
root discovery. Shared physical files retain all reachable roots; each context
must produce a unique visible member and terminal targets must agree. Unknown,
missing and ambiguous contexts cannot fall back to global-name lookup. Direct
anchored use leaves target the source symbol with explicit-import provenance.

Basic module-item public/crate/super/private visibility is checked at each segment.
Arbitrary restricted visibility and ambiguous private ancestry remain unresolved.
The context worklist admits at most 65,536 scope/root pairs before enqueueing;
overflow disables anchored resolution explicitly. Paths admit at most 256
components after the anchor. Syntax nodes normalize ordinary call paths while
preserving raw spelling. Parser policy 16 forces projection refresh.

Source/presence changes rebind cached anchored and structured-use consumers in
addition to external-module declarations. Full lexical import/reexport binding,
unanchored edition rules, general namespace/receiver semantics and narrower
incremental dependency indexes remain separate requirements.

### Lexical Rust import calls (parser policy 17)

Named use leaves create source-bounded `rust_import` lexical bindings. Calls
selected through those bindings retain original spelling, span and binding ordinal;
expanded target paths enter native module resolution with explicit-import
provenance. Module aliases support `alias::member`. Imports are visible throughout
their lexical scope; inner bindings and value initialization rules still apply.
Explicit type-only imports do not shadow value lookup, while namespace-qualified
aliases can coexist with value names. Mixed namespace conflicts requiring unknown
target kinds remain conservative. Parent-module imports do not leak into children.

Unmodeled globs block unknown-name fallback. Added alias target/provenance text is
limited to 8 MiB per file and each expanded target to 4,096 bytes, checked before
allocation; excess retains an explicit unresolved reason. Parser policy 17
refreshes cached facts. Unanchored imports, glob exports, general type
namespace/receiver resolution and type-use binding remain separate work; public
`use` reexports are modeled separately (see "Native Rust public reexports").

### Native ESM export bindings (parser policy 18)

JS/TS extraction retains a separate typed module surface in cached raw facts:
local named/default/namespace imports, explicit export names and aliases,
forwarding sources, star exports, type-only modifiers and original spans. An
absent surface denotes legacy/unavailable facts; an incomplete surface cannot
prove an imported target. Combining separate module extraction passes marks the
surface incomplete instead of inventing a shared lexical scope.

Import resolution traverses this authored export surface before selecting a
symbol. Private same-named declarations are not import targets. Local imports
can forward exports; explicit exports precede stars; stars exclude `default`.
Cycles terminate, multiple paths to one defining symbol agree, and distinct
star-export targets remain ambiguous. Existing lexical shadowing takes precedence.
Direct namespace member calls and named/default imported calls use this lookup;
type-only imports/exports cannot provide runtime call targets. Missing or
unsupported bindings retain explicit unresolved reasons, with no global fallback.

The extraction admits at most 4,096 import/export records and 8 MiB of their
owned text per file. Imported reference expansion separately caps added text at
8 MiB and target names at 4,096 bytes. Export declarations use sorted byte-range
lookup; import normalization uses a local-name map. Each export lookup caps
recursive depth at 64 and visits at 4,096. Exhaustion is unresolved rather than
partial proof. Current module facts share cached extraction identities in the
projector; they are not copied into every graph node.

This is a static source subset, not a runtime loader or complete type checker.
Anonymous default expressions, namespace values/reexports, escaped specifier
strings, CommonJS binding semantics, package export conditions, full type/value
merging and framework template relations remain unmodeled. Named relative imports
may resolve through a selected `tsconfig.json`/`jsconfig.json` (see "Native default
TypeScript project selection"); the existing relative-file candidate policy
applies, including runtime extension substitution, and does not itself promise
Node or TypeScript loader-mode parity.

### Authored Node package maps (source representation 12)

Package metadata retains optional native Node facts: module type, main, exports,
imports, workspace patterns and dependency specifiers. Export/import targets
preserve raw strings, explicit null blocks and unsupported conditional/array
values. Absent/empty maps stay distinct. The 256 KiB manifest cap applies before
projection; retained strings additionally cap at 4,096 entries, 4,096 bytes each
and 256 KiB total. Control characters are unsupported. Unavailable projections contain only a fixed reason. The
independent stored-fact validator checks limits, keys and ecosystem/role ownership.

JS/TS module lookup supports exact package-name self-references through exports
and exact package-private `#` imports. Nearest invalid/unavailable boundaries block
outer inheritance. Main never bypasses the self-export map. Paths must be `./`
package-relative, with no traversal, node_modules segment, encoded path or URL
suffix; lookup uses only known files. Exact targets precede unique runtime/source
extension substitutes (`js` to `ts`/`tsx`, `mjs` to `mts`, `cjs` to `cts`). Multiple
substitutes remain ambiguous; no implicit extension/index search applies here.
Reexport traversal uses the same lookup before checking the target's ESM surface.

Package manifest edits rebind conservatively. File-set changes also revisit
package-map consumers so unresolved default imports can become bound when their
mapped target appears. This remains broader than the desired precise dependency
index. Workspace selection, dependency versions, runtime conditions, external
import-map targets, CommonJS bindings and framework template regions remain
outside this increment. No Node runtime loader or experimental package-map API is
invoked.

### Configured store exclusion

The library resolves the configured store path, including existing symlinks,
when opening an index. Explicit relative `OpenOptions.store` paths remain
relative to the process working directory; configuration-file store paths remain
relative to the workspace. A store equal to or containing the source root is a
configuration error. The store is excluded as an exact subtree from the
shared walk policy. This exclusion applies to indexing, freshness, files, text,
and body-source discovery, independently of hidden/ignore flags and replacement
of default directory-name exclusions. Other directories with the same basename
remain source. External stores do not exclude same-named source directories; explicit search
scopes pointing to the external store are also excluded.

The policy fingerprint includes excluded paths as OS-encoded bytes; coverage
binds them through that fingerprint; `Index::policy` exposes the actual paths. A changed fingerprint causes reconciliation to
rebuild the admitted source projection, removing previously indexed store files.
The normalized path is fixed for the lifetime of the opened index; changing
filesystem symlink targets concurrently with an operation is not supported.

### Authored Node workspaces (source representation 13)

`pnpm-workspace.yaml` is a Node `Workspace` manifest, distinct from a package.json
package boundary. Its bounded native decoder supports explicit block-sequence
membership or an empty array and simple unrelated scalar/flat-list/map settings;
unsupported YAML invalidates the whole workspace projection. Manifest bytes and
retained metadata keep their existing limits. Persisted roles are validated
against the containing manifest filename.

Nearest pnpm declarations precede package.json workspace fields. Without a pnpm
file, nearest package.json workspace declarations are supported, except where an
explicit pnpm manager makes that field non-authoritative. Literal path segments,
whole-segment `*` and `**`, a leading `./`, and negative exclusions are supported;
wildcards do not consume dot-prefixed segments. Unsupported pattern syntax is an
explicit unresolved decision. The root package is included.

A workspace package dependency requires an authored dependency entry using
`workspace:*`, `workspace:^` or `workspace:~`, a unique admitted package name, an
explicit target export map, and a valid ESM export binding. Unknown potential
members prevent incomplete catalogs from claiming uniqueness. A shared one
million-unit construction limit charges ancestor probes, pattern bytes and glob
transitions; exhaustion makes workspace dependency bindings unavailable.

Conditional maps may retain a distinct `InvariantPath` fact when every branch
names the same path and every conditional object has a default, at depth at most
16. Differing, missing-default, array, blocked-branch and invalid-key projections
remain unsupported. This does not choose an execution environment or resolve
installed dependencies. Source version 13 refreshes these facts; parser 18 is
unchanged. Workspace edits participate in conservative manifest rebinding.

Workspace override selectors are retained from pnpm workspace files and root
package.json override/resolution settings. A selector that could affect the
selected package prevents a resolved workspace binding; override targets are not
interpreted. Unrelated simple selectors do not suppress the binding. Unsupported
nested/complex override forms conservatively block affected or all workspace
names. Override checks share the workspace-construction allowance. This avoids
claiming a local call target when configuration may redirect that dependency.

### Embedded script coordinates and binding-pattern expressions (parser 19)

The language adapter library provides `embedded::script` for a caller-selected
UTF-8 JS/TS byte range and a stable file-local domain. It translates symbol,
reference, scope, binding, documentation and ESM spans to original coordinates,
including lexical visibility bounds used during resolution. It namespaces all
symbol keys and key references consistently while preserving authored names and
module specifiers. Hoisted initialization sentinels remain zero. Checked arithmetic
rejects invalid or unrepresentable coordinates; a containing `.tsx` filename cannot
change the explicitly selected ordinary TypeScript grammar.

This helper does not itself identify Svelte/Vue/Astro regions: the registered
framework adapters do (see "Native framework script regions"). Namespaced identity
and `Extraction::merge` still do not prove framework visibility: module/instance
scope relationships are declared per dialect, but template relations are not
modeled and multi-region files retain an explicit incomplete ESM surface.

JS/TS destructured declarations use the same pattern-only identifier traversal as
lexical scopes. Default-value expressions and computed property keys contribute
references, not extra bound declarations; executable pattern expressions are walked
once independently of the right-hand initializer. Parser version 19 invalidates
cached extraction facts with the former behavior. Source representation stays 13;
no production dependency or persisted field was added.


### Raw TypeScript configuration facts (source representation 14)

The default library registry projects policy-visible `.json` and `.jsonc` files
into optional `TypeScriptConfig` facts within their authenticated source records.
This is a candidate configuration projection, not project discovery: ordinary JSON
files do not become projects, and named `extends` inputs are not opened outside
walk policy. A source's path/hash remains the origin of all authored values.

The native JSONC adapter handles a leading UTF-8 BOM, comments and trailing commas
outside strings, retaining exact decoded values for `extends`, `compilerOptions`,
`files`, `include`, `exclude` and `references`. Empty/null/absent fields and array
order remain distinct. Wildcard path keys separately preserve first-property
order; duplicate values replace without moving the key's first slot. Exact names
are independent of pattern precedence. Other compiler-option values stay raw;
configuration facts do not certify their compiler semantics. No compiler runs.

Input is capped at 256 KiB before JSON decoding. Retained metadata is capped at
4,096 values (including the wildcard-order list), depth 32, 4,096 bytes per string
or key and 128 KiB total string/key bytes. Invalid syntax, a non-object root or an
exceeded bound emits a fixed unavailable reason with no partial fields. Persisted
validation independently rechecks these bounds, wildcard-key set consistency,
source identity, path type and the representation floor. Legacy facts omitting
the optional field remain readable; source policy 14 refreshes current indexes.
Custom registries may leave the new optional projection port unsupported.

Native project selection, path-alias resolution and build-output mapping remain
subsequent integration work. A separate inheritance helper is described below. A resolver must validate supported
option semantics, retain origin directories, account for unavailable/excluded
parents, and invalidate affected bindings when configurations change. No such
bindings are introduced merely by storing these facts. Evidence and compiler
oracle scope are recorded in
`research/results/native-implementation/typescript-config-facts/README.md`.


### Native inheritance over selected TypeScript configuration facts

`core::typescript::inherit` takes a canonical workspace-relative configuration path
and one generation's source records. It performs no filesystem reads or default
project selection. Explicit relative bases resolve from the declaring directory:
try the authored file, then append `.json` if it is missing and does not already
end in `.json`. Only indexed `.json`/`.jsonc` facts with a supported source version
and valid metadata are admitted. Package-based and external inheritance are
explicitly unavailable, as are missing or excluded parents.

Bases merge in authored order, then local fields override. Compiler options merge
by option; each option's value replaces as a whole, including the `paths` map and
its wildcard order. Other retained fields replace as a whole. `references` is
local to the selected config and is never traversed. `extends` is consumed.
Relative values remain authored, accompanied by the declaring file for each
option or membership field. Dependencies retain all visited source hashes.

The helper validates the basic shapes of compiler options, membership lists,
references and extends, but does not validate every compiler option or calculate
project membership. It bounds a load to 64 distinct files, 32 active levels and
32 direct bases; cycles return an explicit reason. Effective metadata retains the
raw-fact limits and adds a 128 KiB total limit for origin/dependency keys and
values. Errors expose no partially merged configuration. Results own immutable
shared data; recomputation from a newer generation cannot alter an earlier result.

No default alias bindings, automatic project discovery or config-triggered binding
invalidation are implied by this API. Source/parser/ranker versions remain
14/19/23. See `research/results/native-implementation/typescript-inheritance/README.md`
for compiler comparisons, persistence checks and supported-scope limitations.


### Native selected-configuration alias dispatch

`core::typescript_aliases::Aliases` compiles paths/baseUrl from an effective config.
Exact keys take precedence over wildcard keys. Wildcards choose the longest
matching prefix; equal-prefix ties preserve authored property order. Only the
selected key is attempted, in substitution order, stopping at the first loaded
target. A matched key with no loaded target does not attempt baseUrl. No matching
key may use baseUrl; a separate result variant records that route.

The effective baseUrl resolves relative to its own declaring file and anchors
paths substitutions when present. Otherwise substitutions use the paths map's
origin directory. Relative specifiers remain for the ordinary module loader.
The callback receives a normalized workspace-relative candidate and its original
unexpanded substitution (`None` for baseUrl), preserving whether an extension was
authored or introduced by wildcard expansion. It owns module-mode, extension,
suffix and package-directory semantics. Callback errors stop dispatch.

Compilation checks raw metadata validity, option/array shapes, one wildcard per
key/substitution, origin/path bounds and workspace containment. At most 128
substitutions are allowed per key; expanded paths and eligible specifiers are
limited to 4,096 bytes. Trailing directory separators and empty-wildcard compiler
behavior are retained. No filesystem access, automatic project discovery or
binding publication occurs in this helper. Remaining compiler options must be
interpreted by its caller. Source/parser/ranker versions remain 14/19/23.


### Native modern TypeScript file loading

`core::typescript_files` compiles file-probing options for a caller-selected
bundler, Node16/NodeNext ESM or Node16/NodeNext CommonJS context. Source/runtime
extension families, declarations, authored module suffixes and JSON/custom wrappers
follow the tested TypeScript 6 resolution policy. A literal mapped extension may
probe its exact suffixed file first; wildcard-introduced extensions use replacement
order. ESM excludes implicit extensions and index-directory fallback.

`Lookup` reads only a coherent generation's supplied admitted-file and package-boundary
sets. It records distinct positive/negative paths plus boundary presence, and caps
one resolution at 256 distinct probes across all alias substitutions. Paths cap at
4,096 bytes; suffix lists at 32 entries of 128 bytes. Unsupported options, paths or
exhausted budgets return reasons. Unmodeled package-directory entry rules fail
before guessing index files; an unavailable manifest boundary is not an admitted
source file. Callers must retain observations for future invalidation work.

This helper does not discover projects, infer module mode, apply rootDirs, interpret
package directory entry fields or publish alias bindings. Default resolution and
source/parser/ranker versions remain unchanged (14/19/23) until integration. The
selected-context compiler matrix and publication tests are recorded in
`research/results/native-implementation/typescript-file-loading/README.md`.

### Native default TypeScript project selection

`core::typescript_project` is the first default integration of the configuration,
alias and file-loading helpers. For every generation it selects the nearest
admitted `tsconfig.json`/`jsconfig.json` whose directory contains an importing
file, resolves that configuration's bounded `extends` chain from the same source
records, and compiles its `paths`/`baseUrl` aliases with the selected module mode.

Only two modes are supported: `moduleResolution: "bundler"` and
`moduleResolution: "node16"|"nodenext"` (ESM, or CommonJS when the authored
`module` is `commonjs`). A configuration with `classic`, absent, `node10`, or any
other resolution mode is skipped rather than approximated. More than 32 admitted
configurations in one generation also disables selection explicitly. Failure to
inherit, compile aliases, or compile loader options leaves ordinary relative and
package resolution unchanged; it never guesses a target.

Bare specifiers consult declared aliases before package maps, in compiler order:
an exact `paths` key or longest-prefix wildcard key, then `baseUrl` when no key
matched. Relative specifiers are left to the ordinary relative-path policy. The
file loader probes only admitted paths; an unadmitted directory candidate makes
the lookup unmodeled, so resolution falls through instead of inventing an index
file. Package-directory entry rules keep their existing explicit-unavailable
behavior.

Alias resolution uses the existing bounded probe budget and does not persist new
dependency records yet. A changed or removed configuration conservatively rebinds
every JS-family consumer in the generation (including Svelte/Vue/Astro script
consumers) from cached facts; unrelated languages retain narrower invalidation.
That is broader than a precise alias-dependency index and is recorded as such.
Source/parser versions are 15/20; the ranker is unchanged. The compiler-comparison
and counterfactual evidence is recorded in
`research/results/native-implementation/typescript-aliases/README.md`.

### Native framework script regions (parser 20)

`graph-search-langs` registers Svelte (`.svelte`), Vue (`.vue`) and Astro
(`.astro`) adapters. They recognize declared script regions with a bounded,
dependency-free scanner and reuse the coordinate-preserving `embedded::script`
bridge for extraction:

- Svelte: top-level `<script>` elements. `context="module"` selects the `module`
  domain; otherwise the `instance` domain. `lang="ts"|"typescript"` selects
  TypeScript, `js`/`javascript` or absence selects JavaScript.
- Vue: `<script setup>` selects the `setup` domain; a plain `<script>` selects
  `default`. Language selection matches Svelte.
- Astro: an unindented leading `---` fence selects the TypeScript `frontmatter`
  domain; declared `<script>` elements select the `script` domain.

Tag and attribute names are ASCII case-insensitive. Quoted attribute values may
contain `>`; `<script` inside an authored HTML comment is not a region. A region
with an unsupported `lang`, an unsupported Svelte `context`, a malformed start
tag, an exhausted attribute bound, or no closing tag is recorded with a bounded
reason instead of being silently dropped. A region that fails to parse marks only
itself; other regions of the file still contribute facts.

Each recognized region is persisted as an `EmbeddedRegionFact` in the extraction
(span, kind, domain, extracted flag, reason) and the file's source record keeps
`embedded_regions`, `embedded_unextracted_regions` and `embedded_truncated`.
Coverage exposes `source_framework_region_files`, `source_framework_regions`,
`source_framework_unextracted_regions` and `source_framework_truncated_files`, so
an unmodeled dialect is visible to a caller instead of looking like an empty file.
Markup outside declared regions remains ordinary body text and stays searchable.
Template expressions, component tags, framework events, routes and injection
edges are not modeled; this adapter is not a template engine. Multi-region files
merge domains through `Extraction::merge`, which marks a shared ESM surface
incomplete rather than claiming one module identity. Source representation is 15
and parser policy is 20; no new dependency was added.

### Native Rust public reexports (parser policy 20)

A `pub use` leaf whose target is anchored (`crate::`, `self::`, `super::`) and
whose local name is known is also emitted as an `Export` symbol owned by the
republishing module. The authored target path and type-only flag are retained as
attributes; the exact visibility modifier is preserved. Private `use`
declarations, `use ... as _`, globs and unanchored targets publish no reexport.
Namespaced identities keep the republishing module's qualified name, so the
symbol participates in normal module-member lookup.

Anchored path resolution follows a selected reexport node from the republishing
module's own scope, so `pub use` chains work (`pub use crate::a::X as Y;` then
`pub use crate::Y;`). Visibility of both the reexport and its target is checked
with the existing module rules. Traversal is bounded at 16 hops and a cycle or
exhausted bound is unresolved, never a guess. Glob reexports keep their explicit
`rust_glob_exports_unavailable` reason. Reexport symbols are ordinary graph
nodes, so unchanged generation consumers do not need raw extraction facts
re-hydrated. The counterfactual and chained fixtures are recorded in
`research/results/native-implementation/rust-reexports/README.md`.

### Python extraction and pyproject.toml boundaries (parser policy 24, source representation 16)

The `python` language value (projection schema 4) and the tree-sitter-python
adapter (§7.5) replace indexing `.py`/`.pyi`/`.pyw` files as `unknown`. Parser
policy 24 forces projection refresh. `pyproject.toml` joins the recognized
manifests under the `python` ecosystem; its decoded fact reuses the TOML decoder
already used for `Cargo.toml`, so no dependency was added beyond the grammar.
An invalid document is `invalid_manifest_syntax`, a non-table `project` (or
`tool.poetry`) is `unsupported_project_table`, and a non-string or empty name is
`unsupported_package_name`. Each is an unavailable barrier, like every other
ecosystem. A directory holding both `Cargo.toml` and `pyproject.toml` (a maturin
layout) scopes Rust files to Cargo and Python files to the project; other text
at that level is explicit incomplete scope. Source representation 16 admits the
new ecosystem value in retained facts. uv/Hatch workspace tables, dependency
lists, `src/` layout discovery and import-name mapping are not modeled.

### Workspace crate paths and associated items (parser policy 25)

A Rust path whose first segment is neither `crate`, `self` nor `super` resolves
by the edition-2018 rules: a module declared in the origin scope is walked from
the origin, and otherwise the segment may name a workspace library crate. Each
Cargo package with a selected library root contributes its crate name (the
`[lib]` name, else the package name with `-` as `_`); two packages claiming one
name are `rust_crate_name_ambiguous`. Dependency tables are not consulted, so a
renamed dependency (`package = …`) stays unresolved. A crate outside the
workspace (`std`, `serde`) keeps `rust_import_path_unanchored`. Crossing a crate
boundary admits only `pub` items; `pub(crate)` and private items are
`rust_path_not_visible`. A qualified reference spelled through a workspace crate
(`other::item()`) enters the same resolver when every segment is an identifier.

`pub use` of such a path (`pub use tool::{A, B}`, `pub use other::C`) publishes a
reexport symbol like an anchored one; only a global `::` path publishes none.
`Type::member`, where the penultimate segment names exactly one struct, enum,
trait or type alias (directly or through a reexport) and no module, binds the
associated item or variant qualified `Type::member` in the type's crate. When
the crate declares several same-named types, the type's own file breaks the tie;
across crates only `pub` associated items and variants are visible. A missing
member is `rust_associated_member_missing`.

A member declaring `edition.workspace = true` inherits the nearest enclosing
workspace root's `[workspace.package] edition` for target auto-discovery, which
a virtual workspace manifest now records. A `cfg_attr` whose payload cannot
apply `path`, and `macro_use`, `rustfmt`, `no_std`, `no_implicit_prelude`,
`recursion_limit` and `feature` attributes, no longer make module paths
unavailable.

### Rust receiver types (parser policy 26)

A method call `x.method()` binds to `Type::method` when the receiver's static
type is stated in syntax; the occurrence's resolution class is `receiver`. The
extractor records each field's declared type (`rust_type`) and each callable's
declared return type (`rust_returns`), and the `Option`/`Result` success type of
either (`…_fallible`). A declared type is named through the declaring file's own
bindings: a declaration in the same module (`key:`), a `use` import expanded to
its path or an anchored path (`path:`), or `self` for `Self`. References, `Box`,
`Rc`, `Arc`, `Ref`, `RefMut`, `MutexGuard`, `RwLock` guards, `Cow`, `Pin` and
`dyn`/`impl` trait objects are peeled, since a method call dereferences through
them.

A call records how its receiver's type is stated: `self` (the enclosing impl or
trait), a typed parameter or `let` annotation, a struct literal, a field of
another receiver, the declared return of whatever another call in the file
resolves to (including `Type::new()` returning `Self`, and tuple-struct or
variant constructors), `clone()`/`to_owned()` of another receiver, or the
success type of `?`, `unwrap()` or `expect(..)`. Locals are block-scoped; every
identifier bound by a closure parameter, `match` arm, `for` pattern, `if let`/
`while let` or destructuring `let` shadows an outer local as untyped, so a
stale type is never reused. Anything else leaves the receiver untyped.

Resolution evaluates the description against the workspace: the member must be
a method or function qualified `Type::member` in the type's crate, the type's own
file breaking ties between same-named types, and another crate sees only `pub`
members. Evaluation is memoized per file and bounded at 32 nested steps. Any
step that cannot be proven falls back to the ordinary rules, which keep the call
unresolved; trait dispatch through generics is not modeled. A receiver call's
dependency record includes its member and the fields and types it reaches
through, so an edit to a declared field or return type elsewhere rebinds it.

### Test-owned symbols (parser policy 26)

A symbol or file is test-owned when its adapter marks it (Rust: every item in a
`#[cfg(test)]` item or module, including under `all(..)`/`any(..)` but never
`not(..)`, and every `#[test]`/`#[*::test]` function), when its qualified name
has a `tests` module segment, or when its path has a `tests`, `test`,
`__tests__` or `spec` directory, or a file name word `test`, `tests` or
`__tests__` (`tests.rs`, `test_io.py`, `view.test.ts`, `tests_support.rs`), or a
`spec` word beside another (`view.spec.ts`, not `spec.rs`).

Ranked discovery (`TestRanking::Defer`, the default) moves test-owned hits behind
every other hit, keeping each group's order; they still fill slots nothing else
takes, so a test is demoted, never dropped. Three cases keep a test's rank: an
exact name (the 2.0 tier); a query that spells every content word of the test's
qualified name outside `tests` modules (`log one step` for
`tests::log_one_step`, but not `search` for `UnusedFs::search`); and a query
that asks about tests (`test`, `tests`, `testing`, `spec`, `specs`, `fixture`,
`fixtures`, `mock`, `mocks`). `TestRanking::Neutral` (`--tests neutral`) is the
ablation.

On the frozen doc-07 query sets (exact, split, held-out and natural, three
repositories), deferral loses no hit and gains five; on the release-gate
evidence protocol (56 source-valid tasks) required files rise from 36 to 38,
complete regions from 13 to 16 and mean region coverage from 0.404 to 0.457.
The `nanus` evaluation probes carry no test-owned result in their top eight.
Deferring every non-exact test hit was measured first and rejected: `nanus`
split targets are mostly test functions, and their recall fell from 29/30 to
7/30.

Rust `type_uses` now descend into generic arguments (`Vec<Foo>` uses `Foo`);
the adapter had read a field name the grammar does not define. A `pub use` target
that is itself a reexport is followed at the terminal segment of any path.

### Open Knowledge Format bundles (schema 5, parser policy 27)

The `okf` language, the `concept` and `section` node kinds and the `cites` edge
kind (projection schema 5) and the tree-sitter-okf adapter (§7.6) replace
indexing bundle `.md` files as `unknown`. Parser policy 27 forces projection
refresh. The OKF extractor serves only files walked as `okf` (it accepts `.md`
and `.markdown`), so the extension alone never selects it. Every other extractor still claims files by
path. `deps` includes `cites` among its file relationship kinds. The grammar is
the only new dependency. Scalars are read as strings, never typed.
