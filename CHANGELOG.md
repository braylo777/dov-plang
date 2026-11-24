# Changelog

All notable changes to `.dov` pLang and `dovc`. Versions follow semver; before 1.0, a minor bump
may break programs (each break is listed here).

## 0.3.0 — 2026-11-24 (Phase 1: the type core)

### Added
- **Structs** (value types): literals `P { x: 1 }`, field reads, field assignment through a `var`
  root (`p.a.x = 1;`). A struct may contain itself only through a slice.
- **Enums with payloads** and **`match`** (statement, block arms, one-level patterns): variants
  `E::V(a, _)`, literals and `_`; exhaustiveness and unreachable-arm checks; a `match` whose arms
  all return counts for the return-path rule. Lowered to tagged unions and a C `switch`.
- **Generics** on `fn`, `struct` and `enum`, checked once with opaque type parameters, inferred
  from arguments and the expected type (unification), and monomorphized per concrete instance.
- **Prelude** (written in .dov): `Option<T>`, `Result<T, E>`, `is_some`, `is_none`, `unwrap`,
  `unwrap_or`, `is_ok`, `is_err`, `unwrap_ok`, `unwrap_err`, `ok_or`. Bare `Some`/`None`/`Ok`/`Err`.
- **Strings and slices**: `str` now carries its length; `[T]` slices; `s[i]`, `s[a..b]`, `xs[i]`,
  `xs[a..b]`, `len`, `append` (amortized O(1), never changes a visible value), `concat`, `to_str`,
  `char_of`, `panic`. All indexing is bounds-checked (panic, exit 101).
- **`defer`** (statement or block): runs at block exit and before `return`, latest first.
- **Modules**: `import "file.dov";` merges items into one namespace; each file loads once (cycles
  are fine); diagnostics name the file. `Pos` carries a file id.
- `dovc fmt` formats all of the new syntax (generics, struct literals on one line, slices, `match`).
- 12 examples (`make examples`, snapshot-tested), including `examples/json.dov`, a ~630-line JSON
  parser and printer. 4 agent examples also pass under `-fsanitize=thread` (`make tsan`).
- Dependency-free property tests (`tests/prop.rs`), checker unit tests, golden cases for every
  feature. Tests: 192 → see the README table (generated from `results/metrics.json`).
- `bench/run.py` (compile time per 1,000 lines; 4 kernels vs hand-written C), `scripts/metrics.py`,
  `scripts/readme_numbers.py --check` (run in CI), `docs/ARCHITECTURE.md` + PDF + 5 SVG diagrams.
- Playground: `crates/dovc-wasm` (dependency-free WebAssembly exports, no wasm-bindgen) and a
  static page in `playground/`, deployed at https://dov-playground.pages.dev/.
- CI: rustfmt, clippy `-D warnings`, coverage ≥ 90% lines (cargo-llvm-cov), TSan, README check.

### Changed (breaking)
- New keywords: `struct`, `enum`, `match`, `import`, `defer`. New built-in names: `panic`, `len`,
  `append`, `concat`, `to_str`, `char_of`, and the prelude names; programs that defined them must rename.
- `str` is `{pointer, length}` in C (`dov_str`), so a literal with `\0` keeps its full length for
  `len`, `==` and `print`. `extern "C"` still receives a NUL-terminated `const char *`.
- `+` on a `str`: the hint now points to `concat`. Several messages and hints are
  reworded to mention the new types.

### Changed
- Checked arithmetic and bounds checks are `static inline` in `dov_rt.h` (much faster kernels).
- `dov_panic` is `_Noreturn`.

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
