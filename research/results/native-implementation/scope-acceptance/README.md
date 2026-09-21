# Recommendation 20: lexical-scope acceptance audit

This audit evaluates recommendation 20 against its actual text in
[the review](../../../09-native-search-review.md#20-make-resolution-scope-aware-before-increasing-its-reach--p1).
It does not substitute complete compiler semantics for the requested native scope
model, and it does not close recommendations 21 or 23.

## Requirement-to-evidence mapping

| Required behavior | Current implementation and verification |
| --- | --- |
| File-local scopes, declarations, value/import bindings before global lookup | `langs::scopes::enrich` builds scopes/bindings and attaches each call's selected binding. `core::resolve::resolve_reference` handles dynamic refusals and explicit lexical targets before import, same-file, qualified or unique-name fallback. |
| Blocks, parameters, destructuring, closures, local callable aliases and nested declarations participate | `scopes.rs` integration tables exercise each named category in Rust and JS/TS. The destructuring suite separately distinguishes binding patterns from executable default/computed expressions. Unknown value aliases remain explicit refusals; directly declared immutable arrow functions can bind. |
| Visibility/order rules prevent known false targets | Tests cover Rust module barriers, let/condition/loop/match extent, JS var/let/const hoisting/initialization, nested callable/class shadows, static members, missing/instance/accessor members and deferred versus immediate class contexts. |
| Known shadowing never falls through to an outer/global function | `fact.dynamic` returns Unresolved immediately. Missing/ambiguous `lexical_target` also returns immediately. The public tests reproduce outer-function/class decoys and check targets after publication/reopen. |
| Keep provenance richer than a resolved boolean | Persisted occurrences carry scope/binding ordinals, raw spelling, original spans, target identity, resolution class and unresolved reason. Classes distinguish explicit lexical/import, same-file, qualified and unique-name heuristic from unresolved. Ambiguity is an unresolved reason; a speculative candidate is not published as a target. |
| Composable native facts work incrementally | Scope/binding facts persist in extraction records. Shadow insertion/removal, import aliases, module binding and class-member edits compare complete reopened occurrences with clean rebuilds. Stable source sites retain occurrence ownership. |
| Evaluate target precision before claiming increased reach | New independent JS/TS oracle checks 50 authored calls in 38 fixtures through Index publication and reopened storage. All 22 bound targets contain the compiler-selected declaration-name site in the correct source file; all 28 expected refusals have a reason. Seven independent Rust compile/fail cases validate import/namespace/visibility premises, supplemented by native exact-target and mutation tests. |

## Independent oracle contract

`scope_binding_oracle.py` freezes expected resolved/refused outcomes before either
implementation runs. TypeScript's own type checker supplies declaration sites for
call expressions; UTF-16 compiler offsets are converted to UTF-8 bytes. Native
records come from a reopened production store. The comparison checks source file
and declaration-name containment in the target symbol span, not just target names
or a resolved percentage. Every call site must be present exactly once. Expected
refusals check both absence of a target and explicit unresolved provenance.

This measures the declared lexical subset, not whole-repository precision/recall,
runtime reachability, arbitrary dataflow or compiler diagnostics. In particular,
TypeScript may identify a parameter/value symbol where native graph-search refuses
to invent a function body. No repository-wide quality improvement is claimed.
The Rust oracle checks seven compile/fail premises; it is not counted as seven
additional declaration-site matches. All fixtures and stores are disposable.

## Scope boundary from the original recommendation

The review explicitly states that resolving `let send = other` transitively is a
later dataflow feature and that full compiler semantics are unnecessary. Those
limitations do not keep the implemented lexical-first behavior incomplete.

Project/package resolution, Rust reexports/receivers, TypeScript aliases and project
contexts, and framework script/template behavior remain recommendation 21 work.
Reverse dependency indexes and narrower projection updates remain recommendation
23 work. Corpus-wide release evaluation remains recommendation 30 work. These are
still explicit deliverables, not discarded requirements. Recommendations 11, 12
and 22 also remain open. The global commit/push gate remains incomplete.

No production source, parser policy, dependency manifest or lockfile changes in this
audit. Parser/source/ranker remain 19/14/23. Source and harness hashes are recorded
in `sources.json`; the compiler/binary/fixture hashes are in `environment.json`.

**Acceptance:** recommendation 20 is complete for its stated lexical-scope requirements. 26 focused tests pass, the new probe passes strict release Clippy, all 50 call outcomes match, and all seven Rust compiler premises pass. All 158 recorded source/harness hashes match; production crate hashes are unchanged from the preceding validation. This is not a full-workspace test run or a corpus-wide quality claim.
