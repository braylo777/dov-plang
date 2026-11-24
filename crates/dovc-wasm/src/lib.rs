//! A tiny, dependency-free WebAssembly interface to `dovc` (no wasm-bindgen).
//!
//! JavaScript protocol: `dov_alloc(n)` returns a pointer to `n` writable bytes; write UTF-8 source
//! there; call `dov_run(ptr, n, mode)`; read `dov_out_len()` bytes at `dov_out_ptr()`.
//! Modes: 0 = check and emit C, 1 = format. The result is 0 on success, 1 on a compile error
//! (the output is then the rendered diagnostic).

use std::cell::RefCell;

thread_local! {
    static IN: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Run `mode` on `src`. Returns (ok, text). Also used by the native tests.
pub fn run(src: &str, mode: u32) -> (bool, String) {
    match mode {
        1 => match dovc::fmt::format(src) {
            Ok(s) => (true, s),
            Err(d) => (false, d.render("playground.dov", src)),
        },
        _ => match dovc::compile_file(src, "playground.dov") {
            Ok(c) => (true, c),
            Err(e) => (false, e),
        },
    }
}

#[no_mangle]
pub extern "C" fn dov_alloc(n: usize) -> *mut u8 {
    IN.with(|b| {
        let mut b = b.borrow_mut();
        b.clear();
        b.resize(n, 0);
        b.as_mut_ptr()
    })
}

/// # Safety
/// `ptr` must be the pointer from the last `dov_alloc(n)` call, with `n` bytes written.
#[no_mangle]
pub unsafe extern "C" fn dov_run(ptr: *const u8, n: usize, mode: u32) -> u32 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, n) }.to_vec();
    let src = String::from_utf8_lossy(&bytes).into_owned();
    let (ok, text) = run(&src, mode);
    OUT.with(|o| *o.borrow_mut() = text.into_bytes());
    u32::from(!ok)
}

#[no_mangle]
pub extern "C" fn dov_out_ptr() -> *const u8 {
    OUT.with(|o| o.borrow().as_ptr())
}

#[no_mangle]
pub extern "C" fn dov_out_len() -> usize {
    OUT.with(|o| o.borrow().len())
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn emits_c_and_reports_errors() {
        let (ok, c) = run("fn main() { print(1); }", 0);
        assert!(ok && c.contains("dov_fn_main"));
        let (ok, e) = run("fn main() { let x = 1; x = 2; }", 0);
        assert!(!ok && e.starts_with("playground.dov:1:24: error: cannot assign"));
        let (ok, f) = run("fn main(){}", 1);
        assert!(ok && f == "fn main() {}\n");
    }
}
