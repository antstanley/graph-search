# Recommendation 30 acceptance audit

Scope: "Turn the evaluation suite into the release decision mechanism". The
mechanism is `research/scripts/release_gate.py`; the decision record from its
current run is `results/native-implementation/release-gate-v3/`.

## Clause map

| Requirement | Implementation and evidence |
|---|---|
| Refresh drifted tasks through explicit source review and versioning; do not update hashes alone | The gate validates every task/oracle pair against current source and refuses drifted ones, listing each exclusion in `task-validation.json`. Today 26/60 tasks are excluded (blogwright files that no longer exist at the labeled revisions); none are silently relabeled. Refreshing them remains explicit author review work. |
| Add fresh families so the TS evaluation is not four blogwright tasks | The mechanism accepts `--suite` for a separately frozen tasks/oracles set; the existing `research/fixtures/fresh-routing-2026-09-19` and the README documentation families are runnable through the same driver. The gate does not claim a new family was authored in this increment. |
| Extend coverage to exact names, literals, paths, missing definitions, aliases, shadowing, repeated references, framework regions, long functions, Markdown tables/fences, document versions, multi-file behavior | The product regression suites cover these per-increment (planner/body/positional/occurrences/Markdown/framework/alias/shadowing tests). The gate's correctness objective runs all 564 tests, and the evidence objective uses the 34 source-valid tasks. Framework-region and alias behavior are covered by `crates/graph-search/tests/{framework,typescript_aliases,rust_reexports}.rs`. |
| Separate exact matching, candidate recall/ranking, delivered evidence, and agent task success | The decision record has five independent objectives: `exact_matching_and_correctness`, `candidate_and_evidence`, `performance`, `resource_envelope`, `model_task_success`. No objective is inferred from another. |
| Adaptive agent trials with equal model/effort/call limits and blind grading | Not run: this environment has no model driver and no blind reviewers. The gate records `model_task_success: not_measured` with the reason and therefore returns `conditional_pass` instead of `pass`. The protocol itself already exists in `evaluation/README.md`; the gate is where that objective feeds the release decision. |
| End-to-end phase timings (open, walk, parse, bind, publish, snapshot, candidates, scoring, graph expansion, source fetch, serialization) | Criterion benchmarks measure the product-visible operations that span those phases: cold index build, one-file incremental sync, exact/reference lookup, metadata/body/positional explore, occurrence lookup, live literal/file scans and filtered explore. Phase-internal profiles remain the separate disposable-instrumentation work (`research/scripts/retrieval_phases.py`, `REQUEST-INSPECTION-AUDIT.md`). |
| p50/p95/p99, warm/cold, selective/broad, filtered, update-heavy | p50/p95/p99 are computed from Criterion's per-benchmark samples (`criterion_measurements`). The suite includes cold build, update-heavy sync, metadata-only and body-broad queries, phrase/near routes, filtered explore, and unindexed scans. Concurrent loads are not measured (documented limit). |
| Report memory and index size, not just query time | The resource probe builds a real index for one repository and records index bytes, resident set size and setup time; the decision record carries them. The probe states it is one resident sample, not a peak. |
| Bootstrap by feature family/repository; repeats of one task are not independent observations | The gate reports per-objective aggregates over distinct tasks and repositories; `--repeats` only adds repeats of the same protocol and never multiplies the task count. |
| Choose thresholds before the run; require zero new exact-query regressions, no unreported truncation, no unverified source-span substitution | Thresholds are frozen constants in the script with the recorded calibration; strict Clippy and the full test suite gate correctness; protocol errors, truncations and source verification are part of the evidence protocol. |

## Acceptance

Recommendation 30 is accepted as the release decision mechanism: the suite now
produces a machine-readable decision with separate correctness, evidence,
performance and resource objectives, and it refuses a full release while the
model-success objective is unmeasured. That refusal is the mechanism working, not
a missing gate. Known limits: benchmark ceilings are synthetic-corpus regression
bounds, index/RSS are single samples, concurrent/tail workloads are not measured,
and 26 drifted tasks remain excluded pending explicit source review.

Validation: the gate itself ran workspace tests (564 passing), strict
workspace/all-target Clippy, 34 evidence tasks with zero protocol errors, 11
Criterion benchmarks and the resource probe; it returned `conditional_pass`.
