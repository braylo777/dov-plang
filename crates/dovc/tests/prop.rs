//! Property tests with a small built-in generator (no dependencies, fixed seeds, so every
//! run checks the same cases and a failure prints the program that caused it).
//!
//! Properties:
//! - the lexer and the whole front end never panic, whatever the input;
//! - `dovc fmt` is idempotent and keeps tokens and comments, on random valid programs;
//! - a formatted program still parses and checks the same way;
//! - random well-typed i64 expressions compile and print the value Rust computes
//!   (with the same checked-arithmetic panics).

/// xorshift64* — small, deterministic.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    fn pick<'a>(&mut self, xs: &[&'a str]) -> &'a str {
        xs[self.below(xs.len() as u64) as usize]
    }
    fn chance(&mut self, pct: u64) -> bool {
        self.below(100) < pct
    }
}

/// Random spacing: the formatter must normalize it.
fn ws(r: &mut Rng) -> &'static str {
    ["", " ", "  ", "\n", "\t", " \n  "][r.below(6) as usize]
}

fn int_expr(r: &mut Rng, depth: u32, vars: &[&str]) -> String {
    if depth == 0 || r.chance(30) {
        return if !vars.is_empty() && r.chance(40) {
            r.pick(vars).to_string()
        } else {
            r.below(1000).to_string()
        };
    }
    match r.below(5) {
        0 => format!("-{}", int_expr(r, depth - 1, vars)),
        1 => format!("({})", int_expr(r, depth - 1, vars)),
        2 => format!("len({})", str_expr(r, depth - 1)),
        _ => {
            let op = r.pick(&["+", "-", "*", "/", "%"]);
            let w = ws(r);
            format!(
                "{}{w}{op}{w}{}",
                int_expr(r, depth - 1, vars),
                int_expr(r, depth - 1, vars)
            )
        }
    }
}

fn str_expr(r: &mut Rng, depth: u32) -> String {
    if depth == 0 || r.chance(40) {
        return r
            .pick(&["\"\"", "\"a\"", "\"hello\"", "\"tab\\t\"", "to_str(7)"])
            .to_string();
    }
    match r.below(3) {
        0 => format!(
            "concat({}, {})",
            str_expr(r, depth - 1),
            str_expr(r, depth - 1)
        ),
        1 => format!("to_str({})", int_expr(r, depth - 1, &[])),
        _ => format!("char_of({})", r.below(128)),
    }
}

fn bool_expr(r: &mut Rng, depth: u32, vars: &[&str]) -> String {
    if depth == 0 || r.chance(30) {
        return r.pick(&["true", "false"]).to_string();
    }
    match r.below(4) {
        0 => format!("!({})", bool_expr(r, depth - 1, vars)),
        1 => format!(
            "({}) {} ({})",
            bool_expr(r, depth - 1, vars),
            r.pick(&["&&", "||", "==", "!="]),
            bool_expr(r, depth - 1, vars)
        ),
        _ => format!(
            "{} {} {}",
            int_expr(r, depth - 1, vars),
            r.pick(&["<", "<=", ">", ">=", "==", "!="]),
            int_expr(r, depth - 1, vars)
        ),
    }
}

/// A random program that parses (and usually checks), with odd spacing and comments.
fn program(r: &mut Rng) -> String {
    let mut s = String::new();
    if r.chance(50) {
        s.push_str("// header comment\n");
    }
    s.push_str("struct P<T> {x:T,\n y : i64}\nenum E { A(i64), B }\n");
    s.push_str("fn id<T>(v: T)->T{return v;}\n");
    s.push_str("fn main()");
    s.push_str(ws(r));
    s.push('{');
    let mut vars: Vec<String> = Vec::new();
    for i in 0..(2 + r.below(8)) {
        let refs: Vec<&str> = vars.iter().map(String::as_str).collect();
        let w = ws(r);
        let stmt = match r.below(9) {
            0 => format!("let {w}v{i} = {};", int_expr(r, 3, &refs)),
            1 => format!("var v{i}{w}={w}{};", int_expr(r, 3, &refs)),
            2 => format!("print({}, {});", str_expr(r, 2), bool_expr(r, 2, &refs)),
            3 => format!(
                "if {} {{ print(1); }} else {{ print({}); }}",
                bool_expr(r, 2, &refs),
                int_expr(r, 2, &refs)
            ),
            4 => format!("let v{i} = P {{ x: id({}), y: 2 }};", int_expr(r, 2, &refs)),
            5 => format!(
                "match E::A({}) {{ E::A(n) => {{ print(n); }} _ => {{}} }}",
                int_expr(r, 2, &refs)
            ),
            6 => format!("let v{i} = [{}, {}];", int_expr(r, 1, &refs), r.below(9)),
            7 => format!("defer print({});{w}// trailing", str_expr(r, 1)),
            _ => format!("let v{i} = Some({})  ;", int_expr(r, 2, &refs)),
        };
        // Only i64 bindings are usable in later expressions.
        if stmt.contains(&format!("v{i} = ")) || stmt.contains(&format!("v{i}=")) {
            let is_int = !(stmt.contains("P {") || stmt.contains('[') || stmt.contains("Some("));
            if is_int {
                vars.push(format!("v{i}"));
            }
        }
        s.push_str(ws(r));
        s.push_str(&stmt);
        s.push('\n');
    }
    s.push_str("}\n");
    s
}

