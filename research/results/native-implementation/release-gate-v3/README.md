# Release gate decision (v3)

`research/scripts/release_gate.py` is the release decision mechanism: it runs the
workspace tests and strict Clippy, validates the task labels against the sibling
repositories, executes the evidence-v1 protocol on the source-valid development
tasks, runs the Criterion benchmark suite
(https://docs.rs/criterion/latest/criterion/), probes one real repository for
index bytes and resident size, and applies predeclared thresholds.

```sh
python3 research/scripts/release_gate.py --output <new-directory>
```

## Decision: conditional_pass

| Objective | Status | Notes |
|---|---|---|
| Exact matching and correctness | pass | 564 workspace tests, strict workspace/all-target Clippy, 34 source-valid tasks, zero protocol errors |
| Candidate and evidence | pass | 28/34 required files, 12/34 complete regions, 0.4869 mean region coverage (thresholds 28 / 12 / 0.48) |
| Performance | pass | 11 Criterion benchmarks, all p95 under the frozen ceilings |
| Resource envelope | reported | nanus: 57,716,498 index bytes, 450,000 KiB resident (one sample), 1,745 ms setup |
| Model task success | not measured | No model driver is available here; the agent protocol requires an external model with equal budgets and blind grading |

Because the model objective cannot be measured in this environment, the decision
is explicitly **conditional**: the gate refuses to claim a full release on the
measured objectives alone. The protocol is deterministic (repeated evidence
distributions are identical), so one repeat is used by default; `--repeats 3`
restores the multi-repeat form.

## Measured performance (p50 / p95 / p99, milliseconds)

| Benchmark | p50 | p95 | p99 | p95 ceiling |
|---|---:|---:|---:|---:|
| build/cold_index | 679.0 | 717.2 | 717.2 | 8,000 |
| sync/one_file_edit | 463.8 | 474.6 | 474.6 | 4,000 |
| lookup/exact_symbol | 1.22 | 1.28 | 1.43 | 50 |
| lookup/references | 1.23 | 1.24 | 1.52 | 50 |
| explore/body_multiword | 5.92 | 6.63 | 7.16 | 250 |
| explore/metadata_single_term | 2.27 | 2.36 | 2.42 | 100 |
| explore/positional_route | 2.99 | 3.09 | 3.13 | 250 |
| occurrences/by_name | 1.23 | 1.26 | 1.28 | 100 |
| scan/text_literal | 6.56 | 7.03 | 8.25 | 500 |
| scan/files_glob | 1.31 | 1.34 | 1.45 | 200 |
| scan/filtered_explore | 5.44 | 5.51 | 5.67 | 250 |

## Limits

The benchmark corpus is generated (200 Rust files with 24 functions each, one
Markdown guide per file, plus TypeScript and Svelte fixtures), so these are
regression ceilings for this implementation on a fixed workload, not service-level
objectives or external-repository latency. The resource probe reports one resident
sample, not a peak. Excluded tasks are listed in the decision record: 26 of the 60
suite tasks fail source validation today (most because the sibling repositories
have moved past their labeled revisions), which is why the evidence thresholds are
absolute counts on the remaining 34 tasks. Refreshing those labels requires
explicit source review and is not automated here. A full release decision requires
an external model-success run with blind grading.

`provenance.json` records production source hashes and the gate hash;
`calibration-pre21.json` in the sibling v1 directory records the baseline used to
freeze the accuracy thresholds.
