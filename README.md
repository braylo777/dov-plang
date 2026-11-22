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

Commands: `dovc check | emit-c | build [-o out] | run <file.dov>`.

## Verify

```bash
cargo test --workspace && cargo run -p dovc -- run examples/hello.dov
```

The suite covers the lexer, parser and checker (unit), compiles and runs every example (e2e), checks
6 rejected programs in `tests/fail/` (`// expect:` line = required error), proves agent message order
is deterministic, and compiles all generated C with `-Wall -Wextra -Werror`.

## How it works

`lex → parse → check (types + immutability) → C11 codegen → cc → native binary`

The compiler (`crates/dovc`) is Rust with zero dependencies. The runtime (`runtime/dov_rt.c`) is ~100
lines of C: one FIFO mailbox queue, run to completion. See [docs/DESIGN.md](docs/DESIGN.md).

## Status: v0.1

Working: `i64`/`bool`/`str`, functions, `let`/`var`, `if`/`else`/`while`/`return`, agents with
fields and handlers, `spawn`/`send`, `extern "C"`, checked arithmetic, positioned errors.

Roadmap: v0.2 structs + heap values + ownership/borrow checker + `Option` · v0.3 multi-threaded
agent scheduler · v0.4 C header import + C++ via C ABI · later: LLVM backend, `dov` package tool.

## License

Dual-licensed under [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