#[test]
fn lexer_and_front_end_never_panic() {
    let mut r = Rng(0x5eed_0001);
    let alphabet: Vec<char> =
        "fn let var if else while match defer struct enum import { } ( ) [ ] < > :: .. => , ; : . = + - * / % ! & | \" \\ \n 0 7 x _ é 9_9 // "
            .chars()
            .collect();
    for _ in 0..400 {
        let n = r.below(80) as usize;
        let s: String = (0..n)
            .map(|_| alphabet[r.below(alphabet.len() as u64) as usize])
            .collect();
        let _ = dovc::lexer::lex(&s);
        let _ = dovc::fmt::format(&s);
        let _ = dovc::check_source(&s);
    }
}

#[test]
fn front_end_never_panics_on_mutated_programs() {
    let mut r = Rng(0x5eed_0002);
    for _ in 0..200 {
        let p = program(&mut r);
        let mut chars: Vec<char> = p.chars().collect();
        // Delete or duplicate a few characters to make near-valid programs.
        for _ in 0..3 {
            let i = r.below(chars.len() as u64) as usize;
            if r.chance(50) {
                chars.remove(i);
            } else {
                let c = chars[i];
                chars.insert(i, c);
            }
        }
        let s: String = chars.into_iter().collect();
        let _ = dovc::fmt::format(&s);
        let _ = dovc::check_source(&s);
    }
}

#[test]
fn fmt_is_idempotent_and_keeps_tokens() {
    let mut r = Rng(0x5eed_0003);
    for _ in 0..300 {
        let p = program(&mut r);
        let once = dovc::fmt::format(&p).unwrap_or_else(|d| panic!("{}\n{p}", d.msg));
        let twice = dovc::fmt::format(&once).unwrap();
        assert_eq!(once, twice, "not idempotent:\n{p}");
        let a = dovc::lexer::lex(&p).unwrap();
        let b = dovc::lexer::lex(&once).unwrap();
        let ta: Vec<_> = a.iter().map(|t| &t.tok).collect();
        let tb: Vec<_> = b.iter().map(|t| &t.tok).collect();
        assert_eq!(ta, tb, "tokens changed:\n{p}");
    }
}

#[test]
fn formatting_preserves_check_result() {
    let mut r = Rng(0x5eed_0004);
    for _ in 0..200 {
        let p = program(&mut r);
        let f = dovc::fmt::format(&p).unwrap();
        let a = dovc::check_source(&p).map_err(|d| d.msg);
        let b = dovc::check_source(&f).map_err(|d| d.msg);
        assert_eq!(a, b, "check differs after fmt:\n{p}");
    }
}

#[test]
fn random_programs_mostly_check() {
    // The generator only builds well-typed programs, so every one must check.
    let mut r = Rng(0x5eed_0005);
    for _ in 0..200 {
        let p = program(&mut r);
        dovc::check_source(&p).unwrap_or_else(|d| panic!("{}: {}\n{p}", d.msg, d.hint));
    }
}

/// Rust model of `.dov` i64 semantics: None = the program panics.
fn eval(r: &mut Rng, depth: u32) -> (String, Option<i64>) {
    if depth == 0 || r.chance(25) {
        let v = r.below(2000) as i64 - 1000;
        return if v < 0 {
            (format!("(0 - {})", -v), Some(v))
        } else {
            (v.to_string(), Some(v))
        };
    }
    let (a, va) = eval(r, depth - 1);
    if r.chance(10) {
        return (format!("-({a})"), va.and_then(i64::checked_neg));
    }
    let (b, vb) = eval(r, depth - 1);
    let op = r.pick(&["+", "-", "*", "/", "%"]);
    let v = match (va, vb) {
        (Some(x), Some(y)) => match op {
            "+" => x.checked_add(y),
            "-" => x.checked_sub(y),
            "*" => x.checked_mul(y),
            "/" => x.checked_div(y),
            _ => {
                if y == 0 {
                    None
                } else {
                    Some(x.wrapping_rem(y))
                }
            }
        },
        _ => None,
    };
    (format!("({a} {op} {b})"), v)
}

#[test]
fn compiled_arithmetic_matches_rust() {
    let mut r = Rng(0x5eed_0006);
    let mut src = String::new();
    let mut want = String::new();
    // Only expressions that do not panic go into the program (a panic stops it).
    let mut fns = Vec::new();
    while fns.len() < 60 {
        let (e, v) = eval(&mut r, 4);
        if let Some(v) = v {
            let i = fns.len();
            src.push_str(&format!("fn e{i}() -> i64 {{ return {e}; }}\n"));
            want.push_str(&format!("{v}\n"));
            fns.push(i);
        }
    }
    src.push_str("fn main() {\n");
    for i in &fns {
        src.push_str(&format!("print(e{i}());\n"));
    }
    src.push_str("}\n");
    let dir = dovc::scratch_dir().unwrap();
    let exe = dir.join("arith");
    dovc::build(&src, "arith.dov", &exe).unwrap();
    let out = std::process::Command::new(&exe).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), want);
}

#[test]
fn compiled_arithmetic_panics_like_rust() {
    let mut r = Rng(0x5eed_0007);
    let mut found = 0;
    while found < 5 {
        let (e, v) = eval(&mut r, 5);
        if v.is_some() {
            continue;
        }
        found += 1;
        let src = format!("fn main() {{\n    print({e});\n}}\n");
        let dir = dovc::scratch_dir().unwrap();
        let exe = dir.join("p");
        dovc::build(&src, "p.dov", &exe).unwrap();
        let out = std::process::Command::new(&exe).output().unwrap();
        assert_eq!(out.status.code(), Some(101), "{src}");
        assert!(String::from_utf8_lossy(&out.stderr).starts_with("dov panic: "));
    }
}
