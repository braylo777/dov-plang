# .dov pLang

[![ci](https://github.com/braylo777/dov-plang/actions/workflows/ci.yml/badge.svg)](https://github.com/braylo777/dov-plang/actions/workflows/ci.yml)

> **Braylo = creator of .dov pLang; a rewritten C/C++ for modern-day agentic immutability.**

`.dov` is a systems language with C-level output and three defaults that C/C++ never had:

- **Immutable by default.** `let` cannot change. `var` can. Mutating a `let` is a compile error.
- **Agents, not threads.** State lives inside an `agent`. Other code talks to it only by `send`ing immutable messages. No locks, no shared mutation, no data races by construction.
- **Safe arithmetic, no null.** Overflow, divide-by-zero and out-of-bounds indexes stop the program with a clear panic instead of undefined behavior. Absence is `Option<T>`, failure is `Result<T, E>`.

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

Needs Rust (stable) and a C compiler (`cc`/clang/gcc). No npm, no LLVM install, zero crate dependencies.

```bash
cargo build --release
./target/release/dovc run examples/hello.dov          # Hello, .dov!
./target/release/dovc run examples/json.dov           # a JSON parser and printer in .dov
make examples                                         # runs all 12 examples against snapshots
```

Commands: `dovc check <file.dov>`, `dovc emit-c <file.dov>`, `dovc build <file.dov> [-o out]`,
`dovc run <file.dov>`, `dovc fmt [--check] <file.dov>...`.

## The language in one screen (v0.3)

```dov
struct Point { x: i64, y: i64 }            // value types, copied
enum Shape { Circle(Point, i64), Dot }     // tagged unions

fn first<T>(xs: [T]) -> Option<T> {        // generics: inferred, monomorphized
    if len(xs) == 0 {
        return None;
    }
    return Some(xs[0]);                    // bounds-checked
}

fn main() {
    var p = Point { x: 1, y: 2 };
    p.x = 10;                              // `var` allows field updates; `let` does not
    defer print("bye");                    // runs when main's block ends
    match first([Shape::Circle(p, 3), Shape::Dot]) {
        Some(s) => {
            match s {                      // exhaustive: every variant or `_`
                Shape::Circle(c, r) => { print("circle at x=", c.x, " r=", r); }
                Shape::Dot => { print("dot"); }
            }
        }
        None => { print("empty"); }
    }
    let msg = concat("x = ", to_str(p.x)); // immutable strings with a length
    print(msg, " (", len(msg), " bytes)");
}
```

Modules: `import "other.dov";` (items merge into one namespace, each file loads once).

## Numbers

Generated from [`results/metrics.json`](results/metrics.json) by `scripts/readme_numbers.py`
(CI fails if they differ). Regenerate with `make bench metrics` and
`python3 scripts/readme_numbers.py --write`.

<!-- numbers:start -->
![tests](https://img.shields.io/badge/tests-382%20passing-brightgreen) ![examples](https://img.shields.io/badge/examples-12-blue) ![deps](https://img.shields.io/badge/dependencies-0-blue) ![version](https://img.shields.io/badge/version-0.3.0-informational)

| Metric | Value |
|---|---|
| Tests passing (`cargo test`) | 382 |
| Golden cases (run / fail / fmt) | 138 / 150 / 25 |
| Examples (`make examples`) | 12 |
| Crate dependencies | 0 |
| Rust lines (non-blank, non-comment) | 5095 |
| Compile time, `dovc emit-c` | 2.67 ms per 1,000 lines (10068 lines) |
| Kernel `fib`: .dov / hand C | 13.2 ms / 10.6 ms (1.24x) |
| Kernel `primes`: .dov / hand C | 19.2 ms / 18.6 ms (1.03x) |
| Kernel `collatz`: .dov / hand C | 117.5 ms / 82.4 ms (1.43x) |
| Kernel `slices`: .dov / hand C | 18.5 ms / 7.5 ms (2.46x) |

Measured 2026-11-24T20:41:00Z at `3e37118` on Darwin arm64 Apple M2 Pro with Apple clang version 21.0.0 (clang-2100.1.1.101). Source: [`results/metrics.json`](results/metrics.json).
<!-- numbers:end -->

## Error messages

Every compile error shows where it is (including which imported file), the source line, a caret
and a short hint:

```text
demo.dov:3:5: error: cannot assign to immutable binding `x`
  |
3 |     x = 2;
  |     ^
  = hint: declare it with `var x` instead of `let x` to allow mutation
```

## Verify

```bash
cargo test                      # unit, e2e, property and golden tests
make examples tsan              # examples vs snapshots; agent examples under ThreadSanitizer
python3 scripts/readme_numbers.py --check
```

The golden suite (`tests/golden/`): `run/` programs must be fmt-clean and give the expected stdout
(or panic stderr); their C is built with `-std=c11 -Wall -Wextra -Werror` and with
`-fsanitize=address,undefined`. `fail/` programs must give the exact diagnostic. `fmt/` cases check
the formatter. `DOV_BLESS=1 cargo test --test golden` rewrites expected files (review the diff).
CI runs everything on macOS and Ubuntu, plus coverage (≥ 90% lines).

## Docs

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) ([PDF](docs/ARCHITECTURE.pdf)): why .dov, grammar,
  types, values, agents, a worked C lowering, the pipeline, diagnostics, limits.
- [docs/SPEC.md](docs/SPEC.md): the v0.3 language specification (grammar + semantics).
- [docs/DESIGN.md](docs/DESIGN.md), [docs/MASTERPLAN.md](docs/MASTERPLAN.md), [CHANGELOG.md](CHANGELOG.md).

## Status: v0.3 (Phase 1: the type core)

Working: `i64`/`bool`/`str`, structs, enums + `match`, generics, `Option`/`Result`, slices, string
operations, `defer`, `import`, functions, `let`/`var`, agents (`spawn`/`send`), `extern "C"`,
checked arithmetic and indexing, diagnostics with hints, `dovc fmt`.

Limits: heap data is never freed, one scheduler thread, no methods/traits/closures, `match` is a
statement. Roadmap: [docs/MASTERPLAN.md](docs/MASTERPLAN.md).

## License

Dual-licensed under [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
