# xkbcommon 0.8.0, opened at run time

This is [xkbcommon-rs](https://github.com/rust-x-bindings/xkbcommon-rs)
0.8.0 (crates.io, commit 02e6d6cd1347e76025451a3462933d543bc7f811), the
bindings gpui's Linux platform uses for the keyboard, with one change:
libxkbcommon and libxkbcommon-x11 are opened with `dlopen` the first time
one of their functions is called, instead of being linked
(`src/dl.rs`; the `extern "C"` blocks of `src/xkb/ffi.rs` and
`src/xkb/x11/ffi.rs` became `crate::dl::functions!` blocks with the same
signatures). The workspace's `Cargo.toml` puts it in place of the
crates.io release with `[patch.crates-io]`.

Why: a linked library has to be there for the program to start at all, so
Kalem's Linux binary stopped at once with "error while loading shared
libraries: libxkbcommon-x11.so.0" on a server, a container or a desktop
without that package, even for `kalem tui` and the command-line tools,
which never touch the keyboard library. Opened at run time, the libraries
are needed only by the graphical editor, which checks for them before it
starts (`kalem_ui::missing_libraries`). libxcb, which libxkbcommon-x11
uses, is opened the same way through x11rb's `dl-libxcb` feature
(`crates/kalem-ui/Cargo.toml`).

Left out: `xkb_state_mod_names_are_active` and
`xkb_state_mod_index_are_active` of `xkb::ffi`, which are C-variadic (a
Rust function cannot forward their arguments) and which the safe API does
not call. A function the installed libxkbcommon lacks (an older release)
panics when called, as it would have failed to link before.

`src/lib.rs` allows every warning, as Cargo does for a crates.io
dependency: this copy is a path dependency, whose warnings (four
`mismatched_lifetime_syntaxes` of 0.8.0 under Rust 1.96) CI's
`-D warnings` would make errors.

To move to a newer xkbcommon when gpui does: copy the release's `src`,
make the same change to its `extern "C"` blocks, and set the version
here.

The licence is the original's (`LICENSE`, MIT).
