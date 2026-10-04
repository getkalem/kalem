//! Linker settings of the `kalem` binary.

fn main() {
    // A debug build's binary has more than 16 MB of DWARF unwind
    // information, more than macOS's linker encodes in its compact unwind
    // table, and it warns on every link. Without the table, unwinding (a
    // panic caught) reads the DWARF information instead, which is what the
    // linker would fall back to anyway; release builds are small enough.
    let macos = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos");
    let debug = std::env::var("PROFILE").as_deref() == Ok("debug");
    if macos && debug {
        println!("cargo:rustc-link-arg-bin=kalem=-Wl,-no_compact_unwind");
    }
}
