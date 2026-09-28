//! Any input parses without panicking, round-trips, and can be checked.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        let parse = org_syntax::parse(text);
        assert_eq!(parse.syntax().to_string(), text);
        let _ = parse.diagnostics();
    }
});
