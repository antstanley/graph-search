# Native TypeScript configuration facts

The production registry now captures bounded raw configuration projections from
policy-visible `.json` and `.jsonc` files and publishes them with hash-bound source
facts. This supplies the authored input needed for native inheritance, project
membership, aliases and output-to-source mapping. **Those resolution steps are
still open under recommendation 21.** Capturing a file does not select it as a
project, resolve an alias, load an excluded dependency, or execute a compiler.

Final validation: **491 tests across 40 suites pass**, strict workspace/all-target
Clippy passes, and formatting/whitespace checks pass. All **147 crate files** and
both research driver/probe sources match their frozen hashes. The native probe
matches **19 TypeScript 6.0.3 oracle cases** (seven actual configurations and twelve
synthetic cases), including the two independent precedence selections. No new
production dependencies were introduced.

## Contract

A configuration may extend an arbitrarily named JSON file, so this projection
is available for every admitted JSON/JSONC source rather than only files named
`tsconfig.json`. Ordinary JSON data does not become a project by being captured.
The projection retains `extends`, `compilerOptions`, `files`, `include`, `exclude`
and `references`. It preserves array order, absent versus empty/null values,
relative path spellings, complete raw compiler options and reference objects.
Wildcard `paths` keys additionally retain their authored first-property order,
including duplicate-key replacement without changing precedence. Exact names do
not require wildcard ordering. Independent persisted validation requires the order
list to contain every wildcard key exactly once.
The owning source record carries the exact path/hash; no relative path is rebased
or merged at extraction time. Unknown top-level fields are omitted. Raw values
are not a claim that TypeScript accepts their compiler-option semantics.

A native byte scanner handles a leading UTF-8 BOM, line/block comments and trailing
commas outside strings. It leaves escaped strings intact and delegates JSON
grammar/escape validation to the workspace's existing `serde_json` decoder. It
performs an order-preserving second decode only when wildcard path keys are
present, using a native serde visitor and no ordered-map dependency. It rejects
unterminated block comments and does not turn array holes/missing values
into valid JSON by removing commas. This is bounded JSONC support, not a new
JavaScript evaluator or a claim of every compiler parser recovery behavior.
Original source bytes still feed retrieval regions and their hash; comment
blanking affects only the temporary configuration-decoding buffer.

Limits: 256 KiB input before decoding; 4,096 retained values; depth 32; 4,096 UTF-8
bytes per string/key and 128 KiB total retained string/key bytes. Failed syntax,
non-object roots and exhausted bounds emit one fixed unavailable reason with no
partial fields. `TypeScriptConfig::valid` independently rechecks persisted bounds.
Source validation rejects configuration facts on other path types, representations
older than revision 14, and mismatched source hashes. Legacy records without this
optional field remain readable. Current source policy **14** forces reindexing to
capture the added facts; parser policy remains **19** and ranker policy unchanged.

The optional registry port defaults to no projection, so custom language
registries remain source-compatible. No dependency or production compiler is
added. The default library registry provides the native adapter. Excluded files
remain excluded even when named by `extends`; inherited unavailable inputs must
be handled explicitly by the later resolver.

## Validation

Unit checks exercise JSONC strings/comments/BOM/trailing commas, malformed syntax,
raw-value preservation, serialization and independent representation limits.
Integration checks exercise actual library registry wiring, source identity,
publication/reopen, configuration edits versus complete rebuild, deletion and
no-op publication stability, legacy optional fields, rejected invalid persisted
facts, and source exclusion boundaries.

The independent research probe uses TypeScript already installed in whatsurvey
as an oracle **only**. It indexes disposable copies of seven actual blogwright /
whatsurvey configurations plus syntax cases. Successful raw projections and wildcard ordering compare with
`parseConfigFileTextToJson`; malformed cases must be rejected by both. Two virtual
compiler resolution cases reverse equal-prefix patterns and demonstrate that
precedence changes the selected source; native resolution itself is not yet wired. This
is syntax/projection agreement, not a compiler-backed alias-resolution claim.
The first exclusion fixture accidentally supplied a glob to the existing
directory-name exclusion option. Its failure is preserved in
`workspace-before-exclusion-fixture-fix.txt`; correcting the fixture to `hidden`
passed the complete final run. No production exclusion behavior changed.
The stopped pre-order workspace run is archived as
`workspace-before-pattern-order.txt` and is not final validation. It was terminated
explicitly after discovering the ordering requirement, not restarted on timeout.
The compiler, original configurations and native executable are hashed before
and after. See `checks.json` and `oracle.json` for completed counts/results.

## Semantics used for the next integration

Relative values must retain their declaring configuration's origin through
inheritance; membership fields overwrite inherited lists and project references
are not inherited. See the official [TypeScript extends reference](https://www.typescriptlang.org/tsconfig/extends.html).
Path mappings are relative to the applicable base URL or declaring configuration,
and do not themselves rewrite emitted runtime imports. See the official
[TypeScript paths reference](https://www.typescriptlang.org/tsconfig/paths.html).

## Reproduction

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release --offline --locked --manifest-path research/harness/Cargo.toml --bin typescript_config_probe
python3 research/scripts/typescript_config_oracle.py /tmp/typescript-config-oracle
```
