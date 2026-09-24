# Proportional sync: design and plan

**Date:** 2026-09-24 · **Subject:** `main` `7e46a12` · **Goal:** a sync after an
edit costs in proportion to what the edit touches, not to the workspace.

Storage format 10. Existing stores must be rebuilt (`graph-search index`). There
is no migration and no backwards compatibility: the project is in early
development.

## 1. Where a one-file sync goes today (format 9)

A `sync` after editing one file in a 1,000-file workspace costs about the same as
a full re-index. This is true for OKF and Rust alike (`okf` Criterion bench:
Rust 735 ms against a full re-index of about 1 s). Only extraction, per-reference
resolution and hashing the changed file are proportional to the edit. Everything
else reads, rebuilds or rewrites the whole workspace.

| Stage | What it touches | Where |
|---|---|---|
| Walk, stat, manifest diff | every path (stat only) | `walk.rs`, `manifest.rs` |
| Dependency index load | inflate + parse all records; `validates()` rebuilds `consumers`/`selected_modules` | `store.rs` `dependencies()`, `dependencies.rs` |
| `extend_dependents` | `all_nodes()` + `all_edges()` unconditionally, two full passes over records | `reconcile.rs` |
| `annotate_packages`, `config_sources`, coverage | decode every source record, three times | `reconcile.rs` |
| Symbol table | third `all_nodes()`, every node cloned into 6-8 maps | `reconcile.rs`, `resolve.rs` |
| Rust module catalog and paths, JS surfaces, CSS/HTML cross tables | every node / every JS module | `resolve.rs`, `rust_modules.rs`, `rust_paths.rs` |
| Publish | load every fact, build a fresh in-memory Grafeo from the complete projection, re-validate everything, rebuild summary and dependency index, rewrite `graph.grafeo`, occurrences, dangling, dependencies in full, re-read and inflate every reused pack | `store.rs` `publish_generation`, `persist_prepared` |

Grafeo is the biggest single cost: the whole graph is rebuilt in memory and
re-serialized every publish, and rebuilt again (`open_read_only`) by every
reader. Grafeo's WAL does not help: it is a single-process, write-in-place log,
and the store's model is immutable generations read concurrently by several
processes under reader leases.

## 2. Target

For an edit touching `c` files whose facts total `f`, with `d` dependents to
rebind:

- **Sync work is O(f + d·log n)** in facts read, resolved and written.
- **Per-path bookkeeping stays O(F)** with a small constant: the walk and stat
  are inherently O(F), and a generation still carries a path → record index.
  These are bytes per path, not facts per path.
- **No full-graph read, rebuild or rewrite on the sync path.** `all_nodes` and
  `all_edges` become bulk operations for tests and conformance only.
