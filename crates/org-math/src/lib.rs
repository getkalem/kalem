//! LaTeX math rendered to images (design §9.2): a [`MathEngine`] trait,
//! the [RaTeX](https://crates.io/crates/ratex-layout) engine behind it
//! (decision D4), the pre-processing Org documents need, and a cache.

mod cache;
mod engine;
pub mod source;

pub use cache::Cache;
pub use engine::Ratex;

/// Whether the engine reads formula `latex` (prepared, see
/// [`source::prepare`]), without laying it out: its error if not.
pub fn check(latex: &str) -> Result<(), MathError> {
    ratex_parser::parse(latex)
        .map(|_| ())
        .map_err(|e| MathError {
            message: format!("{e}"),
        })
}

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

/// A formula as an SVG document ([`Ratex::svg`]), with its size in ems.
#[derive(Debug, Clone, PartialEq)]
pub struct Svg {
    /// The SVG document.
    pub svg: String,
    /// Width, in ems.
    pub width: f64,
    /// Height of the whole image, in ems.
    pub height: f64,
    /// How far the image reaches below the baseline, in ems.
    pub depth: f64,
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

/// How many top-level nodes the engine reads in formula `latex`: a
/// definition makes none, so definitions before a formula must leave the
/// count as the formula alone has it.
pub fn top_level_nodes(latex: &str) -> Result<usize, MathError> {
    ratex_parser::parse(latex)
        .map(|n| n.len())
        .map_err(|e| MathError {
            message: format!("{e}"),
        })
}
