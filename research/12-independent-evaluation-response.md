# Response to the independent evaluations (docs 10 and 11)

**Date:** 2026-09-22 · **Authors:** the `graph-search` authors · **Reply to:**
[10-independent-evaluation.md](10-independent-evaluation.md) and
[11-independent-evaluation-round-2.md](11-independent-evaluation-round-2.md).

This document does three things: it **evaluates** the findings in docs 10 and 11
against the current source, it records the **increments landed in response**, and
it **plans** the findings that remain, with the acceptance tests and Criterion
benchmarks that make each step measurable. Raw numbers are committed as
[`results/evaluation-response.json`](results/evaluation-response.json).

The evaluator was careful to separate confirmation from counter-evidence; this
reply keeps that discipline in the other direction. Where a finding is imprecise
or already resolved, it says so.

---

## 1. Scope and method

- Same black-box surface as the evaluations: the release binary and the public
  library, on the same repository (`nanus`, 188 walked files).
- One instrumented build was used **only** to attribute the ~1 s store open (the
  temporary probes were removed; the breakdown is in §3).
- `cargo test --workspace` (574 passed, 0 failed) and
  `cargo clippy --workspace --all-targets -- -D warnings` (clean) are the gate.
- The new Criterion suite is
  `cargo bench -p graph-search --bench evaluation`; it is deliberately separate
  from the frozen release-gate suite in `bench search`.

## 2. Evaluation of the findings

| # | Finding (docs 10/11) | Verdict | Disposition |
|---|---|---|---|
| P0.1 | `status`/one-shot ≈ 1 s, from store open | **confirmed**, and attributed in §3 | **open** — plan §5.1 |
| P0.2 | `explore` 17.5 KB > `text` 12.4 KB | **confirmed** | **fixed** — E2 |
| P1.3 | `impact` ignores `type_uses` | **confirmed** | **fixed** — E1 |
| P1.4 | receiver-variable `x.method()` unbound | **confirmed** | **open** — plan §5.2 |
| P1.5 | `tests::` symbols rank | **confirmed**, worse than reported | **open** — plan §5.3 |
| P2.6 | cross-crate imports dangle | **confirmed** | **open** — plan §5.4 |
| P2.7 | type `refs` uneven / unpredictable | **confirmed**, root-caused | **fixed** — E5 |
| P2.8 | dangling names carry whole expressions | **confirmed** | **fixed** — E4 |

Findings that hold but deserve precision:

- **P0.1 is real and the mechanism is fully explained in §3.** The evaluator
  inferred "store open, scaling with store size" from the 5 ms one-file store;
  the instrumented trace confirms it and shows *which* reads dominate. This
  matters for the plan: it is not one lazy sidecar, it is a group of them plus
  eager index construction.
- **P0.2 is confirmed and the fix is the shape already specified.** `SPEC.md`
  §9.1 always described an `explore` item as `{node, snippet, impact}`; the
  `excerpts`/`evidence` fields were an additive experiment that doubled the
  payload. E2 restores the specified shape on the CLI without deleting the
  richer contract from the library.
- **P1.5 is understated.** Round 1 reported "0–2 of the top 8" test symbols; on
  the current tree `search query literal` returns **5 of 8** and `port failure`
  **3 of 8**. This is a larger precision problem than the report suggests.
- **P2.7 is a bug, not a coverage limit.** The evaluator recorded it as "uneven
  and not predictable from the outside". It is deterministic: `type_uses` were
  emitted only for a declaration's direct `type` field, so a type used *only* as
  a parameter, return, local annotation or nested wrapper (`&T`, `Vec<&T>`)
  produced no edge at all. E5 fixes the walk, and the "uneven" appearance
  disappears: every probed type now reports.
- **The §3.1 reconcile defect remains fixed.** The three-file probe
  (`callers alpha` after editing the target) still holds at 1 edge — the single
  most important precondition the evaluator imposed.

## 3. Attributing the ~1 s store open (P0.1)

Instrumented release build, warm cache, `nanus`:

| Phase | Cost | What it is |
|---|---|---|
| `generation::current` / `select` | **218 ms** | reads and SHA-256s every pointer file (≈55 MB), then verifies the 15 MB source-record and 17 MB extraction-record packs |
| Grafeo `open_read_only` | 52 ms | graph file open |
| `rebuild_maps` | 15 ms | id maps |
| `dangling.jsonl` load | 13 ms | 24 210 records |
| `occurrences.json` parse | **207 ms** | 15 MB serde parse |
| `validate_fact_owners` | 85 ms | every occurrence/source fact checked against the node set |
| `refresh_indexes` | **283 ms** | 45 scan + 48 metadata + **169 `BodyIndex`** + 16 occurrence + 5 adjacency |
| **total** | **≈873 ms** | plus the staleness walk and service setup ≈ 0.9–1.0 s |

