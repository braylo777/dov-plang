# Changelog

All notable changes to `.dov` pLang and `dovc`. Versions follow semver; before 1.0, a minor bump
may break programs (each break is listed here).

## 0.2.0 — 2026-11-22 (Phase 0: foundations)

### Added
- `docs/SPEC.md`: the v0 language specification. It has the EBNF grammar and the semantics of every
  feature, derived from the compiler.
- Diagnostics: every compile error shows `file:line:col: error: …`, the source line, a caret and a
  `= hint:`. Unknown variables, functions, agents and handlers get "did you mean" suggestions.
  A missing `;` is reported just after the previous token.
- `dovc fmt [--check] <file>...`: a deterministic, idempotent formatter. It keeps comments and the
  spelling of literals, and it checks that its output has the same tokens and comments as its input.
- `dovc --help`.
- Golden test suite (`tests/golden/{run,fail,fmt}`). Each file is its own test, and the emitted C is
  built with `-Wall -Wextra -Werror` and with ASan/UBSan. `DOV_BLESS=1` updates the expected files.
- CI for macOS and Ubuntu (`.github/workflows/ci.yml`).
- Tests: 35 → 192.

### Changed (breaking)
- Identifiers are ASCII only (`[A-Za-z_][A-Za-z0-9_]*`). 0.1 accepted Unicode letters, and they were
  emitted into C as is.
- A handler parameter of type `()` is now a compile error. In 0.1 it produced invalid C.
- An `extern "C"` function named after a C keyword (e.g. `int`) is now a compile error. In 0.1 it
  produced invalid C.
- `extern "C"` names that clash with the generated C (`v_*`, `f_*`, `self`, `m`, `NULL`, `size_t`, …)
  are now compile errors. In 0.1 they failed in the C compiler.
- Error messages are reworded. The hint moved to its own line (e.g. "declare it with `var`…").

### Fixed
- A shadowing `let`/`var` that reads the outer name (`let x = x + 1;` in a nested block) read the new,
  uninitialized C variable (undefined behavior). A `let` that shadowed a parameter at the top of a
  body did not compile. Shadowing bindings now get their own C names.
- Handler symbols could collide (agent `A_b` + handler `c` vs agent `A` + handler `b_c`). The agent
  name is now length-prefixed.
- Emitted C failed `-Wall -Werror` for an unused `let`, an uncalled `fn` or an unsent handler.

## 0.1.0 — 2026-11-22

- First release: `dovc` compiles `.dov` to C11. It has `let`/`var` immutability, functions, agents
  with `spawn`/`send` (global FIFO), checked arithmetic with panics, `extern "C"` FFI and `print`.
