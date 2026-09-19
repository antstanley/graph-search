# Task-based search evaluation

This suite asks real debugging and change-planning questions about **nanus,
blogwright and whatsurvey**. It contains 60 manually authored tasks: 20 per
repository, two tasks per feature family, 36 development tasks and 24 held-out
tasks. Each family stays in one split. Answers must explain behavior or propose
a change with regression tests; this version does not apply patches or execute
external repositories' application tests.

Public solver inputs are in `tasks.json`. Source-reviewed answer criteria,
required code regions and relationships are in `oracles.json`. Labels were
written from source before the full benchmark, independently of engine
rankings. This is author review, not independent human adjudication. Regions
carry file and region SHA-256 hashes; any source drift aborts validation. The
suite deliberately includes simple helpers, longer control flow, persistence,
approval/consent decisions, and a Svelte extraction gap.

There are two distinct protocols:

- **Evidence protocol**: search the public prompt, then read up to three distinct
  returned files near their first returned location. No oracle guides actions.
  This measures evidence retrieval only. `task_success` is always null.
- **Agent protocol**: an external model driver chooses search/read actions and
  supplies an answer. Arm-blind reviewers grade every source-backed criterion
  and unsupported claims. Success requires all criteria, no unsupported claims,
  and citations to delivered source. Model answers are never keyword-graded.

## Reproduce

Python 3.10+, Rust (repository toolchain), ripgrep, and optionally the CodeGraph
CLI are required. The Python runner uses only the standard library. Build the
resident public-library host separately:

```sh
cargo build --locked --offline --manifest-path evaluation/harness/Cargo.toml
PYTHONPATH=evaluation python3 -m unittest discover -s evaluation/tests -v
```

Create a local roots JSON file (absolute paths):

```json
{"nanus":"/Users/you/code/nanus","blogwright":"/Users/you/code/blogwright","whatsurvey":"/Users/you/code/whatsurvey"}
```

```sh
PYTHONPATH=evaluation python3 -m taskbench validate --roots /tmp/roots.json
PYTHONPATH=evaluation python3 -m taskbench run \
  --roots /tmp/roots.json --output evaluation/runs/dev-001
# Freeze experiments before explicitly opening the held-out split:
PYTHONPATH=evaluation python3 -m taskbench run \
  --roots /tmp/roots.json --split heldout --allow-heldout \
  --output evaluation/runs/heldout-001
```

Use `--task-id nanus.context.debug` for a smoke test, `--arms text graph-search`
for selected engines, or `--repeats 3 --seed 1729` for repeated trials. Task/arm
order is shuffled reproducibly. A new output directory is required for each
run. All fixtures validate even if a subset is selected. To adopt a new source
revision, review and version prompts, labels and hashes explicitly; do not
silently regenerate labels until validation passes. Historical results refer
to their frozen corpus and source revisions.

## Engine comparison

All arms expose the same `search(query)` and `read(path,start,count)` operations.
The graph-search host owns a resident `Index`, indexes into a disposable external
store and queries the public library (`k=8`, one graph hop). CodeGraph uses its
existing index and `explore --max-files 8`. The text arm is ordinary
case-insensitive ripgrep OR search with fixed stopword removal, up to 32 literal
terms and five matching lines per file in path order. It is a reproducible basic
text baseline, not an optimized human ripgrep strategy.

The graph host's additional structural modes are available for future protocols
but are not exposed to the current solver: search/read have equal tool contracts.
CodeGraph output still includes its relationships; graph-search output includes
its graph metadata. Neither graph spans nor relationship statements count as
source read. Only returned line-numbered source that exactly matches the current
file earns coverage. Gaps remain gaps. Truncation happens before scoring and
before selecting follow-up reads. Shared reads use the current filesystem.

Default budgets are four tool calls, 16 KiB per response, 48 KiB cumulative tool
response content, and 180 seconds per trial. Reads are capped at 200 lines.
Increase `--calls` for agent trials. Tool errors consume calls and context;
timeouts terminate subprocess groups. Setup/index time is recorded separately
from trial time. graph-search is resident, while CodeGraph and ripgrep include
CLI startup per search. Thus reported latency is the measured integration cost,
not an isolated ranking algorithm comparison. No speedup claims should be made
from a single repeat on a shared machine.

