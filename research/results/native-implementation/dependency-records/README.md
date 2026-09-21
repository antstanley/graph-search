# Generation-owned dependency records

Recommendation 23 increment. Native implementation; no added dependencies.

## Implementation and limits

- `core::dependencies::DependencyIndex` keeps exact per-file header identities,
  extraction availability, defined names, unresolved/non-dynamic reference names,
  authored import/reexport specifiers and normalized ECMAScript surfaces. Reverse
  raw-name postings, selected-module postings and incoming file relationships are
  built once per published generation. Module selection uses the existing native
  `resolve_specifier`; it does not claim TypeScript project-aware resolution.
- The binding fingerprint uses the existing normalization contract: IDs, kinds,
  names, visibility, parentage and semantic attributes survive; source coordinates,
  signatures, async presentation and module statement spans do not seed consumer
  repair. Unsupported or missing surfaces remain conservative.
- MemoryStore invalidates dependencies on direct mutation and constructs them when
  committing a coherent manifest. Grafeo constructs them in the private prepared
  generation. Generation format 7 commits `dependencies.json` with the other
  artifacts. A null record explicitly preserves fallback for incomplete direct
  adapter fixtures. Older generations remain readable without a dependency index.
- Reopen authenticates the dependency artifact and checks version, exact headers,
  extraction availability, reverse-name/module maps and relationship owners. The
  selected index belongs to the reader's generation, including after later writes.
- Projector uses the index for repair selection. Missing raw caches still broaden
  repair; Rust context, package boundaries, HTML/CSS and file-presence changes keep
  conservative behavior. Closure includes selected modules even without resolved
  graph edges. Wrapper adapters in the differential suite delegate the new port,
  and the suite asserts the cached path exists before exercising edits.

This does **not** finish recommendation 23. Reconciliation still hydrates all raw
facts and takes complete graph snapshots. Publication still constructs the compact
index from the complete final graph/manifest. Explicit retention of unhydrated
records, selective actual reconciliation, and workload measurements remain. No
latency, peak-memory or end-to-end I/O improvement is claimed here.

## Verification rationale

Premise: dependency selection may change which unchanged files are reprojected;
source-owned changes must always remain pending, and clean-rebuild equality is the
correctness oracle. The compact path uses `surface_unchanged` followed by
`repair_paths`; adapters without the port use the existing full-fact path. The
selected-module reverse map includes syntactic forwarding records, so unresolved
symbol binding cannot erase a file dependency.

A first differential run found needless leaf-file upserts for barrel retargeting:
export aliases had been added to ordinary definition names. The correction retains
export surfaces in the module record and module dependencies, but uses actual
node/raw-symbol definitions for generic raw-name repair. The final differential
run must preserve the previously required upsert sets as well as graph/source/
occurrence equality.

Initial validation also found an iterator type error (`&str` paths require owned
conversion) and a strict-Clippy function-length limit in generation admission.
Both were corrected before final validation. Initial and final logs are retained.

`sources.json` fingerprints the production/tests/build inputs and the affected
standalone probe wrapper. `checks.json` records completed commands and exit codes.

The broad run also found a format-1 emulation fixture that retained the new
dependency artifact. Generation admission correctly rejected it. The fixture now
removes that artifact and descriptor key, like the other newer artifacts. No
production source changed for this repair. Both source fingerprint sets and the
failed broad-run log are retained; remaining suites run separately.

## Final result

All 465 tests in the core, engine and graph-search test suites passed across
32 suites: 168 tests in the 19 successful suites preceding the legacy-fixture
failure, 43 tests in the seven remaining/repaired integration suites, and
254 tests in six core/engine suites. The separately focused 11-test run passed
(including 24 binding adapter/scenario combinations). Strict workspace Clippy,
standalone-probe Clippy and the repaired fixture Clippy passed. All 161 final
source/build fingerprints match. No validation process remains running.
