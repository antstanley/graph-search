# Rust scope-key growth

## Reproduction and change

The bounded extraction regression failed before the production change: at module
nesting depths 4, 8 and 12, total symbol-key bytes were 898, 15,266 and 245,634.
The depth-12 source contains only 184 bytes. See `before.txt`.

`Extractor::qualify` prepended every ancestor key, each of which already held its
own ancestor chain. It now prepends only the immediate parent. For the same
fixtures, totals are 470, 1,790 and 4,438 bytes; depth 16 yields 8,862 bytes for a
232-byte source. These are deterministic allocation-volume proxies, not measured
RSS or latency. The regression's polynomial ceiling rejects the old exponential
construction. Full qualified names still repeat within a chain, so this is not a
claim of linear total storage or bounded parser recursion.

## Verification argument

Premises: each pushed scope key already records its ancestors; symbol parents and
reference owners copy the immediate scope key; display qualified names are built
separately. The old loop repeatedly copied prefixes. Replacing that loop with the
last scope retains one complete ancestry chain. Top-level and one-level keys stay
unchanged; deeper keys change and parser revision 12 forces fact refresh.

The nesting regression verifies every parent points to one emitted symbol and
the leaf call belongs to its caller. A separate fixture verifies same-parent
duplicate declarations keep equal keys (preserving ambiguity handling), while
identical leaf names under different parents remain distinct. Qualified names
remain unchanged. Existing extractor, occurrence, module, scope and incremental
suites are the regression gates; this change does not add lexical/import reach.

`sources.json` records the final code hashes. No dependencies were changed.
Validation results are recorded separately once the workspace run completes.

Final key-growth validation: all 420 workspace tests across 32 suites pass. Strict
Clippy, formatting and whitespace checks pass. The only post-start test edit was
removing the measurement print; assertions and production behavior were unchanged.
The later parser-13 module-boundary change is separately tested and documented.
