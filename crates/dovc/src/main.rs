//! `dovc` command-line driver.

use std::path::PathBuf;
use std::process::{exit, Command};

const USAGE: &str = "dovc 0.1 — the .dov pLang compiler

usage:
  dovc check  <file.dov>              parse + type/immutability check
  dovc emit-c <file.dov>              print generated C11
  dovc build  <file.dov> [-o <out>]   compile to a native binary
  dovc run    <file.dov>              compile and run";

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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cmd, file) = match (args.first(), args.get(1)) {
        (Some(c), Some(f)) => (c.as_str(), f.as_str()),
        (Some(c), None) if c == "--version" || c == "-V" => {
            println!("dovc {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        _ => {
            eprintln!("{USAGE}");
            exit(2)
        }
    };
    let src = read(file);
    match cmd {
        "check" => {
            dovc::check_source(&src).unwrap_or_else(|d| fail(d.render(file)));
            println!("ok: {file}");
        }
        "emit-c" => {
            let c = dovc::compile_to_c(&src).unwrap_or_else(|d| fail(d.render(file)));
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
        _ => {
            eprintln!("{USAGE}");
            exit(2)
        }
    }
}
