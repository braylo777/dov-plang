# .dov language specification — v0 (dovc 0.2.0)

Status: **v0, draft.** This document describes what `dovc` 0.2.0 actually accepts and does.
It is derived from the compiler (`crates/dovc/src`), and every rule has a test under
`tests/golden/`. Where behavior is not fixed yet, the text says **unspecified** and names the
current behavior. Breaking changes before 1.0 are allowed and go in `CHANGELOG.md`.

Contents: 1 Source text · 2 Grammar · 3 Programs and names · 4 Types · 5 Bindings and
immutability · 6 Functions · 7 Statements · 8 Expressions · 9 Integer arithmetic and panics ·
10 Strings and `print` · 11 Agents · 12 C interop · 13 Program execution · 14 Diagnostics ·
15 Canonical format · 16 Known limits

---

## 1. Source text

- A source file is UTF-8 text with the extension `.dov`. One file is one program.
- **Whitespace** (any Unicode whitespace, including `\r`) separates tokens. Otherwise it is ignored.
- **Comments** run from `//` to the end of the line. There are no block comments.
- **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`. Identifiers are ASCII only, so the emitted C names are
  portable. A non-ASCII letter outside a string is an error (`unexpected character`).
- **Keywords** (reserved, cannot be names):
  `agent else extern false fn if let on return send spawn true var while`.
  The type names `i64`, `bool` and `str` are not keywords, but no top-level item may use them as a name.
- **Integer literals**: `[0-9][0-9_]*`. The `_` characters are ignored. The value must fit in `i64`.
  There are no hex, octal or binary literals. A literal is never negative. `-5` is unary minus applied
  to `5`, so `-9223372036854775808` is rejected. Write `-9223372036854775807 - 1`.
- **String literals**: `"` … `"` on one line. Escapes: `\n` `\t` `\\` `\"` `\0`. Any other escape
  is an error. Other characters, including raw tabs and non-ASCII text, are taken literally.
- **Operators and punctuation**: `( ) { } , ; : . -> = + - * / % == != < <= > >= && || !`.
  Two-character tokens are matched first. A lone `&` or `|` is an error.

Example:

```dov
// comment
let big = 1_000_000;          // integer literal with separators
let s = "tab:\tquote:\" é";   // escapes and raw UTF-8
```

## 2. Grammar (EBNF)

Notation: `x?` = optional, `x*` = zero or more, `|` = choice, `"t"` = token, `UPPER` = lexical class.
This grammar matches the recursive-descent parser in `parser.rs`.

```ebnf
program     = item* EOF ;
item        = fn_decl | extern_decl | agent_decl ;

fn_decl     = "fn" IDENT params ret_type? block ;
extern_decl = "extern" STRING(* must be "C" *) "fn" IDENT params ret_type? ";" ;
agent_decl  = "agent" IDENT "{" agent_member* "}" ;
agent_member= field | handler ;
field       = ("let" | "var") IDENT (":" type)? "=" expr ";" ;
handler     = "on" IDENT params block ;

params      = "(" (param ("," param)* ","?)? ")" ;
param       = IDENT ":" type ;
ret_type    = "->" type ;
type        = "i64" | "bool" | "str" | "(" ")" | IDENT (* agent name *) ;

block       = "{" stmt* "}" ;
stmt        = ("let" | "var") IDENT (":" type)? "=" expr ";"
            | IDENT "=" expr ";"
            | if_stmt
            | "while" expr block
            | "return" expr? ";"
            | "send" primary "." IDENT args ";"
            | expr ";" ;
if_stmt     = "if" expr block ("else" (if_stmt | block))? ;

expr        = or_expr ;
or_expr     = and_expr ("||" and_expr)* ;
and_expr    = eq_expr ("&&" eq_expr)* ;
eq_expr     = cmp_expr (("==" | "!=") cmp_expr)* ;
cmp_expr    = add_expr (("<" | "<=" | ">" | ">=") add_expr)* ;
add_expr    = mul_expr (("+" | "-") mul_expr)* ;
mul_expr    = unary (("*" | "/" | "%") unary)* ;
unary       = ("-" | "!") unary | primary ;
primary     = INT | STRING | "true" | "false"
            | IDENT args?                   (* call if followed by "(" *)
            | "spawn" IDENT
            | "(" expr ")" ;
args        = "(" (expr ("," expr)* ","?)? ")" ;
```

