# Configured store exclusion

A valid custom in-tree store was being indexed as source. The initial four-file
whatsurvey setup exposed this; a minimal regression then reproduced it with one
Rust file: the first build also admitted `index-store/index.lock` and the engine
WAL. [Before capture](before.txt) records that failure.

## Native fix

`Index::open` resolves the actual store path before opening the engine, preserving
explicit relative-option and root-relative configuration semantics. Existing
symlinks are resolved; missing trailing components may be created. A store equal
to or containing the source root is rejected before opening the engine.

The configured store adds one absolute subtree to `WalkPolicy.excluded_paths`.
The shared walker prunes that subtree before reading source or descending. This
applies to indexing, incremental checks, files, text and live body discovery.
Query hidden/ignore overrides clone the same exclusion. Scoped search roots are
resolved once and exclusions translated to the walker path, covering symlink and
parent-component aliases without per-entry filesystem resolution. Other directories sharing
the basename remain searchable; external stores leave source siblings searchable while remaining excluded if
explicitly selected as a search scope.

The fingerprint binds exact OS path bytes (including non-UTF-8 names), while
coverage binds them through that fingerprint. `Index::policy` exposes the paths. Policy mismatch triggers a full source rebuild,
so formerly admitted storage facts are removed. No representation version bump
is needed: the inclusion-policy fingerprint itself authorizes that rebuild.
No new dependency is introduced.

## Verification certificate

Premises: all library source enumeration goes through the shared walk policy;
the opened root and store use resolved absolute paths. The writer lock resides
inside the store. Generation publication also remains within that subtree.

Before: `Index::open` creates storage; the name-only walker admits a custom store;
reconciliation publishes more files which freshness subsequently sees as source.
After: the precise store subtree is pruned before enumeration, and its contents
cannot enter the source manifest. Freshness and queries reuse that same policy.

Regression coverage includes clean build, stable no-op generation and reopening,
read-only reopening, hidden/no-ignore queries, direct store-scoped file search,
text and explore source paths, real source edits, same-named source siblings,
external storage, symlinked roots/stores, missing and parent path components,
configuration replacement of default exclusions, ancestor/equal-store rejection,
and rebuilding an already contaminated projection.

The path is fixed for the lifetime of an opened index. Concurrent replacement of
filesystem symlink targets is outside this contract. Core-only projector hosts
remain responsible for supplying their storage exclusions in `WalkPolicy`.

An intermediate run caught a context-budget regression from adding absolute
exclusion paths to every coverage payload. That redundant wire field was removed;
the existing fingerprint carries policy identity without spending evidence space.
The unchanged body/context suite is part of final validation.

## Evidence

- `workspace.txt`: workspace tests (final status recorded in `checks.json`).
- `clippy.txt`: strict workspace/all-target lint result.
- `sources.json`, `change.patch`: exact source hashes and isolated change against
  the preceding Node-package increment.
- `source-probe.json`: four original whatsurvey files copied into a disposable
  root, now with an **in-tree** store, preserving four indexed files through
  mapping removal/restoration. Original source bytes are checked unchanged.

This increment fixes the observed blocker; it does not complete the larger
quality, invalidation, workspace-resolution or release gates in IMPLEMENTATION.md.

## Final results

466 workspace tests passed across 36 suite reports. Strict workspace/all-target
Clippy, formatting and whitespace checks pass. Clippy requested a test-only
qualification of `StoreOptions::default()` after the workspace run; production
was unchanged and all six store tests were rerun successfully afterward.
All 137 crate-file hashes match the final capture. Dependency files are unchanged.

The in-tree whatsurvey replay passes in all three states: mapped, mapping removed
in the copy, and mapping restored. Each state contains exactly four indexed files
and two call occurrences. Original source hashes and the CLI binary hash remain
unchanged during the probe. This is a correctness fixture, not a latency or
repository-wide search-quality measurement.

Reproduce with `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, and:

```sh
python3 research/scripts/node_package_source_probe.py --repo ../whatsurvey \
  --cli target/debug/graph-search --in-tree-store \
  --out /tmp/store-source-probe.json
```
