//! The bundled plugins of `getkalem/plugins` as the WebAssembly components
//! they released (wasm_todo W9): `components.toml` pins each by its
//! release's tag and SHA-256, and with the feature `embed` the build
//! downloads them, checks them and keeps them compressed in the binary;
//! without it [`components`] is empty. Kalem compiles none of their
//! source. With the feature `viewers`, [`viewer`] gives one as a viewer of
//! the plugin host, as Kalem registers it.

use std::io::Read;
use std::sync::OnceLock;

/// A bundled plugin as a component.
pub struct Component {
    /// The plugin's ID (`org.kalem.xlsx`).
    pub id: &'static str,
    /// Its manifest, `plugin.json`, as its release has it.
    pub manifest: &'static str,
    /// The component, compressed (deflate).
    deflated: &'static [u8],
    /// The component, once asked for.
    wasm: OnceLock<Vec<u8>>,
}

impl std::fmt::Debug for Component {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Component")
            .field("id", &self.id)
            .field("deflated", &self.deflated.len())
            .finish_non_exhaustive()
    }
}

impl Component {
    /// The component's bytes, inflated on the first call (a few
    /// megabytes, kept from then on).
    pub fn wasm(&self) -> &[u8] {
        self.wasm.get_or_init(|| {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(self.deflated)
                .read_to_end(&mut out)
                .expect("a component the build compressed");
            out
        })
    }
}

include!(concat!(env!("OUT_DIR"), "/components.rs"));

/// The bundled plugins as components: none unless built with the feature
/// `embed`.
pub fn components() -> &'static [Component] {
    &COMPONENTS
}

#[cfg(feature = "viewers")]
mod viewers;
#[cfg(feature = "viewers")]
pub use viewers::{limits, opens, viewer};
