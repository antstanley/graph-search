# Natural-language retrieval experiments

The main findings and implementation review are in
[../09-natural-language-retrieval.md](../09-natural-language-retrieval.md).
`PROTOCOL.md` records selection rules and later corrections. The comparison base
is `bdcbecef41d9863bca23830ed30615f1c219064d` (merged PR #2).

## Reproduce

Requires Python 3.12+, Rust compatible with the workspace, cached Cargo
packages, and read access to `~/code/nanus`, `~/code/blogwright`, and
`~/code/whatsurvey` at the recorded source snapshots. The new experiments do
not require CodeGraph or modify those repositories or their indexes. Existing
CodeGraph/text task comparisons remain in `evaluation/results/`.

```sh
# Build both pinned baseline and current binaries and dump baseline candidates.
python3 research/natural-language/reproduce.py --prepare
# Choose content fields, then test expansion/diversity separately and combined.
python3 research/natural-language/reproduce.py --development
# Explicitly inspect the frozen confirmation set and run actual engine probes.
python3 research/natural-language/reproduce.py --confirm --public
```

Run these sequentially. The public stage runs the baseline and current engines
on 354 identical requests each, followed by 60 tasks per public-library arm.
Expect several minutes in debug mode. Builds use an external temporary target
directory (override with `--target`). Remove `--offline` in the script deliberately
if Cargo dependencies are not cached. Baseline source comes from `git archive`,
not the current `main` branch. Current source is the working tree being tested.
Taskbench refuses to overwrite an existing run; move an old
`evaluation/runs/natural-language-*-reproduction` directory before repeating.

`public_probe.py --baseline PATH --current PATH` also runs independently after
preparation. It copies the executables into a temporary directory, checks their
hashes, checks parser versions, validates the 60 task fixtures before/after, and
compares complete repository source snapshots before/after. It uses the core
query engine directly. The separate taskbench runs exercise public `Index`,
persistent temporary stores, freshness, output budgets, and the evidence-read
protocol. The latter calls are not model/agent trials.

## Evidence index

- `documentation-queries.json`: nine frozen documentation prompts with source hashes.
- `results/fields.json`: metadata, separate comments/bodies/docs, and all fields;
  weights 0.25, 0.5, 1.0. Full hits and source-prefix hashes retained.
- `results/expansion-diversity.json`: the selected body field, expansion alone,
  and diversity alone. `combined.json` then tests both together.
- `results/field-selection.json`, `final-selection.json`: development decisions.
- `results/confirm.json`: unchanged selected prototype vs metadata on 24
  previously published confirmation tasks; not a secret generalization set.
- `results/development-evidence-proxy.json`: development rankings run through
  the evidence-read protocol with a prototype backend, not the public engine.
- `results/frozen-public-core.json`: final baseline/current frozen-query
  comparison, source snapshots, executable hashes, truncations, all hits/misses.
- `results/public-index-*-{manifest,results,report}.json`: separate public-library
  task runs, with evidence coverage and null task-success fields.
- `results/public-index-profile.json`: sequential debug latency and live store
  size for three exact and three development task prompts per repository, each
  repeated three times. This is a small local profile, not a release benchmark.
- `results/rejected-split-priority.json`: partial failed production attempt;
  exact names survived but split names collided with individual-name priority.
- `results/pilot/`: original development-only documentation normalization bug.
  Corrected before prototype confirmation; retained to avoid hiding failures.

Raw graph dumps and source-bearing responses stay under ignored `private/` and
`evaluation/runs/`. Committed results contain candidate names, paths, source
hashes, ranks, timing and sanitized evidence metrics, not external source bodies.
Original pilot timings and the final core-probe timings include concurrent
local build/research work and must not be treated as controlled performance
comparisons. The task comparison also overlapped the core probe. A separate small sequential
public-library profile reports latency and store size without these jobs running.

## Scope of the prototype

Candidate symbols come from the same baseline graph for every arm. Leading
comments use a deliberately approximate line recognizer. Bodies use whole lines
from the symbol's line span; production uses exact byte spans, so sibling
functions sharing a line are separated correctly. Body bounds include the
function declaration, in-body comments and string literals. Documentation uses
64-line/4,096-character passages from at most 64 KiB per indexed file, after the
normal 1 MiB source-file walk limit. Symbol fields use independent BM25 statistics
over the symbol corpus; documentation has its own passage corpus. Scores merge
at the selected weights. Generic inflection expansion adds matching corpus
variants at half query weight; there is no learned synonym map.

File recall is the selection metric, with reciprocal rank as tie-breaker, then
smaller field weight. A tie between diversity factors keeps the first declared
factor (0.5). Symbol-span overlap is diagnostic and is not the same as delivered
source-line coverage. Evidence-ready means the fixed retrieval/read protocol
returned all required lines, not that an agent solved the task. No provider
model was run and no answer-success improvement is claimed.
