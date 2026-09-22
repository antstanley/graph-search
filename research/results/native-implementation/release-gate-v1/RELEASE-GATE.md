# Release gate decision

**Decision: fail**

Reasons:

- mean region coverage below threshold

## exact_matching_and_correctness: pass
## candidate_and_evidence: fail

| Metric | Value | Threshold |
| --- | ---: | ---: |
| Source-valid tasks | 34 | — |
| Required files | 28 | 28 |
| Complete regions | 12 | 11 |
| Mean region coverage | 0.4869 | 0.5 |
| Mean response bytes | 37471.1 | — |
| Protocol errors | 0 | 0 |

## performance: pass

| Benchmark | p50 ms | p95 ms | p99 ms | p95 ceiling |
| --- | ---: | ---: | ---: | ---: |
| build/cold_index | 684.0278 | 705.5664 | 705.5664 | 8000.0 |
| explore/body_multiword | 5.7323 | 5.7936 | 6.0873 | 250.0 |
| explore/metadata_single_term | 1.9789 | 2.0338 | 2.3442 | 100.0 |
| explore/positional_route | 2.5732 | 2.6068 | 2.6522 | 250.0 |
| lookup/exact_symbol | 1.2213 | 1.2417 | 1.2588 | 50.0 |
| lookup/references | 1.2211 | 1.24 | 1.2913 | 50.0 |
| occurrences/by_name | 1.2272 | 1.2415 | 1.2641 | 100.0 |
| scan/files_glob | 1.2994 | 1.3168 | 1.3269 | 200.0 |
| scan/filtered_explore | 5.4187 | 5.4953 | 5.6436 | 250.0 |
| scan/text_literal | 6.528 | 7.0651 | 8.4418 | 500.0 |
| sync/one_file_edit | 468.0038 | 482.8238 | 482.8238 | 4000.0 |

## resource_envelope: reported

Index bytes: 57716498; resident KiB: 452896; setup ms: 1792. resident set size is one process sample after setup, not a peak

## model_task_success: not_measured

no model driver is available in this environment; the agent protocol requires an external model with equal budgets and blind grading

Criterion reference: https://docs.rs/criterion/latest/criterion/
