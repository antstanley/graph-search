# Agent instructions

## Benchmarking

**Always use [Criterion](https://docs.rs/criterion) for benchmarks.** Never
time code with ad-hoc `Instant::now()` probes, throwaway binaries or shell
timing, and never report a performance change that a Criterion run did not
measure.

- Benchmarks live in `crates/graph-search/benches/` and are registered as
  `[[bench]]` targets with `harness = false` in
  `crates/graph-search/Cargo.toml` (see `storage.rs`, `search.rs`,
  `evaluation.rs`, `okf.rs`, `sync.rs` for the pattern). Criterion is a dev-dependency
  only.
- Run one with `cargo bench -p graph-search --bench <name>`.
- To measure a change, save a baseline on the old revision and compare the new
  one against it. Give each revision its **own** `CARGO_TARGET_DIR` and share
  only Criterion's output through `CRITERION_HOME`. A shared target directory
  is unsafe: cargo can treat the other checkout's binary as fresh and run the
  old code for both halves of the comparison.

  ```sh
  export CRITERION_HOME=/tmp/criterion
  # on the old revision (e.g. a `git worktree` of it)
  CARGO_TARGET_DIR=/tmp/bench-base cargo bench -p graph-search --bench <name> -- --save-baseline before
  # on the new revision
  CARGO_TARGET_DIR=/tmp/bench-head cargo bench -p graph-search --bench <name> -- --baseline before
  ```

  Check that the two runs really differ (for example, a store size or result
  the change should move) before trusting a "no change" verdict.

- Report Criterion's estimate and its change verdict (including "No change in
  performance detected" and regressions), not a single hand-picked timing.
