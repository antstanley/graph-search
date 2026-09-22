# Independent evaluation: black-box trial of the built binary on `nanus`

**Date:** 2026-09-22 · **Evaluator:** an independent external agent (a `nanus`
session), not the authors of docs 01–09 · **Verdict scope:** the shipped CLI and
library, exercised end-to-end on the `nanus` repository.

This document is an *independent, black-box* evaluation. It changes no production
code, adds no dependency, and does not run the author-written research harness.
It indexes a repository with the shipped binary and probes the public command
surface, recording what works, what does not, and what changed between revisions.
Where it overlaps the static review in [09-native-search-review.md](09-native-search-review.md),
it is as *empirical confirmation or counter-evidence*, and each such overlap is
called out in §10.

---

## 1. Scope and method

**What it is:**

- A trial of the release binary on a real, unmodified repository (`nanus`,
  ~188 walked files of Rust plus docs and lockfiles).
- A before/after comparison across three revisions of the branch, using the same
  probes each time.
- Evidence for the §16.6 prediction gate in [`SPEC.md`](../SPEC.md): is the tool
  accurate enough, fast enough, and compact enough to measure?

**What it is not:**

- Not an LLM-agent task trial. No model was driven against the tool; every
  measurement is a direct CLI/library call. The author's task suite
  ([08-task-evaluation.md](08-task-evaluation.md)) is the correct home for
  agent-level measurement.
- Not an attribution study. Timings are single-machine and include process start.
- Not exhaustive. It is one repository, three revisions, and a bounded probe set.

