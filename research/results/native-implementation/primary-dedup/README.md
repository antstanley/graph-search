# Primary source overlap removal

Ranker 19 accounts for delivered primary snippets by path, captured source hash and
line. The first selected carrier retains a line; later carriers retain only unique
runs, without removing candidate metadata or package associations. Fully shared
primaries remain eligible for distinct declaration/body/reference context. Ordinary
payload eviction still suppresses source eligibility. Fragmentation shares the
existing interval cap and reports omitted fragments explicitly.

## Controlled evidence

348 trials cover 58 frozen tasks in nanus, blogwright and whatsurvey, two arms and
three repeats. Both arms use wire 4, source representation 9, package sharing and
the same ranking. The disposable control disables only primary preparation; exact
source and binary hashes and the intervention appear in `build.json`.

All trials completed without errors. Source, binary and sibling CodeGraph snapshots
remained stable. Repeated actions, delivered line sets and evidence were identical.
The control reproduces the preceding package-sharing capture's evidence on all
58 tasks. No task changed required-file recall or region coverage.

| Suite | Tasks | Required files, deduplicated / control | Complete regions, deduplicated / control | Mean region coverage, both |
|---|---:|---:|---:|---:|
| Established | 34 | 28 / 28 | 12 / 12 | 48.97% |
| Fresh routing | 12 | 12 / 12 | 10 / 10 | 84.72% |
| README | 12 | 10 / 10 | 8 / 8 | 77.08% |

Raw API validation covers 116 responses and 464 common nodes. Package identities
agree across arms, every reference resolves, and no orphan table entry remains.
Coordinate accounting finds four duplicate lines across three control tasks and
zero duplicate coordinates in the deduplicated arm:

- `blogwright.fresh-invalid-date-rkey.debug`
- `whatsurvey.storage-envelope.change`
- `whatsurvey.markdown-stacks.debug`

The raw probe records spans rather than source text. This corpus coordinate check
uses each item's path and line; unit tests additionally verify hash distinctions,
exact retained strings and source-union preservation. No latency or model task-success
claim is made. These are convenience samples, not blind holdouts. The remaining
package-budget coverage losses are not resolved by this change.

## Verification

All 380 workspace tests pass in 31 suites, with no failures or ignored tests.
Strict workspace/all-target Clippy passes. Four new core cases cover union/version
preservation, payload eviction, interval-cap omissions and distinct context for
fully shared primaries. The public work-budget test checks exact source-line union,
no duplicate output, retained candidates and unchanged source-read accounting.
No dependency manifests or lockfiles changed.

```sh
python3 research/scripts/markdown_context.py --representation primary_dedup \
  --output <new-output-directory> --repeats 3 \
  --suite research/fixtures/fresh-routing-2026-09-19 \
  --suite research/fixtures/markdown-readme-2026-09-20
python3 research/scripts/package_budget_probe.py --capture <capture-directory> \
  --binaries <raw-build-directory> --all-tasks --verify-sharing
```

Raw binaries and source-bearing transcripts are outside the repository at
`/var/folders/ft/6fdqp3c914xfdlk47w5cblzc0000gn/T/graph-search-markdown-context-6c8mlwr8`.
Recommendation 12 and final delivery gates remain open.
