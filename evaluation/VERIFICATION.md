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
| Verification and documentation | 24 tests after the review fixes; host build, Rust formatting and strict host/dependency Clippy; live 180-entry run; README, benchmark plan and research report | Pass |

Authoritative verification logs and sanitized run artifacts are under `results/`.
The original baseline audit matched every recorded implementation source hash to
the then-current file, verified the 60-task/180-entry counts, confirmed 160 available and 20
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

## Independent review corrections

The independent review of commit `7446760` found four defects missed by the
original 18 tests. The original verdict did not cover these failure paths.

- A timed-out resident graph process was reused and could fail again during
  cleanup. Failed transports are now invalidated, the next search starts a fresh
  host within its timeout, and per-trial restart time is explicit. Host cleanup is
  idempotent and tolerates broken buffered stdin. Suite finalization records each
  cleanup error and still attempts every backend and both final artifacts.
- Terminal unanswered agent trials were omitted from success denominators.
  They now count as failures. Conditional graded-answer success is separate;
  end-to-end rates and paired deltas await any pending answer grades. Evidence
  runs retain null success. Legacy terminal-null records are handled in reports.
- A child closing output before exiting raised `subprocess.TimeoutExpired`
  outside the runner's error contract. `command` now normalizes this to built-in
  `TimeoutError`, which the driver/tool paths already record as a trial error.
- Failed decisions were missing from token-completeness accounting. Every driver
  attempt reserves an unknown usage entry before launch; only validated usage
  replaces it. Incomplete totals remain null, with a separately named known sum.

Six added regression tests cover these paths, downstream report pairing, pending
grades, legacy records, repeated close, remaining backend cleanup and artifact
preservation. The suite now passes 24 tests. A real Rust host was deliberately
timed out, then successfully restarted for the next trial against nanus; source
snapshots remained identical (`results/recovery-smoke.json`). The nine-entry
three-repository smoke run exercises all adapters, with the expected missing
nanus CodeGraph arm. Historical baseline measurements and labels are unchanged.

Resolution: exception normalization is at the shared subprocess boundary;
GraphSearch owns invalidation/restart, while CLI finalization isolates cleanup
errors. Sufficiency: both lifecycle recovery and final artifact preservation are
covered; success fixes include runner, legacy report interpretation and pairing;
usage completeness covers all attempted decisions, not only parsed responses.
Regression paths: blind grading still handles answers only, answered/pending and
evidence-only nulls remain distinct, and source/citation checks are unchanged.

Verdict: the four reproduced review findings are fixed and regression-tested;
original corpus and model-trial limitations still apply.
