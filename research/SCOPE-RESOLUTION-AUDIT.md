# Scope-aware resolution: current audit

Recommendation 20 is accepted after the [current requirement audit](results/native-implementation/scope-acceptance/README.md). Current parser revision: 19.
The historical increments below retain their original status statements; they do
not override this acceptance. Broader project/package work, dependency indexing
and corpus-wide evaluation remain open under recommendations 21, 23 and 30. File-local scopes, declaration/value bindings,
raw call occurrences and public resolution classes are implemented. The tests
cover parameters, destructuring, closures, order/hoisting, condition bindings,
local aliases, nested functions, persisted provenance and incremental rebinding.
These establish a declared syntactic subset, not full compiler name resolution.

## Current module binding increments

Parser revisions 16/17 add native anchored Rust paths, basic module-item visibility,
lexical named imports and module aliases. Their proof boundaries and independent
compiler probes are recorded in [anchored paths](results/native-implementation/rust-anchored-paths/README.md)
and [import bindings](results/native-implementation/rust-import-bindings/README.md).
The earlier module sections below describe intermediate states, not current gaps.

Parser revision 18 adds typed JS/TS ESM surfaces and bounded native export lookup.
Named/default imports, direct namespace member calls, explicit aliases and
reexports now require an exported target; same-named private declarations cannot
satisfy them. Type-only calls, conflicting stars, incomplete surfaces and
unsupported values retain unresolved reasons. Local lexical shadowing remains
authoritative. [Module-binding evidence](results/native-implementation/js-module-bindings/README.md)
contains the exact subset, mutation tests, Node oracle and source identities.

Source representation 12 additionally retains bounded Node package metadata and
resolves exact package self-references and private `#` import maps before ESM
export lookup. [Package-map evidence](results/native-implementation/node-package-maps/README.md)
includes Node probes and a four-file whatsurvey counterfactual.

Source representation 13 adds bounded pnpm/package.json workspace membership and
workspace-protocol dependency selection. Target names must be unique admitted
members with explicit export mappings. Invariant conditional objects can bind
only when every branch names the same path and includes a default; relevant root
overrides prevent unsupported bindings. [Workspace evidence](results/native-implementation/node-workspaces/README.md)
records pnpm/Node oracles, incremental/rebuild comparisons and a six-file
whatsurvey counterfactual covering five actual calls. Declared tsconfig aliases,
build-output/source-root mapping and condition-dependent targets remain unmodeled;
this is not complete project resolution.

Full namespace/access/receiver semantics, Rust reexports and edition-sensitive
unanchored imports, environment-dependent JS package conditions and aliases, framework regions and
corpus-wide target-precision evaluation remain open. These changes do not claim
full compiler resolution or increased task success from a higher resolved count.

## Qualified callable shadow: reproduced and fixed

Before this change, the public graph returned a resolved edge from `declared` to
`Api.send` for this JavaScript fixture:

```javascript
class Api { static send() {} }
function declared() { function Api() {} Api.send(); }
```

The scope pass found the local function but skipped all declarations for qualified
calls. Same-file qualified lookup then selected the unrelated outer class method.
The failing public regression reproduced the exact false target before the fix.

The scope pass now records the unique class/struct/enum/module declaration keys
that may supply namespaces. A dotted call rooted in a local callable declaration
is marked unresolved with `lexical_member_target_unknown`; it cannot fall through
to the outer member. Direct calls to that callable retain their existing lexical
binding. Unknown/dynamic properties are not guessed. Rust `Type::member` keeps its
separate existing qualification route: a function value with the same spelling
does not by itself hide the type namespace. Full Rust namespace modeling remains
part of the open module-resolution work.

**Evidence at parser revision 8.** The nine-test scope suite passed. New public regressions cover both
JS and TS function declarations and immutable arrow bindings, retain an unshadowed
static-member positive control, and retain the Rust function-value/type-name case.
A reopened index undergoes shadow insertion/removal/const replacement; each sync's
occurrence records equal a clean rebuild, including target identity, unresolved
class/reason, source hash and coordinates. Parser revision 8 invalidates the old
extraction facts. No wire/source representation or dependency change is needed.

**Limits of that increment.** The callable-root fix did not establish class or
module qualification. The local direct-static class case and its initialization
checks are implemented below. Module visibility, complete type/value namespaces
and broader receivers remain open; neither increment claims dataflow support. The
broader package/import/framework subset belongs to recommendation 21. External
resolved-edge percentages alone would not prove precision, and this change makes
no repository-wide precision or task-success claim.

