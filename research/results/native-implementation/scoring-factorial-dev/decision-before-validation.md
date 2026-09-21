# Decision before validation

The development capture is stable and baseline equivalence checks passed.
Select **clipped IDF + BM25F normalization**, keeping the existing split analyzer,
fields, weights, k1/b, document population, exact-name tier and tie-breaks fixed,
as the candidate for further validation.

Development target-file presence in the top eight rose from 11/20 to 13/20:
`nanus.context.change` and `nanus.read.change` gained; no task lost top-eight
presence. Top-50 presence remains 18/20 for every variant. Positive IDF alone
fell to 10/20, so do not bundle it into the candidate. Independent per-field
saturation also fell to 10/20. Analyzer-only and qualified-field-only controls
did not change this binary metric (that does not prove their rankings identical).

Next use the 14 currently source-valid tasks carrying the historical `heldout`
label. Those labels were already exposed in earlier experiments, so this is a
separate-family validation set, not a fresh or blinded test. Selection is fixed
before reading that run. Metadata candidate ordering alone cannot authorize a
production default change: context-byte delivery and broader query classes still
need the existing integration/evidence gates.
