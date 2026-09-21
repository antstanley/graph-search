# Recommendation 23 workload matrix

Release-mode native Grafeo, synthetic Rust fanout, 16 and 64 base files, three trials per case. Each non-target file contains one caller and 16 helper declarations. Every trial reopens the sync result and compares graph nodes, edges, source units, occurrences and full manifest entries with an explicit reindex. All **48/48** comparisons passed.

Timings below are medians in milliseconds, with observed min–max in parentheses. Reindex follows sync on the same warm state; case order is fixed, the sample is small, and these are not production p95 estimates or a before/after old-implementation speedup. All agent-launched tests/builds finished before measurement.

| Base files | Workload | Sync ms (range) | Reindex ms (range) | Requested unchanged facts | Upserted files | Explicitly retained records |
|---:|---|---:|---:|---:|---:|---:|
| 16 | noop | 0.36 (0.33–0.44) | 65.12 (63.42–68.14) | 0 | 0 | 0 |
| 16 | body | 76.89 (59.46–81.63) | 83.94 (70.50–122.30) | 0 | 1 | 15 |
| 16 | public_api | 69.20 (68.80–76.96) | 73.33 (72.74–113.18) | 15 | 16 | 0 |
| 16 | rename | 73.22 (70.26–91.68) | 82.61 (77.59–87.12) | 15 | 16 | 0 |
| 16 | delete | 62.98 (59.20–67.36) | 66.76 (66.19–67.88) | 15 | 15 | 0 |
| 16 | duplicate_add | 72.27 (68.24–105.41) | 79.93 (75.62–111.94) | 15 | 16 | 1 |
| 16 | duplicate_remove | 67.62 (61.93–93.94) | 74.84 (59.13–83.17) | 15 | 15 | 1 |
| 16 | missing_cache | 76.46 (75.76–80.40) | 76.74 (75.09–84.85) | 15 | 16 | 0 |
| 64 | noop | 0.63 (0.62–1.72) | 160.51 (156.35–226.27) | 0 | 0 | 0 |
| 64 | body | 188.70 (128.32–200.09) | 244.70 (202.14–423.78) | 0 | 1 | 63 |
| 64 | public_api | 205.16 (177.40–324.03) | 218.62 (194.37–236.76) | 63 | 64 | 0 |
| 64 | rename | 172.10 (151.15–198.59) | 169.03 (155.01–181.21) | 63 | 64 | 0 |
| 64 | delete | 169.23 (137.01–173.87) | 200.94 (156.48–214.46) | 63 | 63 | 0 |
| 64 | duplicate_add | 141.57 (140.81–159.98) | 191.20 (157.09–210.26) | 63 | 64 | 1 |
| 64 | duplicate_remove | 132.88 (132.35–136.59) | 156.39 (146.58–164.23) | 63 | 63 | 1 |
| 64 | missing_cache | 151.63 (141.43–170.10) | 202.02 (158.54–253.13) | 63 | 64 | 0 |

A body edit requested **zero unchanged raw facts and one file upsert** at both sizes. The 64-file body case measured 188.70 ms sync versus 244.70 ms reindex. No-op measured 0.63 ms versus 160.51 ms. Binding-changing workloads correctly repair the high-fanout consumers. Missing-cache repair conservatively rebuilds every file. The 64-file rename median was slightly slower than reindex (172.10 versus 169.03 ms); selective facts do not eliminate full generation preparation.

The raw JSON key `files_before` denotes the configured base corpus size. `duplicate_remove` starts with one extra duplicate file; `duplicate_add` ends with one extra file. “Requested unchanged facts” counts requested paths, including the unavailable cache in the missing-cache case; it is not a byte-read count. No-op publishes nothing, so explicit-retention count is zero although all prior records remain in place.

The first 48-trial execution also completed all comparisons, but `/usr/bin/time -l` exited 1 because the sandbox denied `sysctl kern.clockrate`. Its JSON and diagnostic are retained. The direct rerun exited 0 and is the reported dataset. Per-workload timings come from native `Instant` measurements. Peak RSS was not obtained; no memory-improvement claim is made.

These results satisfy the original eight-workload correctness/benchmark gate. They identify remaining full graph/occurrence/retrieval-index work without making it a new requirement of recommendation 23.
