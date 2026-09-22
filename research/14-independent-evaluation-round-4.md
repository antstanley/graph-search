# Independent evaluation, round 4: `main` @ `10d07e3` (storage format 8)

**Date:** 2026-09-22 · **Evaluator:** the same independent external agent as docs
[10](10-independent-evaluation.md), [11](11-independent-evaluation-round-2.md),
[13](13-independent-evaluation-round-3.md) · **Subject:** `graph-search` release
binary at `main` = `10d07e3` ("Storage format 8: compact source records,
compressed packs, BLAKE3"), re-run over the identical probe suite on the `nanus`
repository.

This revision changes the on-disk format and **has no migration path** — older
indexes must be deleted and rebuilt, as the commit states. All stores from prior
rounds were removed before this run; every measurement is on a fresh format-8
index.

---

## 1. What changed

`10d07e3` replaces the JSON source-fact storage with a GSR1 record codec
(front-coded dictionaries, delta/varint postings), compresses the occurrence,
dangling and dependency sidecars as zstd frames (`*.zst`), moves `content_hash`
to BLAKE3, and stops cloning term maps when building the `BodyIndex`. On the
authors' own repository they report 316 MiB → 24.7 MiB and ~7.0 s → ~2.1 s
one-shot queries. The graph is unchanged: `nanus` node and edge counts are
**identical to round 3** (`calls` 18 328, `type_uses` 4 885, `imports` 1 740),
which is good evidence the format change preserves the projection.

Gate on this revision: **579 tests pass, 0 fail**; strict clippy clean.

## 2. P0.1 (store open ≈ 1 s) — substantially improved, still open

| Metric | Round 3 (`6d57de6`) | Round 4 (`10d07e3`) |
|---|---|---|
| Full `index` (`nanus`) | 2.45 s | **1.74 s** |
| `status` (one-shot) | 0.95 s | **0.53 s** |
| `symbol` | 0.95 s | 0.52 s |
| `text` | 0.95 s | 0.53 s |
| `impact` / `explore` | 0.95 s | 0.52–0.53 s |
| No-op `sync` | 0.95 s | 0.52 s |
| Store on disk | 57 MB | **11 MB** |
| `occurrences.json` sidecar | 14.1 MB (JSON) | **2.33 MB** (`.zst`) |
| `dangling.jsonl` sidecar | 4.42 MB | **289 KB** (`.zst`) |
| `dependencies.json` sidecar | 1.27 MB | **196 KB** (`.zst`) |
| `graph.grafeo` | 4.14 MB | 4.29 MB |

This is a real improvement — roughly **1.8× faster per call** and **5× smaller
on disk** on this repository. But the per-invocation cost is still a flat
**~0.52 s** across `status`, `symbol`, `impact`, `explore` and `sync`, i.e. it is
still **store open**, not the query. The doc-10 target (`status` under ~0.1 s on
a ~200-file repo) is not yet met, so **P0.1 remains the gating item** for
measuring the §16.6 token/latency prediction through the CLI. The authors'
staged lazy-load plan (doc 12 §5.1) is the remaining work, not the format.

## 3. Previously fixed findings — still fixed

| Check | Round 3 | Round 4 |
|---|---|---|
| `impact ToolSchema --depth 2` | (1, 25), (2, 47) | **identical** |
| `refs SearchQuery` | 5 | 5 |
| `refs ToolDefinition` | 9 | 9 |
| `callers AgentRunner::run_step` (`self.method`) | 1 resolved | 1 resolved |
| `explore` default payload | 8 053 B | **8 053 B** (still < `text` 12 351 B) |
| Dangling callee names | 0 multi-line | **0 multi-line** |

## 4. Still-open gaps — unchanged

| Finding | Round 4 |
|---|---|
| **P1.2** receiver-variable `x.method()` | `callers ToolRegistry::execute` **0**; `ToolRegistryHandle::borrow` **0** |
| **P1.1** `tests::` noise in `explore` | **2/8** top results |
| **P2.6** cross-crate imports | **20** `unresolved` import lines in `deps` |

Unchanged good behaviour: honesty channel reports resolved/unresolved
(`callees run_tools` → 30 edges, 3/27, single-line names); `glob`/`grep` parity
exact; the round-1 incremental-reconcile defect remains fixed (1/1/1).

## 5. Verdict

The storage-format change delivers what it claims on this repository — the
index is ~5× smaller and one-shot queries ~1.8× faster — and does so without
altering the graph. Per-invocation latency nevertheless remains ~0.52 s, so the
**P0.1 store-open item stays open** and still gates the CLI-path measurement.
The accuracy picture is unchanged: impact and type queries work, `explore` is
compact, and the remaining ceiling is receiver-variable method binding (P1.2)
and cross-crate imports (P2.6).

Raw numbers: [`results/independent-evaluation-round-4.json`](results/independent-evaluation-round-4.json).

---

VERDICT: STORAGE_GAINS_CONFIRMED_P0_STORE_OPEN_STILL_OPEN
CONFIDENCE: medium
SUMMARY: Format 8 cuts the nanus index 57MB -> 11MB and per-call latency
0.95s -> 0.52s with an identical graph, and all previously fixed findings still
hold (579 tests pass). The per-invocation cost is still a flat ~0.52s store
open, above the ~0.1s target, so it remains the gating item; receiver-variable
binding, test noise in explore, and cross-crate imports are unchanged.
