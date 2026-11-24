//! Golden tests. Each `.dov` file under `tests/golden/` is one test (see build.rs).
//!
//! - `run/NAME.dov`: compile to C, build it with `cc -std=c11 -Wall -Wextra -Werror`, run it, and
//!   compare stdout with `NAME.stdout`. If `NAME.stderr` exists the program must exit 101 (panic)
//!   with exactly that stderr. When the C compiler supports it, the same C is also built with
//!   `-fsanitize=address,undefined` and must give the same result. The source must be `dovc fmt` clean.
//! - `fail/NAME.dov`: `dovc check` must exit 1 with exactly the stderr in `NAME.err`
//!   (position, source line, caret, hint).
//! - `fmt/NAME.dov`: `dovc fmt` output must equal `NAME.fmt`, and formatting that is a no-op.
//!
//! `DOV_BLESS=1 cargo test --test golden` rewrites the expected files (review the diff!).
//! `DOV_REQUIRE_SAN=1` makes a missing sanitizer runtime a failure (CI sets it).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn golden(kind: &str, name: &str, ext: &str) -> PathBuf {
    root().join(format!("tests/golden/{kind}/{name}.{ext}"))
}

fn bless() -> bool {
    std::env::var_os("DOV_BLESS").is_some()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("cannot read {}: {e}", p.display()))
}

/// Compare `got` with the expected file, or write it when blessing.
fn expect_file(p: &Path, got: &str) {
    if bless() {
        std::fs::write(p, got).unwrap();
        return;
    }
    let want = read(p);
    assert!(
        want == got,
        "{} differs\n--- expected\n{want}\n--- got\n{got}",
        p.display()
    );
}

fn cc() -> String {
    std::env::var("CC").unwrap_or_else(|_| "cc".into())
}

const SAN: &[&str] = &[
    "-fsanitize=address,undefined",
    "-fno-sanitize-recover=all",
    "-fno-omit-frame-pointer",
    "-g",
];

/// Can this machine build and run a sanitizer binary?
fn sanitizers() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        let dir = dovc::scratch_dir().unwrap();
        let c = dir.join("probe.c");
        std::fs::write(&c, "int main(void) { return 0; }\n").unwrap();
        let exe = dir.join("probe");
        let built = Command::new(cc())
            .args(SAN)
            .arg("-o")
            .arg(&exe)
            .arg(&c)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        let ok = built
            && Command::new(&exe)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        if !ok {
            assert!(
                std::env::var_os("DOV_REQUIRE_SAN").is_none(),
                "DOV_REQUIRE_SAN is set but `{} -fsanitize=address,undefined` does not work",
                cc()
            );
            eprintln!("note: sanitizers unavailable; skipping ASan/UBSan runs");
        }
        ok
    })
}

/// Build `prog.c` + runtime with strict warnings (plus `extra` flags) and run it.
fn build_and_run(dir: &Path, tag: &str, extra: &[&str]) -> Output {
    let exe = dir.join(format!("prog-{tag}"));
    let o = Command::new(cc())
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror", "-O1"])
        .args(extra)
        .arg("-o")
        .arg(&exe)
        .arg(dir.join("prog.c"))
        .arg(dir.join("dov_rt.c"))
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "strict C compile failed ({tag}):\n{}",
        String::from_utf8_lossy(&o.stderr)
    );
    Command::new(&exe)
        .env("ASAN_OPTIONS", "detect_leaks=0:abort_on_error=0")
        .output()
        .unwrap()
}

fn run_case(name: &str) {
    let src = read(&golden("run", name, "dov"));
    assert_eq!(
        dovc::fmt::format(&src).unwrap(),
        src,
        "run/{name}.dov is not `dovc fmt` clean"
    );
    let c = dovc::compile_to_c(&src).unwrap_or_else(|d| panic!("{}", d.render(name, &src)));
    let dir = dovc::scratch_dir().unwrap();
    std::fs::write(dir.join("dov_rt.h"), dovc::RUNTIME_H).unwrap();
    std::fs::write(dir.join("dov_rt.c"), dovc::RUNTIME_C).unwrap();
    std::fs::write(dir.join("prog.c"), c).unwrap();

    let mut builds = vec![("strict", build_and_run(&dir, "strict", &[]))];
    if sanitizers() {
        builds.push(("asan-ubsan", build_and_run(&dir, "san", SAN)));
    }
    let err_path = golden("run", name, "stderr");
    for (tag, o) in builds {
        let stdout = String::from_utf8_lossy(&o.stdout).to_string();
        let stderr = String::from_utf8_lossy(&o.stderr).to_string();
        if bless() && tag == "strict" {
            expect_file(&golden("run", name, "stdout"), &stdout);
            if o.status.code() != Some(0) {
                expect_file(&err_path, &stderr);
            }
            continue;
        }
        expect_file(&golden("run", name, "stdout"), &stdout);
        if err_path.exists() {
            assert_eq!(o.status.code(), Some(101), "{name} ({tag}) should panic");
            assert_eq!(stderr, read(&err_path), "{name} ({tag}) stderr");
        } else {
            assert!(
                o.status.success(),
                "{name} ({tag}) exited with {:?}: {stderr}",
                o.status.code()
            );
        }
    }
}

fn fail_case(name: &str) {
    let rel = format!("tests/golden/fail/{name}.dov");
    let o = Command::new(env!("CARGO_BIN_EXE_dovc"))
        .args(["check", &rel])
        .current_dir(root())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&o.stderr).to_string();
    assert_eq!(
        o.status.code(),
        Some(1),
        "{name} should be rejected: {stderr}"
    );
    // Shape every diagnostic must have: position, source line, caret, hint.
    let first = stderr.lines().next().unwrap_or("");
    assert!(
        first.starts_with(&format!("{rel}:")) && first.contains(": error: "),
        "{stderr}"
    );
    assert!(
        stderr.lines().any(|l| l.trim_end().ends_with('^')),
        "no caret: {stderr}"
    );
    assert!(stderr.contains(" = hint: "), "no hint: {stderr}");
    expect_file(&golden("fail", name, "err"), &stderr);
}

fn fmt_case(name: &str) {
    let src = read(&golden("fmt", name, "dov"));
    let out = dovc::fmt::format(&src).unwrap_or_else(|d| panic!("{}", d.render(name, &src)));
    expect_file(&golden("fmt", name, "fmt"), &out);
    assert_eq!(
        dovc::fmt::format(&out).unwrap(),
        out,
        "fmt is not idempotent on {name}"
    );
}

include!(concat!(env!("OUT_DIR"), "/golden_cases.rs"));

/// Guard against an empty suite (e.g. a broken build script).
#[test]
fn golden_suite_is_not_empty() {
    let n = |k: &str| {
        std::fs::read_dir(root().join("tests/golden").join(k))
            .unwrap()
            .filter(|e| {
                let n = e.as_ref().unwrap().file_name();
                let n = n.to_string_lossy();
                n.ends_with(".dov") && !n.starts_with("._")
            })
            .count()
    };
    assert!(n("run") >= 30 && n("fail") >= 30 && n("fmt") >= 5);
}
