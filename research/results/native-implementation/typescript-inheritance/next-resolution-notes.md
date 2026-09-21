# Next native alias work: compiler observations, not implemented behavior

The installed TypeScript 6.0.3 compiler (hash in `alias-followup-oracle.json`) was
inspected at `getPathsBasePath`, `tryLoadModuleUsingOptionalResolutionSettings`,
`tryLoadModuleUsingPathsIfEligible` and `tryLoadModuleUsingPaths`. Four virtual-file
compiler probes in that JSON record inputs and results; no sibling files changed.
These are correctness cases for subsequent work, not evidence that graph-search
already resolves aliases or a broad module-mode compatibility claim.

1. `baseUrl`, if present, supplies the paths base even if the paths map was
   inherited. Otherwise use the declaring configuration directory (`pathsBasePath`).
2. Exact keys beat wildcard keys. Wildcards prefer the longest prefix; equal
   prefixes retain authored property order (covered by the preceding raw-facts
   compiler oracle).
3. A matching paths key whose substitutions all miss suppresses the baseUrl probe;
   an unmatched paths key permits it. Node/package fallback is a separate layer.
4. An explicitly suffixed substitution can choose its exact existing file before
   extension substitution: a mapped `x.js` wins even when `x.ts` also exists.
5. In this compiler version, empty matched wildcard text leaves the substitution's
   literal star. A virtual filename containing `*` demonstrates that branch;
   this unusual case is not a recommendation to create such source filenames.

A native implementation needs separate outcomes for no pattern match, a matched
pattern with no file, unsupported settings and a selected file. Reusing the
current generic relative-file candidate list blindly would miss these distinctions.
Module mode, package directory handling, suffix settings and unavailable source
boundaries also need explicit treatment. Project context must survive reexports;
nearest-config guessing alone cannot represent overlapping compilation projects.

Invalidation must include negative lookups, not only successful dependency hashes:
a missing parent can appear later, an earlier substitution can become available,
and a nearer configuration boundary can be added or become unavailable. The
current inheritance error deliberately returns no partial effective config; a
future catalog therefore needs separate attempted-path metadata or conservative
config/presence invalidation before claiming precise dependency updates.
