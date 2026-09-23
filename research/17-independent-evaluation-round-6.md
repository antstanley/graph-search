# Independent evaluation, round 6: `main` @ `1b44235` (accuracy gaps closed, Python added)

**Date:** 2026-09-23 · **Evaluator:** the same independent external agent as docs
[10](10-independent-evaluation.md), [11](11-independent-evaluation-round-2.md),
[13](13-independent-evaluation-round-3.md), [14](14-independent-evaluation-round-4.md),
[16](16-independent-evaluation-round-5.md) · **Subject:** `graph-search` release
binary at `main` = `1b44235` (merge of PR #5 `feature/accuracy-gaps`, plus PR #4
Python support), re-run over the identical probe suite on the `nanus` repository.

Schema is now 4 and parser 26, so the index was replaced and rebuilt fresh before
this run.

---

## 1. What changed

| Commit | Finding it targets |
|---|---|
| `0188f0e` | **P2.6** — resolve workspace crate paths and associated items (`use nanus_domain::…`) |
| `b76547a` | **P1.2** — bind Rust method calls through the receiver's stated type |
| `9604189` | **P1.1** — rank test-owned code behind other `explore` hits |
| PR #4 (`8714e6f`…) | Python extractor (`class`, `function`, `method`, `field`, module variable, type alias; import/call/inheritance/annotation edges) |

Gate on this revision: **607 tests pass, 0 fail**; strict clippy clean.

## 2. P1.2 — receiver-variable method calls: **fixed**

The gap that most limited `callers` on Rust is closed. Both probes that returned
**0 edges** in rounds 1–5 now resolve:

| Probe | Round 5 | Round 6 |
|---|---|---|
| `callers ToolRegistry::execute` (`registry.execute()`) | 0 | **5 edges**, incl. `AgentRunner::run_tools` — the real call site |
| `callers ToolRegistryHandle::borrow` (`handle.borrow()`) | 0 | **6 edges**, incl. `AgentRunner::fmt`, `build_request`, `run_tools`, `gate`, `Pending::fmt` |

The `AgentRunner::run_tools` result is exactly the `registry.execute(calls[*index].clone())`
call at `agent_loop.rs:761` that round 1 flagged as unresolvable.

## 3. P2.6 — cross-crate imports: **fixed**

`deps` unresolved import lines on `crates/nanus-bundle/src/tools/grep.rs` fell from
**20 → 5**, and the 5 that remain are all *external or non-symbol* targets that
correctly cannot resolve inside the workspace:

```
core::fmt::Write        serde_json::json
std::path::PathBuf      super::*          (a glob)
```

Workspace crate paths (`nanus_domain::…`) now link to their definitions.

## 4. P1.1 — test-owned code in `explore`: **fixed**

Top-eight test-owned symbol counts, three queries:

| Query | Round 5 | Round 6 |
|---|---|---|
| `how does a tool call get dispatched and approved` | 2/8 | **0/8** |
| `search query literal` | 2/8 | **0/8** |
| `port failure` | 2/8 | **0/8** |

## 5. Previously fixed findings still hold, and the graph grew

| Check | Round 5 | Round 6 |
|---|---|---|
| `impact ToolSchema --depth 2` | (1, 25), (2, 47) | **(1, 30), (2, 81)** |
| `refs SearchQuery` / `ToolDefinition` / `ToolOutcome` | 5 / 9 / 8 | **6 / 12 / 12** |
| `callers AgentRunner::run_step` (`self.method`) | 1 | 1 |
| `explore` default payload | 8 055 B | 7 745 B (< `text` 12 352 B) |
| Dangling callee names multi-line | 0 | 0 |
| `callees AgentRunner::run_tools` resolved/unresolved | 3 / 27 | **8 / 22** |

Extraction grew accordingly: `type_uses` 4 885 → **6 348**, `contains` 6 147 →
6 414. `glob`/`grep` parity is exact; the round-1 incremental-reconcile defect
remains fixed (1/1/1).

## 6. One-shot latency (format-9 lazy store still holds)

`status` 11–14 ms, `text` 27–32 ms, `symbol` 172–188 ms, `impact` 213 ms,
`explore` 332–335 ms, `callers` 477–519 ms cold / **197 ms warm**. Full `index`
2.94 s (up from 1.82 s — more extraction work), store 13 MB. The lazy open from
round 5 is intact; the higher `callers` figure is query work, not store open.

## 7. New: Python extractor

A two-file fixture (`pkg/mod.py`, `pkg/other.py`) indexes correctly — `python` 2
files, `class Widget`, methods, functions all found; `symbol Widget` resolves;
`deps mod.py` shows 3 import edges.

**New gap:** Python method calls through a receiver do not bind.
`build()` contains `w.render()` where `w: Widget`, but
`callers render` → `Widget.render` with **0 edges**, and `callees build` returns
`['w.render' (unresolved), 'Widget' (resolved)]`. This is exactly the Rust P1.2
gap — now fixed for Rust, still open for Python (`Widget("x")` constructor use
does resolve). Python is new, so this is a next-step item rather than a
regression.

## 8. Verdict

All three accuracy gaps reported in docs 10–16 for Rust are **fixed and
independently verified** (receiver-variable binding, workspace crate imports,
test-ranking), with the graph larger and the payload still compact. Combined
with round 5's lazy store, the tool now meets every finding raised in this
series except Python receiver-call binding. **It is fit to run the §16.6
token/latency measurement**, and the natural next accuracy item is Python
`x.method()` binding, mirroring the Rust `receiver` resolution class.

Raw numbers: [`results/independent-evaluation-round-6.json`](results/independent-evaluation-round-6.json).

---

VERDICT: ALL_RUST_FINDINGS_RESOLVED_PYTHON_RECEIVER_BINDING_NEW_GAP
CONFIDENCE: high
SUMMARY: P1.2 (receiver-variable Rust method calls), P2.6 (workspace crate
imports) and P1.1 (test-owned ranking in explore) are fixed and independently
verified; previously fixed findings still hold and the graph grew (type_uses
4885 -> 6348). 607 tests pass. Python extraction works but Python receiver
method calls do not bind yet, mirroring the Rust gap just closed.
