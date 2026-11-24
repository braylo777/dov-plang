//! Build script: turn every golden case in `tests/golden/{run,fail,fmt}/*.dov`
//! into its own `#[test]` (used only by `tests/golden.rs`). It reads no
//! network and adds no dependencies; if the folder is missing (a packaged
//! crate), it writes an empty list.

use std::fmt::Write;
use std::path::Path;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut out = String::new();
    for kind in ["run", "fail", "fmt"] {
        let dir = root.join(kind);
        println!("cargo:rerun-if-changed={}", dir.display());
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter(|n| n.ends_with(".dov") && !n.starts_with("._"))
            .map(|n| n.trim_end_matches(".dov").to_string())
            .collect();
        names.sort();
        for n in names {
            let ident: String = n
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            let _ = writeln!(out, "#[test] fn {kind}_{ident}() {{ {kind}_case({n:?}); }}");
        }
    }
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap()).join("golden_cases.rs");
    std::fs::write(dest, out).unwrap();
}
