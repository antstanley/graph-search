# Resident memory of retained generation processes

All **24 runs passed**, with **42 checks of retained reader facts**. Source and
binary hashes remained unchanged. This complements the mutation/disk experiment
in [generation-churn](../generation-churn/README.md); it is a separate capture,
not an additional measurement retroactively attached to those timed runs.

## Method

The same native release probe creates disposable Rust sources at 64 files × 8
functions and 256 files × 16 functions. Four arms hold zero readers, one reader,
three readers of one generation, or three readers of distinct generations.
Three repeats rotate arm order. Eight comment edits force source generations;
distinct readers open at steps 0, 2 and 4. All first manifest checks occur after
step 8, so they read lazy extraction data after newer publications. Original
node/edge, source, occurrence and extraction fingerprints must match. Readers
then exit individually, each followed by a publication. Every run returns to two
generation directories after all readers exit.

The driver calls `/bin/ps -o rss= -p PID` only for its own live child processes.
Samples occur after writer construction, each reader opening, churn before lazy
manifest checks, after checks, and after reader release/publication. The host is
macOS; RSS units reported by ps are KiB and are converted to bytes. The driver
requires macOS or Linux, but this capture does not claim Linux validation.
The automatic approval review allowed this scoped process inspection. No other
process is inspected and no sibling repository/index is used or changed.

## Interpretation

RSS is resident process memory, not live Rust allocation size. Graph/index
ownership, mapped pages, allocator retention, temporary fingerprint buffers and
OS paging all affect it. The fingerprint operation serializes facts into
intermediate values; its post-check RSS must not be attributed solely to lazy
manifest decoding. Summing reader RSS can count shared mapped pages repeatedly;
it is **not physical memory consumption**. These are samples, not peak RSS or a
heap census. Variation between arms is not an isolated treatment effect.

Independent handles own graph and derived index state. Readers sharing an old
disk generation do not gain an in-process shared snapshot. Disk reclamation after
release does not promise that a still-running writer's allocator returns pages
to the OS. Arbitrarily many reader handles have no global memory cap.

## Sampled results

Medians across three repeats; writer RSS is sampled after churn.

| Files | Reader arm | Writer RSS MiB | Summed reader RSS before check MiB | After check MiB |
|---:|---|---:|---:|---:|
| 64 | none | 35.12 | 0.00 | 0.00 |
| 64 | one | 34.02 | 14.39 | 21.73 |
| 64 | distinct | 34.23 | 43.28 | 65.56 |
| 64 | shared | 35.48 | 43.27 | 65.61 |
| 256 | none | 204.52 | 0.00 | 0.00 |
| 256 | one | 190.52 | 32.78 | 104.69 |
| 256 | distinct | 189.58 | 160.53 | 291.62 |
| 256 | shared | 197.70 | 129.05 | 295.55 |

## Reproduction

```sh
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin generation_churn_probe
python3 research/scripts/generation_memory.py /tmp/generation-memory
python3 research/scripts/generation_memory_summary.py /tmp/generation-memory
```

`environment.json` records source, driver, executable, Python and platform
identities. `runs.json` retains every sample; `validation.json` certifies all
runs and frozen inputs. No build/test runs concurrently with measurement.
