//! dovc — the .dov pLang compiler.
//!
//! Pipeline: load (prelude + file + imports) -> lex -> parse -> check (types, generics,
//! immutability) -> C11 codegen (monomorphized) -> system `cc`.

pub mod ast;
pub mod check;
pub mod codegen;
pub mod diag;
pub mod fmt;
pub mod lexer;
pub mod parser;

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

pub const RUNTIME_H: &str = include_str!("../../../runtime/dov_rt.h");
pub const RUNTIME_C: &str = include_str!("../../../runtime/dov_rt.c");
/// `Option<T>`, `Result<T, E>` and helpers, loaded before every program (file 0).
pub const PRELUDE: &str = include_str!("prelude.dov");

/// Normalize `a/b/../c` to `a/c` without touching the file system.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir
                if matches!(out.components().next_back(), Some(Component::Normal(_))) =>
            {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

struct Loader {
    sources: diag::Sources,
    seen: HashSet<PathBuf>,
}

impl Loader {
    /// Parse file `id` (already in `sources`) and, recursively, the files it imports.
    /// Imported items come before the importing file's items.
    fn file(&mut self, id: u16, items: &mut Vec<ast::Item>) -> diag::Result<()> {
        let (name, src) = self.sources.files[id as usize].clone();
        let prog = parser::parse(lexer::lex_file(&src, id)?)?;
        let dir = Path::new(&name)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        for item in prog.items {
            let ast::Item::Import(rel, pos) = &item else {
                items.push(item);
                continue;
            };
            let path = normalize(&dir.join(rel));
            let text = std::fs::read_to_string(&path).map_err(|e| {
                diag::Diag::new(
                    *pos,
                    format!("cannot import \"{rel}\": {e}"),
                    format!(
                        "the path is relative to this file's folder; looked for `{}`",
                        path.display()
                    ),
                )
            })?;
            let key = std::fs::canonicalize(&path).unwrap_or(path.clone());
            if !self.seen.insert(key) {
                continue; // each file is loaded once
            }
            let n = u16::try_from(self.sources.files.len()).map_err(|_| {
                diag::Diag::new(
                    *pos,
                    "too many imported files",
                    "a program may have at most 65535 files",
                )
            })?;
            self.sources
                .files
                .push((path.to_string_lossy().into_owned(), text));
            self.file(n, items)?;
        }
        Ok(())
    }
}

/// Load the prelude, the main source and everything it imports into one program.
/// `name` is the main file's display name; imports resolve relative to its folder.
pub fn load(src: &str, name: &str) -> (diag::Sources, diag::Result<ast::Program>) {
    let mut l = Loader {
        sources: diag::Sources {
            files: vec![
                ("<prelude>".into(), PRELUDE.into()),
                (name.into(), src.into()),
            ],
        },
        seen: HashSet::new(),
    };
    if let Ok(c) = std::fs::canonicalize(name) {
        l.seen.insert(c);
    }
    let mut items = Vec::new();
    let r = l.file(0, &mut items).and_then(|_| l.file(1, &mut items));
    (l.sources, r.map(|_| ast::Program { items }))
}

/// Load and check. Returns the sources (for rendering) and the checked program.
pub fn check_named(src: &str, name: &str) -> (diag::Sources, diag::Result<check::Checked>) {
    let (sources, prog) = load(src, name);
    let names: Vec<String> = sources.files.iter().map(|(n, _)| n.clone()).collect();
    let r = prog.and_then(|p| check::check_files(&p, &names));
    (sources, r)
}

/// Front end only: parse and check (imports resolve relative to the current folder).
pub fn check_source(src: &str) -> diag::Result<()> {
    check_named(src, "<input>").1.map(|_| ())
}

/// Full front end + C generation.
pub fn compile_to_c(src: &str) -> diag::Result<String> {
    let ck = check_named(src, "<input>").1?;
    Ok(codegen::generate(&ck))
}

/// Compile the file `file` (text `src`) to C; errors are fully rendered.
pub fn compile_file(src: &str, file: &str) -> Result<String, String> {
    let (sources, r) = check_named(src, file);
    r.map(|ck| codegen::generate(&ck))
        .map_err(|d| sources.render(&d))
}

/// Check the file `file` (text `src`); errors are fully rendered.
pub fn check_file(src: &str, file: &str) -> Result<(), String> {
    let (sources, r) = check_named(src, file);
    r.map(|_| ()).map_err(|d| sources.render(&d))
}

/// Compile `.dov` source to a native binary at `out` with the system C compiler.
/// Errors are rendered with `Diag::render` (position, source line, caret, hint).
pub fn build(src: &str, file: &str, out: &Path) -> Result<(), String> {
    let c = compile_file(src, file)?;
    build_c(&c, out, &[])
}

/// Compile generated C plus the runtime to `out` (`extra` = more `cc` flags).
pub fn build_c(c: &str, out: &Path, extra: &[&str]) -> Result<(), String> {
    let dir = scratch_dir()?;
    let w = |name: &str, body: &str| -> Result<PathBuf, String> {
        let p = dir.join(name);
        std::fs::write(&p, body).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
        Ok(p)
    };
    w("dov_rt.h", RUNTIME_H)?;
    let rt = w("dov_rt.c", RUNTIME_C)?;
    let prog = w("prog.c", c)?;
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    let res = Command::new(&cc)
        .args(["-std=c11", "-O2", "-w"])
        .args(extra)
        .arg("-o")
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let k = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("dovc-{}-{nanos}-{k}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::normalize;
    use std::path::Path;

    #[test]
    fn normalize_paths() {
        assert_eq!(normalize(Path::new("a/b/../c.dov")), Path::new("a/c.dov"));
        assert_eq!(normalize(Path::new("./x.dov")), Path::new("x.dov"));
        assert_eq!(normalize(Path::new("../x.dov")), Path::new("../x.dov"));
    }
}