- **Readers pay only for what they ask.** Opening a generation stays O(1) (format
  9's lazy open); a query loads only the shards and posting ranges it touches.

The integrity contract is kept: no byte is used before it is verified against the
generation's committed fingerprints, generations are immutable and published
atomically, and readers keep their generation under a lease.

## 3. Design (format 10)

### 3.1 Per-file shards replace Grafeo

Every fact a file owns lives in one **shard record**: its file node, its symbol
nodes, every edge it owns (resolved and dangling), its occurrence facts and its
source units. Shards are stored in the existing content-addressed pack format
(`source_records`: BLAKE3-named zstd packs, per-record hashes, hard-link reuse).
A sync writes new shards only for changed and rebound files; every other shard is
carried over by reference.

Node identity already encodes its owning path (`file:<path>`,
`sym:<path>#…`), so `node_by_id` needs no global map: it loads one shard.
`edges_from(id, Out)` reads the owning shard. Incoming edges come from the
posting tables below.

The graph port is served by native structures: an id-sorted node list per shard
and the adjacency postings. Grafeo is removed from `engine` (SPEC §4.3 is
updated; it already anticipated an in-process adjacency map).

### 3.2 Posting tables: sorted segments plus deltas

Global lookups become **posting tables**: sorted `(key, owner path, value)` rows
in immutable, binary-searchable segments. A generation references a **base**
segment and up to a few **delta** segments. A publish writes one new delta
containing:

- a tombstone for every path it replaces or removes, and
- the new rows those paths contribute.

A lookup merges base and deltas, newest first, dropping rows whose owner path is
tombstoned in a newer segment. When the deltas exceed a fraction of the base
(about 25%) or a count (about 8), the publish compacts them into a new base.
Compaction is O(table), so it is amortized over the O(changed) publishes that
made it necessary.

Tables:

| Table | Key | Value | Replaces |
|---|---|---|---|
| `names` | bare name, qualified name | node id, kind | `SymbolTable.by_name`, `qualified_candidates`, `MetadataIndex` name maps |
| `incoming` | target node id / target path | source node id, edge kind | adjacency incoming, `DependencyIndex.incoming` |
| `consumers` | referenced name | consumer path | `DependencyIndex.consumers` |
| `selected` | selected target path | importing path | `DependencyIndex.selected_modules` |
| `cross` | CSS class / element id | rule or element id | `CrossTables` |

Per-file dependency **records** (references, imports, JS surface, flags) move
into the shard. `DependencyIndex` becomes a view over shards and posting tables,
so it is updated rather than rebuilt.

### 3.3 Small catalogs and delta summaries

- **Catalogs** hold what reconciliation needs globally but is small:
  - package manifests and TypeScript configs (today found by decoding every
    source record);
  - the Rust module catalog, crate roots and reachability, recomputed per crate
    only when a `mod` declaration, a Cargo manifest or the set of `.rs` paths
    changes;
  - OKF bundle roots.
- **Summary counts and source coverage** are updated by delta: subtract the
  replaced shards' contributions and add the new ones. The same goes for
  edge-occurrence counts.

### 3.4 Core reconciliation reads keyed lookups

`SymbolTable` becomes a lazy view over snapshot lookups: name → candidates, id →
node, per-file maps loaded from shards on demand. It is no longer built from
`all_nodes()`. Resolution already needs only keyed lookups (every rule in §7.4
asks for a name, a path or a module). `extend_dependents` uses only the
dependency view on the native path. The HTML/CSS "always rebind" rule becomes a
`cross` table lookup.

`GraphSnapshot` gains keyed accessors (`named`, `qualified`, `in_file`,
`incoming`, `consumers_of`, `selectors_of`) with default implementations over the
existing bulk methods, so the in-memory store and conformance tests keep working
unchanged.

## 4. Phases

Each phase is a separate commit that passes the whole suite, keeps sync equal to
a clean rebuild, and is measured with Criterion (see `AGENTS.md`).

0. **Scaling bench.** `sync_scaling` (`cargo bench -p graph-search --bench
   sync`) measures the same one-file body edit in Rust workspaces of 250, 1,000
   and 4,000 modules. Proportional sync means the time stays flat as the
   workspace grows. Format 9 (`7e46a12`, baseline `format9`) is roughly linear:

   | Modules | Criterion estimate | Interval |
   |---|---|---|
   | 250 | 452 ms | 292–709 ms |
   | 1,000 | 1.70 s | 1.29–2.17 s |
   | 4,000 | 5.43 s | 4.52–6.87 s |

   The intervals are wide because unrelated processes were using the CPU during
   the run. Each phase re-measures against this baseline, back to back.
1. **Shards replace Grafeo (format 10). Done.** Nodes, edges (resolved and
   dangling) and occurrences are per-file shard records in 256 KiB packs. The
   `nodes`, `incoming`, `foreign`, `edge_counts` and `package_members` posting
   tables (base plus delta segments, per-block BLAKE3) replace the Grafeo id maps,
   the dangling sidecar, the occurrence sidecar and the edge-count table. Publish
   plans first (validation, surviving identities, untouched files that pointed
   at a removed node), then writes only the replaced shards and source records,
   one delta per table, and updates the summary and the dependency index by
   delta (`DependencyIndex::update`, per-path links). Packs carried forward are
   hard-linked from their header alone; extraction records are verified when
   read. Criterion, back to back against `ec229dd`:

   | Benchmark | Format 9 | Format 10 | Change |
   |---|---|---|---|
   | `sync_scaling` 250 modules | 209 ms | 117 ms | −43.6% |
   | `sync_scaling` 1,000 modules | 757 ms | 220 ms | −71.0% |
   | `sync_scaling` 4,000 modules | 3.68 s | 707 ms | −80.8% |
   | `storage_sync/rust_body_edit` | 203 ms | 134 ms | −34.0% |
   | `storage_sync/json_edit` | 208 ms | 134 ms | −36.0% |
   | `storage_open/open_then_symbol` | 40.7 ms | 31.2 ms | −23.3% |
   | `storage_open/open_then_explore` | 118 ms | 100 ms | −15.1% |
   | `storage_open/open_then_text` | 4.8 ms | 4.9 ms | no change |
   | `storage_publish/reindex_full` | 416 ms | 484 ms | +16.3% (regression) |
   | Store size (storage corpus) | 3,522 KiB | 2,110 KiB | −40% |

   Sync still grows with the workspace (117 ms → 220 ms → 707 ms): core
   reconciliation still reads every node three times and rebuilds the symbol
   table, which phases 3 and 4 remove. A full re-index is slower because every
   shard, table row and dependency record is written through the per-file
   paths.
1b. **Shared objects and keyed reconciliation reads (format 11). Done.** Packs
   and segments live in one store-level `objects/` directory; a publish writes
   new objects and references the rest, instead of hard-linking every pack into
   every generation (a 4,000-module store has 447 shard packs). Each generation
   lists its objects, and a collector removes objects no remaining generation
   lists. Pack indexes record inflated pack sizes, so reuse opens no pack.
   Reconciliation no longer reads the whole graph when a dependency index
   exists, reads single source records through `GraphSnapshot::source_file`,
   takes TypeScript configurations and package manifests from their own posting
   tables, and computes coverage by delta. Criterion against `05b17e5`, each
   revision in its own target directory:

   | Benchmark | Phase 1 | Now | Change |
   |---|---|---|---|
   | `sync_scaling` 250 modules | 117 ms | 68 ms | −40.7% |
   | `sync_scaling` 1,000 modules | 271 ms | 125 ms | −56.4% |
   | `sync_scaling` 4,000 modules | 902 ms | 321 ms | −64.4% |
   | `storage_sync/rust_body_edit` | 136 ms | 59 ms | −58.0% |
   | `storage_sync/json_edit` | 179 ms | 60 ms | −64.3% |
   | `storage_open/open_then_text` | 5.7 ms | 4.9 ms | −11.5% |
   | `storage_publish/reindex_full` | 527 ms | 506 ms | no change |

1c. **Dependency index as keyed lookups (format 12). Done.** Each file's
   `DependencyRecord` (names, references, imports, surface, links) is a record
   in its own pack family; the reverse maps are posting tables (`dep_consumers`,
   `dep_selected`, `dep_candidates`, `dep_incoming`, `dep_flags`, `js_modules`).
   Repair (`dependencies::repair_paths`) reads only keyed lookups through the
   `DependencyLookup` trait. A presence change rechecks only importers with a
   candidate path among the files that appeared or vanished, since a
   selection depends on nothing else. Nothing whole is serialized, loaded or
   validated per sync. Records first lived inside shards; that made every
   bulk shard read (explore's metadata index) decode them, a measured +50%
   on `open_then_explore`, so they moved to their own packs.

   Criterion against `432af1c`, separate target directories. Background load
   was heavy and uneven (the baseline's own `rust_body_edit` read 65, 89 and
   254 ms across three runs), so only the consistent results are listed:

   | Benchmark | Format 11 | Format 12 | Change |
   |---|---|---|---|
   | `sync_scaling` 250 modules | 73 ms | 62 ms | −11.8% |
   | `sync_scaling` 1,000 modules | 138 ms | 100 ms | −28.4% |
   | `sync_scaling` 4,000 modules | 372 ms | 258 ms | −30.6% |
   | `storage_open/open_then_explore` | 108 ms | 105 ms | no change |
   | `storage_publish/reindex_full` | 512 ms | 554 ms | +8.1% (regression) |
   | `storage_open/published_read_only` | 213 µs | 220 µs | +6.7% (regression) |

   `storage_sync/json_edit` read +56% (p < 0.05) in one run and +16%
   (p = 0.24, not significant) in a second; it needs a quiet re-measurement.

1d. **Symbol table as keyed lookups (format 13). Done.** Shards also post
   their symbols by bare name (`names`) and qualified name (`qualified`), each
   row carrying the kind and the `lexical_local` flag, and post whole nodes for
   the structures resolution reads first (`structure`: Rust module
   declarations, markup, package manifests). `SymbolTable` holds the batch's
   symbols and reads stored ones on demand through new `GraphSnapshot`
   accessors (`nodes_in`, `symbol_rows`, `structure`, with defaults over
   `all_nodes`), excluding changed and removed files. Same-file and
   module-member lookups read one shard; a qualified lookup reads its postings
   and their nodes; the unique-name rule decides from postings alone, so a
   common name (`new`, `f0`) never loads its thousands of candidates. The Rust
   path resolver keeps its module structure eager (from `structure`) and
   indexes members one file at a time as a path walks into it. Markup is read
   only when a changed file has elements or CSS rules. A failed store read is
   kept and fails the sync after resolution.

   Criterion against `307b4a5`, separate target directories (a first run was
   void: the baseline half ran beside a VM at full CPU and read 4.7 s for a
   full reindex):

   | Benchmark | Format 12 | Format 13 | Change |
   |---|---|---|---|
   | `sync_scaling` 250 modules | 66 ms | 65 ms | no change |
   | `sync_scaling` 1,000 modules | 95 ms | 74 ms | −23.1% |
   | `sync_scaling` 4,000 modules | 246 ms | 138 ms | −44.7% |
   | `storage_sync/rust_body_edit` | 66 ms | 57 ms | −12.7% |
   | `storage_sync/json_edit` | 67 ms | 62 ms | −10.5% |
   | `storage_sync/noop` | 881 µs | 776 µs | −10.2% |
   | `storage_open/open_then_symbol` | 31.8 ms | 35.0 ms | +7.8% (regression) |
   | `storage_open/open_then_explore` | 101 ms | 104 ms | no change |
   | `storage_publish/reindex_full` | 765 ms (noisy) | 543 ms | no change (p = 0.08) |

   The store grows 1.3% (2,210,190 to 2,238,206 bytes) for the three tables.

2. **Posting tables.** `names`, `incoming`, `consumers`, `selected`, `cross` as
   base and delta segments. Delta summaries and edge counts. Publish writes one
   delta per table.
3. **Core on keyed lookups.** Lazy `SymbolTable`, a dependency view without
   `all_nodes`/`all_edges`, catalogs instead of source-record scans, delta
   coverage, and HTML/CSS through `cross`.
4. **Rust catalog per crate** and **JS surfaces on demand**, removing the last
   whole-workspace passes from resolution.

After phase 3 the `sync_scaling` bench should show a one-file sync nearly flat
from 250 to 4,000 files, apart from the stat walk.

## 5. Risks

- **Compaction must never be on a reader's path.** It runs inside a publish,
  which already holds the writer lock.
- **Delta merges cost query time.** Bounding the number of deltas bounds the
  merge. The evaluation benches (`evaluation`, `search`) guard query latency and
  must not regress.
- **Correctness.** The sync-equals-clean-rebuild tests (`selective_sync`,
  `okf_sync`, `binding_invalidation`, `retrieval_incremental`) are the guard,
  plus the storage atomicity, crash and corruption tests, ported to format 10.
