# Graph-context ablation with the historical evidence renderer

348 paired trials completed without errors or fingerprint drift. Semantic graph
context and graph-disabled control use identical lexical policy and request budgets.
All repeats are deterministic, and paired tool actions agree. Complete-region and
file-recall counts are unchanged. One partial region differs: disabling graph
context increases `nanus.fresh-runtime-tools.debug` from 1/6 to 1/2 coverage. No
other labelled evidence metric changes. Delivered line sets differ on 50 tasks.

**Interpretation limit discovered during review:** the historical taskbench renderer
omits per-item metadata, including caller-impact summaries, retrieval provenance
and node identity fields beyond the candidate heading. The API still spends its
payload budget on these fields. Thus this capture can measure source coverage
under that projection, but cannot fairly measure the consumer value of all graph
context. `relationship_evidence_recall` itself measures complete coverage of source
regions supporting a relationship, not correctness or usefulness of returned edges.

The adapter is corrected in the current worktree. The separate
[metadata-preserving capture](../graph-context-metadata/README.md) must be used for
subsequent graph-context policy decisions. This historical capture remains intact
with its exact driver/source hashes. No default-policy change is justified solely
by this source-only comparison, and no model task-success claim is made.

The raw API identity probe checks 116 responses and 464 common node identities;
all package references resolve and there are no orphan entries. That probe does
not cure the historical renderer's information omission.
