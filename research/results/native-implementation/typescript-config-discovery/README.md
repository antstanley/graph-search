# TypeScript configuration discovery: remaining implementation evidence

Read-only inspection through CodeGraph followed by direct configuration reads
confirms two different unresolved source-mapping cases. No sibling file or index
was modified. `source-identities.json` pins the exact configurations inspected.
This is discovery evidence, not a completed resolver or independent quality score.

- **blogwright:** package `tsconfig.json` files extend `../../tsconfig.base.json`.
  Core and CLI declare `rootDir: src` and `outDir: dist`; the shared base declares
  NodeNext module/resolution semantics and declaration output. Resolving package
  exports that name output files requires authored output-to-source correspondence,
  inherited options and condition-aware agreement. Merely replacing `dist` with
  `src` based on folder names would invent a mapping.
- **whatsurvey:** frontend `tsconfig.json` extends `./.svelte-kit/tsconfig.json`.
  The currently present generated config declares `$lib` and `$lib/*` relative
  to its own location (`../src/lib`, `../src/lib/*`), plus a generated `$app/types`
  target. It also declares `rootDirs` and inclusion/exclusion lists. The root
  config contains project references, not these path aliases. Backend has a
  separate bundler/noEmit configuration without aliases.

The next native integration must preserve which configuration authored a relative
path, account for inheritance and project membership, and report unavailable
configuration instead of silently binding aliases through unrelated package or
global-name fallback. Generated configurations excluded by walk policy cannot
be assumed present in indexed source facts. Native discovery must explicitly
account for that policy boundary. No compiler execution or third-party resolver
will be added to production; an already installed compiler can serve as an
independent experimental oracle in disposable fixtures.

Recommendation 21 remains open. The completed embedded-coordinate helper is also
still separate from native Svelte region discovery and framework visibility.
