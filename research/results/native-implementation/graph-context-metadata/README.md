# Diagnostic: item metadata with expanded source rendering

This is an intermediate evaluation diagnostic, not the final graph-policy gate.
Both arms completed 348 stable trials without errors. Repeats and paired actions
are deterministic. One partial region improves with graph context: the third
`whatsurvey.contact-policy.debug` region rises from 53.23% to 56.45%; all other
labelled metrics match. Full-protocol line sets differ on 47 tasks.

The renderer exposes complete item metadata, including 196 caller-impact records
across the 58 graph-enabled first queries. However, numbered source expansion plus
metadata serialization exceeds the independent 16 KiB transport cap in every
first-query response: **zero of 58 top-level metadata footers survives completely
in either arm**. See `rendering-checks.json`. This formatting artifact prevents a
fair assessment of graph edges and other top-level metadata.

The final implementation therefore delivers compact native API JSON directly.
Evidence accounting reads actual primary/excerpt text from that complete document
and verifies original coordinates/content and supplied source hashes. The final
[graph-context native-JSON capture](../graph-context-native-json/README.md) supersedes
this diagnostic for policy decisions. Historical files retain their exact source
and driver fingerprints. No model task-success or general graph-quality claim is
made from this intermediate experiment.
