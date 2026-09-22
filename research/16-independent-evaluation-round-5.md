# Independent evaluation, round 5: `main` @ `65a5160` (storage format 9, lazy store)

**Date:** 2026-09-22 · **Evaluator:** the same independent external agent as docs
[10](10-independent-evaluation.md), [11](11-independent-evaluation-round-2.md),
[13](13-independent-evaluation-round-3.md), [14](14-independent-evaluation-round-4.md)
· **Subject:** `graph-search` release binary at `main` = `65a5160` ("Storage format
9: lazily loaded store"), re-run over the identical probe suite on the `nanus`
repository.

Format 9 has no migration; every prior store was deleted and rebuilt fresh before
this run, as the commit states.

---

## 1. P0.1 is resolved

The one finding that gated the §16.6 measurement was that every one-shot
invocation paid a flat ~0.5 s store open whatever it asked. Format 9 opens only
`CURRENT`, `manifest.json` and `summary.json`, and builds the graph and retrieval
indexes on first use. On `nanus` (three runs each, release, one-shot):

| Command | Round 4 (format 8) | Round 5 (format 9) |
|---|---|---|
| `status` | 0.53 s | **0.007 s** |
| `text` | 0.53 s | **0.014 s** |
| No-op `sync` | 0.52 s | **0.007 s** |
| `symbol` | 0.52 s | 0.123–0.133 s |
| `callers` | 0.52 s | 0.170–0.175 s |
| `impact` | 0.52 s | 0.168–0.179 s |
| `explore` | 0.53 s | 0.304–0.318 s |
| Full `index` | 1.74 s | 1.82 s |
| Store | 11 MB | 12 MB |

The flat open penalty is gone. What remains is the **real cost of each query**:
metadata and text are I/O-free (~7–14 ms), metadata symbol lookup ~0.13 s,
one-hop traversal ~0.17 s, and combined `explore` ~0.31 s. The doc-10 target
(`status` under ~0.1 s on a ~200-file repo) is met with an order of magnitude to
spare. **P0.1 closed.**

## 2. Lazy-load contract independently verified

A lazily loaded store must fail loudly, not silently, when a needed artifact is
bad. Two corruption probes confirm it:

| Mutation | `status` | Query needing the artifact |
|---|---|---|
| `graph.grafeo` truncated | exit 0 (still correct) | `symbol` exit 1: `store error: generation checksum mismatch: graph.grafeo` |
| `occurrences.json.zst` deleted | exit 0 | `refs`/`explore` exit 1 at first use |

The cheap path stays up; the query that needs the bad artifact fails at first
use and names the cause. (For the deleted sidecar the message is a bare
`No such file or directory (os error 2)` rather than naming the artifact — a
small readability nit, not a correctness issue.)

## 3. Previously fixed findings — still fixed

| Check | Round 4 | Round 5 |
|---|---|---|
| `impact ToolSchema --depth 2` | (1, 25), (2, 47) | identical |
| `refs SearchQuery` / `refs ToolDefinition` | 5 / 9 | 5 / 9 |
| `callers AgentRunner::run_step` (`self.method`) | 1 resolved | 1 resolved |
| `explore` default payload | 8 053 B | 8 055 B (< `text` 12 351 B) |
| Dangling callee names | 0 multi-line | 0 multi-line |

The graph is unchanged: `calls` 18 328, `type_uses` 4 885, `imports` 1 740,
identical to rounds 3 and 4.

## 4. Still-open accuracy gaps — unchanged

| Finding | Round 5 |
|---|---|
| **P1.2** receiver-variable `x.method()` | `callers ToolRegistry::execute` **0**; `ToolRegistryHandle::borrow` **0** |
| **P1.1** `tests::` noise in `explore` | **2/8** top results |
| **P2.6** cross-crate imports | **20** `unresolved` import lines in `deps` |

Unchanged good behaviour: honesty channel (`callees run_tools` → 30 edges, 3/27,
single-line names); `glob`/`grep` parity exact; the round-1 incremental-reconcile
defect remains fixed (1/1/1).

## 5. Verdict

The lazy store resolves the last P0 item: one-shot latency is now the cost of the
query, not of the store, and the doc-10 target is met. **The §16.6 token/latency
measurement is unblocked** — the CLI arm is now a fair representation of the tool
rather than a measurement of store open. The remaining ceiling is accuracy:
receiver-variable method binding (P1.2) and cross-crate imports (P2.6), with the
`tests::` noise (P1.1) a minor precision issue.

Raw numbers: [`results/independent-evaluation-round-5.json`](results/independent-evaluation-round-5.json).

---

VERDICT: P0_RESOLVED_READY_FOR_SECTION_16_6_MEASUREMENT
CONFIDENCE: high
SUMMARY: Format 9 removes the flat ~0.5s store open; nanus one-shot latency is
now 7ms (status/text/sync) to ~0.31s (explore), with the index at 12MB and an
identical graph (582 tests pass). The lazy-load contract fails loudly on a
corrupt graph or a missing sidecar. All previously fixed findings still hold and
the accuracy gaps (receiver-variable calls, cross-crate imports, test noise) are
unchanged. The CLI path is now fit to measure the SPEC 16.6 prediction.
