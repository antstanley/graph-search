# Recommendation 25: native Markdown requirement audit

Scope: the declared native dialect in SPEC.md, source representation 7, chunker 8.
The recommendation explicitly permits a subset; full CommonMark rendering and
recursive MDX/container interpretation are not completion requirements.

| Requirement | Current implementation and direct evidence |
|---|---|
| ATX and Setext headings, raw title spans | `core/markdown.rs` heading scanners; CRLF, Unicode, multiline Setext, invalid-marker and code-container fixtures |
| Both fence delimiters, opaque embedded headings | `markdown::opener/closes/regions/fence_fields`; longer/mismatched/unclosed fences; public persisted-fence and large-fragment tests |
| Paragraphs and lists | `markdown_blocks::refine/list`; separate authored blocks and marker spans; continuation/nesting flags; 1,728 mixed-block partition cases |
| Tables and frontmatter | `markdown_tables::scan`, `markdown::frontmatter_end`; escaped pipes, uneven rows, boundary shielding; long-table/header and frontmatter public tests |
| Links as distinct fields | `markdown_links` raw syntax/label/destination/title spans; code/HTML suppression; Unicode/CRLF and malformed-input fixtures; public persistence, update equivalence and cap tests |
| Identify unsupported syntax | Opaque blocks and `contains_unsupported` preserve raw searchable bytes; SPEC declares unsupported nested containers, MDX expressions and link forms. It does not claim a complete syntax-error diagnostic for every unsupported inline construct |
| Heading ancestry and useful parent context | `units::update_headings` retains original title spans; `evidence::extend` proposes separately labeled authored heading/fence/table context; bounded-parent test proves parent labels do not displace allocated body windows |
| Bounded large blocks and coordinates | 80-line/eight-line-overlap fragments, 8,192-unit cap; shared original block descriptors; UTF-8 span validation; distinct source-unit and link-metadata truncation reporting |
| No generated text confused with source | Source spans and source hashes, independently verified excerpts; headings are references, not replicated analyzer terms |
| Equal-budget comparison with fixed windows | `markdown_context.py`; 348 completed public-API trials, 58 source-valid tasks, two arms, three repeats; same ranking/graph/call/byte limits; one recorded control intervention |
| Version/path occurrences survive hash deduplication | `identical_document_versions_keep_path_occurrences_through_updates_and_reopen`: equal initial hashes, distinct file IDs and exact evidence, independent edit, deletion, sync and reopen |

Current validation: 11 public Markdown tests pass, including the versioned-document
lifecycle; 16 Markdown-filtered core tests pass; strict Clippy for the public test
target passes. The link scanner lives under `units::links`, so the Markdown name
filter does not include it; its prior full core run and dedicated link validation
are documented in the link-statistics artifacts. These focused checks do not
replace the final workspace gate.

The equal-budget outcome is mixed, not an unqualified improvement. Documentation
complete evidence improves 2/12 to 8/12; established code tasks tie 12/34; newer
routing regresses 10/12 to 9/12. Individual losses and source/binary/index stability
are recorded in `results/native-implementation/markdown-context-docs/README.md`.
The structure is retained for source fidelity and demonstrated documentation value.
Ranking/context regression resolution remains explicitly open under 10–12 and the
phase-2 release gate. Completing the specified parser and comparison does not close
those requirements, document-comment extraction (9), or the full goal.
