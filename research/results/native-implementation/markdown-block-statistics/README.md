# Paragraph/list partitioning: corpus-statistic diagnostic

This capture compares current native Markdown structure (chunker 7, source
representation 6) with fixed 80-line/eight-line-overlap windows for Markdown
files only. All code/configuration facts and the scorer are held constant. The
fixed-window arm is built from the same original bytes using native text-unit
extraction. It is a representation control, not an older production binary.

The three queries are the first source-valid task by ID in each authorized
sibling repository, selected before running the probe and retained in
`tasks.json`. Labels have previously been exposed. This is not a blinded quality
evaluation, timing experiment, or model-task-success result.

| Repository | Fixed-window documents | Structured documents | Fixed average terms | Structured average terms |
|---|---:|---:|---:|---:|
| nanus | 10,143 | 11,367 | 44.94 | 39.52 |
| blogwright | 3,902 | 4,976 | 39.16 | 30.23 |
| whatsurvey | 33,235 | 35,129 | 33.91 | 31.96 |

Counts include nonempty retrieval units across the entire indexed corpus, not
only Markdown. Finer boundaries increase the document population and reduce
average length. They also alter document frequency and overlap duplication.
The top-ranked path changes for the nanus query and stays the same for the other
two; only 7, 1 and 2 of the first 20 path positions respectively remain equal.
These differences establish that the quality gate matters; they do not establish
that either ranking is better. Multiple owner results can share a path.

For every query and both representations, the complete native body ranking is
checked against the independent exhaustive scorer: result count, path, unit and
score (absolute tolerance 0.0001). All six comparisons pass without work-budget
truncation. Additional diagnostic rankings separately substitute old IDF,
average length, or both. They are counterfactual statistics, not production modes.

`provenance.json` records the release binary, probe/driver, production files and
manifests/lockfiles. Before/after hashes match. All tracked and nonignored sibling
source files and every existing CodeGraph index file also match before/after.
The probe constructs an in-memory native index; sibling indexes are never rebuilt.

Reproduce after builds/tests finish, with production and driver files frozen:

```sh
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin body_partition_probe
python3 research/scripts/markdown_statistics.py --output /tmp/markdown-block-statistics-repeat
```

The equal-candidate/equal-final-byte context comparison remains required for
recommendations 9/25. This diagnostic does not substitute for that experiment.