The runner never creates CodeGraph indexes. Missing indexes are **unavailable**,
not retrieval misses. Freshness audits read existing SQLite hashes and report
stale/missing sources and coverage differences; they do not refresh them.
External repositories and their indexes must remain read-only. A fresh
graph-search index and a potentially stale CodeGraph index are not identical
corpora; consult the manifest before comparing results.

## Agent driver contract

Pass a JSON argv array, an exact model identifier and effort/configuration:

```sh
PYTHONPATH=evaluation python3 -m taskbench run \
  --roots /tmp/roots.json --output evaluation/runs/agent-001 \
  --agent-command '["/absolute/path/to/python3","/absolute/path/to/driver.py"]' \
  --model 'provider/model-version' --effort 'high' --calls 12 --repeats 3
```

The runner starts the driver afresh for each decision, in a temporary working
directory. It receives one JSON object on stdin containing `protocol: 1`, the
public `task`, tool descriptions, complete `history`, `remaining_calls` and
`remaining_response_bytes`. It must write exactly one JSON object to stdout:

```json
{"action":{"name":"search","arguments":{"query":"context eviction newest turn"}},"usage":{"input_tokens":120,"output_tokens":24}}
```

```json
{"action":{"name":"read","arguments":{"path":"relative/file.rs","start":100,"count":80}}}
```

```json
{"answer":"An explanation with source references.","citations":[{"path":"relative/file.rs","line":110}],"usage":{"input_tokens":300,"output_tokens":90}}
```

Usage is optional and must contain actual per-request provider input/output token
counts. Missing usage makes aggregate provider tokens null. Estimated response
characters/4 are separately labelled and never substituted for provider usage.
`model_input_bytes` sums repeated serialized driver requests; `response_bytes`
measures cumulative tool content. The model/effort settings are recorded, but the
external driver is responsible for honoring them. Give each arm the same driver,
model, sampling, effort and budgets, without persistent cross-task memory.

No provider SDK, credentials or paid model calls are bundled. Drivers are trusted
executables, not OS-sandboxed solvers. Gold files are never included in requests,
but a malicious driver could read the host filesystem. For strict evaluation,
run the driver in a separate sandbox with only the protocol exposed. Do not give
solvers this checkout, oracle files, grading packets or previous trial answers.
Committed held-out labels provide reproducibility, not secrecy; after using the
held-out set to make tuning decisions, author a new version before claiming
independent generalization.

## Blind grading and reporting

```sh
PYTHONPATH=evaluation python3 -m taskbench blind evaluation/runs/agent-001 \
  --output /tmp/review-packets
# Reviewer fills every *.judgments.json: identity, booleans for all criteria,
# and unsupported_claims. Null placeholders are intentionally rejected.
PYTHONPATH=evaluation python3 -m taskbench apply-grades evaluation/runs/agent-001 \
  /tmp/review-packets --output evaluation/runs/graded-001
```

Reviewers receive the public task, answer, citations, delivered line locations
and source-backed rubric. Engine labels and timing metadata are omitted; the
private mapping remains in the run directory. Writing style can still reveal
an engine, so this is metadata blinding, not a guarantee of perfect blinding.
Packet and transcript hashes reject altered/mismatched results. Citation checks
verify that cited locations were seen, while reviewers must judge whether they
support the answer. For disagreements, retain separate reviewer outputs and
adjudicate explicitly; the tool does not invent consensus.

Reports stratify by repository, split, task kind and arm, with scheduled,
available, evidence-scored and graded denominators. Paired deltas use matching
task IDs and repeat numbers; unavailable pairs are excluded explicitly. File
recall, code-region coverage and relationship-evidence coverage are proxies.
Errors remain in available-trial denominators. Ungraded answers stay null;
report graded coverage alongside success to avoid selective-grading claims.

Raw `runs/` files contain external source and answers and are gitignored. Publish
only sanitized results, reports and a manifest with local paths removed. Engine
version/source hashes, corpus hashes, repository revisions/dirty flags, index
freshness and before/after source validation support reproduction. A scripted
fixture driver verifies the protocol in tests; it is not evidence of model task
success.
