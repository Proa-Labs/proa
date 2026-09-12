# RSJS linker diagnostic fixtures

These files intentionally compile through ordinary Rust/proc-macro checking.
Each contains a browser-reachable call to a normal Rust function without a
`#[rsjs::client]` definition record.

The two-compilation linker must reject them as unresolved helper edges. They
must not be added to the proc-macro `trybuild` compile-fail glob: imports,
renames, and re-exports are resolved by rustc, so the proc macro cannot
correctly determine helper eligibility from path tokens.

The `document::...` and `fetch::...` cases are here for the same reason:
namespace spelling is not semantic identity. A user may legally import a pure
annotated helper under either name; origin-free validation belongs to the
helper definition, while a missing definition record belongs to the linker.