Two facts drive the plan: the **raw-byte work** (`select` + records) and the
**eager construction** (`occurrences` parse, `BodyIndex`, validation) are both
paid on every invocation even though `status` needs only the manifest header,
cached counts and staleness.

## 4. Increments landed in response

Every increment below has a regression test and a Criterion benchmark; the gate
is green at 574 tests.

### E1 — `impact` traverses the `refs` vocabulary (P1.3)

`QueryEngine::impact_inner` now uses `REFERENCE_KINDS` (`Calls`, `References`,
`TypeUses`) instead of `[Calls, References]`. `SPEC.md` §8.3 says so explicitly.

| `impact --depth 2` (nanus) | before | after (d1) |
|---|---|---|
| `ToolRegistry` | 0 | 3 |
| `SearchQuery` | 0 | 5 |
| `ToolSchema` | 0 | 25 |
| `ToolOutcome` | 0 | 8 |

Test: `accuracy::impact_includes_type_uses_so_structs_have_a_blast_radius`.

### E5 — Rust `type_uses` cover parameter/return/local/wrapper types (P2.7)

`type_names` descended a closed list of node kinds, and `walk_node` never visited
a lone `type_identifier`, so only direct field types emitted uses. The walk now
descends every wrapper and the dispatcher handles a bare type in position.
`PARSER_VERSION` 21 → 22.

| `refs` (nanus) | before | after |
|---|---|---|
| `SearchQuery` | 0 | **5** (all resolved) |
| `ToolDefinition` | 0 | **9** |
| `ToolOutcome` | 2 | 8 |
| `AgentConfig` | 2 | 9 |
| `ToolSchema` | 1 | 25 |

Cost: `occurrences.json` 13 → 15 MB; the store is 57 MB. The extra facts are the
point, but they make P0.1 more urgent, not less.

Test: `accuracy::rust_parameter_and_return_types_are_type_uses`.

### E2 — compact `explore` detail (P0.2)

New `ExploreDetail { Compact, Full }` on `ExploreQuery`; new CLI `--detail`
(default `compact`). Compact keeps `node`, the primary `snippet` and `impact`,
computes matched-body facts internally to anchor the snippet, and does not
publish `excerpts`/`evidence`. The library keeps `Full` by default so the
rich-evidence contract and its tests are unchanged; the CLI — the surface the
evaluator measured and the one `nanus` drives through `bash` — is compact.

| payload, `nanus`, same query | bytes |
|---|---|
| `explore` before | 17 547 |
| `explore` default after | **8 061** |
| `explore --detail full` | 17 562 |
| `text` search (unchanged) | 12 359 |

Criterion prints the payload size in the benchmark id
(`payload/explore_compact/5793B` vs `payload/explore_full/8810B`), so a future
regression is visible in the benchmark output itself.

Tests: `explore_detail::compact_keeps_the_seed_and_drops_excerpts_and_evidence`,
`cli::explore_defaults_to_compact_detail_and_full_opts_in`.

### E4 — canonicalise dangling display names (P2.8)

Unresolved names are labels, not keys; `to_name` is now collapsed to one bounded
line. On `nanus`: multi-line names **2388 → 0**, longest name **914 → 97 bytes**.

Test: `resolve::dangling_names_are_canonicalised_to_one_bounded_line`.

## 5. What remains, and the plan

### 5.1 P0 — a lazily loaded store (gates the §16.6 measurement)

The target from doc 10 stands: `status` under ~0.1 s on a 200-file repo. §3 shows
that is a coordinated change, not a one-line one. Staged, each stage independently
measurable with `cargo bench -p graph-search --bench evaluation` (`open/published_store`):

1. **Split pointer verification from selection.** `select` reads and hashes every
   artifact before returning. Keep the pointer's hashes, but verify each sidecar
   *on first use* and remember the result. Acceptance: `open/published_store`
   drops by the ~55 MB hash+read time, and a deliberately corrupted sidecar still
   fails loudly on the first query that touches it (a store-corruption test).
2. **Lazy record packs.** `source-records/` (15 MB) and `extraction-records/`
   (17 MB) are verified at open but needed only by body/source queries. Load them
   behind the same first-use gate.
3. **Lazy `occurrences.json` (207 ms) and `BodyIndex` (169 ms).** Parse the
   occurrence sidecar and build the body index on first access. Both are needed
   by `explore`/`occurrences`, never by `status`/`symbol`. The trait methods that
   currently return `&T` need a fallible or guaranteed-initialized shape; the
   chosen design is recorded in the increment, and a poisoning/`unavailable`
   state keeps failures loud.
4. **Split counts from index construction.** `status` needs cached counts, not
   adjacency. Defer `adjacency`/`occurrences` index builds to first use.
5. **Acceptance:** `status` p95 under 100 ms on the 200-file Criterion corpus and
   under 150 ms on `nanus`; `open/published_store` p95 within 25% of the number
   achieved at stage 3; the existing store-corruption and reader-lifetime tests
   unchanged.

