//! The API of Kalem's plugins (D6, D28, T3.1.3).
//!
//! The definition is WIT, in this crate's `wit/` folder: the one source
//! both sides are generated from. A plugin builds against the bindings
//! here (wit-bindgen); Kalem's host, `kalem-script`, generates its side
//! from the same files (wasmtime). `coverage.toml` lists every extension
//! point of the design document (§11.10 to §11.13) with the WIT interface
//! that defines it, or the task that will.
//!
//! A plugin implements a world's exports and names its type with the
//! world's export macro, then is built with `kalem plugin build`:
//!
//! ```ignore
//! use kalem_plugin::viewer::exports::kalem::plugin::viewer::{Guest, GuestDocument};
//!
//! struct Pages;
//! impl Guest for Pages { /* describe, detect, open */ }
//! kalem_plugin::viewer::export_viewer!(Pages);
//! ```

/// The `document-viewer` world (design §11.13, D54): a plugin opening
/// files that are not text, reading the one file the host hands it.
#[allow(
    missing_debug_implementations,
    unreachable_pub,
    clippy::all,
    rust_2018_idioms
)]
pub mod viewer {
    wit_bindgen::generate!({
        path: "wit",
        world: "document-viewer",
        pub_export_macro: true,
        export_macro_name: "export_viewer",
        default_bindings_module: "kalem_plugin::viewer",
    });
}
