# Native workspace dependency resolution

This increment extends recommendation 21 with authored workspace membership,
workspace-protocol dependencies and condition-invariant package maps. It adds
no production dependency and never installs packages or invokes a loader.

## Selection contract

The adapter projects `pnpm-workspace.yaml` as a hash-bound Node workspace
manifest. It recognizes an explicit `packages` block sequence or `packages: []`,
quoted/plain string patterns, comments, and simple unrelated top-level scalar or
flat-list/map settings. Unsupported YAML constructs invalidate the whole
workspace projection: aliases/tags, flow collections other than empty unrelated
values, multiline scalars/quotes, duplicate keys, mixed/unsupported indentation,
non-string package entries and multiple documents. This is deliberately not a
general YAML decoder. The existing 256 KiB manifest cap and metadata entry/string
caps apply. Missing `packages` is unavailable, not a guessed manager default.

Nearest pnpm workspace files take precedence over package.json `workspaces`,
including a child package.json declaration. Without a pnpm workspace file, the
nearest package.json workspace declaration is supported unless an explicit pnpm
manager says it is not authoritative. These rules follow pnpm's
[workspace configuration](https://pnpm.io/settings#packages); they do not infer
what is installed in a node_modules tree. The root package is included.

Membership supports literal directory segments, whole-segment `*`/`**`, a leading
`./`, and `!` exclusions. Wildcards do not consume dot-prefixed directory names;
explicit dot-directory literals may match. Unsupported patterns invalidate the
workspace decision, including brace/class/partial-segment patterns, parent/dot
segments and repeated separators. Exclusions win over inclusions. Only walked,
known package manifests can supply target identities.

A non-self bare import must be declared in the importing package's dependency
facts as `workspace:*`, `workspace:^` or `workspace:~`. Those symbolic ranges
select local identity without needing a semver evaluator. Pinned/ranged versions,
workspace aliases/relative targets, ordinary registry dependencies, overrides and
lockfile decisions are still unmodeled. See the [workspace protocol](https://pnpm.io/workspaces#workspace-protocol-workspace).

The target name must be unique within the selected workspace. Same-named packages
outside membership do not create ambiguity; duplicate members do. An unavailable
potential member prevents a partial name catalog from asserting uniqueness.
An explicit target export map selects the requested root/subpath; blocked and
unlisted exports remain blocked, and actual ESM export visibility still applies.
No global unique-name or suffix fallback is added. Existing exact-file and
unambiguous source/runtime-extension mapping rules remain in force.

## Conditions, limits, and incremental behavior

The adapter retains `InvariantPath` only when every branch of a conditional
object names exactly the same path and every nested condition object has a
`default`. Depth is capped at 16. Invalid keys, arrays, missing defaults,
null/differing branches and other values remain unsupported. The distinction
from a direct authored path is persisted. No runtime condition or object key
order is selected. This proof uses Node's [default and nested-condition rules](https://nodejs.org/api/packages.html#conditional-exports).

Workspace construction shares a one-million-unit allowance across ancestor
probes, pattern bytes, pattern matches and dynamic-programming transitions.
Exhaustion disables all workspace dependency decisions from the partial build;
it never exposes a partially collected name map. Relative imports and package
self/private-map resolution remain independent of that workspace budget.

The persisted validator binds workspace roles to pnpm workspace filenames and
rejects package-only metadata on workspace definitions. `packageManager` is a
bounded optional fact. Source representation 13 refreshes previous projections;
parser 18 remains unchanged. Workspace declarations enter the existing manifest
boundary set, so changes/additions/removals trigger the existing conservative
rebinding. More precise dependency invalidation remains recommendation 23 work.

## Evidence and limits

- `oracle.json`: pnpm 11.24.0 independently selected expected membership in four
  fixtures, including child JSON precedence, recursion, exclusions and hidden
  directories. Node v24.19.0 ran nine synthetic imports across default/types/custom
  conditions: invariant targets stayed equal and differing targets diverged.
  No network, install or application code was used.
- `workspace.txt`, `clippy.txt`, `checks.json`: final verification status.
- `sources.json`, `change.patch`: exact crate/driver hashes and isolated changes
  against the preceding store-exclusion increment.
- `source-probe.json`: six original whatsurvey files copied into a disposable
  fixture. Five real `contactNameParts` calls cross the declared workspace package
  boundary. Removing/restoring membership and then the dependency in the copy
  tests causality after separate CLI invocations/reopening. Original files and
  indexes are not changed. This is not a full-repository search-quality benchmark.

Workspace integration tests additionally cover duplicate/unavailable members,
ordinary ranges, excluded members, uniform/differing exports, nested workspace
changes, explicit pnpm manager declarations, missing dependencies, and complete
occurrence parity between incremental updates and clean rebuilds.

Remaining recommendation 21 work includes declared path aliases, authored
build-output/source-root mappings (needed by blogwright's dist exports), framework
script regions, and the previously documented Rust/module/receiver gaps. This
increment does not mark the broader implementation or release gates complete.

Workspace override selectors are retained from pnpm workspace files and root
package.json override/resolution settings. A selector that could affect the
selected package prevents a resolved workspace binding; override targets are not
interpreted. Unrelated simple selectors do not suppress the binding. Unsupported
nested/complex override forms conservatively block affected or all workspace
names. Override checks share the workspace-construction allowance. This avoids
claiming a local call target when configuration may redirect that dependency.

## Final validation

477 workspace tests passed across 37 suite reports. Strict workspace/all-target
Clippy, formatting and whitespace checks pass. All 140 crate-file hashes match
the captured final sources; dependency declarations and the lockfile are unchanged.
The five-state whatsurvey probe retained six indexed source files in each state:
five calls resolved initially, all five became unresolved after removing target
membership, restoration recovered the exact original occurrences, removing the
dependency made all five unresolved again, and restoring it again recovered the
original occurrences. Source originals and the CLI binary stayed unchanged during
the probe. Index storage was inside the disposable source root and stayed excluded.

Reproduce with `cargo test --workspace`,
`cargo clippy --workspace --all-targets -- -D warnings`, and:

```sh
python3 research/scripts/node_workspace_oracle.py --out /tmp/workspace-oracle.json
python3 research/scripts/node_workspace_source_probe.py --repo ../whatsurvey \
  --cli target/debug/graph-search --out /tmp/workspace-source-probe.json
```