Nothing here changes the on-disk format; it changes *when* verified bytes are
read. That keeps the integrity contract (verify before exposing) intact while
removing the eager cost.

### 5.2 P1 — scope/receiver-aware `x.method()` binding

Unchanged and confirmed (`callers ToolRegistry::execute` = 0). The plan is the
one already sketched in doc 09/10: resolve a method call through the receiver's
declared type (`let`/parameter annotations, constructor calls, `self`), with an
explicit unresolved fallback. Acceptance: a small fixture set per receiver shape,
plus `callers ToolRegistry::execute` and `callers ToolRegistryHandle::borrow`
becoming non-zero on `nanus` without a new false positive on a held-out sample.
This is the largest remaining recall gap and the most expensive; it should not be
attempted before P0.1 lands.

### 5.3 P1 — de-rank test-owned symbols in `explore`

Confirmed and worse than reported (§2, P1.5). The plan is a generation-owned
test-ownership fact (path under `tests/`/`test_*`, `#[cfg(test)]` module owner,
`tests::` qualification) and a **bounded, measured** de-rank, because a naive
penalty can hide a legitimate test lookup. Acceptance: `search query literal` and
`port failure` top-8 test-owned counts fall to ≤1 on `nanus` while the frozen
held-out retrieval suites (docs 07/08) do not regress. The penalty is chosen from
the benchmark, not guessed.

### 5.4 P2 — cross-crate import targets

`deps` lists `nanus_domain::ContentBlock` as `(unresolved)`. This needs the
workspace package graph to link an external crate name to its in-repo package
(`packages.rs` already knows the manifests). Acceptance: a fixture with two
crates where the dependency resolves to the defining file, and `deps` on a
`nanus` file no longer reports a workspace-internal target as unresolved.

## 6. Validating progressive improvement

Three mechanisms, in order of cost:

1. **Acceptance tests** (`cargo test --workspace`) — each finding that is fixed
   has a test that failed before and passes now. The tests are named after the
   finding so a regression is self-explanatory.
2. **Criterion**, two suites:
   - `cargo bench -p graph-search --bench evaluation` (new): `open/published_store`
     for P0.1, `graph/refs_type` + `graph/impact_type` for E1/E5, and
     `payload/explore_compact/<N>B` vs `payload/explore_full/<N>B` vs
     `payload/text_search/<N>B` for E2. The byte counts are in the benchmark ids.
   - `cargo bench -p graph-search --bench search` (unchanged): the release gate's
     frozen suite, which must not move for these changes.
3. **Black-box probes** on `nanus` (Appendix A of doc 10), reproduced in
   [`results/evaluation-response.json`](results/evaluation-response.json). These
   are the end-to-end check that the bench-scale improvements survive a real
   repository.

A change is "progressive" only if the acceptance test flips, the relevant
Criterion number moves in the right direction, and the release-gate suite and the
held-out retrieval suites do not regress.

## 7. Threats to validity

- **One repository** (`nanus`) for the black-box probes; the Criterion corpus is
  synthetic and cannot substitute for blogwright/whatsurvey.
- **Timings are single-machine** and were measured under shared load; the §3
  breakdown is an attribution, not a controlled benchmark. The *ranking* of
  phases is large enough (169 ms vs 13 ms) to be robust.
- **`type_uses` growth is a tradeoff**, recorded honestly: E5 improves recall and
  grows the store by ~2 MB. It also increases the `open` cost until P0.1 lands —
  the two findings interact, and P0.1 is the gate.
- **Compact is the CLI default, not the library default.** A host using the
  library in-process (shape 3) must opt into `ExploreDetail::Compact` to get the
  measured payload; the plan is for the future `nanus` adapter to do so. This is
  a deliberate split, not an oversight.
- **No agent trial.** As in docs 10/11, none of this measures whether a model
  chooses the tool or how the payload interacts with a context window.

---

VERDICT: FINDINGS_CONFIRMED_FOUR_LANDED_ONE_GATED
CONFIDENCE: high (each changed finding has a regression test and a black-box
probe; the open one has an instrumented attribution)
SUMMARY: Both evaluations are accurate. Two of the eight reported items were
imprecise in the honest direction: the "uneven type refs" gap was a deterministic
extractor bug now fixed (E5), and the test-symbol noise is worse than reported.
Four items are fixed and independently reproducible — `impact` includes
`type_uses` (E1), Rust parameter/return/local types are type uses (E5), `explore`
has a compact CLI default that is smaller than the `text` search it replaces
(E2), and dangling names are canonicalised (E4) — with 574 tests and a new
Criterion suite. The ~1 s store open (P0.1) is confirmed and attributed by phase;
it is the gate for the §16.6 measurement and now has a staged plan. Receiver
binding, test-symbol de-ranking and cross-crate imports remain open with stated
acceptance criteria.