Notes:
- A trailing comma is accepted in `params` and `args`.
- The statement form `IDENT "=" …` is chosen when an identifier is directly followed by `=`.
- All binary operators are left-associative. The chained forms `a < b < c` and `a == b == c` parse,
  but they fail to type-check (§8).
- The `send` target is a `primary`, so `send spawn G.hi("x");` and `send (h).hi("x");` are valid.
- There is no `for`, `break`, `continue`, struct, array or module syntax in v0.

## 3. Programs and names

- A program is a list of items in any order. An item may use a later item (mutual recursion works).
- Functions, `extern` functions and agents share **one namespace**. A name defined twice is an error.
- The name `print` is reserved for the built-in (§10). The names `i64`, `bool` and `str` cannot be item names.
- The program must define `fn main()`. It takes no parameters and returns `()`. `main` cannot be called.
- Local names (bindings and parameters) are a separate namespace from items. `let f = 1;` does not hide
  a function `f`, and `f()` still calls the function.

## 4. Types

| Type | Values | C representation |
|---|---|---|
| `i64` | signed 64-bit integers | `int64_t` (`long` in `extern` prototypes) |
| `bool` | `true`, `false` | `bool` |
| `str` | immutable string literals | `const char *` |
| `()` | the unit value (no value) | `void` |
| `A` (an agent name) | a handle to an agent of kind `A` | `dov_agent_A *` |

- Types are compared by name. There are **no implicit conversions** and no casts.
- `()` is only a return type. Parameters, bindings and fields cannot have type `()`.
- An agent handle is an opaque, copyable, immutable value. Handles cannot be compared or printed.
- Type annotations are optional on `let`/`var` and fields. Without one, the type is the type of the initializer.
  With one, the annotation must equal that type exactly.

## 5. Bindings and immutability

Immutability is the default. Mutation needs an explicit `var`.

| Declared with | Can be assigned? |
|---|---|
| `let x = …;` | no: compile error *cannot assign to immutable binding* |
| `var x = …;` | yes, with a value of the same type |
| function or handler parameter | no: *immutable parameter* |
| agent field `let f = …;` | no: *immutable field* |
| agent field `var f = …;` | yes, but only inside that agent's own handlers (§11) |

- Assignment `x = e;` is a statement, not an expression. It needs `x` to be visible and mutable,
  and `e` must have the type of `x`.
- **Scopes**: each block `{ … }` opens a scope. The parameters are in a scope that encloses the body,
  so a `let` in the body may shadow a parameter. A name cannot be declared twice in the same scope,
  and that includes a duplicate parameter name. An inner block may shadow an outer name.
  A binding is visible from its declaration to the end of its block.
- In a handler, a name resolves to locals and parameters first, then to the agent's fields.
  A parameter or local therefore shadows a field of the same name.
- A `let` binding is also emitted as a C `const` object, so the C compiler checks the rule a second time.
- Every v0 value is immutable (integers, booleans, string literals, handles). The only mutable
  things are `var` bindings and `var` fields.

```dov
fn main() {
    let a = 1;
    var b = a;
    b = b + 1;      // ok
    // a = 2;       // error: cannot assign to immutable binding `a`
}
```

## 6. Functions

- `fn name(p: T, …) -> R { … }`. Without `-> R`, the return type is `()`. Writing `-> ()` is the same thing.
- Arguments are passed by value. The number of arguments and each argument's type must match exactly.
- **Return rule**: a function with a return type other than `()` must return on every path. The checker
  is conservative. A block returns when it directly contains a `return`, or an `if` with an `else`
  where both branches return. A `return` inside a `while` does not count. Code after a `return` is
  allowed and is never run.
- `return;` is only valid when the return type is `()`. `return e;` needs `e` to have the return type.
- Recursion is allowed. Deep recursion can overflow the native stack. This crashes the program
  without a panic message (§16).

```dov
fn fact(n: i64) -> i64 {
    if n <= 1 {
        return 1;
    }
    return n * fact(n - 1);
}
```

## 7. Statements

- `let` / `var`: evaluate the initializer, then bind. Binding a `()` value is an error.
- Assignment: §5.
- `if c { … } else { … }`: `c` must be `bool`. `else if` chains are allowed.
- `while c { … }`: `c` must be `bool`. It is checked before each iteration.
- `return`: §6.
- `send h.m(a, …);`: §11.
- Expression statement `e;`: evaluates `e` and discards the result, for example `print(...)`, a call, or even `1 + 2;`.

## 8. Expressions

Precedence, from lowest to highest. All binary operators are left-associative.

