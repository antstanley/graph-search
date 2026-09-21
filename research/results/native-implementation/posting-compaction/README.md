# Native index composition and rejected posting compaction

**Decision: withdraw unconditional `shrink_to_fit` for immutable posting lists.**
The candidate preserved checked contents and rankings and reduced vector
capacity, but did not demonstrate a reliable process-memory saving. It added
maintenance cost. Production files were restored to the measured baseline;
this is research evidence, not a shipped memory optimization or codec.

## Actual hot posting composition

The disposable instrumentation reads private index fields after reopening an
independent native store for each authorized sibling repository. It accounts
for both metadata analyzers and both body analyzers. All figures below are
decimal MB; raw byte counts are in the JSON artifacts.

| Repository | Live posting payload | Allocated vector capacity | Unused capacity | Dictionary UTF-8 bytes | Metadata norms |
|---|---:|---:|---:|---:|---:|
| nanus | 20.70 MB | 30.26 MB | 9.55 MB | 0.319 MB | 0.834 MB |
| blogwright | 7.33 MB | 11.69 MB | 4.37 MB | 0.237 MB | 0.288 MB |
| whatsurvey | 57.98 MB | 87.29 MB | 29.30 MB | 0.663 MB | 2.488 MB |

On the measured 64-bit ABI, metadata postings occupy 48 bytes: an 8-byte
document ordinal, a 4-byte weighted frequency, two 16-byte field-frequency
arrays, and padding. Body postings occupy 24 bytes: document ordinal,
frequency and line anchor, including padding. The ordinary metadata lane's
whole-frequency arrays are all zero. Those arrays still occupy space.

Lists of length at most four account for 15,366/24,645 lists in nanus,
28,975/35,727 in blogwright and 56,424/76,616 in whatsurvey. The raw artifacts
also report the entry counts in each bucket: list prevalence must not be
mistaken for prevalence among posting entries.

Source-region facts retain repeated terms and absolute **line occurrences**,
not a token-position stream. Their line-vector length/capacity is respectively
3.43/9.30 MB, 1.08/3.09 MB and 8.10/21.67 MB. No raw source-text blob is retained
in these native postings. Exact-name maps, adjacency ordinals, node/edge inline
capacity and serialized source/occurrence sizes are reported separately.

**Accounting limits:** vector length/capacity and ABI sizes are exact; the sum
is not total heap usage. BTree node allocation/slack, allocator metadata,
String spare capacity, Grafeo internals and nested node/edge/occurrence strings
are not fully accounted. JSON sizes are explicitly marked `not_heap` and must
not be added to native-memory totals. Arc headers and Vec headers overlap as
described in the records. The diagnostic traversal itself allocates temporary
data, so resident memory is sampled while the child waits **before** it runs.

## Candidate and outcome

[candidate.patch](candidate.patch) compacts vectors at initial metadata/body
construction and after rebuilding changed metadata lists. Unchanged Arc lists
remain shared. Only those three production files differ between frozen builds.
No representation version, ranking rule, dependency or persisted format changes.

The compacted capacity equals the live payload in all four lanes on all three
repositories. Every other reported composition field is identical. That is an
allocation-accounting result, not proof that the allocator returned memory to
the operating system.

| Repository | Baseline median RSS | Candidate median RSS | Baseline median open | Candidate median open |
|---|---:|---:|---:|---:|
| nanus | 248.55 MB | 247.14 MB | 788.2 ms | 879.8 ms |
| blogwright | 93.63 MB | 93.90 MB | 252.0 ms | 280.3 ms |
| whatsurvey | 590.36 MB | 629.26 MB | 2246.6 ms | 2252.1 ms |

These are three fresh processes per capture, baseline capture followed by
candidate capture. They are diagnostic, not randomized paired latency or
RSS-effect estimates. In particular, whatsurvey's baseline RSS ranges from
581.75 to 632.80 MB and overlaps the candidate range. The correct conclusion
is **no reliable resident-memory win demonstrated**, not a proven 39 MB
memory regression. Open/build work includes graph/fact loading and native
index construction. `time -l` peak figures include subsequent diagnostics and
must not be substituted for the pre-diagnostic `ps` samples.

The separate synthetic maintenance experiment alternates whole-process arm
order across three repeats. Each process warms each case, then performs five
alternating full/delta pairs for 1,000 and 50,000 symbols. Input cloning is
included; disk/graph publication is excluded. At 50,000 symbols, pooled median
full rebuild cost rises 3.09–4.17%; delta cost rises 0.67–8.18%, with the largest
increase for insertion before existing ordinals. At 1,000 symbols, that early
insertion delta rises 12.51%. Repeats are timing samples, not independent tasks.
See [maintenance-comparison.json](maintenance-comparison.json) for every case.

This evidence does not justify unconditional compaction. It also does not prove
that tighter allocation, a different representation or longer-lived allocator
reuse could never help. Those are distinct candidates with their own gates.

## Verification and next decisions

- Candidate: 164 core tests and 48 public integration tests pass; strict
  workspace/all-target Clippy, formatting and diff checks pass.
- Synthetic: full/delta scores and order agree in every case; old-generation
  results remain unchanged. Ranking hashes also agree across both builds and
  all three process repeats: 12 cases, 360 timed pairs (720 builds/updates).
- Composition: all non-capacity diagnostic fields agree; each capture's three
  opens return identical composition. Production/source/binary and sibling
  CodeGraph fingerprints remain stable during each experiment.
- Restoration: every `crates/` file matches the baseline capture's SHA-256;
  the candidate survives only as a patch and separate evidence.

Compression recommendation 27 remains open. The doc-ID-only delta-varint byte
lower bound saves 4.92/1.74/13.09 MB respectively, before offsets, skips, payload,
checksums and decoding cost. It is not a codec implementation. It saves less
logical space than the vector slack removed by the rejected candidate, which
illustrates why byte estimates alone are insufficient.

Prioritize native experiments that avoid redundant allocations in the first
place: sharing identical analyzer payloads, separating inactive field arrays,
and compact fixed-width ordinals/frequencies with explicit overflow handling.
Measure retained source-fact and occurrence lookup overhead before assuming
postings dominate the whole process. Any codec still needs safe decoding,
seek/scan costs, exact ranking and lifecycle checks, and end-to-end memory/
maintenance evidence. No unsafe code or third-party component is proposed.

## Reproduction

Instrumentation templates are in `research/instrumentation/storage/`; they are
appended only to disposable copied sources and are never production APIs.
`storage_composition.py` requires macOS and permission to sample its child with
`ps`. Its temporary build paths are recorded in each provenance file.

```sh
python3 research/scripts/storage_composition.py --output /tmp/composition-baseline
# In a disposable checkout, apply candidate.patch, then capture the candidate:
python3 research/scripts/storage_composition.py --output /tmp/composition-candidate
python3 research/scripts/posting_compaction.py /tmp/composition-baseline /tmp/composition-candidate --output /tmp/compaction-maintenance
```

The actual captures are [baseline](../storage-composition-measured/),
[candidate](../storage-composition-compacted/) and this directory. Frozen source,
instrumentation, toolchain and binary hashes accompany them. Both builds finish
before maintenance timing starts. The initial sandbox-denied attempt is retained
separately and excluded from all numbers.

`driver.py` preserves the executed maintenance driver. The current script also
accepts output/capture directories outside the repository when writing provenance;
that path-formatting correction does not alter its benchmark protocol.
