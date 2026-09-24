# Agent instructions

## Benchmarking

**Always use [Criterion](https://docs.rs/criterion) for benchmarks.** Never
time code with ad-hoc `Instant::now()` probes, throwaway binaries or shell
timing, and never report a performance change that a Criterion run did not
measure.

- Benchmarks live in `crates/graph-search/benches/` and are registered as
  `[[bench]]` targets with `harness = false` in
  `crates/graph-search/Cargo.toml` (see `storage.rs`, `search.rs`,
  `evaluation.rs`, `okf.rs` for the pattern). Criterion is a dev-dependency
  only.
- Run one with `cargo bench -p graph-search --bench <name>`.
- To measure a change, save a baseline on the old revision and compare the new
  one against it, sharing one `CARGO_TARGET_DIR` so Criterion finds the
  baseline:

  ```sh
  export CARGO_TARGET_DIR=/tmp/bench-target
  # on the old revision (e.g. a `git worktree` of it)
  cargo bench -p graph-search --bench <name> -- --save-baseline before
  # on the new revision
  cargo bench -p graph-search --bench <name> -- --baseline before
  ```

- Report Criterion's estimate and its change verdict (including "No change in
  performance detected" and regressions), not a single hand-picked timing.