| Level | Operators | Operand types → result |
|---|---|---|
| 1 | `\|\|` | `bool`, `bool` → `bool` (short-circuit) |
| 2 | `&&` | `bool`, `bool` → `bool` (short-circuit) |
| 3 | `==` `!=` | same type, one of `i64`/`bool`/`str` → `bool` |
| 4 | `<` `<=` `>` `>=` | `i64`, `i64` → `bool` |
| 5 | `+` `-` | `i64`, `i64` → `i64` (checked, §9) |
| 6 | `*` `/` `%` | `i64`, `i64` → `i64` (checked, §9) |
| 7 (prefix) | `-` `!` | `-`: `i64` → `i64` (checked); `!`: `bool` → `bool` |

- `str` equality compares contents. Agent handles cannot be compared.
- `spawn A` has type `A` (§11). A call has the callee's return type.
- **Evaluation order.**
  - `&&` and `||` evaluate left to right and skip the right side when the result is already known.
  - The operand order of other binary operators and of function-call arguments is
    **unspecified**. Today it follows the C compiler. Do not depend on it. A later version will make
    it left to right.
  - `print` evaluates and prints its arguments one at a time, left to right (§10).
  - `send` evaluates its arguments left to right, then the target handle (§11).

## 9. Integer arithmetic and panics

`.dov` has no undefined behavior in integer arithmetic. A result that cannot be represented
**panics**.

| Operation | Panics when | Panic message |
|---|---|---|
| `a + b` | overflow | `integer overflow in \`+\`` |
| `a - b` | overflow | `integer overflow in \`-\`` |
| `a * b` | overflow | `integer overflow in \`*\`` |
| `a / b` | `b == 0` | `division by zero` |
| `a / b` | `a == i64::MIN && b == -1` | `integer overflow in \`/\`` |
| `a % b` | `b == 0` | `remainder by zero` |
| `-a` | `a == i64::MIN` | `integer overflow in unary \`-\`` |

- Division truncates toward zero (`-7 / 2 == -3`). The remainder has the sign of the dividend
  (`-7 % 3 == -1`). `i64::MIN % -1` is `0`. It does not panic.
- **A panic:** stdout is flushed, then `dov panic: <message> (line <N>)` is written to stderr, and the
  process exits with code **101**. `<N>` is the line of the operator. Nothing else runs after a
  panic, including queued messages.

## 10. Strings and `print`

- `str` values are string literals. There is no concatenation, indexing or length in v0.
- A string is passed to C as a NUL-terminated `const char *`. A literal that contains `\0` therefore
  looks shorter to `==` and to `print`.
- `print(e1, e2, …)` is a built-in with any number of arguments of type `i64`, `bool` or `str`. It writes
  each argument with no separator, then a newline. `print()` writes an empty line. The result type is `()`.
  `i64` prints in decimal and `bool` prints as `true`/`false`.
- `print` evaluates and writes one argument at a time. If a later argument panics, the earlier
  arguments are already written.

```dov
print("n = ", 42, ", ok = ", true);   // n = 42, ok = true
```

## 11. Agents

An agent owns state. Other code reaches that state only by sending messages.

```dov
agent Counter {
    var total: i64 = 0;          // mutable field
    let name = "counter";        // immutable field

    on add(n: i64) {             // handler
        total = total + n;
    }

    on report() {
        print(name, " total = ", total);
    }
}

fn main() {
    let c = spawn Counter;       // c: Counter
    send c.add(5);
    send c.add(37);
    send c.report();             // counter total = 42
}
```

- **Fields** need an initializer. An initializer may call functions and use `spawn`. It cannot see
  locals, parameters or other fields. A field cannot have type `()`. Field names in one agent must be unique.
- **Handlers** `on m(params) { … }` return `()`. Handler names in one agent must be unique (no
  overloading). Inside a handler, fields are in scope by name (§5). A handler can assign only its
  own agent's `var` fields. No code anywhere can read or write another agent's fields.
- **`spawn A`** allocates a new agent of kind `A`. It runs the field initializers in declaration order,
  at the time of the spawn, and returns a handle. A field initializer that spawns its own kind recurses
  without end (§16).
- **`send h.m(args);`**: `h` must be a handle of kind `A`, and `A` must have a handler `m` whose
  parameter types match `args`. The arguments are copied into a message, and the message is added to
  the end of the **single global FIFO queue**. `send` does not run the handler. It returns at once.
