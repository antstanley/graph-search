# Native generation reader leases

**Fixed:** a live reader could lose its lazy extraction records when two newer
generations were published. Current/previous retention alone protected neither
older independent handles nor their later manifest reads. The new regression
failed with `No such file or directory` before the fix; see [before.txt](before.txt).

## Native protocol

Each published store owns one read-only file handle with a shared OS lock on its
generation's existing mandatory `dangling.jsonl` sidecar. That sidecar is written
as a fresh file for each generation; source/extraction pack hard-link reuse does
not share its lease identity. This needs no new persisted file, dependency,
unsafe code, format change or reader-side write permission.

Selection acquires the lease before reading/checking generation artifacts. A
prepared writer also acquires it before publishing CURRENT. `GrafeoStore` keeps
the handle until its graph/fact fields have been dropped, covering lazy packed
extraction reads throughout its lifetime. Borrowed snapshots cannot outlive it.

Reclamation keeps current and previous generations, then tries an exclusive
nonblocking lease for each older directory. It removes paths only while holding
that exclusive lease. Lock contention and other open/lock errors skip cleanup.
An unfinished directory missing the mandatory sidecar cannot have an admitted
reader and may be removed. Skipped cleanup is retried by a later publication.
Normal process exit and exit without Rust destructors both release OS leases.
The underlying primitives are Rust's native
[file locks](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock_shared).

A reader can read CURRENT just before its selected directory is reclaimed. On a
selection error, it retries only if CURRENT's bytes demonstrably changed. A
stable checksum/format/locking error is returned immediately. After eight failed
selections with changing pointers, opening returns a retryable error. No polling
thread, lock-file cleanup daemon, PID-liveness heuristic or indefinite wait is
introduced.

## Correctness argument and test scope

Premises: published artifacts are immutable; generation names are not reused;
writers/publication cleanup are serialized by the existing writer contract;
all participating binaries use this lease protocol.

1. If the reader obtains a shared lease first, cleanup cannot acquire exclusive
   ownership. All lazy paths remain present while the reader is admitted.
2. If cleanup obtains exclusive ownership first, reader acquisition fails or
   subsequent validation sees retired paths. The changed CURRENT observation
   permits a bounded retry; no incomplete selection becomes a store handle.
3. If opening/validation fails after acquiring a lease, ordinary ownership drops
   its file handle. A selected lease moves into the store without a release gap.
4. Failed preparation cannot publish an unleased handle: writer pin acquisition
   is before the pointer rename. Existing failure/crash tests still cover this
   path along with checksummed graph, source, occurrence and extraction state.
5. Releasing one reader does not release another independently opened reader's
   lease. Only the last lease release permits subsequent reclamation.

New regression coverage:

- A reopened reader and an unchanged prepared writer both retain original graph
  and lazy manifest facts through five publications. Cleanup returns to two
  directories after both are dropped and another publication occurs.
- Two **processes** hold the same old generation through eight publications,
  checking original nodes and lazy extraction facts each time. The parent changes
  both graph names and manifest/reference facts. Normal exit of one process
  leaves retention in place; the other's deliberate `process::exit(87)` releases
  the final lease without executing Rust destructors. Next publication reclaims
  the old directory. During this fixture, at most three generations are retained.
- A deterministic race retires the observed pointer's directory before selection
  can pin it; selection retries successfully. The selected lease then protects its
  directory even before a complete store is loaded.
- Stable errors are attempted once; perpetual pointer change stops after eight
  attempts with `WouldBlock`.

The process helpers use a bounded readiness/check protocol, and parent-side
cleanup kills/reaps helpers if an assertion fails. They exercise live retained
handles during publication, not a production throughput benchmark or proof of
all scheduler interleavings. Platform validation in this capture is macOS; no
claim of tested Linux/Windows execution is made.

## Resource and contract limits

There is one extra file descriptor per persistent store handle. Independently
opened stores still own their graph/index memory; this change does not introduce
shared in-process snapshots. Disk retention is current + previous + distinct
leased generations, plus any cleanup failures or unpublished preparation state.
Arbitrarily many deliberately retained generations can therefore retain
arbitrarily much history. Existing pack hard links may share bytes across those
directories; logical directory sizes must not be summed as physical usage.
Closing readers makes their history reclaimable at the next publication, not
immediately. No universal disk cap or measured RSS reduction is claimed.

`GraphStore: Send` and the borrowed snapshot/`Index` lock contract are unchanged.
This does not make one `Index` `Sync`, automatically refresh an existing reader,
or synchronize with older writers that do not implement leases. Unsupported
filesystem locking is an explicit opening error, not a silent loss of protection.
Arbitrary external deletion/replacement of published files is outside the
cooperative publication protocol; corruption checks remain applicable.

Recommendation 28 remains open for broader churn/retention resource measurements
and the mutation matrix. This increment closes the reproduced reader-lifetime
failure and establishes the independent-handle retention contract.

## Reproduction

```sh
cargo test -p graph-search-engine
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The captured workspace test, strict lint and source-hash records accompany this
report: **444 tests across 33 suites pass**, strict workspace/all-target Clippy
passes, and formatting/diff checks pass. Every crate source still matches the
recorded hashes after validation. [change.patch](change.patch) isolates this
increment from the larger uncommitted implementation. No sibling repository or
CodeGraph index is modified by these tests.
