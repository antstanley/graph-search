# Independent evaluation, round 3: `main` @ `6d57de6`

**Date:** 2026-09-22 · **Evaluator:** the same independent external agent as docs
[10](10-independent-evaluation.md) and [11](11-independent-evaluation-round-2.md)
· **Subject:** `graph-search` release binary at `main` = `6d57de6` (parser 23),
re-run over the identical probe suite on the `nanus` repository.

This round covers the two commits that respond to docs 10–11:
`74cec3f` ("Fix four independent-evaluation findings; add evaluation benchmarks")
and its follow-up `6d57de6` ("Fix two regressions from the independent-evaluation
response"). Method, environment, and probes are unchanged: black-box CLI calls, a
fresh store under `/tmp`, no repository edits, release binary.

---

## 1. What changed

| Commit | Claim |
|---|---|
| `74cec3f` | `impact` traverses the refs vocabulary (`Calls`, `References`, `TypeUses`); Rust `type_uses` cover parameter/return/local/wrapper types (parser 21→22); `explore` gains `ExploreDetail {Compact, Full}` with a compact CLI default; dangling display names canonicalised to one bounded line; new `cargo bench --bench evaluation`. |
| `6d57de6` | Fixes two regressions in `74cec3f`: generic parameters and `Self` were emitted as `type_uses` (fabricating edges to any same-named type); dangling-name truncation merged distinct long names into one edge identity. Parser 22→23. |

Gate on this revision: **575 tests pass, 0 fail**; strict workspace `clippy`
clean.

## 2. Independently verified fixes

Every claim below was reproduced from a fresh index and the CLI.

### 2.1 `impact` traverses `type_uses` (was P1.3) — **fixed**

| Target | Round 2 | Round 3 |
|---|---|---|
| `impact ToolRegistry` | (1, 0) | **(1, 3), (2, 7)**, top 10 |
| `impact SearchQuery` | (1, 0) | (1, 5), top 5 |
| `impact ToolSchema` | (1, 0) | **(1, 25), (2, 47)**, top 50 |
| `impact ToolOutcome` | (1, 0) | (1, 8), (2, 10), top 18 |

"What breaks if I change this struct/trait" now answers.

### 2.2 Type `refs` coverage (was P2.7) — **fixed**

`type_uses` edges rose 2 590 → **4 885**; all resolve:

| Target | Round 2 | Round 3 |
|---|---|---|
| `refs SearchQuery` | 0 | **5** (resolved) |
| `refs ToolDefinition` | 0 | **9** (resolved) |
| `refs ToolOutcome` | 2 | 8 (resolved) |
| `refs AgentConfig` | 2 | 9 (resolved) |

### 2.3 Compact `explore` (was P0.2) — **fixed**

Same query (`tool call dispatch approval`), same repo:

| Call | Round 2 | Round 3 |
|---|---|---|
| `explore` (default) | 17 540 B | **8 053 B** |
| `explore --detail full` | 17 540 B | 17 555 B |
| `text "ToolRegistry" --include '*.rs'` | 12 352 B | 12 352 B |

The default is now **smaller than the `text` search it replaces** — the context
thesis holds again for the CLI path. The full shape remains available behind
`--detail full`.

### 2.4 Dangling names canonicalised (was P2.8) — **fixed**

`callees AgentRunner::run_tools` unresolved names are now single-line and
bounded (`'(0..calls.len()).filter'`); multi-line names fell from the thousands
the response reports to **0** in this probe.

### 2.5 Regression fix verified (`6d57de6`) — **confirmed**

```rust
pub struct T;
pub struct Real;
pub fn generic<T>(arg: T) -> T { arg }
pub fn concrete(arg: Real) -> Real { arg }
```

`refs T` → **0 edges** (the generic parameter no longer fabricates a reference to
the struct `T`); `refs Real` → **1 resolved `type_uses`**. Independent
confirmation of the fix. Consistent with it, `calls` edges on `nanus` fell
18 404 → 18 328 (fabricated generic/`Self` edges removed).

## 3. Still open

| Finding | Round 3 result | Status |
|---|---|---|
| **P0.1** store open ≈ 1 s/call | `status`/`symbol`/`impact`/`explore`/`sync` **0.95 s**; store **57 MB** (`occurrences.json` ~14–15 MB) | open — confirmed and attributed by the response (doc 12 §3), staged lazy-load plan §5.1 |
| **P1.2** receiver-variable `x.method()` | `callers ToolRegistry::execute` **0**; `ToolRegistryHandle::borrow` **0** | open (doc 12 §5.2) |
| **P1.1** de-rank `tests::` in `explore` | **2/8** top results are `tests::…` on the NL query | open (doc 12 §5.3) |
| **P2.6** cross-crate imports | `deps` shows **20** `unresolved` import lines (`nanus_domain::…`) | open (doc 12 §5.4) |

Unchanged good behaviour: the honesty channel reports resolved/unresolved
(`callees run_tools` → 30 edges, 3 resolved / 27 unresolved); `glob`/`grep`
parity is exact (`*.rs` anchored top-level; cap notice and empty strings
verbatim); the round-1 incremental-reconcile defect remains fixed (1/1/1).

## 4. Round-over-round summary

| Metric | Round 2 (`0f6ec02`) | Round 3 (`6d57de6`) |
|---|---|---|
| Tests / parser | 568 / 21 | 575 / 23 |
| `type_uses` edges | 2 590 | **4 885** |
| `calls` edges | 18 404 | 18 328 |
| `impact` on a struct | 0 | **non-zero** |
| `refs SearchQuery` | 0 | **5** |
| `explore` default payload | 17 540 B | **8 053 B** |
| Per-call latency | 0.94–0.96 s | 0.95 s |
| Store | 55 MB | 57 MB |
| Receiver-call `callers` | 0 | 0 |
| Test noise in `explore` | 2/8 | 2/8 |

## 5. Verdict

Four of the seven findings from docs 10–11 are **fixed and independently
confirmed** (impact, type refs, compact payload, dangling names), plus the
response's own regression is confirmed fixed. The graph is materially more
useful: type-level "what breaks" and type-usage queries now work, and `explore`
is again cheaper than `text`.

The single **P0** blocker is unchanged: **~1 s per one-shot call, from a 57 MB
store opened on every invocation.** That remains the reason the §16.6 token and
latency prediction should not be measured through the CLI; the response's staged
lazy-load plan is the right fix and is the gating item before the measurement.
The remaining accuracy ceiling is receiver-variable binding (P1.2) and
cross-crate imports (P2.6).

Raw numbers: [`results/independent-evaluation-round-3.json`](results/independent-evaluation-round-3.json).

---

VERDICT: ACCURACY_FINDINGS_RESOLVED_P0_STORE_OPEN_OPEN
CONFIDENCE: medium
SUMMARY: `74cec3f`/`6d57de6` fix and independently verify impact-via-type_uses,
type-ref coverage, compact explore payload, dangling-name canonicalisation, and
their own generic-type regression; 575 tests pass. The ~1s per-call store open
and 57MB store are unchanged and remain the gating item, with receiver-variable
method binding and cross-crate imports the remaining accuracy gaps.
