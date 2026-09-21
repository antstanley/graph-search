# Metadata IDF and normalization factorial

Nine frozen variants separate IDF, normalization, analyzer and qualified-field changes. All use the same native extracted symbol corpus, global symbol-document DF, field boosts 8/2/1/4, k1=1.2, b=0.75, exact-name priority and deterministic path/start-line/id ties. No body fields are added. Means include missing fields as zero and are floored at one. Whole/split aliases use per-field maxima rather than sum.

The 2×3 scoring factorial is clipped versus positive IDF crossed with combined-length normalization, independent per-field saturation, and BM25F (normalize frequencies, combine weighted evidence, saturate once). Three representation controls separately add whole identifiers, qualified names, or both while retaining clipped IDF and combined length. Each control uses the appropriate existing query analyzer.

## Candidate-file results

| Variant | Development top 8 / 20 | Validation top 8 / 14 | Development top 50 / 20 | Validation top 50 / 14 |
|---|---:|---:|---:|---:|
| clipped-bm25f-whole0-qualified0 | 13 | 10 | 18 | 11 |
| clipped-combined-whole0-qualified0 | 11 | 8 | 18 | 11 |
| clipped-combined-whole0-qualified1 | 11 | 10 | 18 | 11 |
| clipped-combined-whole1-qualified0 | 11 | 8 | 18 | 11 |
| clipped-combined-whole1-qualified1 | 11 | 10 | 18 | 11 |
| clipped-independent-whole0-qualified0 | 10 | 5 | 18 | 11 |
| positive-bm25f-whole0-qualified0 | 12 | 9 | 18 | 12 |
| positive-combined-whole0-qualified0 | 10 | 8 | 18 | 12 |
| positive-independent-whole0-qualified0 | 10 | 5 | 18 | 11 |

Development covers 20 source-valid tasks in ten families, from Nanus and Whatsurvey. Blogwright development labels have drifted and were excluded. Validation covers 14 source-valid tasks in seven other families, including Blogwright. The validation set retains the historical `heldout` label but was exposed in earlier experiments; it is not blinded or fresh. All 26 source-invalid tasks were excluded with reasons retained in the label manifests. Paired debug/change tasks are not independent samples; family-macro recall is included in summary files.

The clipped-IDF BM25F candidate was selected and recorded in `scoring-factorial-dev/decision-before-validation.md` before running validation. Development gains are `nanus.context.change` and `nanus.read.change`; validation gains are `nanus.argument-types.change` (rank 9→6) and `whatsurvey.survey-draft.change` (10→7). Neither split loses a previously retrieved top-eight target file. Top-50 target-file presence is unchanged for this candidate.

Positive IDF alone loses a development task; independent field saturation loses development and validation tasks. Qualified-name fields help validation but change representation, so they are not bundled into the normalization candidate. Identical counts in this table do not imply identical rankings.

## Validation and decision

Independent source-field reconstruction reproduced native lexical scores in 1,370,496 bit-exact checks across all four field representations. Both existing production representations also matched native metadata top-50 IDs and score bits. Both captures have identical before/after source, driver, binary, label, sibling content and sibling CodeGraph identities. Research hosts use in-memory native stores and do not mutate sibling repositories.

Implement clipped-IDF BM25F as an explicit native metadata policy, leaving combined-length normalization as the default. The candidate now has sufficient file-ranking evidence to enter the integrated context comparison, but this experiment alone does not justify a default switch. Native policy equivalence, public Boolean/filter/budget contracts, and delivered evidence under identical byte budgets remain required.

This is metadata candidate ordering, not complete-region delivery, body/metadata fusion, latency, RSS, storage overhead, or model task success. There is no universal-scoring claim and no new dependency. Do not interpret a 50-candidate file hit as eight-seed evidence success.

## Artifacts and reproduction

Development and validation subdirectories retain per-task ranks, variants, field means, query terms, exclusions, native equivalence counts and provenance. Build `scoring_probe` with the existing research manifest, then run:
```sh
python3 research/scripts/scoring_review.py --output /tmp/new-scoring-dev
python3 research/scripts/scoring_review.py --split heldout --output /tmp/new-scoring-validation
```

Use new output directories. The native scorer subsequently gained an opt-in BM25F policy; later runs additionally check its scores against the independent experiment. The original captures and their source hashes remain unchanged.

## Native implementation verification

The opt-in native BM25F scorer subsequently passed 2,740,992 bit-exact checks
against the independent reconstruction, covering both legacy and BM25F scores
across all four field representations. Native metadata top-50 order/score bits
also matched the corresponding formula. `scoring-native-dev` and
`scoring-native-validation` retain compact equivalence reports and stable
before/after provenance; the original candidate rankings are unchanged.

`scoring_review.py --reference <earlier-capture> --output <new-directory>` verifies
unchanged source repositories, compares every retained ranking row against that
capture, and writes reference hashes rather than another full copy of ranks.
Use `--split heldout` for the historical validation capture. Production still
defaults to combined-length normalization pending full-context evidence gates.
