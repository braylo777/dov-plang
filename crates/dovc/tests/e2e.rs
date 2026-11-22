//! End-to-end: compile real `.dov` files with the `dovc` binary and run them.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dovc(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_dovc"))
        .args(args)
        .current_dir(root())
        .output()
        .unwrap()
}

fn run_ok(file: &str) -> String {
    let o = dovc(&["run", file]);
    assert!(
        o.status.success(),
        "{file} failed: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

fn temp_src(name: &str, src: &str) -> String {
    let f = dovc::scratch_dir().unwrap().join(name);
    std::fs::write(&f, src).unwrap();
    f.to_str().unwrap().to_string()
}

#[test]
fn hello_world() {
    assert_eq!(run_ok("examples/hello.dov"), "Hello, .dov!\n");
}

#[test]
fn agent_counter() {
    assert_eq!(
        run_ok("examples/agent_counter.dov"),
        "[1] counter total = 42\n"
    );
}

#[test]
fn ffi_and_loops() {
    assert_eq!(
        run_ok("examples/ffi.dov"),
        "labs(-42) = 42\nfib(50) = 12586269025\n"
    );
}

#[test]
fn agent_messages_are_fifo_and_deterministic() {
    let f = temp_src(
        "fifo.dov",
        r#"
agent Echo { on say(n: i64) { print(n); } }
agent Relay {
    on fwd(to: Echo, n: i64) { send to.say(n * 10); }
}
fn main() {
    let e = spawn Echo;
    let r = spawn Relay;
    send r.fwd(e, 1);
    send e.say(2);
    send r.fwd(e, 3);
}
"#,
    );
    let first = run_ok(&f);
    // Global FIFO: fwd(1), say(2), fwd(3), then the relayed say(10), say(30).
    assert_eq!(first, "2\n10\n30\n");
    for _ in 0..3 {
        assert_eq!(run_ok(&f), first);
    }
}

#[test]
fn overflow_panics_instead_of_wrapping() {
    let f = temp_src(
        "ovf.dov",
        "fn main() {\n  var x = 9223372036854775807;\n  x = x + 1;\n}\n",
    );
    let o = dovc(&["run", &f]);
    assert_eq!(o.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&o.stderr).contains("integer overflow in `+` (line 3)"));
}

#[test]
fn division_by_zero_panics() {
    let f = temp_src(
        "div.dov",
        "fn d(a: i64, b: i64) -> i64 { return a / b; }\nfn main() { print(d(1, 0)); }\n",
    );
    let o = dovc(&["run", &f]);
    assert_eq!(o.status.code(), Some(101));
    assert!(String::from_utf8_lossy(&o.stderr).contains("division by zero"));
}

#[test]
fn strings_bools_and_branches() {
    let f = temp_src(
        "misc.dov",
        r#"
fn sign(n: i64) -> str {
    if n < 0 { return "neg"; } else if n == 0 { return "zero"; } else { return "pos"; }
}
fn main() {
    print(sign(-5), " ", sign(0), " ", sign(7));
    print("a" == "a", " ", "a" != "b", " ", !(1 > 2) && true);
    print("tab\there \"quoted\" é");
}
"#,
    );
    assert_eq!(
        run_ok(&f),
        "neg zero pos\ntrue true true\ntab\there \"quoted\" é\n"
    );
}

/// Every file in tests/fail must be rejected with the `// expect:` text.
#[test]
fn negative_cases() {
    let mut n = 0;
    for entry in std::fs::read_dir(root().join("tests/fail")).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap().to_string();
        if !name.ends_with(".dov") || name.starts_with("._") {
            continue;
        }
        let src = std::fs::read_to_string(&p).unwrap();
        let expect = src
            .lines()
            .next()
            .unwrap()
            .strip_prefix("// expect: ")
            .expect("first line must be `// expect: ...`");
        let o = dovc(&["check", p.to_str().unwrap()]);
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(!o.status.success(), "{name} should fail");
        assert!(
            err.contains(expect),
            "{name}: expected `{expect}`, got `{err}`"
        );
        n += 1;
    }
    assert!(n >= 6, "only {n} negative cases found");
}

fn strict_cc(file: &Path) {
    let o = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-fsyntax-only"])
        .arg(file)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{}: {}",
        file.display(),
        String::from_utf8_lossy(&o.stderr)
    );
}

/// Generated C and the runtime must pass a strict compile, not only `-w`.
#[test]
fn generated_c_is_warning_free() {
    let dir = dovc::scratch_dir().unwrap();
    std::fs::write(dir.join("dov_rt.h"), dovc::RUNTIME_H).unwrap();
    std::fs::write(dir.join("dov_rt.c"), dovc::RUNTIME_C).unwrap();
    for ex in ["hello", "agent_counter", "ffi"] {
        let src = std::fs::read_to_string(root().join(format!("examples/{ex}.dov"))).unwrap();
        let c = dir.join(format!("{ex}.c"));
        std::fs::write(&c, dovc::compile_to_c(&src).unwrap()).unwrap();
        strict_cc(&c);
    }
    strict_cc(&dir.join("dov_rt.c"));
}

#[test]
fn version_and_usage() {
    let o = dovc(&["--version"]);
    assert!(String::from_utf8_lossy(&o.stdout).starts_with("dovc 0.1"));
    assert_eq!(dovc(&[]).status.code(), Some(2));
}
