# Rust authored visibility facts

The regression in `before.txt` shows all explicit function visibilities extracted
as `None`. The adapter requested a nonexistent `visibility` field; the installed
Rust grammar instead uses a named `visibility_modifier` child with no fields.

The adapter now locates that child and uses its syntax child to distinguish
`pub`, `pub(crate)`, `pub(super)` and `pub(self)`, including the corresponding
single-component `pub(in ...)` forms. Grammar nodes avoid whitespace/comment
spelling assumptions. A modifier with parse errors is not assigned visibility.
The exact authored modifier is retained as `rust_visibility_modifier` for future
module-aware interpretation of arbitrary restricted paths.

Omitted visibility stays `None`: effective defaults vary by declaration context.
An arbitrary `pub(in crate::area)` restriction also stays unclassified rather
than being promoted to public. These facts do not themselves enforce access
control, resolve inherited visibility or implement Rust's module identity model.
See the [Rust visibility rules](https://doc.rust-lang.org/reference/visibility-and-privacy.html).
Parser revision 14 refreshes cached extraction; no dependency change is required.

The new unit regression covers functions, structs and fields, comments/whitespace,
omissions and exact authored restricted paths. The separate JS/TS renamed-import
shadow regression passes without a production change: native lexical scope binding
already precedes import normalization in both adapters (`alias-regression.txt`).
It protects local-target precedence while retaining an unshadowed imported call.

Validation: all 13 language unit tests and all 21 scope/module integration tests
pass on parser revision 14. Strict workspace/all-target Clippy, formatting and
whitespace checks pass; recorded source hashes match and Cargo manifests/lockfiles
are unchanged. The previous full workspace run was parser revision 12 (420 tests),
not a full-workspace run of this revision. See `checks.json`.
