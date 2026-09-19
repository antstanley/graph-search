# Benchmark plan

The measurement protocol for the pre-registered prediction in
[`SPEC.md`](../SPEC.md) §16.6. Fill in `## Results` when runs happen; do not
change §16.6 after the fact.

## Executable task suite

[`evaluation/`](../evaluation/README.md) implements the source-grounded task
corpus, read-only engine adapters, budgeted trials, external agent-driver
protocol, blind rubric grading and paired reports. It contains 60 debugging and
change-planning tasks across nanus, blogwright and whatsurvey, with family-level
development/held-out splits. See the [first evidence run](../research/08-task-evaluation.md).

The executable suite compares resident graph-search, existing CodeGraph, and
ordinary text search through a shared search/read contract. This is a new
retrieval/answer evaluation, not the original nanus-as-shipped versus CLI versus
in-process integration experiment below. The latter and the §16.6 prediction
still require actual controlled model trials. File hits are not task success;
no model success or token-reduction claim follows from deterministic retrieval.

## What is measured

| Metric | How |
|---|---|
| Tool calls | fold `ToolCall` events in the session log |
| Steps / turns | fold `StepStart` / `TurnStart` |
| Prompt + completion tokens | session usage totals (`Usage`) |
| Wall time | timestamps on `TurnStart` / `TurnEnd` |
| Residual context | estimated tokens in the transcript at turn end |
| Correctness | rubric score per task (must-mention symbols/files + structural answer) |
| Adoption | fraction of discovery tasks where the treatment tool was called |
| Staleness incidents | results taken from a stale index, or a missed file |

## Arms (§16.2)

- **Baseline** — `nanus` as shipped.
- **Treatment A (CLI)** — `nanus` + `graph-search` via `bash` (shape 1).
- **Treatment B (in-process)** — a harness that links the library (shape 3) and
  holds the `Index` open across a turn.

Hold constant: model, reasoning effort, approval/sandbox state, turn budget,
workspace, and task set. Warm the index before any timed run.

## Protocol

1. `graph-search index` (or `Index::open` + `reindex`) before each repo's run.
2. Run each task in a fresh session; capture the log.
3. Record metrics per task and per archetype (trivial, discovery, definition,
   structural, broad).
4. Score correctness blind to the arm where possible.
5. Compare arms A vs B to isolate the cost of *not* being in-process.

## Repos

- `graph-search` (small; self-hosting).
- `nanus` (~200 files of Rust).
- One third-party Rust repo and one TypeScript repo (larger).

## Results

_Empty until the evaluation runs._

| Run | Repo | Arm | Calls | Steps | Tokens | Wall (s) | Rubric | Notes |
|---|---|---|---|---|---|---|---|---|
| | | | | | | | | |
