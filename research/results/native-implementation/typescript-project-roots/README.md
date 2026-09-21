# Native TypeScript project root files

`typescript_roots::enumerate` interprets the effective `files`, `include`, and
`exclude` fields for an explicitly selected configuration. The preceding
inheritance implementation retains the declaring configuration of each field and
option; root enumeration now uses those origins rather than rebasing inherited
paths onto the child configuration.

## Implemented behavior

- Explicit `files` entries remain roots even when missing, excluded by wildcards,
  or outside the normal extension set. They do not become admitted sources.
- Default include is recursive only when both `files` and `include` are absent.
  An empty list is meaningful. Explicit excludes replace default output-directory
  exclusions; `outDir` and `declarationDir` retain independent origins.
- Native include/exclude matching uses bounded dynamic programming, without a
  regex or glob dependency. It handles recursive and component wildcards, implicit
  directory globs, exclusion prefixes, hidden/package directories, explicit hidden
  paths, minified JavaScript rules, and UTF-16 question-mark semantics.
- Root extension groups preserve source/declaration/runtime priority, literal
  roots, include order and compiler traversal order. Declaration/runtime pairs
  are not assumed mutually exclusive. JSON roots require a JSON include pattern
  unless explicit; allowJs/checkJs/jsconfig defaults are distinguished.
- Paths/specs are bounded, with explicit reasons for unsupported templates,
  workspace escape, malformed fields, trailing include recursion, oversized
  inventories, spec limits and exhausted shared comparison work. Partial root
  sets are not returned on failure.

This API requires a complete case-sensitive file namespace. An admitted-only set
cannot prove that an excluded higher-priority candidate is absent. The controlled
oracle and persistence fixtures provide complete inventories; production discovery
and namespace capture still need integration. Root enumeration is not imported-file
closure, project ownership, reference traversal or a default alias binding.

## Validation and findings

The compiler oracle uses TypeScript 6.0.3 already installed in the sibling repository
and disposable fixtures. It compares root-file sets rather than diagnostics or
full-program success. Empty/missing explicit roots can still have compiler diagnostics.
`environment.json` records the compiler, binary, driver and fixture hashes. No sibling
source or CodeGraph index is changed and no production dependency is added.

The first 37-case oracle agreed. A subsequently added hand-authored unit expectation
incorrectly excluded `.d.cts` beside `.cts`; the existing compiler oracle showed both
are retained. The test was corrected, not the native algorithm. Inspection also
found jsconfig's implicit allowJs remains true when checkJs is false; that production
default was corrected and a 38th compiler case added. Final contract review added
rejection of configDir templates in literal files entries, and identified inferred
module-resolution JSON defaults that the explicit-bundler fixtures did not cover.
Sixteen additional cases now cover omitted options, legacy modules, Node modes
and explicit resolution overrides (54 total). Initial logs and oracle data
are retained separately from final evidence. The first persistence fixture also
used graph-search's default `dist` exclusion, so its inventory was incomplete.
The fixture now explicitly disables directory exclusions, as the compiler oracle
already did; the production policy is unchanged. That failed run is retained.

Four root tests and two pattern tests cover meaningful boundaries, errors and
limits. The persistence test composes inheritance and roots over reopened indexed
facts, checking parent include edits, child exclude replacement, file deletion and
explicit missing roots against clean rebuilds. It does not assert default graph
edge publication.

Project discovery, references, imported-file closure, overlapping contexts,
module-mode selection, default binding publication and persisted presence dependency
invalidation remain open. Recommendations 21/23 and the ledger remain unchanged
at 23 checked / 7 open. Source/parser/ranker remain 14/19/23.

The expanded oracle initially collided fixture directories named from Python None
and the string `none` on the case-insensitive filesystem. The omitted-option case
now has the distinct name `omitted`; this was a fixture-name correction with no
production change. That failure is retained separately.

Final validation: **205 focused tests passed, zero failed** (197 core/property
and eight TypeScript persistence tests). Strict workspace/all-target Clippy,
offline/locked release build, **54 compiler root-file-set comparisons**, formatting
and whitespace passed. All 158 crate/harness/driver hashes match. Session 44269
completed tests/lint/build then exited 1 at the fixture collision; session 81672
completed the corrected oracle and formatting with exit 0 using the same production
binary. No full-workspace test count is inferred from this focused validation.
