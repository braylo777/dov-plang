# .dov pLang

> **Braylo = creator of .dov pLang; a rewritten C/C++ for modern-day agentic immutability.**

`.dov` is a systems language with C-level output and three defaults that C/C++ never had:

- **Immutable by default.** `let` cannot change. `var` can. Mutating a `let` is a compile error.
- **Agents, not threads.** State lives inside an `agent`. Other code talks to it only by `send`ing immutable messages. No locks, no shared mutation, no data races by construction.
- **Safe arithmetic, no null.** Overflow and divide-by-zero stop the program with a clear panic instead of undefined behavior.

It still talks to C directly: `extern "C" fn` calls libc or any C library, and the compiler emits plain C11.

```dov
agent Counter {
    var total: i64 = 0;
    on add(n: i64) { total = total + n; }
    on report() { print("total = ", total); }
}

fn main() {
    let c = spawn Counter;
    send c.add(5);
    send c.add(37);
    send c.report();      // total = 42
}
```

## Quick start

Needs Rust (stable) and a C compiler (`cc`/clang). No npm, no LLVM install.

```bash
cargo build --release
./target/release/dovc run examples/hello.dov          # Hello, .dov!
./target/release/dovc run examples/agent_counter.dov  # [1] counter total = 42
./target/release/dovc run examples/ffi.dov            # calls libc labs()
```

Commands: `dovc check <file.dov>`, `dovc emit-c <file.dov>`, `dovc build <file.dov> [-o out]`,
`dovc run <file.dov>`, `dovc fmt [--check] <file.dov>...`.

## Error messages

Every compile error shows where it is, the source line, a caret and a short hint:

```text
demo.dov:3:5: error: cannot assign to immutable binding `x`
  |
3 |     x = 2;
  |     ^
  = hint: declare it with `var x` instead of `let x` to allow mutation
```

Unknown names get a "did you mean" hint when a close name exists.

## Formatter

`dovc fmt file.dov` rewrites a file in the canonical layout (4-space indent, one statement per line,
comments and literal spelling kept). `dovc fmt --check file.dov` changes nothing and exits 1 if the
file is not formatted. The formatter is idempotent and checks its own output. The rules are in
[SPEC.md §15](docs/SPEC.md).

## Verify

```bash
cargo build --release && cargo test
./target/release/dovc fmt --check examples/*.dov
```

The suite has 192 tests:
- unit tests for the lexer, parser, checker, diagnostics and formatter;
- e2e tests that run the examples, check deterministic agent order and test the `fmt` CLI;
- the golden suite in `tests/golden/`:
  - `run/`: 47 programs. Each must be fmt-clean and must give the expected stdout (or panic stderr).
    Its emitted C is built with `-std=c11 -Wall -Wextra -Werror` and with `-fsanitize=address,undefined`.
  - `fail/`: 84 rejected programs, one per error kind. Each must give the exact diagnostic.
  - `fmt/`: 16 formatter cases.

`DOV_BLESS=1 cargo test --test golden` rewrites the expected files. Review the diff before you commit.
CI (`.github/workflows/ci.yml`) runs all of this on macOS and Ubuntu.

## How it works

`lex → parse → check (types + immutability) → C11 codegen → cc → native binary`

The compiler (`crates/dovc`) is Rust with zero dependencies. The runtime (`runtime/dov_rt.c`) is ~100
lines of C: one FIFO mailbox queue, run to completion. The language rules are in [docs/SPEC.md](docs/SPEC.md) (grammar + semantics). The reasons for them are in
[docs/DESIGN.md](docs/DESIGN.md).

## Status: v0.2 (Phase 0: foundations)

Working: `i64`/`bool`/`str`, functions, `let`/`var`, `if`/`else`/`while`/`return`, agents with
fields and handlers, `spawn`/`send`, `extern "C"`, checked arithmetic, diagnostics with hints, `dovc fmt`.

Roadmap: see [docs/MASTERPLAN.md](docs/MASTERPLAN.md). Next is Phase 1 (v0.3): structs, enums, `match`,
generics, `Result`/`Option`, modules, strings and slices, `defer`. Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## License

Dual-licensed under [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
