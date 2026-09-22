# Release gate decision

**Decision: conditional_pass**

## exact_matching_and_correctness: pass
## candidate_and_evidence: pass

| Metric | Value | Threshold |
| --- | ---: | ---: |
| Source-valid tasks | 34 | — |
| Required files | 28 | 28 |
| Complete regions | 12 | 12 |
| Mean region coverage | 0.4869 | 0.48 |
| Mean response bytes | 37469.1 | — |
| Protocol errors | 0 | 0 |

## performance: pass

| Benchmark | p50 ms | p95 ms | p99 ms | p95 ceiling |
| --- | ---: | ---: | ---: | ---: |
| build/cold_index | 682.2835 | 684.8731 | 684.8731 | 8000.0 |
| explore/body_multiword | 5.7435 | 5.8229 | 5.9255 | 250.0 |
| explore/metadata_single_term | 1.9738 | 1.9951 | 2.0097 | 100.0 |
| explore/positional_route | 2.5668 | 2.5855 | 2.6348 | 250.0 |
| lookup/exact_symbol | 1.218 | 1.2358 | 1.2756 | 50.0 |
| lookup/references | 1.2252 | 1.2483 | 1.3514 | 50.0 |
| occurrences/by_name | 1.2261 | 1.2461 | 1.2774 | 100.0 |
| scan/files_glob | 1.2975 | 1.3121 | 1.337 | 200.0 |
| scan/filtered_explore | 5.4221 | 5.5118 | 5.6044 | 250.0 |
| scan/text_literal | 6.5695 | 6.632 | 6.8181 | 500.0 |
| sync/one_file_edit | 468.5517 | 481.2825 | 481.2825 | 4000.0 |

## resource_envelope: reported

Index bytes: 57716495; resident KiB: 453408; setup ms: 1715. resident set size is one process sample after setup, not a peak

## model_task_success: not_measured

no model driver is available in this environment; the agent protocol requires an external model with equal budgets and blind grading

Criterion reference: https://docs.rs/criterion/latest/criterion/