- **Delivery**: after `main` returns, the runtime takes messages from the front of the queue one at a
  time. It runs each handler to completion before it starts the next. Messages sent by handlers go
  to the end of the queue. The program ends when the queue is empty.
- Consequences: handlers never run concurrently, there are no locks and no data races, and the same
  program with the same input always gives the same output.
- Agents and messages are never freed in v0. A message's argument block is freed after its handler runs.

## 12. C interop

```dov
extern "C" fn labs(x: i64) -> i64;
extern "C" fn srand(seed: i64);

fn main() {
    print(labs(-42));
}
```

- `extern "C" fn name(params) -> R;` declares a C function. Only the ABI string `"C"` is accepted.
- Parameter types: `i64`, `bool` or `str`. The return type: `i64`, `bool`, `str` or `()`. Agent
  handles cannot cross into C.
- Mapping: `i64` → C `long`, `bool` → `bool`, `str` → `const char *`, `()` → `void`.
  `long` is 64-bit only on LP64 targets, so the runtime header has a static assert for LP64
  (64-bit macOS and Linux).
- The emitted C prototype is `R name(T1, T2, …);`, with the name unchanged. The name therefore
  cannot be a C keyword, `main`, or begin with `dov_`. All three are compile errors.
- **Prototypes are trusted.** If the declaration does not match the real C function (for example
  `strlen`, which returns `size_t`, or `abs`, which takes `int`), the behavior is that of C:
  undefined, or a C compile error. Variadic functions such as `printf` cannot be declared.
- An extern call is an ordinary call expression, and it follows the call rules in §6 and §8.
- The program is linked with the system C library only.

## 13. Program execution

1. `dovc` lexes, parses and checks the file. A compile error stops it here (§14).
2. It emits one C11 file that includes only `dov_rt.h`, and compiles it with the embedded runtime using
   `$CC` (default `cc`) and `-std=c11 -O2`.
3. The program runs `main`, then delivers messages (§11), then exits with code 0. It exits with 101
   after a panic.

CLI: `dovc check | emit-c | build [-o out] | run <file.dov>`, `dovc fmt [--check] <files…>`,
`dovc --version`, `dovc --help`. A compile error exits with 1. A usage or I/O error exits with 2.
`dovc run` exits with the program's exit code.

## 14. Diagnostics

Every compile error has a position, the source line, a caret and a short hint:

```text
demo.dov:3:5: error: cannot assign to immutable binding `x`
  |
3 |     x = 2;
  |     ^
  = hint: declare it with `var x` instead of `let x` to allow mutation
```

- The first line, `file:line:col: error: message`, is the stable, machine-readable part. Lines and
  columns are 1-based, and columns count characters. The caret line copies tabs, so it lines up.
- `dovc` reports the first error only.
- Every error kind has a golden test in `tests/golden/fail/`, whose `.err` file holds its exact output.
- Hints may suggest a near name (`did you mean \`total\`?`). The exact hint text is not a stable interface.

## 15. Canonical format

`dovc fmt` produces the canonical layout. `dovc fmt --check` exits with 1 if a file would change.
- 4 spaces of indent per `{`. One statement, field or handler per line. `{` ends its line, `}` starts
  its own line, `} else {` stays on one line, and an empty block is `{}`.
- One space around binary operators, `=` and `->`. One space after `,` and `:`. No space inside `( )`,
  before the `(` of a call or parameter list, around `.`, or after a unary `-` or `!`.
- Blank lines are kept but collapsed to one. There are none at the start or end of a block. A
  top-level item that ends in `}`, or a `fn`/`agent` that follows an `extern`, gets one blank line
  before the next item.
- Comments stay in place. A comment on its own line stays on its own line at the current indent. A
  trailing comment comes after one space. Literals keep their source spelling. Lines are never wrapped.
- The output ends with exactly one newline, and an empty file stays empty. Formatting is idempotent,
  and it never changes tokens or comments (the formatter checks this every time it runs).
- Only files that parse can be formatted. A syntax error is reported as in §14.

## 16. Known limits (v0)

- The operand order of binary operators and call arguments is unspecified (§8).
- Strings are literals only. A literal with `\0` is cut short at the NUL for C.
- There is one file per program, with no modules, structs, arrays, heap values or `Option`/`Result`.
- Agents and messages are never freed. There is one global queue and one thread.
- Deep recursion, and a field initializer that spawns its own agent kind, overflow the native stack
  without a panic message.
- `extern "C"` prototypes are trusted (§12). Only LP64 targets are supported.
- Only the first compile error is reported.
