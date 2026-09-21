# Binding-surface invalidation

This implements the next part of recommendation 23: a changed source file no
longer automatically seeds transitive consumer repair. The recommendation remains
open for persisted reverse dependencies and selective raw-fact loading.

## Native behavior

For Rust, JavaScript and TypeScript, the projector compares old and new binding
surfaces. Symbol IDs, paths, kinds, names, qualified names, parentage, visibility
and semantic attributes must agree. Authored ESM import/export names, specifiers,
type-only flags and completeness must agree. Source coordinates, signatures and
async flags do not affect this resolver's cross-file target choice and are omitted
from that comparison. Module syntax coordinates are similarly omitted.

The edited file always remains in the batch. Its nodes, signatures, source regions,
documentation, occurrences and outgoing edges are freshly projected. Stable-target
updates from the preceding increment retain unchanged sources' incoming edges.
Changing outgoing calls therefore does not itself require rewriting callers.
Signature-only edits likewise refresh the target without implying type inference:
the current resolver selects by kind/name rather than parameter or return types.

Changed binding surfaces retain raw-name/import dependency repair and conservative
incoming-edge closure. File-presence, package-boundary and package-manifest changes
retain their existing repair paths. Missing facts, quarantine and mismatched old
file/manifest fingerprints prevent the surface shortcut. HTML/CSS retain broad
cross-language repair. No dependency or representation version was added.

## Correctness argument and limits

Starting from a coherent projected generation, consider a single edit with an
unchanged file set and package context. If this comparison succeeds, every
cross-file symbol identity/name/kind/parent/visibility/attribute consulted by the
current resolver remains equal. ESM lookup reads the preserved semantic module
fields; it does not use import/export coordinates. Rust origin spans select the
caller's own module, so unchanged callers retain their own origin coordinates.
The edited file's lexical lookup and all its coordinate-bearing facts are rebuilt.
Consequently, omitted display/location changes cannot change an untouched caller's
chosen target under this resolver. All other symbol/module differences still seed repair.

The comparison must evolve whenever the resolver begins consulting additional
cross-file inputs. This is not a compiler/type-system equivalence claim, nor a
repair guarantee for arbitrary corrupted or manually altered graph state. Package,
file-presence, missing-cache and unsupported-language cases deliberately retain
broader existing invalidation.

## Experiments

The native probe hashes every generated source to establish that only `n0.js`
changed. It captures actual submitted batches, then compares complete nodes, edges,
source units and occurrence records with an independent clean rebuild. Both native
adapters are exercised; Grafeo is reopened before the comparison.

| Chain files | Previous MemoryStore upserts | Current MemoryStore upserts | Current Grafeo upserts after reopen |
|---|---:|---:|---:|
| 2 | 2 | 1 | 1 |
| 8 | 8 | 1 | 1 |
| 32 | 32 | 1 | 1 |

The before counts come from the retained
[stable-target baseline](../stable-target-baseline/README.md). Persistent chain
before counts were not separately captured there. All six current cases match
clean rebuilds and preserve 1/7/31 call edges. The target-only replacement probe
also still preserves its incoming call and caller-owned occurrences in both stores,
including persistent reopen. These are work-count and correctness results, not
latency or memory measurements. Builds/linting overlapped during the initial probe;
no timing conclusion is drawn from either capture.

The production matrix covers 12 scenarios in each adapter (24 combinations):

- JS literal width, changed outgoing calls and coordinates, parameters/async;
- TS parameter/return-signature changes;
- Rust body/documentation and signature changes;
- JS export removal, reexport retargeting, and imported local export retargeting;
- Rust visibility changes, target rename, and shifted duplicate declaration IDs.

Every case checks exact submitted upsert paths, full clean-rebuild equivalence,
persistent reopen, and a subsequent no-op with no submitted upserts. An unchanged
README is present to exercise ordinary non-parser facts. The missing-cache test
now changes only a body and asserts that both the edited owner and missing-cache
owner are reparsed, proving conservative fallback independently of binding changes.
Existing suites cover ambiguity addition/removal, import precedence, packages,
module paths, coverage, occurrences and source units.

## Validation and remaining work

- Broad core/library and affected integration validation: **265 tests / 11 suites**.
- Final strengthened matrix/incremental run: **6 tests**, including 24 adapter/scenario combinations.
- Strict workspace Clippy across all targets and strict native probe Clippy: passed.
- Final native release probe, binary identity and all 159 source hashes: `checks.json`.

The only changes after broad validation were documentation markup and strengthening
one missing-cache regression; the final focused run and lint cover those changes.
The initial documentation-only Clippy failure is retained.

Dependency maps and incoming closure are still rebuilt from broad graph/raw-fact
reads. ESM preparation still consumes broad module facts. Grafeo still prepares a
whole graph generation, persists whole occurrence state and rebuilds derived
indexes. Persist reverse dependency records with the generation, use them to select
fact reads and repair work, then measure no-op/body/API/rename/delete/ambiguity/cache
repair workloads before accepting recommendation 23. No model-task-success or
end-to-end performance gate is claimed here.
