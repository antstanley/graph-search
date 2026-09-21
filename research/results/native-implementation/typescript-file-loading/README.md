# Native modern TypeScript file loading

`core::typescript_files` implements bounded file and index-directory lookup for an
explicitly selected bundler, Node16/NodeNext ESM or Node16/NodeNext CommonJS mode.
It composes with the existing native configuration inheritance and alias dispatcher.
It does not discover projects or change default search bindings by itself.

## Implemented behavior

- Runtime/source extension priority for JS/JSX, MJS/MTS, CJS/CTS and declarations;
  explicit authored mapping extensions retain their exact-file priority, while
  extensions introduced by wildcard expansion use replacement priority.
- Implicit extensions and `index` files in bundler/CommonJS modes; ESM excludes
  those fallback routes. A trailing directory separator prevents a file probe.
- Authored `moduleSuffixes` in suffix order within each extension. An omitted empty
  suffix does not invent an unsuffixed fallback; an empty array uses normal lookup.
- JSON and custom-extension declaration wrappers. JSON defaults follow TypeScript
  6's bundler resolution or NodeNext/Node20 **module setting**, with an explicit
  resolveJsonModule override. This is resolution behavior, not whole-program
  validation of whether a resolved import is permitted by every compiler option.
- A per-resolution object records distinct positive/negative presence probes and
  package boundaries. Reuse across alias substitutions shares a 256-probe limit;
  paths cap at 4,096 bytes and suffixes at 32 entries of 128 bytes each.
- Package-directory entry rules are explicitly unmodeled. A known package.json
  boundary prevents guessing an index file, including when the manifest's source
  was too large to admit. A boundary alone cannot become a resolved source target.

The loader only reads supplied generation inventories. It performs no filesystem
I/O and uses no new production dependency. Callers must supply the selected mode,
case-sensitive admitted paths and unavailable package boundaries from a coherent
generation. This API does not infer missing facts from the live filesystem.

## Validation

`oracle.json` records **173 supported comparisons** with TypeScript 6.0.3 and **2
explicit package-directory refusals**. The native probe indexes and reopens a
disposable fixture, then composes production inheritance, aliases and this loader.
Unlike the preceding dispatcher-only probe, file loading is now production code.

The matrix covers three modes and wildcard/literal/baseUrl routes; competing
source/runtime files; implicit extensions; index directories; trailing separators;
JSON flags/defaults; custom wrappers; declaration families; suffix ordering and
missing unsuffixed fallback; Unicode; dotfiles; workspace-root directories; and
package boundaries. Unsupported package cases are not counted as resolved matches.
The compiler uses a case-sensitive fixture inventory, not the host's filesystem
case-folding behavior. Fixtures explicitly disable default directory exclusions
and enable hidden files; production defaults are unchanged.

The first driver run mismatched six cases because its inventories were not
aligned: graph-search excluded directories literally named `target`, while the
compiler host mishandled trailing separators and reported the fixture's ancestor
directories as absent. The corrected host models existing ancestors and canonical
directory paths, and both implementations receive the same fixture inventory.
`*-before-host-and-admission-fix` and `*-before-ancestor-host-fix` artifacts retain
those failed observations separately. No production loader change was needed to
make the 175-case comparison pass.

Seven core regression tests cover lookup order, ESM exclusions, suffix/declaration
rules, unavailable boundaries, option/path rejection, deduplication and probe-limit
exhaustion. Two library tests compose the helpers over reopened published facts:
preferred-file creation/removal/rename and config edits agree with clean rebuilds;
oversized package boundaries block directory guesses and become admitted direct
JSON targets only after valid publication. These tests recompute composed helper
results; they do **not** claim default alias-edge rebinding already exists.

`checks.json` and logs record final focused tests, strict workspace/all-target
Clippy, offline/locked probe build, compiler comparisons and formatting. Exact
crate and harness hashes are in `sources.json`; `change.patch` is relative to the
snapshot taken before this increment. The compiler/binary/fixture hashes in
`environment.json` are checked unchanged. No sibling code or CodeGraph index is
modified by these synthetic experiments.

Final validation (session 74323, terminal exit 0): **195 focused tests passed,
0 failed across 6 suite reports** (189 core/property and 6 configuration/file-loader
persistence tests). Strict workspace/all-target Clippy, the offline/locked probe
build, all 173 supported compiler comparisons and 2 explicit refusals, formatting
and whitespace checks passed. Hashes match for all 152 crate files and both
main harness inputs. Standalone probe formatting passes, and dependency manifests
and lockfiles remain unchanged. The separately recorded unavailable-candidate gap
is intentionally not counted as successful binding equivalence.

## Open integration

**Follow-up:** [candidate presence](../typescript-presence/README.md) changes the
loader API to require a presence provider and reproduces the oversized candidate
as an explicit refusal. The evidence below records the earlier admitted-only
implementation; default project/publication integration remains open.

Before publishing default bindings, integration must also distinguish an absent
ordinary candidate from a policy-excluded, quarantined or otherwise unavailable
file. An admitted-file set alone does not prove physical absence. The current
compiler fixture provides a complete controlled inventory; production callers
must validate negative probes against coverage/presence information before treating
a lower-priority candidate as a definite binding. Package boundaries already carry
that distinction in this helper. This remains integration work, not a claim that
ignored files can safely be treated as missing. `unavailable-candidate-gap.json`
reproduces the issue: an oversized `entry.ts` is absent from admitted facts, so an
admitted-only helper selects `entry.js` while the compiler selects `entry.ts`. Its
standalone probe and input policy are retained. This is a known integration gap,
not a passing equivalence case, and no default alias binding is published by it.

Project selection/membership and overlapping project contexts, module-mode
classification, rootDirs routing, directory package entry fields, reexport context,
output/source mapping and persisted dependency invalidation remain open. The
caller owns package/workspace fallback after optional alias lookup. This is an
explicit modern resolution subset: classic/Node10 and unmodeled settings return
reasons. Complete TypeScript semantics are not a requirement or a claim.

Recommendations 21/23 remain open; ledger 23 checked / 7 open. Source/parser/ranker
versions remain 14/19/23 because this increment does not yet publish new default
bindings. The preceding full-workspace result is historical; this increment
records its own focused scope rather than relabeling that earlier run.

```sh
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin typescript_file_probe
python3 research/scripts/typescript_file_oracle.py /tmp/graph-ts-file-results
cargo test -p graph-search-core
cargo test -p graph-search --test typescript_config --test typescript_inheritance --test typescript_files
cargo clippy --workspace --all-targets -- -D warnings
```
