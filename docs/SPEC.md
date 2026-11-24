# .dov language specification — v0.3 (dovc 0.3.0)

Status: **v0.3, draft.** This document describes what `dovc` 0.3.0 actually accepts and does.
It is derived from the compiler (`crates/dovc/src`), and every rule has a test under
`tests/golden/`. Where behavior is not fixed yet, the text says **unspecified** and names the
current behavior. Breaking changes before 1.0 are allowed and go in `CHANGELOG.md`.

Contents: 1 Source text · 2 Grammar · 3 Programs, names and imports · 4 Types · 5 Bindings and
immutability · 6 Functions · 7 Statements · 8 Expressions · 9 Integer arithmetic and panics ·
10 Strings, slices and built-ins · 11 Agents · 12 C interop · 13 Program execution · 14 Diagnostics ·
15 Canonical format · 16 Known limits · 17 Structs · 18 Enums and `match` · 19 Generics ·
20 The prelude · 21 `defer`

---

## 1. Source text

- A source file is UTF-8 text with the extension `.dov`. One file is one program.
- **Whitespace** (any Unicode whitespace, including `\r`) separates tokens. Otherwise it is ignored.
- **Comments** run from `//` to the end of the line. There are no block comments.
- **Identifiers**: `[A-Za-z_][A-Za-z0-9_]*`. Identifiers are ASCII only, so the emitted C names are
  portable. A non-ASCII letter outside a string is an error (`unexpected character`).
- **Keywords** (reserved, cannot be names):
  `agent defer else enum extern false fn if import let match on return send spawn struct true var while`.
  The type names `i64`, `bool` and `str` are not keywords, but no top-level item may use them as a name.
- **Integer literals**: `[0-9][0-9_]*`. The `_` characters are ignored. The value must fit in `i64`.
  There are no hex, octal or binary literals. A literal is never negative. `-5` is unary minus applied
  to `5`, so `-9223372036854775808` is rejected. Write `-9223372036854775807 - 1`.
- **String literals**: `"` … `"` on one line. Escapes: `\n` `\t` `\\` `\"` `\0`. Any other escape
  is an error. Other characters, including raw tabs and non-ASCII text, are taken literally.
- **Operators and punctuation**: `( ) { } [ ] , ; : :: . .. -> => = + - * / % == != < <= > >= && || !`.
  Two-character tokens are matched first. A lone `&` or `|` is an error. Because `>=` is one token,
  write `Option<i64> = x` with a space, not `Option<i64>= x`.

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
item        = fn_decl | extern_decl | agent_decl | struct_decl | enum_decl | import ;

import      = "import" STRING ";" ;
fn_decl     = "fn" IDENT tparams? params ret_type? block ;
extern_decl = "extern" STRING(* must be "C" *) "fn" IDENT params ret_type? ";" ;
agent_decl  = "agent" IDENT "{" agent_member* "}" ;
agent_member= field | handler ;
field       = ("let" | "var") IDENT (":" type)? "=" expr ";" ;
handler     = "on" IDENT params block ;
struct_decl = "struct" IDENT tparams? "{" (IDENT ":" type ("," IDENT ":" type)* ","?)? "}" ;
enum_decl   = "enum" IDENT tparams? "{" variant ("," variant)* ","? "}" ;
variant     = IDENT ("(" (type ("," type)* ","?)? ")")? ;
tparams     = "<" IDENT ("," IDENT)* ","? ">" ;

params      = "(" (param ("," param)* ","?)? ")" ;
param       = IDENT ":" type ;
ret_type    = "->" type ;
type        = "i64" | "bool" | "str" | "(" ")" | "[" type "]"
            | IDENT ("<" type ("," type)* ","? ">")? (* struct, enum, agent or type parameter *) ;

block       = "{" stmt* "}" ;
stmt        = ("let" | "var") IDENT (":" type)? "=" expr ";"
            | place "=" expr ";"
            | if_stmt
            | "while" expr block
            | "return" expr? ";"
            | "send" postfix "." IDENT args ";"
            | "match" expr "{" arm* "}"
            | "defer" (block | stmt)
            | expr ";" ;
place       = IDENT ("." IDENT)* ;
if_stmt     = "if" expr block ("else" (if_stmt | block))? ;
arm         = pattern "=>" block ;
pattern     = "_" | "-"? INT | STRING | "true" | "false"
            | (IDENT "::")? IDENT ("(" (binder ("," binder)* ","?)? ")")? ;
