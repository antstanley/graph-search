# Native TypeScript alias dispatch

`core::typescript_aliases::Aliases` compiles the paths/baseUrl portion of an
explicitly selected `EffectiveConfig`. Its resolver implements native dispatch:
exact keys first, longest matching wildcard prefix, authored order for equal
prefixes, ordered substitutions and first-success short-circuiting. A matched
paths key suppresses baseUrl even when all its substitutions miss. No pattern
match may attempt baseUrl. Separate result variants retain these distinctions.

Values resolve relative to the effective baseUrl's declaring file when baseUrl
exists, otherwise the paths map's declaring file. Unicode matches, dot-prefixed
bare names, empty matched wildcard text and trailing directory separators are
preserved. Compilation validates basic shapes, wildcard count, origin paths and
workspace bounds; attempts are capped at 128 substitutions and 4,096 bytes per
normalized path/specifier. The retained raw-fact bounds also apply.

The caller supplies the module-mode loader. It receives the normalized candidate
and the **original, unexpanded substitution** (`None` for baseUrl). It must decide
extension substitution, suffix handling, directory/package lookup and fallback
under the selected mode. Loader errors stop dispatch without selecting another
substitution or baseUrl. The callback can record negative lookup candidates for
future invalidation work. This API performs no filesystem reads itself.

## Evidence and scope

- Six core regressions cover precedence, ordered attempts, missing candidates,
  loader errors, mixed origins, directory intent, Unicode, dot-prefixed names,
  empty wildcards, invalid shapes, escapes and bounded expansion. One proves that
  identical expanded paths retain different authored substitutions for the loader.
- `oracle.json` compares 17 native dispatch cases with TypeScript 6.0.3. The probe
  indexes/reopens a disposable fixture and uses a deliberately exact-file research
  callback. Substitutions/specifiers name `.ts` files, with no competing extension
  or directory/package candidates. Two cases use copied Whatsurvey/Svelte configs
  in their original relative layout, with synthetic source stubs. This verifies
  dispatch and origins, **not a complete native module loader**.
- The oracle covers exact keys versus a competing wildcard, longest-prefix and
  equal-prefix precedence, a selected-pattern miss despite another matching
  pattern having a file, ordered fallback, baseUrl suppression, empty-map override,
  inherited origins, child baseUrl override and two real authored Svelte aliases.
- `environment.json` records unchanged sibling source, compiler and native-binary
  identities. Hidden files are explicitly enabled only in the disposable fixture
  so the copied generated config is admitted. No sibling index is modified.
- `extension-priority.json` and its standalone compiler probe explain a necessary
  interface correction: a literal `x.js` mapping prefers the existing JS file,
  whereas a wildcard that expands to `x.js` can prefer `x.ts`. The initial boolean
  callback parameter lost this distinction. It was replaced with authored text,
  and a native regression protects the interface. These two compiler-only cases
  are not counted as native module-resolution coverage.

The first validation was explicitly stopped to correct that proven interface
issue (session 89664, own process group 71401, SIGTERM, terminal 143). Its
`*-before-callback-fix` logs are historical, not final validation evidence.
`checks.json` records the corrected source's focused core/persistence tests,
strict workspace/all-target Clippy, offline/locked probe build, oracle and format
checks. `sources.json` binds results to the exact crate/driver files; `change.patch`
is relative to the snapshot immediately before this increment.

Final validation (session 83033, terminal exit 0): **186 focused tests passed,
0 failed across 5 suite reports** (182 core and 4 configuration-persistence tests).
Strict workspace/all-target Clippy, the offline/locked probe build, all 17 compiler
comparisons, formatting and whitespace checks passed. All 150 crate hashes and
both driver/probe hashes match. The standalone research-probe formatting check
also passed, and root/crate/research-harness dependency manifests and lockfiles
have no changes.

## Remaining work

This is callable native core dispatch, not automatic alias edges in search. A
project-aware module loader, default project discovery/membership, overlapping
project contexts, reexport context propagation, excluded config boundaries,
output/source mapping and dependency invalidation remain open. The full compiler
option schema is not validated here. The module loader must interpret remaining
options; the dispatcher deliberately does not invent their behavior.

No production dependency or persisted-version change is introduced. The ledger
remains 23 checked / 7 open. The preceding full workspace result (497 tests)
applies to the inheritance increment; this increment records focused tests and
strict workspace compilation rather than claiming a new full-workspace test run.

```sh
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin typescript_alias_probe
python3 research/scripts/typescript_alias_oracle.py /tmp/graph-ts-alias-results
cargo test -p graph-search-core
cargo test -p graph-search --test typescript_config --test typescript_inheritance
cargo clippy --workspace --all-targets -- -D warnings
```
