//! The LaTeX parser never panics and its tree is the input.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let p = latex_syntax::parse(text);
    assert_eq!(p.syntax().text().to_string(), text);
});
