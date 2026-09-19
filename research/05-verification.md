# Verification and correctness certificate

## Obligations

This branch must preserve ordinary exact symbol lookup and supported traversal, fix the documented concrete defects, expose ambiguity/coverage honestly, leave the user's main checkout and external source trees untouched, and retain reproducible evidence separating correctness repairs from ranking experiments.

## Evidence and scope

- `results/baseline-tests.log`: original workspace tests pass before modifications.
- `results/initial-regression-failures.log`: the first accuracy suite reproduces eight failures before the query/resolution/sync fixes. Three extractor regressions were already fixed at that point; this log is **not** claimed to be an untouched-baseline run of every final regression.
- `results/semantics-baseline.json`: synthetic results from the saved baseline harness, before production edits.
- `results/semantics-fixed.json`: the same request set after repairs. Some formerly arbitrary answers now correctly produce ambiguity errors.
- `crates/graph-search/tests/accuracy.rs`: 25 end-to-end regression tests through the public library and persistent Grafeo adapter.
- `results/final-tests.log`: complete final workspace suite.
- `results/reproduction.log`: successful complete baseline-to-patched reproduction run (exit 0).
- `results/source-stability.json`: every walked external source file remained unchanged during measurement.
- `results/cli-smoke.json`: CLI-specific byte-envelope, language rejection, and hidden-file flag checks.
- `results/clippy.log`: `cargo clippy --workspace --all-targets -- -D warnings`.
- `results/*expanded*.json`, `*retrieval.json`, and `*natural.json`: fixed query labels and ranked outputs, not just aggregate scores.
- `results/comparator-provenance.json`: CodeGraph FTS schema, source-hash freshness audit, and environment versions.
- `results/application-edges.json`: source-confirmed application edges checked before/after.

Changed Rust files are formatted with rustfmt; `git diff --check` is clean. Repository-wide formatting is not applied to unrelated files. The research harness is a separate Cargo workspace with a committed lockfile; its shared runtime dependency versions match the main workspace lockfile. The additional dependencies present only in the main lockfile are CLI/dev-test dependencies.

## Function resolution trace

1. JS/TS call extraction runs through `JsExtractor::reference`, where `(qualified_name, key)` is now destructured correctly. The fact's `from_key` reaches `Projector`'s `symbol_ids`, so `edges_for_extraction` can choose the symbol instead of the fallback file.
2. Nested call visitors walk both the callee subtree and arguments. They retain the outer unresolved reference; they do not infer a receiver type from an unrelated method suffix.
3. Named JS imports are applied to facts after extraction. `resolve_reference` treats `via_import` as authoritative and checks candidates in the resolved target file. Dynamic parameter references stop before that binding stage.
4. Qualified resolution retains candidate multiplicity; unknown prefixes cannot be stripped into a bare global hit. Simple Rust `self` calls use the lexical owner known to the extractor. This is sufficient for the tested inherent-method cases, not for every trait/receiver case.
5. Query target resolution distinguishes missing, unique, and ambiguous results. Exact IDs returned by `symbol` are supported by downstream queries.
6. Changed sync reprojection rebuilds the reference set before store replacement, so unchanged incoming callers are included in the batch. Removed files do not remain candidates. Lazy freshness checks compare running parser/schema versions with the manifest and entries.

## Sufficiency

Each fix is scoped to its reproduced contract. Fixing JS call ownership alone would still leave receiver calls, import bindings, freshness, and ranking failures, so it is not presented as a complete accuracy solution. FTS improves retrieval under constant extracted candidates but cannot recover unsupported Svelte definitions or repair false graph bindings. Conservative resolution can reduce resolved counts while improving precision; the report never treats resolution rate as correctness.

The final implementation deliberately retains baseline multi-term scoring. An intermediate attempt at general punctuation/stop-word normalization caused a task-language regression and was narrowed before final measurement. The replacement lexical ranker remains research-only pending broader evaluation.

## Regression paths

- Existing engine conformance, core properties, parser fixtures, CLI contracts, library end-to-end tests, and documentation tests run with the new code.
- Exact lookup is checked on 90 sampled production functions, not just synthetic names.
- JS and Rust nested receiver calls, JS aliases, explicit source extensions, duplicate methods, and callable parameters exercise the modified extraction/binding paths.
- Files/text are checked both under a subdirectory and after disk changes while an `Index` remains resident.
- Graph limits, filtered results, bad globs, identity paths, explore snippets/budgets, two-hop bridges, and diamond-shaped impact exercise downstream assembly.
- Sync tests cover target edits, additions, and deletion with unchanged callers. Existing tests cover ordinary no-op and rename behavior; this is not a comprehensive fault-injection campaign.

## Material tradeoffs

- Any changed sync reparses the walked tree. This is intentional until raw facts/reverse binding dependencies exist.
- File search scans rather than using the previously inconsistent resident shortcut.
- Ambiguous names now fail explicitly; callers must choose exact IDs.
- Unsupported qualified resolution now dangles instead of inventing an edge.
- Explore can add intermediate nodes after its k lexical seeds, subject to result/byte caps; evaluation restricts retrieval metrics to the first eight returned items.
- Tiny JSON budgets return an error if metadata cannot fit.
- Parser version 2 invalidates old projections. No output schema fields were added, but the new ambiguity error and compact bounded explore JSON are behavior changes.

This is self-verification using the reasoning-semiformally checkpoints, not an independent second-agent review. It validates the repaired paths and records the remaining research limits; it does not certify the graph as compiler-complete.

VERDICT: LIKELY_CORRECT
CONFIDENCE: medium
SUMMARY: The reproduced defects have targeted fixes and passing regression evidence; semantic binding completeness, production ranking, and bounded-work architecture remain explicit follow-up work.