Parser-revision-8 validation passed: **397 workspace tests in 31 suites**, strict workspace/
all-target Clippy, formatting and diff checks. The [captured evidence](results/native-implementation/qualified-callable-shadow/checks.json)
includes source hashes, the failing-before public regression and the passing scope
suite. Dependency manifests and lockfiles are unchanged.

## Direct static members of the visible local class

The next public regression reproduced `nested.Api.send()` resolving to outer
`Api.send`. Parser revision 9 introduces a native file-local class/member index:
lookup first selects the lexical class declaration, then its unique directly
extracted static callable. The occurrence carries the exact member extraction
key and `ExplicitLexical` resolution. Missing/ambiguous or instance-only members
cannot fall through to an outer class. This extends the preceding callable-root
fix; it does not replace it with global qualified-name guessing.

The shared JS/TS adapter marks ordinary static methods as `static_callable`.
Getters/setters are excluded: accessing an accessor and invoking its returned
value is not a direct call to the accessor body. Duplicate extraction keys stay
ambiguous. Names must match the direct dotted spelling; computed/string-property
normalization, inheritance, arbitrary property chains and runtime property
assignment are not inferred. Existing Rust `::` handling remains separate.

Class bindings hide outer values throughout their lexical scope. Calls before
the declaration completes stay unresolved. Method parameter/body intervals are
recorded separately because they execute on method invocation; a call from a
class method can therefore resolve a static member of its own class. Heritage,
computed names and field initializers do not receive that exception. Their
unmodeled class-initialization context is explicit, rather than claiming runtime
initialization order. This remains a conservative syntactic model, not interprocedural
control-flow analysis or a guarantee about reassigned JavaScript properties.

The new field-initializer test also reproduced a JS extraction omission. The
pinned JavaScript grammar exposes a field's `property`; TypeScript uses `name`.
The shared adapter now accepts both fields and walks their initializers, preserving
calls that were previously absent. No grammar or parser dependency changed.

**Verification.** Twelve scope tests pass. New JS/TS cases check nested versus
outer target identity, absent/instance-only members, initialization order, static
getters, calls within method parameters/bodies, computed/field initialization and
unsupported inherited lookup. A five-state static/absent/instance/outer/static
mutation sequence syncs and reopens each generation; complete occurrence records
match an independent clean rebuild at every state.

**Still open.** Imported/default/namespace class bindings, package/module
visibility, TypeScript access-control rules, complete type/value namespace
separation, inheritance, framework regions, and dataflow-sensitive receivers
remain recommendation 20/21 work. The local static-member tests do not establish
repository-wide target precision or answer success. The pre-change failure and
post-change checks are retained under
[class-static-bindings](results/native-implementation/class-static-bindings/).

Parser-revision-9 validation: **400 workspace tests in 31 suites**, strict workspace/
all-target Clippy, formatting and diff checks pass. Captured source hashes match
the tested files. No dependency manifests or lockfiles changed.

## Cargo target metadata foundation

Source representation 10 retains bounded authored target tables and explicit
edition/discovery settings from Cargo package manifests. Package identity remains
valid if target metadata is unavailable; legacy facts omit the optional field.
The independent stored-fact validator rejects malformed targets and partial
unavailable projections. Edit/reopen/clean-rebuild tests exercise changed library
paths, binary feature gates, discovery settings and invalid-to-valid metadata.

This was preparation for recommendation 21, which remains open. At that stage, Rust path
resolution assumed repository-level `src/` for `crate::`; module ownership also
needs explicit inline/external module structure, custom target roots, repeated
`super`, path attributes and conservative ambiguity handling. Raw target facts do
not establish that a file exists or belongs to an active Cargo target. No module
resolution or target-precision improvement is claimed by this increment.

## Native module declaration resolution

Parser 11 now resolves external `mod` declarations with exact source spans,
inline ancestry and supported path attributes. Cargo root discovery distinguishes
custom entries, ordinary module files, automatic target families and build scripts.
Source 11 preserves explicit empty target arrays and build settings. The native
implementation uses existing decoded facts and walked paths; no compiler or Cargo
process is part of indexing.

The [disposable oracle](results/native-implementation/rust-modules/cargo-oracle.json)
checks 25 Cargo cases and compiles a Rust module fixture containing wrong-path
`compile_error!` decoys. It caught a material detail obscured by a broad reading of
the documentation: edition-2015 automatic discovery defaults are per family, and
empty arrays count as explicit family declarations. Native root tests encode these
results. The [driver](scripts/cargo_roots_oracle.py) is reproducible without network
or third-party packages; its compiler results are correctness evidence, not timing.

