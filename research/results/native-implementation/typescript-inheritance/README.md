# Native selected TypeScript configuration inheritance

This increment adds `graph_search_core::typescript::inherit`. It consumes indexed
Source-14 facts for a caller-selected config and returns merged authored values,
per-field/per-option declaring files, ordered wildcard patterns and every visited
configuration's source hash. It uses no third-party production component and does
not change default search resolution or persisted representation versions.

## Behavior and independent evidence

`oracle.json` contains 16 comparisons with the already installed TypeScript 6.0.3
compiler. The compiler is a research oracle only. Seven configurations are copied
from Blogwright and Whatsurvey, preserving their relative layout. Nine synthetic
selected configs exercise chained inheritance with mixed option origins, ordered
bases, whole-map `paths` replacement and empty overrides, local-only references,
explicit `.jsonc`, diamond inheritance, dotted-name `.json` fallback, missing
parents and cycles. Exact diagnostics and the checked fields are recorded per case.

The comparison covers selected effective option values, origin-relative baseUrl,
rootDir and outDir, pathsBasePath, wildcard key order, authored membership-list
origins, local references and all dependency hashes. It does not compare every
compiler option, emitted output, complete project membership or alias resolution.
The copied files have no application source population, so compiler no-input
warnings are retained as evidence, not treated as an inheritance disagreement.
Missing parents/cycles must produce compiler diagnostics and native errors.

The harness explicitly enables hidden files in its disposable fixture's own
`.graph-search/config.toml` so the copied `.svelte-kit` config is admitted. This
is not a bypass of production exclusions or a claim that a default index sees
ignored generated configuration. No sibling index is opened for writing.
`environment.json` records unchanged source, compiler and native-binary hashes.

The compiler comparison caught a real gap before validation: TypeScript appends
`.json` to a missing relative target even if its name already has a different
extension. The native helper now handles `./base.custom` → `base.custom.json`.
The first driver run also exposed macOS temporary-directory spelling differences;
its fixture root is now canonicalized before either implementation runs. A second
compiler probe found that the dotfile `.json` must not fall back to `.json.json`.
The implementation now uses the compiler's literal suffix rule, covered by both a
core regression and the sixteenth oracle case. The earlier full validation was
explicitly stopped (session 96567, own process group 51319, SIGTERM, terminal 143)
to make this correction; `*-before-dotfile-fix` logs are not final evidence.

## Regression coverage

Five core tests cover ordered chains and origin maps, replacement/empty override
rules, unavailable inputs and malformed field shapes, and independent depth,
file-count and merged-metadata limits. A library integration test indexes and
reopens real source records, then edits, corrupts, deletes and recreates a parent.
At each step the inherited result agrees with a separate clean rebuild. Retained
prior results are unchanged, and parent source hashes follow published content.

`checks.json` and the corresponding logs record the final workspace tests, strict
all-target Clippy, offline/locked probe build, oracle, formatting and diff checks.
`sources.json` binds these checks to crate and harness hashes. `change.patch`
is relative to the saved source snapshot immediately before this increment;
it is not the full accumulated implementation diff.

Final validation (session 24756, terminal exit 0): **497 tests passed, 0 failed,
41 suite reports**. Strict workspace/all-target Clippy, the offline/locked probe
build, all 16 compiler comparisons, formatting and whitespace checks passed.
Hashes match for all 149 crate files and both driver/probe files. No new dependency
or source/parser version change was introduced (14/19); sibling source hashes
remain unchanged. The new research probe also passes standalone rustfmt checking.

## Limits and remaining integration

- Maximum 64 distinct files, 32 active levels and 32 direct bases. Effective
  config values retain raw-fact bounds; origin/dependency strings total ≤128 KiB.
- Only canonical workspace-relative selected paths and explicit relative bases.
  `.json`/`.jsonc` facts must be admitted and available. Package-based inheritance,
  workspace escapes, absent facts, cycles and exhausted bounds return reasons
  without partially merged output. No filesystem reads occur in the helper.
- Compiler options remain authored values. Basic container/member shapes are
  checked, not the complete TypeScript compiler-option schema.
- Project references are retained locally, never inherited or traversed.
- No automatic project selection, alias edges, output-to-source mapping, module
  mode interpretation or config-dependent rebinding is implemented by this API.
  Those remain work under recommendations 21/23; the ledger stays 23 checked /
  7 open. No corpus-wide accuracy or latency improvement is claimed here.

Reproduce from the repository root with the existing dependencies installed:

```sh
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin typescript_inheritance_probe
python3 research/scripts/typescript_inheritance_oracle.py /tmp/graph-ts-inheritance-results
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
