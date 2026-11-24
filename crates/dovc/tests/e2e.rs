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
    let extra = "fn f(n: i64) -> i64 { if n == 0 { return 0; } return n + f(n - 1); }\n\
                 fn unused(x: i64, y: bool) {}\n\
                 fn main() { while f(1) != 1 { } unused(1, true); print(f(3)); }\n";
    let c = dir.join("extra.c");
    std::fs::write(&c, dovc::compile_to_c(extra).unwrap()).unwrap();
    strict_cc(&c);
    strict_cc(&dir.join("dov_rt.c"));
}

#[test]
fn fmt_cli_check_and_rewrite() {
    let f = temp_src("messy.dov", "fn main(){print(1);}\n");
    let o = dovc(&["fmt", "--check", &f]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("is not formatted"));
    assert!(dovc(&["fmt", &f]).status.success());
    assert_eq!(
        std::fs::read_to_string(&f).unwrap(),
        "fn main() {\n    print(1);\n}\n"
    );
    assert!(dovc(&["fmt", "--check", &f]).status.success());
    let bad = temp_src("bad.dov", "fn main( {\n");
    let o = dovc(&["fmt", &bad]);
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("= hint: "));
}

#[test]
fn examples_are_formatted() {
    for ex in ["hello", "agent_counter", "ffi"] {
        let o = dovc(&["fmt", "--check", &format!("examples/{ex}.dov")]);
        assert!(
            o.status.success(),
            "{ex}: {}",
            String::from_utf8_lossy(&o.stderr)
        );
    }
}

#[test]
fn build_errors_render_full_diagnostics() {
    let f = temp_src("err.dov", "fn main() {\n    let x = 1;\n    x = 2;\n}\n");
    let o = dovc(&["run", &f]);
    assert_eq!(o.status.code(), Some(1));
    let e = String::from_utf8_lossy(&o.stderr);
    assert!(
        e.contains(":3:5: error: cannot assign to immutable binding `x`"),
        "{e}"
    );
    assert!(
        e.contains("3 |     x = 2;\n  |     ^\n  = hint: declare it with `var x`"),
        "{e}"
    );
}

#[test]
fn version_and_usage() {
    let o = dovc(&["--version"]);
    assert!(
        String::from_utf8_lossy(&o.stdout).starts_with(concat!("dovc ", env!("CARGO_PKG_VERSION")))
    );
    assert_eq!(dovc(&[]).status.code(), Some(2));
}

/// Each example must print exactly `examples/expected/NAME.stdout`, and its C must
/// compile with `-std=c11 -Wall -Wextra -Werror`.
fn example(name: &str) {
    let file = format!("examples/{name}.dov");
    let want =
        std::fs::read_to_string(root().join(format!("examples/expected/{name}.stdout"))).unwrap();
    assert_eq!(run_ok(&file), want, "{file}");
    let src = std::fs::read_to_string(root().join(&file)).unwrap();
    let c = dovc::compile_file(&src, root().join(&file).to_str().unwrap()).unwrap();
    let dir = dovc::scratch_dir().unwrap();
    std::fs::write(dir.join("dov_rt.h"), dovc::RUNTIME_H).unwrap();
    std::fs::write(dir.join("dov_rt.c"), dovc::RUNTIME_C).unwrap();
    std::fs::write(dir.join("prog.c"), c).unwrap();
    strict_cc(&dir.join("prog.c"));
}

macro_rules! examples {
    ($($name:ident),*) => { $( #[test] fn $name() { example(stringify!($name).trim_start_matches("example_")); } )* };
}

examples!(
    example_hello,
    example_agent_counter,
    example_ffi,
    example_shapes,
    example_generics,
    example_wordfreq,
    example_defer,
    example_sieve,
    example_json,
    example_agent_pipeline,
    example_agent_bank,
    example_agent_pingpong
);

#[test]
fn there_are_twelve_examples() {
    let n = std::fs::read_dir(root().join("examples"))
        .unwrap()
        .filter(|e| {
            let n = e.as_ref().unwrap().file_name();
            let n = n.to_string_lossy();
            n.ends_with(".dov") && !n.starts_with("._")
        })
        .count();
    assert_eq!(n, 12);
}

/// The agent examples also run under ThreadSanitizer (skipped where TSan is unavailable,
/// unless DOV_REQUIRE_SAN is set).
#[test]
fn agent_examples_pass_thread_sanitizer() {
    let probe_dir = dovc::scratch_dir().unwrap();
    let probe = probe_dir.join("p.c");
    std::fs::write(&probe, "int main(void) { return 0; }\n").unwrap();
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let ok = Command::new(&cc)
        .args(["-fsanitize=thread", "-o"])
        .arg(probe_dir.join("p"))
        .arg(&probe)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
        && Command::new(probe_dir.join("p"))
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    if !ok {
        assert!(
            std::env::var_os("DOV_REQUIRE_SAN").is_none(),
            "TSan required but unavailable"
        );
        eprintln!("note: -fsanitize=thread unavailable; skipping");
        return;
    }
    for name in [
        "agent_counter",
        "agent_pipeline",
        "agent_bank",
        "agent_pingpong",
    ] {
        let file = root().join(format!("examples/{name}.dov"));
        let src = std::fs::read_to_string(&file).unwrap();
        let c = dovc::compile_file(&src, file.to_str().unwrap()).unwrap();
        let exe = dovc::scratch_dir().unwrap().join(name);
        dovc::build_c(&c, &exe, &["-g", "-fsanitize=thread"]).unwrap();
        let out = Command::new(&exe).output().unwrap();
        assert!(
            out.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let want = std::fs::read_to_string(root().join(format!("examples/expected/{name}.stdout")))
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            want,
            "{name} under TSan"
        );
    }
}
