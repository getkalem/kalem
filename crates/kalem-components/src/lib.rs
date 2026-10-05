//! The bundled plugins of `getkalem/plugins` built as WebAssembly
//! components (wasm_todo W4), from the sources Kalem pins in its
//! `Cargo.toml`: the same code as the native copies the `viewers`
//! feature compiles in, built for `wasm32-unknown-unknown` against this
//! checkout's plugin API, wrapped as components, refused when one imports
//! WASI. Built with the feature `build`; without it [`components`] is
//! empty. With the feature `viewers`, [`viewer`] gives one as a viewer
//! of the plugin host, as Kalem registers it.

/// A bundled plugin as a component.
#[derive(Debug, Clone, Copy)]
pub struct Component {
    /// The plugin's ID (`org.kalem.xlsx`).
    pub id: &'static str,
    /// Its manifest, `plugin.json`, as the plugin's sources have it.
    pub manifest: &'static str,
    /// The component's bytes.
    pub bytes: &'static [u8],
    /// The plugin's folder in the sources it was built from, for tests
    /// reading its corpus.
    pub source: &'static str,
}

include!(concat!(env!("OUT_DIR"), "/components.rs"));

/// The bundled plugins as components: none unless built with the feature
/// `build`.
pub fn components() -> &'static [Component] {
    COMPONENTS
}

#[cfg(feature = "viewers")]
mod viewers;
#[cfg(feature = "viewers")]
pub use viewers::{limits, opens, viewer};
