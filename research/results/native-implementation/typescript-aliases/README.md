# Native default TypeScript project selection and aliases

Scope: recommendation 21's TypeScript clause ("relative modules, source/runtime
extension mapping, named/default/namespace imports, package/workspace names,
reexports, and declared path aliases"; unsupported conditional exports or runtime
loaders stay unresolved).

## Implemented contract

- `core::typescript_project` selects, per generation, the nearest admitted
  `tsconfig.json`/`jsconfig.json` whose directory contains the importing file.
  `tsconfig.json` wins over `jsconfig.json` in one directory; more than 32
  admitted configurations disables selection explicitly.
- The selected configuration's bounded `extends` chain is resolved from the same
  generation's source records (slim identity/version/hash projections only; no
  raw unit payload is cloned).
- Only `moduleResolution: "bundler"` and `"node16"|"nodenext"` (ESM or CommonJS
  by authored `module`) are supported. `classic`, absent or `node10` modes are
  skipped rather than approximated.
- Bare specifiers consult declared aliases before package maps in compiler order
  (exact key, longest-prefix wildcard, then `baseUrl` when no key matched).
  Relative specifiers keep the ordinary policy. The file loader probes admitted
  paths only; unadmitted directory candidates fall through.
- Svelte/Vue/Astro files participate in JS-family module resolution.
- A changed or removed configuration conservatively rebinds every JS-family
  consumer from cached facts; unrelated languages keep narrower invalidation.

## Evidence

- `crates/core/src/typescript_project.rs` unit fixtures: nearest-config wildcard
  expansion; unsupported mode and alias-free configurations resolve nothing.
- `crates/graph-search/tests/typescript_aliases.rs`: a declared alias binds a real
  call edge; removing the mapping returns the call to unresolved; restoring it
  matches a clean rebuild; `moduleResolution: "classic"` stays unresolved.
- Full validation: 564 Rust tests across 50 suite reports pass, strict
  workspace/all-target Clippy and `cargo fmt --all --check` pass. Dependency
  manifests and lockfiles unchanged.

## Limits

This is a declared subset. Package export conditions, runtime loaders, `rootDirs`,
project references, `include`/`exclude` membership, declaration-only emit maps and
persisted alias-dependency records are not modeled. Invalidation is a correct
superset (all JS-family consumers), not a precise alias dependency index. An
unadmitted directory candidate cannot be classified as absent here, so those
lookups fall through to ordinary resolution instead of probing the filesystem.

`provenance.json` records the exact versions and source hashes for this capture.
