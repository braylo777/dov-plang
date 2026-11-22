//! dovc — the .dov pLang compiler.
//!
//! Pipeline: lex -> parse -> check (types + immutability) -> C11 codegen -> system `cc`.

pub mod ast;
pub mod check;
pub mod codegen;
pub mod diag;
pub mod lexer;
pub mod parser;

use std::path::{Path, PathBuf};
use std::process::Command;

pub const RUNTIME_H: &str = include_str!("../../../runtime/dov_rt.h");
pub const RUNTIME_C: &str = include_str!("../../../runtime/dov_rt.c");

/// Front end only: parse and check.
pub fn check_source(src: &str) -> diag::Result<()> {
    let prog = parser::parse(lexer::lex(src)?)?;
    check::check(&prog)?;
    Ok(())
}

/// Full front end + C generation.
pub fn compile_to_c(src: &str) -> diag::Result<String> {
    let prog = parser::parse(lexer::lex(src)?)?;
    let types = check::check(&prog)?;
    Ok(codegen::generate(&prog, &types))
}

/// Compile `.dov` source to a native binary at `out` with the system C compiler.
/// Errors are rendered as `file:line:col: error: msg`.
pub fn build(src: &str, file: &str, out: &Path) -> Result<(), String> {
    let c = compile_to_c(src).map_err(|d| d.render(file))?;
    let dir = scratch_dir()?;
    let w = |name: &str, body: &str| -> Result<PathBuf, String> {
        let p = dir.join(name);
        std::fs::write(&p, body).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
        Ok(p)
    };
    w("dov_rt.h", RUNTIME_H)?;
    let rt = w("dov_rt.c", RUNTIME_C)?;
    let prog = w("prog.c", &c)?;
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let res = Command::new(&cc)
        .args(["-std=c11", "-O2", "-w", "-o"])
        .arg(out)
        .arg(&prog)
        .arg(&rt)
        .output()
        .map_err(|e| format!("cannot run C compiler `{cc}`: {e}"))?;
    if !res.status.success() {
        return Err(format!(
            "internal error: C compiler failed (generated C kept in {})\n{}",
            dir.display(),
            String::from_utf8_lossy(&res.stderr)
        ));
    }
    Ok(())
}

/// A fresh per-build directory under the OS temp dir.
pub fn scratch_dir() -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("dovc-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}
