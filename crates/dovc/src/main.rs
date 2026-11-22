//! `dovc` command-line driver.

use std::path::PathBuf;
use std::process::{exit, Command};

const USAGE: &str = concat!(
    "dovc ",
    env!("CARGO_PKG_VERSION"),
    " — the .dov pLang compiler

usage:
  dovc check  <file.dov>              parse + type/immutability check
  dovc emit-c <file.dov>              print generated C11
  dovc build  <file.dov> [-o <out>]   compile to a native binary
  dovc run    <file.dov>              compile and run
  dovc fmt    [--check] <file.dov>... format files in place (--check: only report)"
);

fn read(file: &str) -> String {
    std::fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("dovc: cannot read {file}: {e}");
        exit(2)
    })
}

fn fail(msg: String) -> ! {
    eprintln!("{msg}");
    exit(1)
}

fn usage() -> ! {
    eprintln!("{USAGE}");
    exit(2)
}

/// `dovc fmt [--check] files...`: exit 0 if all files are (now) formatted,
/// 1 if `--check` found unformatted files or a file has a syntax error.
fn fmt(args: &[String]) -> ! {
    let check = args.iter().any(|a| a == "--check");
    let files: Vec<&String> = args.iter().filter(|a| *a != "--check").collect();
    if files.is_empty() {
        usage();
    }
    let mut bad = false;
    for file in files {
        let src = read(file);
        match dovc::fmt::format(&src) {
            Err(d) => {
                eprintln!("{}", d.render(file, &src));
                bad = true;
            }
            Ok(out) if out == src => {}
            Ok(_) if check => {
                eprintln!("dovc fmt: {file} is not formatted (run `dovc fmt {file}`)");
                bad = true;
            }
            Ok(out) => {
                if let Err(e) = std::fs::write(file, out) {
                    fail(format!("dovc: cannot write {file}: {e}"));
                }
                println!("formatted {file}");
            }
        }
    }
    exit(i32::from(bad))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or_else(|| usage());
    if cmd == "--version" || cmd == "-V" {
        println!("dovc {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if cmd == "--help" || cmd == "-h" || cmd == "help" {
        println!("{USAGE}");
        return;
    }
    if cmd == "fmt" {
        fmt(&args[1..]);
    }
    let file = args.get(1).map(String::as_str).unwrap_or_else(|| usage());
    let src = read(file);
    match cmd {
        "check" => {
            dovc::check_source(&src).unwrap_or_else(|d| fail(d.render(file, &src)));
            println!("ok: {file}");
        }
        "emit-c" => {
            let c = dovc::compile_to_c(&src).unwrap_or_else(|d| fail(d.render(file, &src)));
            print!("{c}");
        }
        "build" => {
            let out = match args.iter().position(|a| a == "-o") {
                Some(i) => PathBuf::from(
                    args.get(i + 1)
                        .unwrap_or_else(|| fail("dovc: -o needs a path".into())),
                ),
                None => PathBuf::from(std::path::Path::new(file).file_stem().unwrap_or_default()),
            };
            dovc::build(&src, file, &out).unwrap_or_else(|e| fail(e));
        }
        "run" => {
            let dir = dovc::scratch_dir().unwrap_or_else(|e| fail(e));
            let out = dir.join("a.out");
            dovc::build(&src, file, &out).unwrap_or_else(|e| fail(e));
            let status = Command::new(&out)
                .status()
                .unwrap_or_else(|e| fail(format!("dovc: cannot run: {e}")));
            exit(status.code().unwrap_or(1));
        }
        _ => usage(),
    }
}
