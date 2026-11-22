# Phase 1 (v0.3) build order

Approved by Braylo on 2026-11-22 ("y P1"). Each step is one or more small commits. Each step adds
SPEC.md text and golden run, fail and fmt cases. The whole suite stays green after every commit.

| # | Step | Why this order |
|---|---|---|
| 1 | **Type core**: `Type` gets struct, enum, slice and type-parameter forms; `Pos` gets a file id | Every later feature needs it |
| 2 | **Modules + `import "file.dov";`** | Diagnostics must name the right file before the programs grow |
| 3 | **Structs** (value types, literals, field read, field assignment through `var`) | The simplest new type; it proves the C layout |
| 4 | **Enums + `match`** (payload variants, exhaustiveness, return-path rule) | Needed for `Option`/`Result` |
| 5 | **Generics** (monomorphized `fn`, `struct`, `enum`; inference from arguments and the expected type) | Turns enums into `Option<T>`/`Result<T, E>` |
| 6 | **Prelude**: `Option<T>`, `Result<T, E>` and helpers, written in .dov | No null; it exercises generics |
| 7 | **Strings + slices**: `str` with length, `s[i]`, `s[a..b]`, `[T]` literals, `xs[i]`, `xs[a..b]`, `len`, `append`, `concat`, `to_str`, `char_of` (all bounds-checked) | The JSON parser needs them |
| 8 | **`defer`** (runs at block exit and before `return`, in reverse order) | Independent; last of the language work |
| 9 | **`examples/json.dov`**: a JSON parser and printer of about 500 lines | The Phase 1 "done when" proof |
| 10 | Version 0.3.0, CHANGELOG, verifier | Release gate |

Design decisions:
- Values are copied. Heap data (string bytes, slice items) is immutable after creation and is never
  freed in v0.3. That keeps the "no data races" rule without a borrow checker.
- `match` is a statement with block arms. Patterns are one level deep: `E::V(a, _)`, a literal, or `_`.
- Generics are checked once, with opaque type parameters, and then emitted once per concrete instance.
- `import` merges the imported items into one namespace. Each file is loaded once. Qualified names
  (`mod::f`) are later work.
- Limits: no methods, no traits, no closures, no nested patterns, no recursive types except through
  slices.