**Reproduction:** every command is listed in [Appendix A](#appendix-a--reproduction).
Stores were written to `/tmp`; **neither the `graph-search` checkout nor the
`nanus` checkout was modified** during measurement.

**Environment:** macOS (Darwin), `rustc 1.98.1`, release build
(`--release`, `lto = "thin"`); `nanus` at its working-tree state of 2026-09-22;
host load average 3–5 during the run (timings are indicative, not controlled).

## 2. Revisions evaluated

| Rev | Commit | Schema / parser | Tests | `nanus` index (walked) | Store |
|---|---|---|---|---|---|
| **A** | `4dd6af6` | schema 1 / parser 1 | 65 | 185 files, 2 625 ms (debug) | 18 MB |
| **B** | `47808b1` | schema 2 / parser 2 | 95 | ~185 files | 18 MB |
| **C** | `81a5d02` | schema 3 / parser 20 | 564 | 188 files, 2 011 ms (release) | 55 MB |

Extraction grew between A and C (all `nanus`):

| Metric | A (`4dd6af6`) | C (`81a5d02`) |
|---|---|---|
| `calls` edges | 12 656 | **18 404** |
| `type_uses` edges | 2 343 | 2 590 |
| `imports` edges | 1 669 | 1 740 |
| `contains` edges | 5 684 | 6 147 |
| methods | 1 272 | 1 395 |
| functions | 2 132 | 2 287 |

The public surface also grew: a `--verify-content` flag, human-readable
`coverage`/approximation lines in text output, and new `excerpts`/`evidence`
fields in `explore` results.

## 3. Correctness

### 3.1 Defect found in revision A: incremental reconcile dropped incoming edges

**Impact: critical.** Editing a file that is the *target* of another file's edge
silently removed that edge, and the index still reported `stale: false`.

**Reproduction (three files):**

```
a.rs: pub fn alpha() {}
b.rs: pub fn beta() { alpha(); }
index            → callers alpha = 1 edge    ✓
edit a.rs        → callers alpha = 0 edges   ✗  (stale: false)
index again      → callers alpha = 1 edge    ✓
```

`callees beta` was also empty and `refs alpha` was empty after the edit. Editing
the *caller* (`b.rs`) was fine; adding a new file was fine. Only editing a
**target** file failed.

**Root cause (revision A, `crates/engine/src/store.rs::apply`):** the changed
file's subtree was deleted with `delete_node`, and Grafeo drops a node's incident
edges with it. Insertions re-emitted edges only for files in `batch.upserts` —
the *changed* files — so an unchanged file's edge into the changed file was never
restored. The manifest was then committed, so the store claimed to be current.
This is the confident false negative the honesty contract (§7.4, §13) exists to
prevent, and it broke `callers`/`callees`/`refs`/`impact` on a moving tree.

**Verified fixed in B and C.** The same probe now holds the edge across a callee
edit, a second callee edit, a `search`, and an explicit `sync` (1 edge
throughout). Revision C also passes an independent re-run of the full sequence.
The fix is documented in [07](07-lexical-and-incremental.md) (incoming-edge
closure; replaced subtrees removed before insertion; conformance fixture applied
twice). **Independently confirmed.**

> Any evaluation of this tool on a workspace that is being edited is invalid
> before this fix. It is the single most important defect found here.

### 3.2 Other correctness observations

- **Ambiguity is explicit.** Revision A silently chose among duplicates; C
  prints `graph-search: ambiguous target: <target>` and tells the caller to use
  the symbol mode to choose an exact id. Good failure behaviour; it does mean a
  bare name that maps to two symbols is now an error for `refs`/`impact`.
- **`path` became honest, not broken.** `path AgentRunner::run_tools → ToolRegistry::execute`
  returned a path in A and **Nothing found** in C. The A
  result came through the arbitrary-suffix-resolution behaviour later removed as
  a false-positive source (finding F04 in [02](02-findings.md)). The C answer is
  the correct one for a receiver-typed call the resolver cannot bind.
- **Coverage is reported.** C's text output includes
  `coverage: 0 oversized, 0 binary, 0 invalid UTF-8, 0 source read failures, 1 parser quarantines`,
  making extractor loss visible rather than silent.

## 4. Retrieval accuracy (`explore`)

### 4.1 Natural-language ranking (A → B/C)

| Query | A (`4dd6af6`) | B/C |
|---|---|---|
| `how does a tool call get dispatched and approved` | `apps/website/…GET`, `SecretBackend::get` (stopword "get" dominated) | `Held::is_approved`, `ToolRegistry::execute`, `BundleError::Tool` |
| `ToolRegistry execute approval` | `AgentRunner::approval`, unrelated `tests::*::execute`; `ToolRegistry` absent | relevant symbols present |

### 4.2 Split-name retrieval (C)

Independent spot-checks on `nanus`, first three results:

| Query | Top results |
|---|---|
| `tool registry handle` | `ToolRegistryProvider::registry`, `ToolRegistryHandle`, `AgentRunner::tools` |
| `search query literal` | `SearchQuery`, `impl SearchQuery`, `InputBuffer::search_query` |
| `port failure` | `port_failure`, `ToolOutcome::Failure`, … |
| `where is the context budget enforced` | `AgentConfig::with_context_budget`, `default_context_budget`, `NanusConfig::context_budget` (0/8 test noise) |

These are consistent with the author-reported gains (32/90 → 89/90 split-name
targets, [07](07-lexical-and-incremental.md)). The identifier-aware BM25 +
tokenization change is visible and effective.

### 4.3 Residual retrieval noise

- **Test functions still rank.** Across probes, **0–2 of the top 8** were
  `tests::…` symbols (e.g. `tests::registry_with_echo`). Improved but present; a
  `#[cfg(test)]`/`tests::` de-rank would tighten precision.
- **Out-of-domain queries behave as expected.** `explore "reconcile manifest content hash"`
  and `"lexical bm25 ranking"` return weak results *because
  `nanus` has no reconcile/BM25 concepts*; this is not a defect, and is recorded
  only to keep the evidence honest.

## 5. Graph accuracy (C, fresh index on `nanus`)

The graph modes are where the value proposition lives, so they were probed
directly. Results are the raw CLI outputs.

| Probe | Result | Assessment |
|---|---|---|
| `callers AgentRunner::run_step` (`self.run_step`) | 1 edge → `AgentRunner::run_turn` | **fixed** (F05) |
| `impact AgentRunner::run_tools --depth 2` | d1 `run_step`, d2 `run_turn` | **works** |
| `callers port_failure` (free fn) | 2 resolved edges | works |
| `refs ToolOutcome` / `AgentConfig` / `ToolSchema` | 2 / 2 / 1 `type_uses` edges | partial |
| `refs SearchQuery` / `ToolDefinition` | **0 edges** | **gap** |
| `refs ToolCall` | ambiguity error (use `symbol`) | expected |
| `callers ToolRegistry::execute` (`registry.execute()`) | **0 edges** | **gap** |
| `callers ToolRegistryHandle::borrow` (`handle.borrow()`) | **0 edges** | **gap** |
| `impact` on any struct/trait (`ToolRegistry`, `SearchQuery`, `ToolSchema`, `ToolOutcome`) | **0** | **gap** |
| `deps <file>` | 22 import edges, but cross-crate targets `(unresolved)` | partial |

**Gap 1 — receiver-variable method calls do not bind.** `x.method()` where `x`
is a local/parameter (`registry.execute()`, `handle.borrow()`) produces no
callers. Only `self.member` with a known lexical owner is resolved. This is the
largest remaining recall gap for Rust `callers`, because most method calls are
receiver-based. Confirmed as a known limit (F05 scope; recommendation 20 in
[09](09-native-search-review.md), "scope-aware binding").

**Gap 2 — `impact` ignores `type_uses`.** `QueryEngine::impact` traverses
`Calls` + `References` only, so "what breaks if I change this struct/trait"
returns nothing even though 2 590 `type_uses` edges exist and some resolve.
Including `TypeUses` in the impact cone would answer a top-three agent question.

**Gap 3 — type `refs` is uneven.** `ToolOutcome`/`AgentConfig`/`ToolSchema`
resolve; `SearchQuery` and `ToolDefinition` do not, despite heavy use. Type-usage
coverage is partial and not predictable from the outside.

**Gap 4 — cross-crate imports dangle.** `deps` on a file lists
`nanus_domain::ContentBlock` and similar as `(unresolved)`, so cross-crate
import targets are not linked to their definitions.

## 6. Output contract and context cost

- **Contract version advanced twice:** `schema_version` 1 → 2 (B) → 3 (C). Any
  JSON consumer must accept version 3; the envelope keys otherwise remain stable
  (`schema_version`, `command`, `root`, `query`, `stale`, `results`, `edges`,
  `truncations`, `approximation`, `stats`), with `context`/`evidence` additions in
  `explore` items.
- **`impact` returns an object** (`{by_depth, top}`), not an array, so its
  `results` shape still differs from the §9.1 envelope used by other modes.
- **`explore` payload roughly tripled.** Same query
  (`tool call dispatch approval`), same repo:

  | Query | A | C |
  |---|---|---|
  | `explore "tool call dispatch approval"` | **5 667 bytes** | **17 538 bytes** (~3.1×) |
  | `text "ToolRegistry" --include '*.rs'` | 12 508 bytes | 12 351 bytes (unchanged) |

  The new `excerpts` (~557 B/item) and `evidence` (~485 B/item) fields, plus the
  existing `snippet` (~382 B/item), make `explore` **larger than the `text`
  search it is meant to replace**. Per call that is fine; across several calls it
  erodes the §16.6 token prediction. A compact default (node + snippet + impact)
  with `excerpts`/`evidence` behind a flag would restore the intended shape.

## 7. Performance

All release-mode, one-shot CLI (the shape `nanus` drives through `bash`):

| Operation | A/B (18 MB store, schema 1–2) | C (55 MB store, schema 3) |
|---|---|---|
| Full `index` (`nanus`) | ~1.7 s | **2.0 s** |
| `status` | ~0.19 s | **0.99 s** |
| `symbol` / `explore` / `impact` | 0.10–0.19 s | **~1.0 s** |
| No-op `sync` | ~0.10 s | **1.0 s** |
| Store on disk | 18 MB | **55 MB** |

Binary startup is **5 ms** (`--help`), and a one-file store answers `status` in
**5 ms** — so the ~1 s is **store open, scaling with store size**, not process
start or the walk. `--no-reconcile` does not avoid it.

Revision C's single generation holds large sidecars that dominate:

| File | Size |
|---|---|
| `occurrences.json` | **14.0 MB** |
| `graph.grafeo` | 4.1 MB |
| `dangling.jsonl` | **4.4 MB** |
| `dependencies.json` | 1.3 MB |
| `manifest.json` | 58 KB |

Earlier (B) the same repository measured a ~9.7 MB manifest for a 158-file copy
and ~0.1 s no-op sync. The regression is real and, per the author's own account
([09](09-native-search-review.md) recommendation 23), expected from keeping raw
facts in the hot path. Two consequences for planning:

- **"Sync after every prompt" costs ~1 s per invocation on a mid-size repo**, and
  grows with corpus size. The in-process shape 3 pays it once, not per call.
- **The one-shot CLI arm under-sells the tool**; Treatment B (in-process) is the
  representative measurement.

## 8. Quality gate

| | A | B | C |
|---|---|---|---|
| `cargo test --workspace` | 65 passed | 95 passed | **564 passed, 0 failed** |
| `cargo clippy --workspace --all-targets -- -D warnings` | — | clean | **clean** |

The test count grew ~8.7× across the three revisions; the incremental-sync and
retrieval regressions added in B/C are the right shape for the defects found
here (though none specifically asserts the A-level edge-loss repro until fixed).

## 9. Relation to the §16.6 prediction

`SPEC.md` §16.6 predicted: −30% to −55% tokens on a mixed suite, ~0% on trivial
tasks; −25% to −55% wall time on discovery tasks; residual context lower than
baseline; success gated on accuracy not regressing.

What this trial says about those:

- **Accuracy prerequisite: met.** The reconcile defect that produced confident
  wrong answers is fixed; retrieval ranking is materially better; the graph is
  larger and more honest. The tool is fit to run the M5 measurement.
- **Token prediction: at risk from payload growth.** `explore` now costs ~3×
  more per call and exceeds `text`. Unless the default is slimmed, the
  "fewer, denser results" mechanism is offset for `explore`-heavy tasks.
- **Latency prediction: directionally fine, mechanism changed.** Per-call ~1 s is
  dominated by store open; if the in-process arm is measured (shape 3), the
  latency win should hold. If measured through the one-shot CLI, ~1 s/call adds
  to every step.
- **Accuracy ceiling:** `callers`/`refs`/`impact` remain partial for
  receiver-based method calls and for type-level questions, so the discovery
  tasks most likely to benefit are exactly the ones with the most unresolved
  edges. Expect the honesty contract to report many unresolved edges and the
  model to fall back to `text`/`read` for those.

## 10. Relation to the existing research docs (01–09)

This trial is black-box; the author docs are mostly white-box. Where they meet:

| This document | Existing doc |
|---|---|
| §3.1 reconcile edge loss (found, then verified fixed) | fixed and described in [07](07-lexical-and-incremental.md) / [05](05-verification.md) point 6 |
| §4 retrieval improvement | [07](07-lexical-and-incremental.md) retrieval measurements |
| §5 gap 1 receiver binding | [09](09-native-search-review.md) recommendation 20 (scope-aware binding); [02](02-findings.md) F05 scope |
| §5 gap 2 `impact` ignores `type_uses` | not specifically called out in 09; recorded here |
| §6 `explore` payload / graph payload bounds | [09](09-native-search-review.md) recommendation 5 (payload budgets), recommendation 12 (source selection) |
| §7 store open / sidecars | [09](09-native-search-review.md) recommendation 23 (separate facts from the hot manifest); [07](07-lexical-and-incremental.md) manifest-size notes |

It adds: end-to-end confirmation on a real repository, the exact A→C deltas, and
the measured per-invocation cost of the schema-3 sidecars.

## 11. Prioritized recommendations

**P0 — gates the measurement:**

1. **Bound the per-invocation store open.** Lazy-load `occurrences.json`,
   `dangling.jsonl`, and `dependencies.json` so `status` and metadata-only
   queries do not read them. Target: `status` back under ~0.1 s on a 200-file
   repo. This is what makes both the "sync every prompt" design and the one-shot
   CLI arm viable.
2. **Restore `explore` compactness.** Default to node + snippet + impact; move
   `excerpts`/`evidence` behind a flag (or a `--detail` level). Today `explore`
   (17.5 KB) is larger than the `text` search (12.4 KB) it replaces.

**P1 — accuracy:**

3. **Include `TypeUses` in the `impact` cone** so "what breaks if I change this
   struct/trait" answers at all. Cheap, and it uses edges that already exist.
4. **Scope/receiver-aware binding** for `x.method()` (constructor, parameter, and
   `let` annotations), the largest remaining `callers` recall gap.
5. **De-rank `tests::`/`#[cfg(test)]` symbols in `explore`.**

**P2:**

6. Resolve cross-crate import targets (`nanus_domain::…` currently dangles).
7. Make type-`refs` coverage predictable, or report it as a coverage statistic.

## 12. Threats to validity

- **Single machine, shared load** (load average 3–5). Timings are indicative, not
  a controlled benchmark; the *relative* A→C difference (≈8×) is larger than the
  observed noise and is explained by the sidecar sizes.
- **One repository** (`nanus`). Blogwright/whatsurvey are not re-measured here.
- **No agent trial.** These are direct calls; they cannot speak to whether a model
  *chooses* the tool or how the payload interacts with a real context window.
- **Evaluator-chosen queries.** Not a held-out set; the retrieval examples are
  illustrative, not a scored sample.
- **Time-bound.** Repository state and code are as of 2026-09-22; revision C is
  `81a5d02`. Binary version string is `0.1.0`, which does not identify the commit.
- **Out-of-domain queries** (`reconcile`, `bm25` on `nanus`) are recorded as
  expected noise, and must not be read as retrieval failures.

## Appendix A — reproduction

```sh
cd graph-search
cargo build --release
B=target/release/graph-search
R=/Users/ant/code/nanus
rm -rf /tmp/gsn && $B --root $R --store /tmp/gsn index      # full build
$B --root $R --store /tmp/gsn status --json                 # counts, staleness
$B --root $R --store /tmp/gsn search files 'crates/nanus-domain/src/*.rs'
$B --root $R --store /tmp/gsn search text 'register(' --include '*.rs'
$B --root $R --store /tmp/gsn search symbol 'ToolRegistry'
$B --root $R --store /tmp/gsn search callers 'AgentRunner::run_step' --json
$B --root $R --store /tmp/gsn search impact  'AgentRunner::run_tools' --depth 2 --json
$B --root $R --store /tmp/gsn search refs    'SearchQuery' --json
$B --root $R --store /tmp/gsn search deps    'crates/nanus-bundle/src/tools/grep.rs'
$B --root $R --store /tmp/gsn search explore 'where is the context budget enforced' --json
```

Incremental-correctness probe (the §3.1 reproduction):

```sh
rm -rf /tmp/gsw /tmp/gss && mkdir -p /tmp/gsw/src
printf 'pub fn alpha() {}\n' > /tmp/gsw/src/a.rs
printf 'pub fn beta() { alpha(); }\n' > /tmp/gsw/src/b.rs
$B --root /tmp/gsw --store /tmp/gss index
$B --root /tmp/gsw --store /tmp/gss search callers alpha   # 1 edge
printf 'pub fn alpha() {}\npub fn gamma() { beta(); }\n' > /tmp/gsw/src/a.rs
$B --root /tmp/gsw --store /tmp/gss search callers alpha   # must still be 1 edge
```

## Appendix B — raw measurements

Structured numbers, including per-revision counts, timings, payload sizes, and
the probe matrix, are committed as
[`results/independent-evaluation.json`](results/independent-evaluation.json).

---

VERDICT: LIKELY_IMPROVED_WITH_OPEN_REGRESSION
CONFIDENCE: medium
SUMMARY: The correctness defect that produced confident false negatives is fixed
and independently verified; extraction and ranking are materially better and the
gate is green (564 tests). Against that, per-invocation latency regressed ~8× to
~1 s on a 188-file repo because the schema-3 store loads multi-MB sidecars on
open, and the `explore` payload tripled past the `text` search it is meant to
replace. Receiver-based method calls, `type_uses` in `impact`, and cross-crate
imports remain unresolved. Fix the store-open and payload regressions before
running the §16.6 measurement; then the accuracy gaps are the remaining ceiling.
