# Native selective extraction reads

Recommendation 23 remains open. This increment provides the selective raw-fact
read operation required by dependency-driven reconciliation. The current projector
still hydrates the full manifest; no end-to-end sync improvement is claimed here.

## Implemented contract

`GraphStore::extraction_facts(paths)` returns a file-keyed map containing only the
requested cached extractions. Unknown paths and absent caches are omitted. A present
empty extraction remains a present map entry. The compatibility default loads the
full manifest for nonempty requests; MemoryStore instead performs direct selected
lookups and clones only the requested shared facts.

Grafeo selects record references from the immutable descriptor associated with its
opened generation. It groups references by pack, then by exact byte range and record
hash. It opens only packs containing requested records, seeks to those ranges, reads
each unique slice once and checks its record hash before JSON decoding. It does not
read unrelated pack data or decode unrequested records. Returned extraction records
must match the entire per-file fingerprint in the pinned manifest header.

Initial generation opening still verifies complete pack/record hashes and bounds.
Full manifest hydration still rechecks whole pack hashes. Selected reads instead
recheck each selected record hash; they do not assert that unselected bytes remain
unchanged after opening. This is sufficient to authenticate the returned records
against the pinned descriptor. Generation leases keep old lazy reads available
across later publication and reclamation. No on-disk layout or dependency changed.

Selected hydration merges weak identity-cache entries without evicting other
selected identities or retaining extraction payloads. Mutation or expiration still
invalidates the existing identity-based reuse proof. Legacy embedded manifests use
the full-read compatibility path. Empty requests perform no fact I/O; handles made
unavailable by post-publication durability errors still refuse reads.

## Native evidence

The production range reader is exercised through a counted `Read + Seek` input.
Two selected paths share one 10-byte record in a 4,108-byte pack containing an
unrequested 4,098-byte record. The reader consumes **10 bytes**, returning both
selected paths. Corrupting unselected bytes does not affect that selected read;
corrupting selected bytes fails their checksum. This measures the range-reader
primitive, not filesystem latency or complete generation-open I/O. Inspection of
its production caller verifies that it performs only file metadata operations
before passing the opened file to that same reader.

Other regressions establish:

- A removed unrequested pack does not prevent selected reads from another pack;
  requesting the removed record or fully hydrating the store fails.
- Unrequested malformed typed JSON in the same pack is not decoded; selecting it
  or fully hydrating fails. Selected facts are validated, not every cold value.
- A wrong selected manifest fingerprint fails rather than returning empty facts.
- Separate selected reads preserve prior weak identities, and dropping results
  leaves no retained raw-fact payload in that identity cache.
- Both native adapters omit unknown/missing caches and return the requested facts.
- An old reader retains its original facts through four later publications, while
  a reopened reader sees the new facts.
- Legacy embedded manifests preserve compatible selected results.
- Existing generation failure/retry, crash, publication and reader-lifetime checks
  now also assert selective-read agreement with each handle's committed manifest.
  Post-publication unavailable handles reject both nonempty and empty selections.

## Validation

The first build exposed a test fixture incorrectly assuming `FileEntry: Default`;
the fixture now specifies its complete fingerprint. The failure log is retained.
The initial pack-granularity implementation passed 48 engine tests before the
range reader was added. Final validation passed **49 engine tests**, followed by
**6 incremental integration tests**, including the 24 adapter/scenario matrix and
strengthened missing-cache fallback. Strict workspace lint and the frozen source
identities are recorded in `checks.json`.

No production dependency was added. The captured source set contains 157 Rust and
Cargo inputs (this capture does not include the separate research harness).
The 10/4,108-byte fixture is a direct primitive I/O assertion, not a workload benchmark
or evidence that normal sync already uses selective reads.

## Remaining integration

Persist compact reverse dependencies and module surfaces in the same generation,
then use them to choose raw-fact paths. Reconciliation and publication must preserve
untouched fact references explicitly rather than interpreting an unhydrated entry
as a missing cache. Keep legacy/missing-dependency fallback and exact fingerprint
validation. After wiring that path, measure no-op, body/API edits, rename, deletion,
ambiguity changes and cache repair against clean rebuilds. Generation-open checks,
whole graph preparation and other remaining costs must be reported separately.
