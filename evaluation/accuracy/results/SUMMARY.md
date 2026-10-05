# Accuracy benchmark summary, 2026-10-05

Pooled results of `evaluation/accuracy` over local Python, TypeScript/JavaScript
and Rust repositories and one OKF v0.2 research bundle. Figures are
micro-averaged percentages, `before → after (n after)`; n is the number of
scored definitions, queries or call sites. Each comparison uses the same
oracle, sampling seed and repositories on both sides. The raw runs (per-repository
metrics and every miss) are kept locally and are not committed.

## Round 2: `1bdf88a` → `b82455e`

23 repositories. The oracle reads `select!`, `proptest!` and
inline-snapshot macro bodies (`87adf2d`).

| Metric | Python | TS/JS | Rust |
|---|---|---|---|
| Definitions found (extraction recall) | 100.0 → 100.0 (1725) | 100.0 → 100.0 (3655) | 100.0 → 100.0 (5826) |
| Definitions correct (extraction precision) | 100.0 → 100.0 (1691) | 99.9 → 99.9 (3372) | 100.0 → 100.0 (5177) |
| Symbol lookup: rank 1, repo-unique names | 98.7 → 100.0 (714) | 97.4 → 99.0 (2120) | 97.0 → 99.5 (1173) |
| Symbol lookup: rank 1, all names | 75.9 → 77.3 (997) | 73.3 → 75.4 (3145) | 69.6 → 71.4 (1822) |
| Callers: call sites found | 89.9 → 94.0 (1463) | 86.5 → 93.6 (6445) | 75.0 → 80.2 (3299) |
| Callers: reported callers correct | 100.0 → 99.8 (1170) | 100.0 → 100.0 (2849) | 99.9 → 99.9 (1995) |
|   bare calls `f()` | 100.0 → 100.0 (1235) | 91.6 → 98.0 (5986) | 96.8 → 97.1 (1230) |
|   `self.f()` / `this.f()` / `Self::f()` | 87.7 → 87.7 (57) | 97.7 → 97.7 (87) | 99.3 → 99.2 (244) |
|   Rust path calls `Type::f()` | – | – | 84.1 → 89.4 (407) |
|   calls on a variable `x.f()` | 17.5 → 52.6 (171) | 1.3 → 21.8 (372) | 43.8 → 46.5 (768) |
|   Rust calls inside macro arguments | – | – | 56.2 → 75.4 (650) |
| Ambiguous `self.f()`: bound to the right `f` | 96.7 → 98.0 (153) | 94.3 → 99.5 (383) | 92.9 → 93.3 (609) |
| Ambiguous `self.f()`: precision when bound | 100.0 → 100.0 (150) | 100.0 → 100.0 (381) | 100.0 → 100.0 (568) |
| Explore by doc comment: definition rank 1 | 62.2 → 75.9 (344) | 84.2 → 87.8 (1231) | 91.2 → 91.2 (775) |
| Explore by doc comment: definition in top 8 | 81.1 → 84.6 (344) | 90.6 → 92.4 (1231) | 94.7 → 94.8 (775) |

| OKF v0.2 bundle | rank 1 |
|---|---|
| Concept lookup by title | 100.0 → 100.0 |
| Section lookup by heading | 68.5 → 68.5 |
| Explore by description: concept | 10.3 → 98.3 |

`links_to` and `cites` relationships: recall and precision 100.0% / 100.0% and 100.0% / 100.0%.

The one new Python caller outside the name-based oracle is a correct binding
through an aliased import. Rust explore rank 1 is flat: test functions gained
call edges, and some tests now rank below the production code they exercise.

## Round 1: `6fc3143` → `1bdf88a`

27 repositories that indexed without exclusions in both runs
(the original oracle). Before `1bdf88a`, two repositories could not be indexed
at all: three same-named definitions on one line aborted the index.

| Metric | Python | TS/JS | Rust |
|---|---|---|---|
| Definitions found (extraction recall) | 100.0 → 100.0 (987) | 100.0 → 100.0 (3822) | 100.0 → 100.0 (8251) |
| Definitions correct (extraction precision) | 100.0 → 100.0 (980) | 99.8 → 99.8 (3471) | 100.0 → 100.0 (7154) |
| Symbol lookup: rank 1, repo-unique names | 98.4 → 98.4 (494) | 97.0 → 97.0 (2313) | 97.1 → 97.1 (1563) |
| Symbol lookup: rank 1, all names | 72.0 → 72.0 (722) | 72.6 → 72.6 (3448) | 70.3 → 70.6 (2422) |
| Callers: call sites found | 89.7 → 89.7 (755) | 87.1 → 88.1 (6821) | 49.9 → 71.8 (4831) |
| Callers: reported callers correct | 100.0 → 100.0 (540) | 100.0 → 100.0 (2768) | 100.0 → 99.7 (2644) |
|   bare calls `f()` | 100.0 → 100.0 (623) | 93.4 → 93.4 (6312) | 79.3 → 96.5 (1769) |
|   `self.f()` / `this.f()` / `Self::f()` | 100.0 → 100.0 (45) | 31.4 → 85.1 (121) | 88.2 → 96.3 (270) |
|   Rust path calls `Type::f()` | – | – | 78.1 → 85.1 (549) |
|   calls on a variable `x.f()` | 10.3 → 10.3 (87) | 2.6 → 2.6 (388) | 25.8 → 33.0 (1311) |
|   Rust calls inside macro arguments | – | – | 2.5 → 64.5 (932) |
| Ambiguous `self.f()`: bound to the right `f` | 100.0 → 100.0 (13) | 35.6 → 95.8 (239) | 73.2 → 93.9 (848) |
| Ambiguous `self.f()`: precision when bound | 100.0 → 100.0 (13) | 100.0 → 100.0 (229) | 100.0 → 100.0 (796) |
| Explore by doc comment: definition rank 1 | 64.7 → 64.7 (204) | 83.4 → 83.6 (1265) | 84.0 → 84.1 (1075) |
| Explore by doc comment: definition in top 8 | 83.3 → 83.3 (204) | 90.5 → 90.7 (1265) | 90.4 → 90.4 (1075) |

Rust caller precision 99.7: every caller the original oracle could not confirm
was a real call inside `tokio::select!`, `json!`, `proptest!` or an inline
snapshot, which that oracle could not read.

## Method and limits

Ground truth comes from parsers independent of graph-search: CPython `ast`,
the TypeScript compiler parser, `syn`, and a CommonMark-subset OKF reader. See
[`../README.md`](../README.md). Caller targets are uniquely and distinctively
named, so caller precision mainly catches callers with no matching call; the
ambiguous-binding task is the precision test. The oracle cannot see aliased
imports or dynamic dispatch, and does not read `json!` bodies. Doc-comment
queries overlap lexically with indexed text; they are not a paraphrase test.
Performance is not measured here; use the Criterion benches.
