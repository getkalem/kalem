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
//!
//! A plugin adding commands implements [`kalem::Plugin`] and registers
//! them, with their handlers, when it is activated:
//!
//! ```ignore
//! use kalem_plugin::kalem::{self, Plugin, Scope};
//!
//! struct WordCount;
//! impl Plugin for WordCount {
//!     fn activate() -> Result<(), String> {
//!         kalem::command(kalem::spec("wordcount.show", "Word Count", Scope::all()), |_args| {
//!             Ok("null".into())
//!         })?;
//!         Ok(())
//!     }
//! }
//! kalem_plugin::export_plugin!(WordCount);
//! ```

/// The `document-viewer` world (design §11.13, D54): a plugin opening
/// files that are not text, reading the one file the host hands it. With
/// the feature `grid`, the `spreadsheet-viewer` world instead: the same,
/// and the `grid` interface of sheets of cells (T3.7.4); with `flow`, the
/// `flow-viewer` world, and the `flow` interface of documents of flowing
/// text (API 0.2.7); with both, the `full-viewer` world, which exports
/// both. Every one exports `annotations`.
#[allow(
    missing_debug_implementations,
    unreachable_pub,
    clippy::all,
    rust_2018_idioms
)]
pub mod viewer {
    #[cfg(not(any(feature = "grid", feature = "flow")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "document-viewer",
        pub_export_macro: true,
        export_macro_name: "export_viewer",
        default_bindings_module: "kalem_plugin::viewer",
    });
    #[cfg(all(feature = "grid", not(feature = "flow")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "spreadsheet-viewer",
        pub_export_macro: true,
        export_macro_name: "export_viewer",
        default_bindings_module: "kalem_plugin::viewer",
    });
    #[cfg(all(feature = "flow", not(feature = "grid")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "flow-viewer",
        pub_export_macro: true,
        export_macro_name: "export_viewer",
        default_bindings_module: "kalem_plugin::viewer",
    });
    #[cfg(all(feature = "grid", feature = "flow"))]
    wit_bindgen::generate!({
        path: "wit",
        world: "full-viewer",
        pub_export_macro: true,
        export_macro_name: "export_viewer",
        default_bindings_module: "kalem_plugin::viewer",
    });
}

/// The `extension` world (design §11.2 to §11.4): a plugin adding
/// commands and keys and hearing events, and which document has the focus
/// (API 0.2.18; [`focus`]). [`kalem`] is its Rust side in the
/// shape of §11.4. With the feature `layer`, the `extension-layer` world
/// instead: the same, and the `layer` interface of layers over a mode's
/// view (API 0.2.10; [`layer`]); with the feature `completer`, the
/// `extension-completer` world: those, and the `completer` interface
/// (API 0.2.11; [`completer`]); with the feature `layer-2`, the
/// `extension-layer-2` world: those, and the `layer-2` interface of layers
/// with lines of their own (API 0.2.13; [`layer`]); with the feature
/// `layer-3`, the `extension-layer-3` world: those, and the `layer-3`
/// interface, whose lines under lines may be typed into (API 0.2.15);
/// with the feature `formatter`, the `extension-formatter` world: those,
/// and the `formatter` interface of its languages' documents formatted
/// (API 0.2.16; [`formatter`]); with the feature `outline`, the
/// `extension-outline` world: those, and the `outline` interface of its
/// languages' documents' headings (API 0.2.18; [`outline`]); with the
/// feature `layer-4`, the `extension-layer-4` world: those, and the
/// `layer-4` interface, whose lines under lines may start blocks of a
/// file's outline (API 0.2.18); with the feature `layer-5`, the
/// `extension-layer-5` world: those, and the `layer-5` interface, whose
/// layers show text at places of a line (API 0.2.20); with the feature
/// `executor`, the `extension-executor` world: those, and the `executor`
/// interface, which runs its languages' source blocks for Babel (API
/// 0.2.21; [`executor`]).
#[allow(
    missing_debug_implementations,
    unreachable_pub,
    clippy::all,
    rust_2018_idioms
)]
pub mod extension {
    #[cfg(not(feature = "layer"))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(all(feature = "layer", not(feature = "completer")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-layer",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(all(feature = "completer", not(feature = "layer-2")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-completer",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(all(feature = "layer-2", not(feature = "layer-3")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-layer-2",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(all(feature = "layer-3", not(feature = "formatter")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-layer-3",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(all(feature = "formatter", not(feature = "outline")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-formatter",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(all(feature = "outline", not(feature = "layer-4")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-outline",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(all(feature = "layer-4", not(feature = "layer-5")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-layer-4",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(all(feature = "layer-5", not(feature = "executor")))]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-layer-5",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
    #[cfg(feature = "executor")]
    wit_bindgen::generate!({
        path: "wit",
        world: "extension-executor",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "kalem_plugin::extension",
    });
}

pub mod adapter;
pub mod canvas_view;
pub mod canvas_view_2;
pub mod clock;
pub mod completer;
pub mod decorations;
pub mod documents;
pub mod editor;
pub mod executor;
pub mod focus;
pub mod formatter;
pub mod fs;
pub mod graph_view;
pub mod kalem;
pub mod later;
pub mod layer;
pub mod net;
pub mod outline;
pub mod problems;
pub mod process;
pub mod settings;
pub mod state;
pub mod ui;
