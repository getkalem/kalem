//! LaTeX math rendered to images (design §9.2): a [`MathEngine`] trait,
//! the [RaTeX](https://crates.io/crates/ratex-layout) engine behind it
//! (decision D4), the pre-processing Org documents need, and a cache.

mod cache;
mod engine;
pub mod source;

pub use cache::Cache;
pub use engine::Ratex;

/// What to render.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    /// The formula without its delimiters (`x^2`, `\begin{align}…`), after
    /// [`source::prepare`].
    pub latex: String,
    /// Display style (`\[ \]`, `$$ $$`, environments) rather than inline.
    pub display: bool,
    /// The font size in pixels (one em).
    pub size: f32,
    /// Device pixels per pixel.
    pub scale: f32,
    /// The color of the formula, RGBA.
    pub color: [u8; 4],
}

/// A rendered formula: RGBA pixels (not premultiplied) at `scale`.
#[derive(Clone, PartialEq)]
pub struct Image {
    /// Width in device pixels.
    pub width: u32,
    /// Height in device pixels.
    pub height: u32,
    /// Rows of RGBA pixels.
    pub rgba: Vec<u8>,
    /// The baseline, in device pixels from the top.
    pub baseline: f32,
    /// Device pixels per pixel.
    pub scale: f32,
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("baseline", &self.baseline)
            .finish_non_exhaustive()
    }
}

/// A formula the engine cannot render: the editor shows its source in a
/// red frame instead (§9.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathError {
    /// The engine's message.
    pub message: String,
}

impl std::fmt::Display for MathError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MathError {}

/// A math engine: another one can replace RaTeX (D4).
pub trait MathEngine: Send + Sync {
    /// Renders a formula.
    fn render(&self, request: &Request) -> Result<Image, MathError>;
}
