# Native authored-link metadata: corpus validation

This is the final capture for source representation 7/chunker 8, including the
URI-validation work reservation. The [earlier capture](../markdown-link-statistics-initial/README.md)
predates that reservation; all result JSON values are identical, but source and
binary provenance distinguish the implementations.

| Repository | Recognized distinct link spans | Files with link fields | Files reaching a link cap |
|---|---:|---:|---:|
| nanus | 126 | 13 | 0 |
| blogwright | 166 | 25 | 0 |
| whatsurvey | 113 | 8 | 0 |

All 405 recognized link descriptors address current original UTF-8 source bytes.
The probe verifies the full-file hash and field slice boundaries; production
publication also applies semantic coordinate, ordering and record-cap validation.
There are no duplicate overlapping-window link records in these three captures.
This is not an independent full-Markdown conformance/recall evaluation: reference,
email and multiline links and recursive inline parsing are outside the declared
initial dialect. Zero capped files does not mean every possible Markdown link was
recognized.

The same source-valid query in each repository is evaluated against both current
structure and native fixed Markdown windows. All six complete body rankings match
the independent exhaustive scorer (same path/unit/order and absolute score
tolerance 0.0001), without work truncation. This is a corpus/scorer diagnostic,
not a fixed-response-budget context or model-success evaluation.

Compared with [the prior block capture](../markdown-block-statistics/README.md),
nonempty structured units change from 11,367 to 11,334 in nanus, 4,976 to 4,973 in
blogwright, and 35,129 to 35,127 in whatsurvey. Inline-link/autolink lines no longer
automatically become opaque blocks. On the same unchanged sibling sources, 16/20,
20/20 and 20/20 top candidate path/owner positions respectively stay equal. Scores
and unit ordinals may differ. No relevance gain is inferred from these changes.

`provenance.json` captures the release binary, probe/driver, production sources,
manifests and lockfiles. Before/after hashes match. Tracked and nonignored sibling
sources and every existing CodeGraph index file also match. Native indexing occurs
in memory; sibling indexes are not rebuilt. Raw repository text is not copied into
these result artifacts.

Reproduce with production/driver files frozen and no concurrent builds/tests:

```sh
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin body_partition_probe
python3 research/scripts/markdown_statistics.py --output /tmp/markdown-link-statistics-repeat
```
