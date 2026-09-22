# Recommendation 21 acceptance audit

Scope: "Model packages, imports, and framework regions natively" in
`09-native-search-review.md`. Each clause is mapped to implemented behavior and
retained evidence; declared limits are recorded rather than inferred away.

## Rust: roots, module trees, crate/self/super, grouped imports, aliases, reexports

| Requirement | Implementation and evidence |
|---|---|
| Discover workspace/package roots and module trees | Native Cargo target facts (source 10), external `mod` directory contexts, edition/family autodiscovery, at most two contexts per walked file, filename/context conflicts unresolved. `results/native-implementation/rust-modules/README.md`. |
| `crate`, `self`, `super` paths | Anchored module-member traversal over post-update roots with module-item visibility checks. `results/native-implementation/rust-anchored-paths/README.md`. |
| Grouped imports | Iterative use-tree traversal preserving group prefixes, `self`, aliases, globs, absolute roots and trailing `self`. `results/native-implementation/rust-use-trees/README.md`. |
| Import aliases in scope | Lexical named-import bindings, hoisting, type-only namespaces, block extent and module barriers. `results/native-implementation/rust-import-bindings/README.md`. |
| Reexports | `pub use` leaves are published as module-owned `Export` nodes with authored targets; anchored resolution follows them, including `as` aliases and chains, bounded at 16 hops; private `use` publishes nothing; globs stay explicitly unavailable. `results/native-implementation/rust-reexports/README.md`, `crates/graph-search/tests/rust_reexports.rs`. |
| Bounded receiver facts before dispatch | Direct lexical declarations, `const` function expressions, local callable shadows and visible class static members; missing/ambiguous/instance-only/accessor members and dynamic receivers stay unresolved. `SCOPE-RESOLUTION-AUDIT.md`. |

Not delivered and explicitly out of the declared subset: trait/dynamic dispatch,
macro expansion, `cfg`-dependent module trees, glob exports and restricted
`pub(in ...)` visibility paths (which keep an explicit unsupported reason).

## TS/JS: an explicitly supported subset of project resolution

| Requirement | Implementation and evidence |
|---|---|
| Relative modules and extension mapping | Existing relative candidate policy plus ESM export-surface traversal; parser 18 module facts and Node probes. `results/native-implementation/js-module-bindings/README.md`. |
| Named/default/namespace imports | Typed module facts, aliases, local forwarding, explicit/star reexports, ambiguity and private-declaration rejection. Same evidence. |
| Package/workspace names | `package.json` self-references, private `#` maps, pnpm/package.json workspace membership, explicit exports and conditional-target invariants. `node-package-maps`, `node-workspaces`. |
| Reexports | Explicit export names, forwarding sources, star exports and `default` exclusion in the ESM surface. `js-module-bindings`. |
| Declared path aliases | Nearest admitted `tsconfig.json`/`jsconfig.json` per importing file, bounded inheritance, `paths`/`baseUrl` dispatch and bundler/Node16+ file loading; unsupported modes are skipped. `results/native-implementation/typescript-aliases/README.md`. |
| Dependency graph for these decisions | Generation-owned dependency records drive binding-surface repair; configuration edits conservatively rebind all JS-family consumers. This is a correct superset, not a persisted alias-dependency index. |
| Unsupported conditional exports or runtime loaders | Explicitly unresolved; never resolved by suffix matching. Source-14 configuration facts and the ESM probe matrix retain the negative cases. |

Not delivered: conditional export maps, runtime loaders, `rootDirs`, project
references, `include`/`exclude` membership and emit-map awareness. The review
permits leaving these unresolved rather than guessing.

## Framework regions

| Requirement | Implementation and evidence |
|---|---|
| Svelte/Vue/Astro script-region extraction feeding JS/TS adapters with offset translation | Registered `svelte`/`vue`/`astro` languages and adapters over `embedded::script`; declared `module`/`instance`, `setup`/`default` and `frontmatter`/`script` domains; original coordinates for symbols, references, scopes, bindings, documentation and ESM spans. `results/native-implementation/framework-regions/README.md`. |
| Template references need separately declared rules | Not modeled. Markup remains body-searchable; template expressions are not turned into symbols or edges, and this is documented as a declared limit rather than a partial claim. |
| JSX component uses, event handlers, route registrations, dependency-injection edges only where syntax/configuration supports the relation | Not implemented; they are optional additions ("only where ... supports the relation") and remain future framework work. |
| Parser coverage map for unsupported embedded regions | `EmbeddedRegionFact` facts, source-record counters and four coverage counters expose unmodeled script dialects, malformed regions and scan-bound truncation. `framework-regions` evidence. |

## Acceptance

Recommendation 21 is accepted for the declared native subset above. Files,
scripts, configuration, imports, aliases and reexports are modeled where the
syntax or configuration supports a static answer; everything else stays
explicitly unresolved with a reason. No compiler, runtime loader, package manager
or third-party component is invoked. Model-task success and the broader release
gates are tracked separately under recommendations 12/30.

Validation for this acceptance: `cargo test --workspace --locked` passes 564
tests across 50 suite reports, strict workspace/all-target Clippy passes, and
`cargo fmt --all --check` passes. Dependency manifests and lockfiles are
unchanged.
