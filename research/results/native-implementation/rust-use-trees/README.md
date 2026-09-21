# Native Rust use-tree facts

`before.txt` reproduces two errors in string-based import expansion: grouped
`self` was dropped, and `beta` from a nested group was assigned its parent's path.
The replacement traverses the installed Rust syntax tree with an explicit worklist,
keeping group prefixes separate. It does not split source strings at commas.

Each use leaf retains the normalized target path, local binding name (including
aliases), glob flag, type-only status for `self` imports, exact authored reexport
visibility, original leaf bytes/span,
and file-local lexical scope. `as _` has no local binding; grouped `self` targets
the group's prefix. Trailing `::self` retains the same type-only distinction. Leading absolute paths and comments are preserved/interpreted
through syntax nodes. Empty groups emit no binding. Unsupported/error syntax
produces an explicit unresolved fact instead of a partial guessed expansion.

The new optional `rust_use` field defaults absent for legacy references. Parser
revision 15 refreshes extraction facts. No third-party component or dependency was
added. Source and ranker representation versions are unchanged.

Tests cover nested siblings, `self`, aliases, globs, raw identifiers, comments,
absolute paths, CRLF/Unicode coordinates, lexical scope attachment, legacy JSON
and structured-fact roundtrips. A public integration test checks complete raw
references/scopes against fresh extraction after edits, packed storage and reopen.

This supplies source-backed facts for recommendation 21; it does not yet resolve
those bindings through logical module identities, choose glob export sets, enforce
visibility or resolve reexports. Recommendations 20/21 remain open. Do not infer
corpus precision or recall improvements from the local extraction regressions.

Syntax/namespace behavior follows the [Rust use-declaration reference](https://doc.rust-lang.org/reference/items/use-declarations.html).

Final validation: 30 language/type unit tests and 22 public module/scope integration
tests pass. Strict workspace/all-target Clippy, formatting and whitespace checks
pass. Final source hashes match, and Cargo manifests/lockfiles are unchanged.
This increment has focused validation; it is not a new full-workspace run.
