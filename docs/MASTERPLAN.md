# .dov pLang masterplan: v0.1 → 1.0 (draft for Braylo's approval, 2026-11-22)

Goal: a language people can install in one line, build real projects on, and trust not to break them.
Today (v0.1): `dovc` (Rust, ~2.4K LOC) compiles `.dov` → C11; `let`/`var`, agents + `send`, safe arithmetic, `extern "C"` FFI; 35 tests; 3 examples.

## North-star rules
1. **Immutable by default, agents not threads, no UB.** Never trade these for features.
2. **C11 output stays the backend through 1.0.** It runs everywhere a C compiler runs. LLVM is post-1.0.
3. **Zero required dependencies** for users: one binary + a C compiler.
4. **Every feature ships with spec text, tests and a doc page.**

## Phases

| Phase | Version | What ships | Done when | Effort |
|---|---|---|---|---|
| **0. Foundations** | v0.2 | Language spec v0 (grammar + semantics) · error messages with line/col + hints · `dovc fmt` · golden-test suite · CI (mac/linux) | Spec covers every v0.1 feature; 100+ tests; CI green on 2 OS | 1–2 wks |
| **1. Real programs** | v0.3 | Structs + enums + `match` · generics (monomorphized) · `Result`/`Option` (no null) · modules + `import` · strings + slices · `defer` | A 1K-line program compiles; JSON parser example works | 3–4 wks |
| **2. Standard library** | v0.4 | `std`: io, fs, fmt, collections (Vec/Map, immutable + persistent), time, process, net (TCP/HTTP client), json | Examples use only `std` + FFI; std has its own tests | 4–6 wks |
| **3. Tooling** | v0.5 | `dov` CLI: `new`, `build`, `run`, `test`, `fmt`, `doc` · `dov.toml` manifest · lockfile · **package manager** (git-based first) · LSP (`dov-lsp`: diagnostics, go-to-def, hover) · VS Code + Zed + tree-sitter syntax | `dov new app && dov run` works; packages install from a git URL; editor shows errors live | 4–6 wks |
| **4. Agentic runtime** | v0.6 | Agent supervision trees (restart on panic) · mailboxes with backpressure · `ask`/reply · timers · capability-scoped FFI (agents declare what C they may call) · deterministic replay mode for debugging | 10K-agent benchmark; crash in one agent doesn't kill the app; replay reproduces a bug | 4–6 wks |
| **5. Safety + interop** | v0.7 | Ownership/borrow-lite for `var` across agent boundaries · C header import (`dovc bindgen foo.h`) · call `.dov` from C (export ABI) · fuzzing + sanitizer CI (ASan/UBSan on emitted C) · security model doc | Wraps SQLite via bindgen; fuzzers run nightly with 0 crashes | 4 wks |
| **6. Distribution** | v0.9 RC | Release binaries mac (arm64/x64), linux, windows · one-line installer (`curl … | sh`, no npm) · Homebrew tap · registry site (packages + docs) · docs site + tutorial + "dov by example" · web playground (compile to C/WASM server-side) | Fresh machine → hello world in < 2 min on all 3 OS | 3–4 wks |
| **7. 1.0** | v1.0 | Stability promise (no breaking changes in 1.x; editions for future breaks) · semver + changelog · governance (Braylo = BDFL, RFC process) · CoC + CONTRIBUTING | Spec frozen; 3 flagship apps run in prod; 0 P0 bugs | 2 wks |

Total to 1.0: about 6–8 months solo with Claude Code; faster with 1–2 contributors.

## 3 flagship projects built in .dov (prove it's real)
1. **agentd**: a tiny agent server: N agents behind an HTTP API, supervision, replay logs. (Shows the core idea.)
2. **lockgate-dov**: port of MCP LockGate's quarantine core to `.dov` (immutable graph checks). Benchmarked vs the Rust version.
3. **dov-kv**: an immutable, append-only key-value store with agent-per-shard. (Shows systems-level performance.)

## Positioning
- One line: "C-level speed, agent-level safety: immutable by default, no data races, no UB."
- Audience: systems + AI-agent builders who want C interop without C's footguns.
- Proof assets: benchmarks vs C/Rust/Go on the 3 flagships; "why .dov" page; talks (fits the Digital Sovereignty keynote).

## Risks
| Risk | Mitigation |
|---|---|
| Scope creep (generics, borrowck) | Phase gates; each phase ships alone |
| C backend limits (debug info, perf) | `#line` directives for debuggers; LLVM after 1.0 |
| No users | Flagships + playground + tutorial before 1.0; post on HN/r/ProgrammingLanguages at v0.6 |
| Name clash ".dov" | Check registry/trademark before v0.9 |

## Next 3 steps (Phase 0)
1. Write `docs/SPEC.md` v0 from the current compiler (grammar in EBNF + semantics).
2. Add GitHub Actions CI (macOS + Ubuntu): build, test, emit-C + compile with `-Wall -Werror -fsanitize=address,undefined`.
3. Upgrade diagnostics (line/col, caret, hint) + `dovc fmt`.

Approve with "y" to start Phase 0. Nothing is pushed or published without Braylo's "y".
