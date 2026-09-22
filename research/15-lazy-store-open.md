# P0.1 resolved: a lazily loaded store

**Date:** 2026-09-22 · **Subject:** branch `perf/lazy-store-open` off `main`
`49144a7` · **Finding:** P0.1 of docs [10](10-independent-evaluation.md),
[12](12-independent-evaluation-response.md) §5.1 and
[14](14-independent-evaluation-round-4.md) §2: every one-shot invocation paid a
flat store open (~0.52 s on `nanus`), whatever it asked.

Storage format 9. Existing stores must be rebuilt (`graph-search index`); there is
no migration.

## 1. Where the open went (format 8, `nanus`, warm cache)

| Phase | ms | Needed by `status`? |
|---|---|---|
| select: hash every artifact, parse dependencies, verify extraction packs | ~31 | header only |
| Grafeo `open_read_only` (snapshot rebuilt into the LPG, per property) | ~64 | no |
| id maps | 17 | no |
| dangling sidecar | 15 | no |
| source records | 54 | coverage only |
| occurrence sidecar | 32 | no |
| `validate_fact_owners` | 72 | no |
| indexes: nodes 28, edges 26, metadata 53, body 66, occurrences 20, adjacency 6 | 199 | counts only |
| **the status query itself** | **3** | |

Status needed about 1% of what open did.

## 2. What changed

1. **Open reads the header only.** Selection validates the descriptor, rejects
   uncommitted artifacts, takes the reader lease, and verifies and parses
   `manifest.json` and `summary.json`. Every other artifact is verified against
   its CURRENT fingerprint by its first reader (`generation::Committed::read`).
2. **Facts and indexes load on first use.** The graph, dangling references,
   source and occurrence facts, extraction and dependency indexes, and the
   node list, metadata, body, occurrence and adjacency indexes each live in a
   `OnceLock`. Facts are validated against the graph when they load, as the
   eager open did. A failed load returns the error and is not cached. Snapshot
   accessors that may load (`source_files`, `occurrence_files`, `occurrences`,
   `body`, `metadata`) now return `Result`.
3. **`summary.json` (format 9)**: exact counts and source coverage, written by
   the publisher. `status` and every query's result context use it, so they
   never touch the graph or its facts.
4. **`edge-occurrences.bin` (format 9)**: a sorted table of BLAKE3(edge id) and
   occurrence count. Relationship queries fill `occurrence_count` by binary
   search instead of decoding, validating and indexing every occurrence fact.
   `GraphSnapshot::occurrence_count` defaults to the occurrence index, so the
   in-memory store is unchanged.
5. **Ranked-search postings are built on demand.** `MetadataIndex` builds its
   two BM25 lexical indexes only for ranked search. Name, prefix and path
   lookups never need them.
6. **Filters need the metadata index only for a language filter.**
7. **The CLI skips teardown after a read.** Dropping a loaded index cost ~28 ms
   at exit. The OS reclaims it for free, and the reader lease is a file lock.

The integrity contract is unchanged. No byte is used before it is verified
against CURRENT, and no fact is exposed before it is validated. What changed is
*when* this happens: at the first read, as doc 12 §5.1 stage 1 specified. The
store-corruption tests now assert that open succeeds and the first reader fails
loudly. `tests/lazy_open.rs` corrupts every non-header artifact. It asserts that
status and counts still answer and that each fact family fails with a checksum
mismatch, and that a failure is not cached.

## 3. Results (Criterion)

`cargo bench -p graph-search --bench evaluation`. Baseline: a clean `main`
worktree with the same bench file. The `repo` group is opt-in via
`GRAPH_SEARCH_BENCH_REPO=../nanus`. Each iteration opens the index, answers one
query and drops the index. That is the one-shot library shape. The CLI also
skips the final drop, so it is somewhat faster than these figures.

| Benchmark | `main` | branch | |
|---|---|---|---|
| repo/status (`nanus`) | 495.3 ms | **2.41 ms** | 206× |
| repo/text | 499.5 ms | 9.47 ms | 53× |
| repo/sync (no-op) | 494.5 ms | 2.52 ms | 196× |
| repo/symbol | 494.2 ms | 121.3 ms | 4.1× |
| repo/callers | 491.6 ms | 170.4 ms | 2.9× |
| repo/impact | 493.5 ms | 171.2 ms | 2.9× |
| repo/explore | 499.0 ms | 404.5 ms | 1.2× |
| open/published_store (300 files) | 61.0 ms | 0.30 ms | |
| open/status | 62.9 ms | 1.33 ms | 47× |
| open/refs | 62.5 ms | 38.0 ms | 1.6× |
| open/explore | 64.4 ms | 48.6 ms | 1.3× |

Doc 12 §5.1 acceptance:

- `status` under 100 ms on the Criterion corpus: **1.3 ms**.
- `status` under 150 ms on `nanus`: **2.4 ms**.
- `open/published_store` is now 0.3 ms.
- Test gate: 582 pass, 0 fail. Strict clippy is clean.

Per-invocation cost is no longer flat. It now scales with what the query reads.

## 4. What remains (not P0.1)

Graph queries still load the whole graph through Grafeo. That costs about 55 ms
to rebuild the snapshot plus about 50 ms to convert it into id maps, nodes and
edges, which puts `symbol` at ~120 ms and traversals at ~170 ms. Removing it
needs a read path that doesn't go through Grafeo (or a Grafeo with a cheaper
load), which is an architecture decision (SPEC §4.3). `explore` also decodes and
validates source facts and builds body postings (~100 ms), and it drops them at
the end. Per-file selective loading of source and occurrence facts is the
natural next step there.

Raw numbers: [`results/lazy-store-open.json`](results/lazy-store-open.json).
