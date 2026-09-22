# Native framework script regions

Scope: recommendation 21's framework clause ("native Svelte/Vue/Astro
script-region extraction can feed existing JS/TS adapters with offset
translation"; "a parser coverage map should make unsupported embedded regions
visible"). This is not a template engine.

## Implemented contract

- New closed-vocabulary languages `svelte`, `vue`, `astro` (schema version 3),
  registered in the default extension table and enabled by default.
- `graph-search-langs` adapters recognize declared script regions with a bounded,
  dependency-free scanner and reuse `embedded::script` for extraction:
  Svelte `module`/`instance` (via `context`), Vue `setup`/`default` (via `setup`),
  Astro `frontmatter` (leading `---` fence) and `script`.
- `lang="ts"|"typescript"` selects TypeScript; `js`/`javascript` or absence
  selects JavaScript. Tag/attribute names are ASCII case-insensitive; quoted
  attribute values may contain `>`; HTML comments do not create regions.
- Unsupported `lang`, unsupported Svelte `context`, malformed start tags,
  attribute-bound exhaustion and unclosed regions are recorded with bounded
  reasons rather than dropped. A region that fails to parse marks only itself.
- `EmbeddedRegionFact` spans/domains/extracted/reason are persisted with the
  extraction; source records retain `embedded_regions`,
  `embedded_unextracted_regions` and `embedded_truncated`.
- Coverage exposes `source_framework_region_files`, `source_framework_regions`,
  `source_framework_unextracted_regions` and `source_framework_truncated_files`.
- Markup outside declared regions remains body-searchable. Template expressions,
  component tags, events, routes and injection edges are not modeled.

## Evidence

- `crates/langs/src/framework.rs` unit fixtures: namespaced instance/module
  extraction, unsupported-language visibility, comment/case shielding, Astro
  frontmatter, unclosed-region reporting, quoted `>` in a start tag.
- `crates/graph-search/tests/framework.rs`: end-to-end indexing, file-node
  language, template body search, persisted region/counters after reopen,
  unextracted-region coverage for Vue, Astro incremental-edit equality against a
  clean rebuild.
- Full validation: 564 Rust tests across 50 suite reports pass
  (`cargo test --workspace --locked`), strict workspace/all-target Clippy passes,
  `cargo fmt --all --check` passes. Dependency manifests and lockfiles unchanged.

## Limits

Template relations, framework-convention edges (JSX components, events, routes,
dependency injection) and embedded parsers for template syntax are not modeled.
Multi-region files merge through `Extraction::merge`, which marks a shared ESM
surface incomplete instead of claiming one module identity. A `.svelte` file
whose script is only reachable through preprocessor syntax is reported as an
unmodeled region, not guessed.

`provenance.json` records the exact versions and source hashes for this capture.
