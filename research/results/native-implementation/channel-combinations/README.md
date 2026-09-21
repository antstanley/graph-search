# Native channel score-combination diagnostic

Recommendation 11 requires evaluating normalized score combinations separately
from rank fusion and representation/file diversity. This capture fills that
missing experiment; it does not close recommendation 11 or change production.

The current native indexes were built in memory from the three sibling repositories.
All 58 included tasks passed source/hash validation; 26 drifted established tasks
remain explicitly excluded. Nine predeclared policies reuse the same top-50-per-lane
scores and entity identities. Read the [frozen protocol](PROTOCOL.md) for score
semantics, zero/flat-lane handling, exact tier, tie rules and promotion criteria.

## Results

| Policy | All required files at 8 / 58 | At 50 / 58 | Gains/losses at 8 versus body |
|---|---:|---:|---:|
| body | 50 | 56 | 0/0 |
| metadata | 27 | 40 | 3/26 |
| rrf | 44 | 57 | 3/9 |
| max-0.25 | 32 | 41 | 3/21 |
| max-0.5 | 41 | 52 | 2/11 |
| max-0.75 | 48 | 56 | 0/2 |
| minmax-0.25 | 27 | 57 | 2/25 |
| minmax-0.5 | 50 | 57 | 2/2 |
| minmax-0.75 | 51 | 57 | 1/0 |

`minmax-0.75` means 75% body and 25% metadata after separate query-local min-max
normalization of each retained lane. It gains `blogwright.secret-upsert.change`
without losing any task at eight, meeting the predeclared candidate-promotion
criterion. The fixed input union contains up to 100 entities; truncating its new
ordering at 50 is not the same as enlarging a source budget or finding new facts.

The promising combination still needs a native end-to-end experiment with the
same query, work and source-payload budgets. Production currently merges indexed
and live-overlay body lanes by rank; this clean-generation capture does not
establish how a normalized-score policy should handle stale/mixed statistics.
Do not add raw scores from different corpora or treat normalized values as
correctness probabilities. No default change has been made.

`summary.json` includes repository and debug/change task-kind groups;
`changes.json` and `rankings.json` preserve every task-level gain/loss and selected
identity. These exposed convenience suites emphasize broad discovery. The single
gain is not an independent generalization result, nor does required-file presence
prove that the returned region answers the question. Multiple required regions in
one file are deliberately not counted as multiple candidate-file successes.

## Validation and reproduction

The native probe uses existing metadata/body scoring and owner identities; it
asserts neither lane exhausts its work allowance. It captures source hashes and
spans but does not copy source bodies. Python boundary checks cover empty lanes,
flat min-max lanes, missing-channel candidates and protected exact tiers. Strict
probe Clippy and release/offline/locked build pass. All 162 captured input hashes
match, as do before/after sibling-source and every sibling CodeGraph-file hash.
No production file or Cargo dependency changed. All commands are terminal; see
`checks.json`, `stability.json`, `before.json`, and logs.

```sh
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin channel_scores_probe
python3 research/scripts/channel_combinations.py capture
python3 research/scripts/channel_combinations.py analyze
```

The commands overwrite this diagnostic's capture files; archive existing results
before rerunning against changed sources. Historical comparisons require the
recorded source identities. Candidate/context and actual model-task-success gates
remain open under recommendations 11/12/30.
