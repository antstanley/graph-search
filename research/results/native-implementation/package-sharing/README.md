# Shared package identity under the source budget

Wire 4 / ranker 18 stores repeated selected package identities once in
`context.packages`, with result-local `package_ref` fields. Single-use identities
remain inline. Path, manifest hash, ecosystem and name all participate in identity;
no information is omitted. Source representation 9 and parser policy 7 are unchanged.

## Controlled comparison

348 trials: 58 frozen tasks across nanus, blogwright and whatsurvey, two arms and
three repeats. `structured` enables sharing; `fixed` disables only interning in
the disposable control. Both use wire 4, ranker 18, the same package facts, source
boundaries, ranking, graph context, and fixed evidence protocol: four calls,
16 KiB per response, 48 KiB cumulative context, and a 180-second deadline.

All trials complete without errors. Sources, binaries and sibling CodeGraph indexes
remain stable. Repeats deliver identical source-line sets, actions and evidence.
The inline control reproduces the previous package-context evidence on every task.
Build hashes and the exact intervention are recorded in `build.json`.

| Suite | Tasks | Required files, shared / inline | Complete regions, shared / inline | Mean region coverage, shared / inline |
|---|---:|---:|---:|---:|
| Established | 34 | 28 / 28 | 12 / 12 | 48.97% / 48.20% |
| Fresh routing | 12 | 12 / 12 | 10 / 10 | 84.72% / 84.72% |
| README | 12 | 10 / 10 | 8 / 8 | 77.08% / 77.08% |

Two previously lost regions recover exactly to their pre-package-context coverage:

- `nanus.grep.change`, region r2: 70.83% → 100%.
- `whatsurvey.contact-policy.debug`, region r3: 20.97% → 56.45%.

No measured region/file coverage decreases on another task. Complete-task counts
remain unchanged because other required regions are still absent. The losses on
`nanus.read.change`, `nanus.temporary-paths.change` and
`nanus.fresh-runtime-tools.debug` remain open. Newer suites are convenience samples,
not blind holdouts, and this protocol does not measure model answer/patch success.

## Identity and byte accounting

A separate probe inspects the unrendered API results for all 58 tasks in both arms:
116 responses, 464 common nodes: 430 known identities and 34 absent associations
agree exactly between arms. Every reference resolves and no unreferenced table
entries remain.
Binary/adapter hashes and sibling source/index snapshots match the controlled
capture and stay unchanged (`raw-budget-checks.json`, `raw-identity-validation.json`).

Compact JSON reconstruction of identity fields, references and shared tables shows
savings on 57 tasks, zero losses, and one unchanged task: 42,725 bytes total,
736.64 bytes mean per query, range 0–1,191 bytes. These are reconstructed field
costs, not literal wire-byte measurements or a latency claim. Savings are available
before source allocation, so delivered response bytes may increase as source fills
the freed space. The evaluation renderer does not score package identity's utility;
its preservation is established separately by the raw differential.

Coordinate accounting also identifies a separate outstanding context issue:
primary snippets overlap on three tasks (`coordinate-overlaps.json` and
`overlap-details.json`): storage-envelope, invalid-date/rkey, and Markdown stacks.
The duplicate coordinates occur in both arms and are unchanged by package sharing.
This is direct remaining work under recommendation 12, not a sharing regression.

## Verification and reproduction

All 376 workspace tests pass (31 suites, no failures or ignored tests), including
ranked/phrase results at tight budgets, JSON round trips, same-name/different-path
and hash distinctions, legacy inline decoding, and final CLI envelope pruning.
Strict workspace/all-target Clippy, formatting and whitespace checks pass.
No dependency manifests or lockfiles changed.

```sh
python3 research/scripts/markdown_context.py --representation package_sharing \
  --output research/results/native-implementation/package-sharing --repeats 3 \
  --suite research/fixtures/fresh-routing-2026-09-19 \
  --suite research/fixtures/markdown-readme-2026-09-20
python3 research/scripts/package_budget_probe.py --capture <capture-directory> \
  --binaries <raw-build-directory> --all-tasks --verify-sharing
```

The capture command requires a new output directory. Raw source-bearing transcripts
and binaries are outside the repository at
`/var/folders/ft/6fdqp3c914xfdlk47w5cblzc0000gn/T/graph-search-markdown-context-_hq_ej3g`.
Sanitized repository artifacts retain identity/span accounting and evidence metrics.
Recommendation 12 and the broader release gates remain open.
