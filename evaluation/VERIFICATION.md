# Completion verification

Scope: implement a task-based evaluation suite comparing graph-search,
CodeGraph and ordinary text search on source-backed debugging/change-planning
questions across nanus, blogwright and whatsurvey, with held-out tasks and
correctness/call/latency/context measurements.

| Obligation | Evidence | Outcome |
|---|---|---|
| Real tasks across all three repositories | `tasks.json`: 60 tasks, 20 per repo; 30 feature families, two task kinds | Pass |
| Source-backed answer and relationship labels | `oracles.json`: hashed code regions, explicit facts, relationship descriptions and change-test criterion | Pass; author-reviewed, not independent human review |
| Held-out split without related variants crossing it | 36 dev / 24 held-out; validator rejects family leakage; `test_public_gold_and_split_separation` | Pass |
| Solver actions independent of gold | `trial` accepts public task/backend only; separate scoring after return; driver test asserts no oracle fields | Pass within trusted-driver boundary |
| Three engine adapters, external repos read-only | Resident public-library host with temporary store; existing CodeGraph index only; ripgrep; source snapshot before/after equality | Pass |
| Call, latency and context measurement | Per-trial history and sanitized metrics; separate setup, tool/model wall, returned bytes, repeated input bytes, optional provider usage | Pass |
| Actual task-success scoring | Blind packet, all-criterion judgment, unsupported-claim judgment, observed citations, transcript/packet hash checks; CLI end-to-end test | Pass; no actual model trial claimed |
| Retrieval does not masquerade as correctness | All 180 published entries have null task success; explicit coverage proxy fields | Pass |
| Budgets, errors and missing comparators | Shared response/context/call budgets; subprocess timeout/output limits; unavailable index separate from miss | Pass |
| Repeatable and auditable results | Frozen corpus hashes, implementation/host hashes, versions, revisions, source snapshots, CodeGraph freshness; seed and paired reports | Pass |
| Verification and documentation | 18 tests; host build, Rust formatting and strict host/dependency Clippy; live 180-entry run; README, benchmark plan and research report | Pass |

Authoritative verification logs and sanitized run artifacts are under `results/`.
The final audit matched every recorded implementation source hash to the current
file, verified the 60-task/180-entry counts, confirmed 160 available and 20
unavailable trials, checked zero recorded tool errors, and compared repository
snapshots before and after. Both existing CodeGraph indexes were fresh (247 and
1,039 matching file hashes). Root main remains untouched.

## Correctness reasoning

**Resolution:** each adapter normalizes only actually returned source lines;
shared reads resolve relative paths beneath the repository and reject traversal
or symlink escape. Gold regions resolve against those same current files.
Graph spans and CodeGraph source gaps are never expanded into unseen code.

**Sufficiency:** output is truncated before extracting seen lines or planning
reads. File recall alone cannot set task success. Missing judgments cannot pass,
and graded answers remain linked to the exact original transcript. Unavailable
arms are excluded explicitly, while tool errors remain in available-trial
denominators. Paired comparisons match task ID and repeat number.

**Regression paths:** tests cover source drift, split leakage, public-schema
separation, Unicode truncation, false/truncated lines, path traversal, missing
indexes, CodeGraph gaps, subprocess hangs and output flooding, malformed drivers,
real ripgrep, budgets, usage accounting, blind packets and CLI grading/reporting.
The real run exercises both indexed external comparators and the resident Rust
library. Production Rust code is unchanged.

## Limits

This is self-verification. Tasks are source-derived scenarios and mostly
within-file reasoning, not independent issue reports or applied-code tasks.
Model-driver settings are declared rather than enforced by a provider adapter;
drivers are trusted executables and must be isolated externally for adversarial
contamination resistance. No paid/live model trial was run. Committed held-out
gold is not secret, and use for tuning retires its independent-test status.
The fixed text-search protocol is intentionally basic and its poor results must
not be generalized to adaptive grep. Complete code-region coverage is a strict
proxy; human answer grading remains the semantic authority. Single-repeat debug
build timings are descriptive, not performance claims.

Verdict: implemented and verified within the documented task-suite scope.