binder      = IDENT | "_" ;

expr        = or_expr ;
or_expr     = and_expr ("||" and_expr)* ;
and_expr    = eq_expr ("&&" eq_expr)* ;
eq_expr     = cmp_expr (("==" | "!=") cmp_expr)* ;
cmp_expr    = add_expr (("<" | "<=" | ">" | ">=") add_expr)* ;
add_expr    = mul_expr (("+" | "-") mul_expr)* ;
mul_expr    = unary (("*" | "/" | "%") unary)* ;
unary       = ("-" | "!") unary | postfix ;
postfix     = primary ( "." IDENT | "[" expr "]" | "[" expr ".." expr "]" )* ;
primary     = INT | STRING | "true" | "false"
            | IDENT args?                          (* call if followed by "(" *)
            | IDENT "::" IDENT args?               (* enum variant *)
            | IDENT "{" (IDENT ":" expr ("," IDENT ":" expr)* ","?)? "}"   (* struct literal *)
            | "[" (expr ("," expr)* ","?)? "]"     (* slice literal *)
            | "spawn" IDENT
            | "(" expr ")" ;
args        = "(" (expr ("," expr)* ","?)? ")" ;
```

Notes:
- A trailing comma is accepted in `params` and `args`.
- The statement form `IDENT "=" …` is chosen when an identifier is directly followed by `=`.
- All binary operators are left-associative. The chained forms `a < b < c` and `a == b == c` parse,
  but they fail to type-check (§8).
- The `send` target is a `postfix`, so `send spawn G.hi("x");` and `send (h).hi("x");` are valid.
  In `send a.b.h(x);` the last `.IDENT(` is the handler; the parts before it are fields.
- A struct literal `Name { f: e }` is not allowed directly in the head of `if`, `while` or `match`
  (there `Name {` starts the block). Put it in parentheses: `if (P { x: 1 }).x > 0 { ... }`.
- The left side of `=` is parsed as an expression and must be a `place` (§5).
- There is no `for`, `break` or `continue`. Patterns are one level deep (§18).

## 3. Programs, names and imports

- A program is a list of items in any order. An item may use a later item (mutual recursion works).
- Functions, `extern` functions, agents, structs and enums share **one namespace**. A name defined
  twice is an error, also across files and with the prelude (§20).
- The built-in names `print`, `panic`, `len`, `append`, `concat`, `to_str` and `char_of` (§10) cannot
  be redefined. The names `i64`, `bool` and `str` cannot be item names.
- **Imports.** `import "path.dov";` loads another file. The path is relative to the folder of the
  file that contains the `import`. All items of all files go into the one namespace; there are no
  qualified names (`mod::f`) yet. Each file is loaded once, however often and from wherever it is
  imported, so import cycles are allowed. An `import` may appear anywhere at the top level.
  Diagnostics name the file that contains the error.
- The program must define `fn main()`. It takes no parameters and returns `()`. `main` cannot be called.
- Local names (bindings and parameters) are a separate namespace from items. `let f = 1;` does not hide
  a function `f`, and `f()` still calls the function.

## 4. Types

| Type | Values | C representation |
|---|---|---|
| `i64` | signed 64-bit integers | `int64_t` (`long` in `extern` prototypes) |
| `bool` | `true`, `false` | `bool` |
| `str` | immutable byte strings with a length | `dov_str` = `{ const char *p; int64_t len; }` (`p` is NUL-terminated) |
| `()` | the unit value (no value) | `void` |
| `A` (an agent name) | a handle to an agent of kind `A` | `dov_h_A` (a pointer) |
| `S` / `S<T, …>` (a struct) | a record of named fields (§17) | a C `struct` per concrete instance |
| `E` / `E<T, …>` (an enum) | one of its variants (§18) | a tagged union per concrete instance |
| `[T]` | an immutable slice of `T` values | `dov_slice` = `{ buf, off, len }` |
| `T` (a type parameter) | inside a generic item only (§19) | the concrete type of each instance |

- Types are compared structurally by name and type arguments. There are **no implicit conversions** and no casts.
- `()` is only a return type. Parameters, bindings, fields, payloads and slice items cannot have type `()`.
- An agent handle is an opaque, copyable, immutable value. Handles cannot be compared or printed.
- Type annotations are optional on `let`/`var` and fields. Without one, the type is the type of the
  initializer. With one, the initializer is checked against it (so `let x: Option<i64> = None;` works).
- **Values.** Every value is copied on assignment, on a call and into a message. A struct or enum is
  copied field by field. A `str` or slice copies only its small descriptor; the bytes or items it
  points to live on the heap, are **immutable**, and are never freed in v0.3. So no two places can
  ever observe a change to shared data, and values can cross between agents freely.

## 5. Bindings and immutability

Immutability is the default. Mutation needs an explicit `var`.

| Declared with | Can be assigned? |
|---|---|
| `let x = …;` | no: compile error *cannot assign to immutable binding* |
| `var x = …;` | yes, with a value of the same type |
| function or handler parameter | no: *immutable parameter* |
| agent field `let f = …;` | no: *immutable field* |
| agent field `var f = …;` | yes, but only inside that agent's own handlers (§11) |
| a field `x.f` / `x.f.g` of a struct | only if the root `x` is a `var` binding or `var` field |
| an item `s[i]` of a slice or string | never: heap data is immutable |
| a `match` binding | no |

- Assignment `place = e;` is a statement, not an expression. The place is a name or a field chain
  `x.f.g`. Its root `x` must be visible and mutable, and `e` must have the type of the place.
  Assigning to a field changes only that copy of the struct.
- **Scopes**: each block `{ … }` opens a scope. The parameters are in a scope that encloses the body,
  so a `let` in the body may shadow a parameter. A name cannot be declared twice in the same scope,
  and that includes a duplicate parameter name. An inner block may shadow an outer name.
  A binding is visible from its declaration to the end of its block.
- In a handler, a name resolves to locals and parameters first, then to the agent's fields.
  A parameter or local therefore shadows a field of the same name.
- A `let` binding is also emitted as a C `const` object, so the C compiler checks the rule a second time.
- Every value is immutable data. The only mutable things are `var` bindings and `var` fields (and
  the fields of structs stored in them).

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
  is conservative. A block returns when it directly contains a `return`, a `panic(...);` statement,
  an `if` with an `else` where both branches return, or a `match` where every arm returns (a `match`
  is always exhaustive, §18). A `return` inside a `while` does not count. Code after a `return` is
  allowed and is never run. If control reaches the end anyway (it cannot today), the program panics
  with `missing return in \`f\``.
- Generic functions: §19.
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
- `match e { pattern => { … } … }`: §18.
- `defer stmt` / `defer { … }`: §21.
- Expression statement `e;`: evaluates `e` and discards the result, for example `print(...)`, a call, or even `1 + 2;`.

## 8. Expressions

Precedence, from lowest to highest. All binary operators are left-associative.

| Level | Operators | Operand types → result |
|---|---|---|
| 1 | `\|\|` | `bool`, `bool` → `bool` (short-circuit) |
| 2 | `&&` | `bool`, `bool` → `bool` (short-circuit) |
| 3 | `==` `!=` | same type, one of `i64`/`bool`/`str` → `bool` (not structs, enums, slices) |
| 4 | `<` `<=` `>` `>=` | `i64`, `i64` → `bool` |
| 5 | `+` `-` | `i64`, `i64` → `i64` (checked, §9) |
| 6 | `*` `/` `%` | `i64`, `i64` → `i64` (checked, §9) |
| 7 (prefix) | `-` `!` | `-`: `i64` → `i64` (checked); `!`: `bool` → `bool` |

- `str` equality compares length and bytes. Agent handles cannot be compared.
- `spawn A` has type `A` (§11). A call has the callee's return type.
- Postfix: `e.f` reads a struct field (§17). `e[i]` indexes a slice or a `str`; `e[a..b]` takes a
  range (§10). Both are bounds-checked.
- **Expected types.** The checker passes the type a context expects (an annotation, a parameter
  type, a return type, a field type) into the expression. That is how `None`, `[]` and generic calls
  get their type arguments (§19).
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
| `s[i]`, `xs[i]` | `i < 0` or `i >= len` | `string index out of bounds` / `slice index out of bounds` |
| `s[a..b]`, `xs[a..b]` | not `0 <= a <= b <= len` | `string range out of bounds` / `slice range out of bounds` |
| `char_of(c)` | `c` not in `0..=255` | `` `char_of` needs a byte value 0..255 `` |
| `panic(msg)` | always | `msg` |

- Division truncates toward zero (`-7 / 2 == -3`). The remainder has the sign of the dividend
  (`-7 % 3 == -1`). `i64::MIN % -1` is `0`. It does not panic.
- **A panic:** stdout is flushed, then `dov panic: <message> (line <N>)` is written to stderr, and the
  process exits with code **101**. `<N>` is the line of the operator or call. For the prelude
  helpers (`unwrap`, `unwrap_ok`, `unwrap_err`) it is the line inside the prelude (§20). Nothing
  else runs after a panic, including queued messages.

## 10. Strings, slices and built-ins

**Strings.** A `str` is an immutable sequence of bytes with a length (UTF-8 text is just bytes).
Literals, `concat` results and ranges are all `str`. Indexing works on bytes.

**Slices.** `[T]` is an immutable sequence of `T` values. `[a, b, c]` builds one; all items need the
same type. `[]` needs an expected type (`let xs: [i64] = [];`). Slices of slices, of structs and of
enums are allowed, and a struct or enum may contain a slice of itself (that is how recursive data
such as trees is written).

| Built-in | Type | Result |
|---|---|---|
| `print(e1, …)` | any number of `i64`/`bool`/`str` | `()`; writes each argument, then a newline |
| `len(x)` | `str` or `[T]` → `i64` | number of bytes or items |
| `x[i]` | `str` → `i64` (a byte, 0..255); `[T]` → `T` | bounds-checked |
| `x[a..b]` | `str` → `str`; `[T]` → `[T]` | items `a` up to (not including) `b`, bounds-checked |
| `concat(a, b)` | `str, str` → `str`; `[T], [T]` → `[T]` | a new value; `a` and `b` are unchanged |
| `append(xs, x)` | `[T], T` → `[T]` | a new slice with `x` at the end; `xs` is unchanged |
| `to_str(x)` | `i64`, `bool` or `str` → `str` | decimal, `true`/`false`, or `x` |
| `char_of(c)` | `i64` → `str` | the one-byte string with byte `c` (0..255) |
| `panic(msg)` | `str` → `()` | stops the program (§9) |

- `append` is amortized O(1): when `xs` ends where its buffer's written items end, the new item is
  written in place after them. No existing slice can see that slot, so nothing visible changes.
  Otherwise the items are copied. A range `xs[a..b]` shares the buffer and copies nothing.
- `print` evaluates and writes one argument at a time. If a later argument panics, the earlier
  arguments are already written. `i64` prints in decimal and `bool` as `true`/`false`. A `str` is
  written byte for byte, including any `\0` bytes.

```dov
let s = concat("hello, ", "world");
print(len(s), " ", s[0], " ", s[7..12]);   // 12 104 world
var xs: [i64] = [];
xs = append(xs, 1);
print(len(concat(xs, [2, 3])));            // 3
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
  Fields and message arguments may have any value type: structs, enums, slices and handles too.
  Because heap data is immutable, a message never shares mutable state with its sender.
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
- Agents and messages are never freed. A message's argument block is freed after its handler runs.
- There is one thread, so the generated programs have no data races by construction; the agent
  examples are also built with `-fsanitize=thread` in CI.

## 12. C interop

```dov
extern "C" fn labs(x: i64) -> i64;
extern "C" fn srand(seed: i64);

fn main() {
    print(labs(-42));
}
```

- `extern "C" fn name(params) -> R;` declares a C function. Only the ABI string `"C"` is accepted.
- Parameter types: `i64`, `bool` or `str`. The return type: `i64`, `bool`, `str` or `()`. Structs,
  enums, slices and agent handles cannot cross into C.
- Mapping: `i64` → C `long`, `bool` → `bool`, `str` → `const char *`, `()` → `void`. A `str` argument
  passes its NUL-terminated bytes (so C sees it only up to the first `\0`). A `str` result is read
  with `strlen`; a `NULL` result becomes `""`.
  `long` is 64-bit only on LP64 targets, so the runtime header has a static assert for LP64
  (64-bit macOS and Linux).
- The emitted C prototype is `R name(T1, T2, …);`, with the name unchanged. The name therefore
  cannot be one of these, and each is a compile error:
  - a C keyword, or `main`;
  - a name that begins with `dov_` or `DOV_`;
  - a name the generated C uses: `v_*`, `v<digit>*`, `f_*`, `self`, `args`, `m` (all generated
    types and functions start with `dov_`);
  - a name from `<stdint.h>`, `<stdbool.h>` or `<stddef.h>`, such as `NULL`, `size_t`, `bool`,
    `int64_t` or `INT64_MAX`.
- **Prototypes are trusted.** If the declaration does not match the real C function (for example
  `strlen`, which returns `size_t`, or `abs`, which takes `int`), the behavior is that of C:
  undefined, or a C compile error. Variadic functions such as `printf` cannot be declared.
- An extern call is an ordinary call expression, and it follows the call rules in §6 and §8.
- The program is linked with the system C library only.

## 13. Program execution

1. `dovc` loads the prelude (§20), the file and its imports (§3), then lexes, parses and checks
   them. A compile error stops it here (§14).
2. It emits one C11 file that includes only `dov_rt.h`, with one C function or type per concrete
   generic instance, and compiles it with the embedded runtime using `$CC` (default `cc`) and
   `-std=c11 -O2`. The emitted C also compiles cleanly with `-Wall -Wextra -Werror` (tested).
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
- `dovc` reports the first error only. An error in an imported file names that file.
- Every error kind has a golden test in `tests/golden/fail/`, whose `.err` file holds its exact output.
- Hints may suggest a near name (`did you mean \`total\`?`). The exact hint text is not a stable interface.

## 15. Canonical format

`dovc fmt` produces the canonical layout. `dovc fmt --check` exits with 1 if a file would change.
- 4 spaces of indent per `{`. One statement, field, handler, `match` arm, struct field or enum
  variant per line. `{` ends its line, `}` starts its own line, `} else {` stays on one line, and an
  empty block is `{}`. A struct literal stays on one line: `P { x: 1, y: 2 }`, `P {}`.
- One space around binary operators, `=`, `=>` and `->`. One space after `,` and `:`. No space inside
  `( )` or `[ ]`, before the `(` of a call or parameter list, before an index `[`, around `.`, `::`
  and `..`, around the `<` `>` of type parameters and arguments, or after a unary `-` or `!`.
- Blank lines are kept but collapsed to one. There are none at the start or end of a block. A
  top-level item that ends in `}`, or a `fn`/`agent` that follows an `extern`, gets one blank line
  before the next item.
- Comments stay in place. A comment on its own line stays on its own line at the current indent. A
  trailing comment comes after one space. Literals keep their source spelling. Lines are never wrapped.
- The output ends with exactly one newline, and an empty file stays empty. Formatting is idempotent,
  and it never changes tokens or comments (the formatter checks this every time it runs).
- Only files that parse can be formatted. A syntax error is reported as in §14.

## 16. Known limits (v0.3)

- The operand order of binary operators, call arguments and struct literal fields is unspecified (§8).
- Heap data (string bytes, slice items) and agents are never freed. Long-running programs grow.
- There are no methods, traits, closures, nested patterns, `for`, `break` or `continue`.
  `match` is a statement, not an expression. Patterns are one level deep.
- `==` works only on `i64`, `bool` and `str`. Compare structs and enums with `match` or a function.
- Imports merge everything into one namespace; there are no qualified names or visibility rules.
- There is one global queue and one thread. Deep recursion, and a field initializer that spawns its
  own agent kind, overflow the native stack without a panic message.
- `extern "C"` prototypes are trusted (§12). Only LP64 targets are supported.
- Only the first compile error is reported.

## 17. Structs

```dov
struct Point { x: i64, y: i64 }
struct Pair<A, B> { first: A, second: B }

fn main() {
    var p = Point { x: 1, y: 2 };
    p.x = 10;                        // ok: p is `var`
    let q = p;                       // a copy
    print(q.x, " ", Pair { first: "a", second: 1 }.second);
}
```

- A struct declares named, typed fields. Field names must be unique; no field can have type `()`.
- A struct literal `Name { f: e, … }` must give every field exactly once, in any order.
- `e.f` reads a field. Fields are assigned through a `var` root only (§5).
- A struct may not contain itself by value, directly or through other structs and enums (it would
  have infinite size). Through a slice is fine: `struct Node { kids: [Node] }`.
- C layout: one C `struct` per concrete instance with members `f_<name>` in declaration order.

## 18. Enums and `match`

```dov
enum Shape { Circle(i64), Rect(i64, i64), Empty }

fn area(s: Shape) -> i64 {
    match s {
        Shape::Circle(r) => { return 3 * r * r; }
        Shape::Rect(w, h) => { return w * h; }
        Shape::Empty => { return 0; }
    }
}
```

- An enum has one or more variants. A variant has zero or more payload types. A value is written
  `E::V` or `E::V(a, b)`; the number and types of values must match the variant.
- `match e { pattern => { … } … }` is a statement. Each arm has a block. Arms are tried in order.
- Patterns: `_` (matches anything); an `i64`, `bool` or `str` literal (for a scrutinee of that type);
  or a variant `E::V(a, _)` for an enum. In a pattern the `E::` may be left out (`V(a, _)`), because
  the scrutinee's type is known. Each binder is a new immutable binding in the arm's block, or `_`.
  A pattern binds every payload value.
- **Exhaustiveness.** A `match` on an enum must cover every variant or end with `_`. A `match` on
  `bool` must cover `true` and `false` or have `_`. A `match` on `i64` or `str` needs `_`.
- An arm after `_`, or a variant or literal that an earlier arm already matches, is an error
  (unreachable pattern).
- You cannot `match` on a struct, slice or agent handle.
- C layout: `struct { int32_t tag; union { struct { T0 a0; … } v_<Variant>; … } u; }`. The tag is the
  variant's index in declaration order. An enum `match` becomes a C `switch` on the tag; a literal
  `match` becomes an `if`/`else` chain.

## 19. Generics

```dov
fn first<T>(xs: [T]) -> Option<T> {
    if len(xs) == 0 { return None; }
    return Some(xs[0]);
}
```

- Functions, structs and enums may have type parameters: `fn f<T, U>(…)`, `struct S<T> {…}`,
  `enum E<T> {…}`. A type parameter name may appear once per declaration.
- A generic body is **checked once**, with each type parameter opaque: no operator applies to a
  value of type `T` (no `+`, no `==`, no `print`). It can be stored, passed, returned, put in
  structs, enums and slices, and given to other generic functions.
- **Inference.** Type arguments are never written at a call or literal. The checker infers them
  from the arguments and from the expected type, with unification. `let x = None;` is an error
  (nothing fixes `T`); `let x: Option<i64> = None;`, `var y = None; y = Some(1);` and
  `let xs: [i64] = empty();` are fine. Every type must be known at the end of each function body.
- Written types must have the right number of type arguments: `Option<i64>`, not `Option`.
- **Monomorphization.** The compiler emits one C function per concrete instance that the program
  reaches (`dov_fn_first__l` for `first<i64>`), and one C type per concrete struct or enum instance.
  A generic function that is never called is checked but not emitted.

## 20. The prelude

Every program starts with this file (`crates/dovc/src/prelude.dov`), as if imported:

```dov
enum Option<T> { Some(T), None }
enum Result<T, E> { Ok(T), Err(E) }
fn is_some<T>(o: Option<T>) -> bool          fn is_none<T>(o: Option<T>) -> bool
fn unwrap<T>(o: Option<T>) -> T               // panics on None
fn unwrap_or<T>(o: Option<T>, fallback: T) -> T
fn is_ok<T, E>(r: Result<T, E>) -> bool      fn is_err<T, E>(r: Result<T, E>) -> bool
fn unwrap_ok<T, E>(r: Result<T, E>) -> T      // panics on Err
fn unwrap_err<T, E>(r: Result<T, E>) -> E     // panics on Ok
fn ok_or<T, E>(o: Option<T>, e: E) -> Result<T, E>
```

- The variants of `Option` and `Result` may be written bare: `Some(x)`, `None`, `Ok(v)`, `Err(e)`
  (the qualified forms `Option::Some(x)` also work). Other enums always use `E::V` in expressions.
- These names are in the program's namespace, so a program cannot define another `unwrap`.
- There is no null in .dov: an absent value is `None`, a failure is `Err(e)`.

## 21. `defer`

```dov
fn copy(src: str) -> bool {
    let f = open(src);
    defer close(f);          // runs however the function leaves
    if bad(f) { return false; }
    return true;
}
```

- `defer s` or `defer { … }` registers its statement or block to run when the enclosing block is
  left: at the end of the block, and before a `return` (for every enclosing block of the function).
- Several defers run in reverse order (last registered first). A defer that was not reached is not
  run. A defer inside a loop body runs at the end of each iteration.
- On `return e;` the value `e` is computed first, then the defers run, then the function returns.
- A deferred block sees the names that were visible where it was written, even when it runs from a
  deeper block that shadows them. It may assign `var` bindings. It cannot contain `return`.
- A panic stops the program at once; defers do not run.
