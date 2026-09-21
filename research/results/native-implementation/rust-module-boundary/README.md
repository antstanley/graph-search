# Rust module lexical boundary

A public graph regression reproduced a false call edge from `child::invalid` to
its parent module's `send`, despite no import or qualified path. `before.txt`
contains the exact edge. The independent compiler probe rejects this case and
accepts a locally declared `send` and `super::send` (`compiler-oracle.json`). This
matches the [Rust Reference](https://doc.rust-lang.org/reference/names/scopes.html#item-scopes).

The native lexical walk now stops unqualified call lookup at the nearest Rust
module after checking that module's bindings. Failure to find a binding records
`rust_module_binding_unknown` and suppresses generic bare-name fallback. Local
functions continue to resolve. Qualified paths retain their existing resolution
path; this is not a claim that qualified/import resolution is complete. The
shared scope adapter creates `module` scopes only for Rust `mod_item` nodes, so
JavaScript and TypeScript scope traversal is unchanged.

Parser revision 13 refreshes cached extraction facts. Source and ranker versions
and dependencies are unchanged. All 14 public scope integration tests pass,
including adding/removing a module-local declaration across sync/reopen, with
complete occurrence records equal to clean builds. See `scopes.txt`.

Limits: native Rust `use` bindings, namespaces, logical module identities and
qualified cross-module resolution remain open under recommendations 20/21. A
valid imported name without an explicit modeled binding can remain unresolved;
it must not become a false parent-module lexical edge. This is a precision fix,
not evidence of complete Rust name resolution or corpus-wide recall improvement.

Strict workspace/all-target Clippy, formatting and whitespace checks pass on
these final sources. The prior parser-12 full workspace run passed 420 tests across 32 suites;
it is not a full-workspace validation of this parser-13 change. See `checks.json`.
