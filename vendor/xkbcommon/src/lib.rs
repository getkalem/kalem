// Kalem: no warnings, as when Cargo builds the crates.io release (it caps
// a registry dependency's lints); a path dependency's warnings would
// fail CI's `-D warnings` (four `mismatched_lifetime_syntaxes` of 0.8.0).
#![allow(warnings)]

extern crate libc;
#[cfg(feature = "wayland")]
extern crate memmap2;

mod dl;
pub mod xkb;
