# Native TypeScript candidate presence

The preceding [oversized-candidate reproduction](../typescript-file-loading/unavailable-candidate-gap.json) demonstrated false fallback: admitted facts omitted a preferred TypeScript file and the composed loader chose JavaScript. This increment requires a presence provider for unindexed candidates. Only proven absence permits fallback. Unavailable files, unknown metadata, provider errors and exhausted budgets return explicit reasons. Cached errors retain their meaning, and a provider cannot invent admission.

The native `module_presence::Capture` adapter observes namespace metadata without reading source contents. It distinguishes regular files, directories, absent paths, opaque entries and unknown metadata; stops at observed symlink prefixes; records missing parents; and caps observations at 4,096. Validation rechecks kinds in prefix order and rejects changed observations. This is not an atomic snapshot: callers still own source-generation coherence, project selection and persisted freshness. These helpers are not yet wired into default alias bindings.

## Evidence

- Nine core loader tests cover the existing resolution subset plus unavailable/unknown precedence, cached errors and inconsistent providers.
- Four adapter tests cover missing and obstructed paths, invalid paths, metadata bounds, symlink prefixes, changed namespace kinds and content changes that do not alter presence.
- Three composed persistence tests cover normal priority/config mutations, package boundaries, and oversized/ignored ordinary candidates. Each mutation compares reopened published facts with a clean rebuild. Removing the preferred file allows JavaScript; admitting it selects TypeScript; ignoring it produces an unavailable result.
- `oracle.json`: 173 supported TypeScript 6.0.3 matches and two explicit package-directory refusals. The existing matrix was rerun with the native presence adapter.
- `unavailable-candidate.json`: native resolution now refuses the oversized preferred file; the compiler selects that TypeScript file. This confirms priority and safe refusal, not positive target equivalence.
- Strict workspace/all-target Clippy and the offline/locked release probe build pass. Focused validation logs and source hashes accompany this report.

No production dependency was added. Sibling sources and CodeGraph indexes are unchanged. Source/parser/ranker remain 14/19/23. Recommendations 21/23 remain open; the ledger remains 23 checked / 7 open.

Final focused validation: **202 tests passed, zero failed**, including 191 core/property, four metadata adapter and seven config/inheritance/file-loader integration tests. All 156 recorded crate/harness/driver hashes matched. Formatting and whitespace checks passed; all recorded final sessions exited zero. No full-workspace test count is inferred from this focused run.