Resolution refuses both alternative files, unsupported path/macro attributes,
unknown roots and conflicting directory contexts rather than guessing. A worklist
propagates physical-parent contexts through path attributes and stem contexts
through ordinary external modules; each file has at most two such states. Shared
files resolve only when their reached contexts agree. Cycles terminate and
unreachable source files retain unknown context. Compiler probes exposed why the
source filename alone cannot determine a path-loaded module's child directory. Raw cfg facts
remain potential relationships; no active build is selected. Unsupported escaped
path strings, file-level inner path attributes, block-local modules and workspace
edition inheritance are explicit limits. Ancestry is capped; no target outside the walked workspace is opened.

At parser revision 11, recommendations 20/21 still required the logical module tree, anchored use paths,
imported bindings/reexports, visibility, namespaces and cross-module call/type
resolution. The parser-11 generic `crate::` handling still had its repository-level
`src/` assumption for those paths; this increment supersedes that helper only for
identified module declarations. Recommendation 23 also retains the requirement to
replace conservative rebinding of all external module declarations with a complete native dependency model.

## Parser 19: destructuring declarations versus executable expressions

The independent embedded-script source probe found that the ordinary JS/TS
adapter omitted calls in destructuring defaults, while its separate declaration
collector incorrectly included identifiers from default expressions and computed
keys as bound symbols. The adapter now uses the existing native scope pass's
pattern-only traversal to collect declarations, and visits pattern expressions
separately for calls. This affects ordinary JS/TS as well as the new range adapter;
parser version 19 expires earlier extraction facts.

Regression fixtures distinguish actual nested object/array/rest/renamed bindings
from computed keys and default callees, verify exactly one occurrence per authored
call, and preserve lexical/import provenance. The new range adapter additionally
translates lexical bounds and key references, rather than only displayed spans.
It does not yet register framework extensions or establish module/instance scope
inheritance. [Implementation, independent source probe and validation](results/native-implementation/embedded-script-coordinates/README.md).


## Source 14: native TypeScript configuration inputs

[Raw configuration facts](results/native-implementation/typescript-config-facts/README.md)
now preserve the authored metadata needed by a future project resolver, including
wildcard declaration order that a sorted JSON map would lose. Actual compiler
probes demonstrate equal-prefix tie sensitivity. These are source-owned inputs,
not resolved alias edges: native project selection, inheritance and dependent
binding invalidation are still open under recommendations 21/23. The projection
never follows an excluded parent or executes a compiler/runtime configuration.


## Native selected-configuration inheritance

The core now resolves explicit relative configuration inheritance over admitted
Source-14 facts, keeping each option's origin, ordered path patterns and all base
hashes. This prevents later alias work from accidentally interpreting generated
Svelte configuration paths relative to the child config. No selected project is
inferred by this helper. Seven copied real configurations and nine synthetic
cases match TypeScript 6.0.3 for the checked inheritance fields and origin rules.
Parent edits/deletion/recreation match clean rebuilds via published source records.
[Evidence and limits](results/native-implementation/typescript-inheritance/README.md).

Recommendation 21 remains open: default project selection, overlapping projects,
excluded configuration boundaries, module-resolution options, aliases, build-output
mapping and framework visibility are separate requirements. Recommendation 23
still needs those dependencies connected to binding invalidation.


## Native paths/baseUrl dispatch

The selected-configuration dispatcher now preserves compiler precedence and
origin rules, including matched-map misses that suppress baseUrl. Its loader
callback retains unexpanded substitution text because a literal `.js` target and
one introduced by `*` require different extension priority. Six native regressions
and 17 compiler comparisons check the dispatch stage using an exact-file research
callback. This does not establish a complete module loader or default alias edges.
[Evidence and open integration](results/native-implementation/typescript-alias-dispatch/README.md).
Recommendations 21/23 remain open for project context, loading and invalidation.


## Native modern file lookup

Inherited alias dispatch now composes with a native loader for explicitly selected
bundler/modern Node contexts. 173 supported compiler cases agree, and two package
entries remain explicitly unmodeled. Negative presence probes and unavailable
boundaries are retained as facts for later invalidation. Published-fact mutation
tests verify priority changes against clean rebuilds. Default project/alias-edge
integration and persisted dependencies remain open under 21/23.
[Evidence and limits](results/native-implementation/typescript-file-loading/README.md).


Default integration must validate negative file probes against policy/coverage
information: lack of an admitted source record is not proof that a higher-priority
file is physically absent. Package-boundary handling is implemented; ordinary
unavailable-candidate classification remains part of the project/inventory work.
