# Native ESM module bindings

This increment addresses part of recommendations 20, 21 and 23. It adds no
production dependency and does not claim full compiler or module-loader parity.

## Failure and change

The prior JS/TS resolver matched imported names to declarations in the target
file without proving those declarations were exported. Extraction bound named
imports only; default/namespace imports were incomplete. Export clauses were
looked up as an AST field, although the pinned grammars expose a named child.
Export aliases and forwarding identity were therefore not represented reliably.

A typed file-local `JsModule` surface now records imported local names, public
export names, forwarding sources, type-only modifiers and original spans. Cached
extractions retain these facts; the projector shares their immutable identities
in its module catalog. Parser policy 18 invalidates older extraction facts.

Resolution follows explicit exports, local imported aliases and star forwarding
to a defining source symbol. It preserves existing lexical shadowing decisions.
Explicit exports precede stars, stars exclude default, cycles terminate, and
multiple paths to one defining symbol agree. Distinct targets remain ambiguous.
Unsupported or unavailable evidence yields an unresolved reason, never the old
unrestricted target-file name match. These rules draw on the
[ECMAScript ResolveExport algorithm](https://tc39.es/ecma262/multipage/ecmascript-language-scripts-and-modules.html#sec-resolveexport),
verified against the authored Node fixtures in `oracle.json`.

Export leaf references use the same facts as resolution, including every
variable in a multi-binding exported declaration. Escaped strings are explicitly
unsupported instead of being interpreted as one arbitrary string fragment.
CommonJS `require` file edges inspect only the actual first literal argument;
they do not imply modeled CommonJS binding/export semantics.

## Bounds and conservative boundaries

- At most 4,096 import/export records and 8 MiB of retained record text per file.
  Records are admitted individually; no unbounded export staging vector exists.
- Export declarations use sorted byte-range lookup, not one whole-symbol scan
  per export. Import normalization uses a local-name map, retaining duplicate
  bindings as ambiguity.
- Imported reference expansion adds at most 8 MiB of target/specifier text;
  each target is at most 4,096 bytes. Exhaustion records a specific reason.
- Each export resolution admits at most 4,096 visits and depth 64. These are
  static bounds, not a claim of constant-time lookup or a complete work-budget
  integration. Star traversal may revisit a diamond through different paths.
- Anonymous default expressions and namespace values/reexports remain unmodeled.
  Existing relative-file candidates apply; package conditions, declared aliases,
  framework regions and complete TS type/value behavior remain future work.
- Multiple module extraction passes cannot silently claim one lexical scope:
  merging their surfaces marks the result incomplete. Legacy extractions without
  the optional field remain deserializable.

## Validation

`crates/graph-search/tests/js_modules.rs` exercises named/default/namespace
identity, private rejection, shadowing, aliases, local forwarding, cycles,
diamonds, conflicting stars, explicit-over-star priority, type-only calls,
missing default and the recursion limit. It compares complete occurrence records
after export/alias edits, barrel deletion and restoration, sync and reopen with
clean rebuilds. Invalid duplicate exports are deliberately retained as ambiguity.

`crates/langs/tests/js_modules.rs` checks comments, aliases, source spans,
multi-binding declarations, anonymous-value uncertainty, complete versus empty
module surfaces, escaped strings, require argument selection, record/text caps
and repeated-call expansion limits. Type tests check legacy deserialization,
roundtrip and conservative merge behavior.

The independent Node v24.19.0 oracle executes only ten small authored fixtures
in a temporary directory. All ten meet their expected success/failure outcomes.
Its driver is `research/scripts/js_module_oracle.py`; this does not execute
sibling application code or import third-party project dependencies.

Final validation passes **454 tests across 35 suites** (including doc-test reports),
strict workspace/all-target Clippy, formatting and whitespace checks. All 134
captured crate hashes match the frozen tested source; manifests/lockfiles are
unchanged. `workspace.txt`, `clippy.txt` and `checks.json` retain the results.
The isolated patch is relative to the previously captured reader-retention
source state, not repository HEAD. Source hashes identify the accumulated
working implementation. No corpus-wide recall/precision or latency improvement
is inferred from these fixtures. Recommendations 20/21/23 and final delivery
remain open.
