# Embedded JS/TS coordinates and declaration identities

This increment supplies the composition layer needed by native framework
adapters. `graph_search_langs::embedded::script` accepts an explicitly identified
UTF-8 byte range, JavaScript or TypeScript dialect, and stable file-local domain.
It reuses the existing adapters and returns facts in original file coordinates.
**It does not yet register `.svelte`, `.vue`, or `.astro` extraction.** Native
region discovery, framework scope rules, template references and parser coverage
reporting remain work under recommendation 21.

## Implemented contract

- Checked byte/line translation covers symbols, references (including their
  separate line field), lexical scopes, binding declaration spans and visibility/
  initialization thresholds, documentation comments, and ESM imports/exports.
- Lexical start/end attributes used by the core resolver are translated too.
  Shifting only displayed symbol spans would leave incorrect visibility checks.
- All declaration keys, parent keys, lexical-target keys, binding target keys,
  documentation owner keys and lexical-key attributes receive the same stable
  `embedded:<domain>>` prefix. Source names, qualified names, signatures, import
  paths, exported names and raw reference spelling remain authored values.
- A zero initialization threshold remains the existing hoisted-binding sentinel.
  A zero visibility offset is an actual coordinate and moves with the region.
- Domains are 1–128 ASCII letters/digits/underscore/hyphen. Invalid UTF-8 ranges,
  reversed/out-of-source ranges, unsupported languages and coordinate overflow
  return a parse error without publishing partial facts. An empty region is valid.
- Explicit TypeScript selection uses the ordinary TS grammar even when the
  containing filename ends in `.tsx`. No preprocessing or generated code runs.
- Existing `Extraction::merge` offsets scope/binding ordinals and refuses to
  claim that two module surfaces are one complete ESM scope. Key namespacing is
  **identity separation**, not proof of visibility isolation: a framework adapter
  must still control unresolved-name fallback and cross-region visibility.

The regular extractor registry, default extension policy, on-disk schema and
source representation version remain unchanged. The independent source probe
also exposed an existing JS/TS destructuring bug, corrected in this increment:

- The former declaration collector walked every identifier in a pattern,
  including identifiers used in default values and computed keys. Those are now
  excluded using the same native binding-pattern traversal as lexical scopes.
- Defaults and computed keys were not visited for executable expressions. They
  are now visited once, independently of collecting the actual bound names.
- Regression fixtures check nested object/array patterns, renaming, rest elements,
  computed-key calls, default calls, explicit lexical targets and imported aliases.

Parser version **19** invalidates old extraction facts for this behavior change;
source representation remains 13. There are no new dependencies. The failed
pre-fix source probe and earlier test log are retained separately and are not
evidence for the final implementation. The obsolete pre-fix workspace run was
explicitly terminated (exit 143) after the parser correction; the final run uses
the newly frozen sources.

## Verification

Final result: **484 tests pass across 39 suite reports**, strict workspace/all-target
Clippy passes, and formatting/diff checks pass. All **143 crate file hashes** and
both probe-driver hashes match the frozen verification inputs. Production Cargo
manifests and lockfile have no diff. `checks.json` and `environment.json` retain
these checks and the compiler/runtime versions.

The integration tests use real JS/TS extraction and the core resolver, not mocked
facts. They check every coordinate-bearing fact family against original UTF-8
bytes with CRLF, a script that begins partway through a line, documentation and
import/export aliases. Two domains with identical function names retain distinct
explicit lexical targets after merging. Nested declarations remain invisible to
an outside call after relocation. Header edits preserve keys, the containing
filename cannot select TSX, and empty scripts retain valid scope coordinates.
A unit test covers malformed domains/ranges and integer-coordinate rejection.

The independent source probe parses copied whatsurvey
`ContactProfileFields.svelte` using its already-installed Svelte compiler, then
passes the compiler's script range to the native helper. UTF-16 compiler offsets
are explicitly converted to UTF-8 byte offsets. The probe compares every top-level
function and identifier-call span, and verifies the core resolver's
`onTagKeydown → addTag` target. It repeats with a Unicode/CRLF prefix, checks key
stability and verifies that the original file is unchanged.

Both cases match **three function spans and ten identifier-call spans** against
Svelte **5.56.10**. The `addTag()` call is at original bytes 3167–3175, line 87;
after the 21-byte Unicode/CRLF prefix it is at bytes 3188–3196, line 88. Its explicit
lexical target and caller key remain unchanged.

This oracle is a research check, not a production component. It does not install
packages, load application configuration, execute the application or write sibling
sources/indexes. Supplying the range from the compiler means it **does not test
native framework boundary discovery**. See `probe.json`, `workspace.txt`,
`clippy.txt`, `checks.json`, `sources.json` and the isolated `change.patch` for the
completed run's evidence.

## Remaining integration work

[Svelte's documented rules](https://svelte.dev/docs/svelte/svelte-files) make
module bindings visible to an instance but not the reverse; instance-level
exports also do not become ordinary module exports. A framework adapter must
model these distinctions, including text order versus module initialization order,
instead of concatenating script text or enabling same-file/global fallback.

Boundary discovery must distinguish top-level scripts from markup, comments,
attributes, template expression strings and nested template blocks. Unsupported
preprocessing/external-script attributes must produce explicit coverage evidence.
The existing HTML parser is useful but does not by itself prove Svelte expression
boundaries. Template calls, event handlers, component uses and styles need their
own supported syntax/relationship rules. These are open requirements, not
capabilities claimed by this coordinate adapter.

## Reproduce

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --offline --locked --manifest-path research/harness/Cargo.toml --bin embedded_script_probe
python3 research/scripts/embedded_script_probe.py --binary research/harness/target/debug/embedded_script_probe --output /tmp/embedded-script-probe.json
```

The probe requires the existing sibling whatsurvey checkout and its installed
Svelte compiler. Core production tests do not depend on that checkout or compiler.
