# .dov pLang — design (v0.1)

## Goals
A modern rewrite of C/C++ for agentic systems: C-level output and C interop, with immutability,
memory safety and message-passing concurrency as defaults, not add-ons.

## Grammar (v0.1)
```
program  = item*
item     = fn | extern | agent
fn       = "fn" IDENT "(" params? ")" ("->" type)? block
extern   = "extern" "\"C\"" "fn" IDENT "(" params? ")" ("->" type)? ";"
agent    = "agent" IDENT "{" (field | handler)* "}"
field    = ("let" | "var") IDENT (":" type)? "=" expr ";"
handler  = "on" IDENT "(" params? ")" block
type     = "i64" | "bool" | "str" | "()" | IDENT        (IDENT = agent handle)
stmt     = ("let"|"var") IDENT (":" type)? "=" expr ";"
         | IDENT "=" expr ";"
         | "if" expr block ("else" (if | block))?
         | "while" expr block
         | "return" expr? ";"
         | "send" primary "." IDENT "(" args? ")" ";"
         | expr ";"
expr     = binary with precedence  || < && < == != < < <= > >= < + - < * / %  ; unary - !
primary  = INT | STRING | true | false | IDENT | IDENT "(" args ")" | "spawn" IDENT | "(" expr ")"
```
Comments: `// ...`. Integers allow `_` separators.

## Immutability
- `let` bindings, parameters and `let` agent fields cannot be reassigned. The checker rejects it, and
  codegen also emits `const` so the C compiler agrees.
- `var` opts in to mutation. Agent `var` fields can change only inside that agent's own handlers.
- All v0.1 values (`i64`, `bool`, `str` literals, agent handles) are immutable values, so every
  message is immutable by construction.

## Agents
- `spawn A` allocates agent state and returns a handle (type `A`).
- `send h.m(args)` copies the arguments into a message and appends it to one global FIFO queue.
- After `main` returns, the runtime delivers messages in order, one at a time, each to completion.
- Result: no locks, no data races, and the same program always prints the same output.
- v0.3 plan: N worker threads, one mailbox per agent, same per-agent ordering guarantee.

## Safety
- `+ - *` and unary `-` panic on overflow; `/` and `%` panic on zero (exit code 101, line number).
- No null and no raw pointers in safe code (v0.1 has no pointer types at all).
- Ownership: v0.1 has no heap values, so there is nothing to move yet. v0.2 adds structs and heap
  values with single ownership, moves, and `&` / `&var` borrows checked at compile time.

## C interop
- `extern "C" fn name(...) -> T;` emits a C prototype and a direct call. Allowed types: `i64`
  (C `long`), `bool`, `str` (`const char *`), `()` (void).
- Generated code includes only `dov_rt.h`, which declares no libc functions, so extern
  declarations cannot clash with headers.
- v0.1 targets LP64 platforms (macOS, Linux 64-bit). A static assert enforces it.

## Known v0.1 limits
- Call arguments with side effects follow C order of evaluation (unspecified). v0.2 lowers them
  left to right.
- Strings are literals only; no concatenation or heap strings yet.
- One file per program; no modules.
