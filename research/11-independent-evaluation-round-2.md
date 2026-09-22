# Independent evaluation, round 2: `main` @ `0f6ec02`

**Date:** 2026-09-22 · **Evaluator:** the same independent external agent as
[10-independent-evaluation.md](10-independent-evaluation.md) · **Subject:**
`graph-search` release binary at `main` = `0f6ec02` (parser 21), re-run over the
identical probe suite on the `nanus` repository.

Round 1 (doc 10) evaluated `81a5d02` (schema 3, parser 20). This round re-runs it
against the next commit, `0f6ec02` ("Fix seven correctness findings from a
semi-formal engine review"), to separate **what the new commit fixed** from
**what remains**. Method and environment are unchanged: black-box CLI probes, a
fresh store under `/tmp`, no repository edits, release binary
(`shasum f35732843384d5fe…`, `--version` 0.1.0).

---

## 1. What the commit changes

`0f6ec02` touches `langs/js_common.rs`, `langs/rust.rs`, `core/packages.rs`,
`core/typescript_aliases.rs`, `core/lexical_update.rs`, `engine/store.rs`,
`graph-search/index.rs`, and bumps `PARSER_VERSION` 20 → 21 and
`CHUNKER_VERSION` 9 → 10. Its stated fixes: JS/TS `this.method()` resolution,
Rust impl-generic normalization (`Foo<T>` → `Foo`) for `self.method()` across
impls, a pnpm-workspace boundary, a read-only open contract, and two smaller
items. Gate on this revision: **568 tests pass, 0 fail**; `clippy -D warnings`
clean.

## 2. Independently verified fixes

Both headline resolution fixes were reproduced from minimal fixtures:

**Rust — `self.method()` across impls that spell generics differently:**

```rust
pub struct Foo<T> { pub v: T }
impl<T> Foo<T> { pub fn outer(&self) { self.inner(); } }
impl<A> Foo<A> { pub fn inner(&self) {} }
```

`search callers inner` → `[Foo::outer, Foo::inner]`, edge `Foo::outer → Foo::inner` **resolved**.
The symbol is exposed as `Foo::inner` (generics normalized), and the cross-impl
call binds. **Confirmed.**

**TypeScript — `this.method()` to the enclosing class member:**

```ts
class C { a() { this.b(); } b() { return 1; } }
```

`search callers b` → `[C.a, C.b]`, edge `C.a → C.b` **resolved**. **Confirmed.**

Neither fix changes the `nanus` graph materially (its corpus has few such
shapes): node and edge counts are **byte-identical to round 1** — `calls` 18 404,
`type_uses` 2 590, `imports` 1 740. That is expected and is not a weakness of the
fix; it is why an independent regression fixture (above) is the right check.

## 3. Regressions from round 1: unchanged

Neither P0 item from doc 10 moved.

| Metric | Round 1 (`81a5d02`) | Round 2 (`0f6ec02`) |
|---|---|---|
| Full `index` (`nanus`) | 2.01 s | **2.42 s** |
| `status` (one-shot) | 0.99 s | **0.94 s** |
| `symbol` | 0.99 s | 0.95 s |
| `text` | — | 0.95 s |
| `callers` / `impact` / `explore` | ~1.0 s | 0.94–0.96 s |
| No-op `sync` | 1.00 s | 0.96 s |
| Store size | 55 MB | **55 MB** |
| `explore` payload (same query) | 17 538 B | **17 540 B** |
| `text` payload (same query) | 12 351 B | 12 352 B |
| `explore` item fields | `excerpts`, `evidence`, `node`, `snippet`, `impact` | identical |
| Tests / parser | 564 / 20 | 568 / 21 |

The ~1 s per invocation is still **store open**, and the store still loads the
same sidecars on every call:

| File | Size |
|---|---|
| `occurrences.json` | 14.09 MB |
| `dangling.jsonl` | 4.42 MB |
| `graph.grafeo` | 4.14 MB |
| `dependencies.json` | 1.27 MB |
| `manifest.json` | 58 KB |

`explore` remains **larger than the `text` search it replaces** (17.5 KB vs
12.4 KB), so the round-1 P0 recommendation — lazy sidecar loading, and a compact
`explore` default with `excerpts`/`evidence` behind a flag — still stands unchanged.

## 4. Graph gaps: unchanged

Re-run of the doc-10 matrix on `0f6ec02`:

| Probe | Result | Status |
|---|---|---|
| `callers AgentRunner::run_step` (`self.method`) | 1 edge → `run_turn` | works |
| `callers port_failure` | 1 resolved edge | works |
| `refs ToolOutcome::Failure` | 11 resolved edges | works |
| `callers ToolRegistry::execute` (`registry.execute()`) | **0 edges** | **gap** |
| `callers ToolRegistryHandle::borrow` (`handle.borrow()`) | **0 edges** | **gap** |
| `impact` on `ToolRegistry` / `SearchQuery` / `ToolSchema` / `ToolOutcome` | **0** | **gap** (`type_uses` not traversed) |
| `refs SearchQuery` / `ToolDefinition` | **0 edges** | **gap** |
| `refs ToolOutcome` / `AgentConfig` / `ToolSchema` | 2 / 2 / 1 `type_uses` | partial |
| `deps <file>` | 20 `unresolved` import lines | partial |

The honesty channel is intact: `callees AgentRunner::run_tools` returns **31
edges, 3 resolved, 28 unresolved**, with the approximation note attached. Parity
with `nanus` `glob`/`grep` is also intact (`*.rs` anchored top-level; the cap
notice and empty-result strings verbatim).

## 5. New observation: dangling callee names can be whole expressions

The 28 unresolved `callees` edges carry `to_name` values that are sometimes
multi-line source expressions rather than identifiers, for example:

```
'(0..calls.len())\n            .filter'
'batch\n                    .iter()\n                    .map(|index| registry.execute(...))\n                    .collect'
```

These are real unresolved references (method chains the resolver cannot bind),
but emitting the raw multi-line text as the unresolved name bloats the payload
and is unreadable in the text rendering. Truncating/canonicalising dangling names
(the outermost callee segment, single line) would keep the honesty signal without
the noise.

## 6. Verdict and priorities

The correctness fixes are real and independently confirmed; the perf and payload
regressions and the graph gaps are untouched by this commit. Priorities are
unchanged from doc 10:

- **P0** — lazy-load `occurrences`/`dangling`/`dependencies` so metadata queries
  do not pay a ~1 s store open; restore a compact `explore` default.
- **P1** — include `type_uses` in `impact`; scope/receiver-aware binding for
  `x.method()`; de-rank `tests::` symbols.
- **P2** — resolve cross-crate imports; predictable type-`refs` coverage;
  canonicalise dangling edge names.

The correctness prerequisite for the §16.6 measurement remains met; the tool is
**not yet** in a state where the token/latency prediction should be measured
through the one-shot CLI, because ~1 s/call and the tripled `explore` payload
would dominate the result.

Raw numbers: [`results/independent-evaluation-round-2.json`](results/independent-evaluation-round-2.json).

---

VERDICT: CORRECTNESS_FIXES_CONFIRMED_REGRESSIONS_OPEN
CONFIDENCE: medium
SUMMARY: `0f6ec02` fixes and independently verifies the JS/TS `this.method()` and
Rust cross-impl generic `self.method()` resolutions, and the gate is green
(568 tests). The doc-10 P0 regressions are unchanged: ~1 s per one-shot call from
a 55 MB store whose sidecars load on open, and an `explore` payload still larger
than the `text` search it replaces. Receiver-variable method calls, `type_uses`
in `impact`, uneven type refs, and cross-crate imports remain gaps.
