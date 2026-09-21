# Native package self-references and private import mappings

This increment extends recommendation 21 using package.json facts and the native
ESM export resolver. It adds no production dependencies or runtime loader.

## Implemented

The existing JSON adapter now retains bounded authored module type, main entry,
export/import maps, workspace patterns and dependency specifiers. A target is a
raw path, an explicit null block, or an unsupported value. Conditional objects and
arrays are never collapsed to a guessed file. Absent maps, empty maps and null
exports remain distinct. Invalid/over-budget metadata discards the whole binding
projection with a reason while retaining the package's independently valid identity.
Legacy manifests deserialize without invented Node metadata.

The module resolver finds the nearest known package boundary, including invalid
or unavailable boundaries. Within it, package-name self-references require an
explicit exports map; main cannot bypass absent, blocked or unsupported exports.
Exact private `#` imports use the same package's imports map. Reexports call the
same resolver. Selecting a file still requires its actual ESM export surface;
private declarations cannot satisfy an imported binding.

These rules follow the documented
[Node package self-reference and entry-point semantics](https://nodejs.org/api/packages.html#self-referencing-a-package-using-its-name).
Targets are package-relative, remain inside the walked file set and reject
traversal, node_modules segments, backslashes, encoded paths and URL suffixes. Control characters make binding metadata unavailable.
Exact files win. A missing `.js` target may map to one `.ts` or `.tsx` source;
`.mjs`/`.cjs` may map to `.mts`/`.cts`. Multiple source substitutes stay ambiguous.
There is no implicit extension or directory-index search for package-map targets.

Source representation 12 refreshes the authored metadata. Parser 18 and ranker
23 are unchanged. Manifest edits already rebind cached dependents conservatively;
file-set changes now also revisit JS package-map consumers, including unresolved
default imports whose target names cannot identify the newly created file.

## Bounds and limits

The manifest decoder retains its 256 KiB source cap. Node metadata additionally
limits individual strings to 4,096 bytes, cumulative retained strings to 256 KiB
and the number of retained string entries to 4,096. Keys and values each consume
an entry. The stored-fact validator enforces those limits, map-key structure,
module type and the absence of partial data in unavailable projections.

This is a declared source subset. Workspace/dependency facts are now available,
but cross-package workspace selection, lockfile/semver decisions, tsconfig aliases,
patterned mappings, external-package import-map targets and runtime conditions
remain unmodeled. The stored module type does not validate runtime executability.
There is no CommonJS binding analysis or Node experimental package-map support.
Broader dependency-invalidation precision and framework regions remain open.

## Evidence

- Six module integration tests pass in the focused run, including prior ESM
  coverage and two new package tests. New cases cover scoped self names, default
  and subpath imports, `#` mappings, reexports, private symbols, null/conditional
  targets, package escapes and nested boundaries.
- Mutation tests create a previously missing default target, edit export maps,
  remove the self-export map, introduce competing TS/TSX substitutes and add an
  exact JS target. Every state syncs, reopens and compares full occurrence records
  with a clean rebuild.
- Two adapter tests preserve authored distinctions and reject partial/over-budget
  projections. Type/core tests cover legacy roundtrip and independently reject
  invalid persisted metadata.
- `oracle.json` records thirteen independent Node v24.19.0 probes and their expected
  outcomes. The conditional-export positive probe explicitly demonstrates a valid
  Node feature the native subset leaves unresolved; it is not claimed as parity.
  A control-character probe also demonstrates Node URL normalization selecting
  `api.js` while a literal newline-named decoy exists. Native metadata rejects
  that spelling so filesystem interpretation cannot select the decoy.
  Only disposable authored fixtures execute. The driver is
  `research/scripts/node_package_oracle.py`.

The source-backed probe copies four whatsurvey files into a disposable source
root with a separate store. Both `compileFlowJson` calls resolve through `#core`
and the barrel's `.js` reexport to the authored TS definition. Removing only the
copied import map makes both calls unresolved with `node_import_map_missing`;
restoring it reproduces the complete original occurrence records after sync and
reopen. `source-probe.json` retains hashes and occurrence metadata, not source
bodies or latency claims. The original four files remain byte-identical.

The discarded first setup exposed a separate store-exclusion bug: configuring
an in-tree store outside the default hidden directory allows its publication
files into the source walk. `custom-store-observation.json` retains that evidence.
It is excluded from package acceptance evidence and requires a follow-up fix
before overall delivery.

Final frozen validation passes **460 tests across 35 suite reports**, strict
workspace/all-target Clippy, formatting and whitespace checks. All 136 crate
hashes and both driver hashes match; the source probe binary remains identical.
`workspace.txt`, `clippy.txt` and `checks.json` retain the evidence.
`change.patch` is isolated against the preceding ESM increment. Its reconstructed
before files match that increment's captured hashes; `sources.json` identifies
all current crate files and the oracle driver. No sibling source/index, dependency
manifest or lockfile is changed, and no corpus-wide quality/performance claim is
made. Recommendations 20/21/23 and the overall commit/push gate remain open.

The subsequent [configured-store exclusion fix](../store-exclusion/README.md) addresses this bug and repeats the source-backed probe using an in-tree store. The original observations above remain historical.
