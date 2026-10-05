# Accuracy benchmark runs

[`SUMMARY.md`](SUMMARY.md) holds the pooled results. The run directories are
kept locally and are not committed (they carry symbols, paths and doc text from
the measured repositories); regenerate them with `run.py`.

Each directory is one `run.py` invocation: `<repo>.json` holds the metrics,
`<repo>.failures.json` every miss with its query and top answers,
`REPORT.md`/`summary.json` the pooled report (`report.py`), and `repos.json`
the repositories the run was given (absolute local paths). Repositories
removed from the machine between runs are absent from later runs.

| Run | Code | Oracle | Notes |
|---|---|---|---|
| `2026-10-05` | `6fc3143` | first version | sidekick and railblocks indexed with directories excluded (an indexing abort, fixed in `1bdf88a`); security-scans left out of pooled figures |
| `2026-10-05-fixes` | `1bdf88a` | first version | no exclusions needed |
| `2026-10-05-baseline` | `1bdf88a` | reads more Rust macros (`87adf2d`) | baseline for the comparison below |
| `2026-10-05-round2` | `b82455e` | reads more Rust macros (`87adf2d`) | compare with `-baseline` |

Compare `-baseline` with `-round2` only: they share the oracle and sampling.
The first two runs used an oracle that could not see calls inside `select!`,
`proptest!` and inline-snapshot macros.
