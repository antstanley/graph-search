# Native positional routes: sibling workload characterization

Fixed explicit predicates, not a conceptual relevance benchmark. The production library indexed each original repository into a temporary native store. No sibling source or CodeGraph index was modified. The driver captured identical before/after hashes for production sources, experiment sources, binary, sibling Git-visible content, and all sibling CodeGraph files.

An independent batch tokenizer and interval oracle checked original byte offsets and source hashes for every delivered primary witness. It does not call the production analyzer, posting filter, positional kernel, or owner selector. Its small hand-checked tests cover Unicode byte offsets, repeated query positions, adjacency, unordered windows, and whole identifiers. Source inclusion uses the default native walk policy; repositories with a graph-search config are rejected to avoid silently changing the oracle population.

## Results

18 fixed workloads × five measured warm repetitions = 90 trials, after one warmup per workload. All 2,860 delivered primary witnesses across measured repetitions passed verification. Results were deterministic across repeats. Thirteen workloads had no truncation and returned every oracle-matching file. Five reported result/output truncation; file recall below 100% in those bounded outputs is reported rather than interpreted as a positional false negative.

| Repository | UTF-8 files | Source bytes | Whole tokens | Simple position/offset payload estimate | Native build ms |
|---|---:|---:|---:|---:|---:|
| nanus | 163 | 3,341,891 | 407,413 | 4,888,956 | 1381.1 |
| blogwright | 167 | 949,978 | 116,946 | 1,403,352 | 427.9 |
| whatsurvey | 1,224 | 8,215,575 | 884,910 | 10,618,920 | 3787.8 |

The estimate is 12 bytes per token for three uncompressed u32 values (position, start byte, end byte). It excludes term dictionaries, file headers, compression, graph/source facts, publication, and update cost. It is an illustrative payload calculation, not a storage format or an assertion that 12 bytes is optimal.

| Repository | Predicate | Warm median ms | Files scanned | Matching files returned / oracle | Reported truncations |
|---|---|---:|---:|---:|---|
| nanus | phrase: `return None` | 43.68 | 86 | 28 / 28 | none |
| nanus | phrase: `not found` | 27.34 | 54 | 5 / 5 | none |
| nanus | phrase: `pub fn` | 71.14 | 106 | 15 / 76 | results, bytes |
| nanus | phrase: `throw new Error` | 9.69 | 4 | 1 / 1 | none |
| nanus | phrase: `export async function` | 7.21 | 4 | 0 / 0 | none |
| nanus | near: `request response` | 16.54 | 28 | 9 / 9 | none |
| blogwright | phrase: `return None` | 4.86 | 8 | 0 / 0 | none |
| blogwright | phrase: `not found` | 7.80 | 30 | 9 / 9 | none |
| blogwright | phrase: `pub fn` | 3.44 | 0 | 0 / 0 | none |
| blogwright | phrase: `throw new Error` | 15.49 | 48 | 33 / 33 | none |
| blogwright | phrase: `export async function` | 11.17 | 38 | 22 / 22 | none |
| blogwright | near: `request response` | 5.10 | 14 | 0 / 0 | none |
| whatsurvey | phrase: `return None` | 63.93 | 138 | 0 / 0 | none |
| whatsurvey | phrase: `not found` | 98.66 | 123 | 59 / 76 | bytes |
| whatsurvey | phrase: `pub fn` | 29.69 | 1 | 0 / 0 | none |
| whatsurvey | phrase: `throw new Error` | 103.96 | 157 | 39 / 67 | bytes |
| whatsurvey | phrase: `export async function` | 78.61 | 128 | 74 / 74 | bytes |
| whatsurvey | near: `request response` | 84.22 | 107 | 28 / 33 | bytes |

## Decision and limits

Keep streaming verification as the native implementation for these explicit routes. It found exact source-backed witnesses under the existing work/output contracts across Rust and TypeScript-heavy workspaces. This run does not establish that persisted positions would be faster overall: service latency includes freshness inspection, walking, source reads/hashing, verification, owner lookup, graph connections, and payload fitting. No stage timing, cold-cache comparison, RSS measurement, or maintained positional-index prototype was included.

The simple additional positional payload would be 1.29–1.48 times the eligible source bytes in these repositories before headers and dictionaries. That cost and the observed sub-105 ms warm medians do not justify adding a persistent format by themselves. Revisit persistence only with an isolated verifier bottleneck and a measured query-plus-maintenance win. Common syntax queries already hit output limits; positional compression cannot solve those output/evidence-selection limits.

These are common syntax/prose predicates deliberately chosen before execution, including absent matches; they are not representative natural-language coding tasks. The run does not measure relevance, answer success, or conceptual proximity boosts. Existing evidence/task-family gates remain separate. Timing is descriptive, with sequential repository workloads and no causal baseline comparison.

## Reproduce

```sh
cargo test --offline --locked --manifest-path research/harness/Cargo.toml --bin positional_probe
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin positional_probe
python3 research/scripts/positional_review.py --output /tmp/new-positional-results
```

Use a new output directory. No build, source edit, or concurrent performance experiment should run during capture. `summary.json` contains compact results, repository JSON files retain individual timings, counters, omissions and truncations, and the provenance files retain the exact identities.
