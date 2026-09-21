# Incomplete first codec run

This attempt built the disposable release binary and the nanus index, but stopped
before codec measurements: sandbox process inspection denied `ps` with
`PermissionError: [Errno 1] Operation not permitted`. The runner exited with code
1 and closed the waiting child. No successful measurement or memory result is
claimed from this directory.

The subsequent complete run uses `--skip-process-memory` and is recorded in
[posting-codec-measured](../posting-codec-measured/README.md). Null memory fields
mean unmeasured, not zero. This first-run provenance is retained to distinguish the
failed observation from successful evidence; it must not be pooled with results.
