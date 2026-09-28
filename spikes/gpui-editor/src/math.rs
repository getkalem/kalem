//! Formula images for inline LaTeX fragments.
//!
//! The pipeline: RaTeX (the engine chosen in T0.7) lays the formula out and
//! writes an SVG with glyph outlines; resvg rasterizes it at the display
//! scale; gpui paints it as an inline image.

use std::sync::{Arc, LazyLock};

use gpui::{Pixels, RenderImage, Size, px, size};
use smallvec::SmallVec;

pub struct MathImage {
    pub image: Arc<RenderImage>,
    pub size: Size<Pixels>,
    /// Height above the text baseline.
    pub ascent: Pixels,
}

/// RaTeX writes glyphs as outlines, so no fonts are needed here.
static OPTIONS: LazyLock<resvg::usvg::Options<'static>> = LazyLock::new(resvg::usvg::Options::default);

/// The body of a fragment: `$x$`, `$$x$$`, `\(x\)` or `\[x\]`.
pub fn body(src: &str) -> Option<&str> {
    for (a, b) in [("$$", "$$"), ("\\(", "\\)"), ("\\[", "\\]"), ("$", "$")] {
        if src.len() >= a.len() + b.len()
            && let Some(inner) = src.strip_prefix(a).and_then(|s| s.strip_suffix(b))
        {
            return Some(inner);
        }
    }
    None
}

/// Rasterizes `svg` to `width` × `height` pixels at `scale` device pixels
/// per pixel.
pub fn rasterize(svg: &str, width: f32, height: f32, scale: f32) -> Option<Arc<RenderImage>> {
    let tree = resvg::usvg::Tree::from_str(svg, &OPTIONS).ok()?;
    let (w, h) = ((width * scale).ceil() as u32, (height * scale).ceil() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1))?;
    let k = width * scale / tree.size().width();
    resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pixmap.as_mut());
    // gpui images are BGRA with straight alpha.
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        data.extend_from_slice(&[c.blue(), c.green(), c.red(), c.alpha()]);
    }
    let buffer = image::RgbaImage::from_raw(w.max(1), h.max(1), data)?;
    Some(Arc::new(RenderImage::new(SmallVec::from_elem(image::Frame::new(buffer), 1))))
}

/// Renders the fragment `src` for text of `font_size` pixels.
pub fn render(src: &str, font_size: f32, scale: f32) -> Option<MathImage> {
    let body = body(src)?.trim();
    if body.is_empty() {
        return None;
    }
    let display = src.starts_with("$$") || src.starts_with("\\[");
    let nodes = ratex_parser::parse(body).ok()?;
    let style = if display { ratex_types::math_style::MathStyle::Display } else { ratex_types::math_style::MathStyle::Text };
    let opts = ratex_layout::LayoutOptions { style, ..Default::default() };
    let list = ratex_layout::to_display_list(&ratex_layout::layout(&nodes, &opts));
    // KaTeX sets math slightly larger than the surrounding text.
    let em = font_size * 1.1;
    let svg_opts = ratex_svg::SvgOptions { font_size: em as f64, padding: 0.0, embed_glyphs: true, ..Default::default() };
    let svg = ratex_svg::render_to_svg(&list, &svg_opts);
    let (w, h) = ((list.width as f32 * em).max(1.), ((list.height + list.depth) as f32 * em).max(1.));
    let image = rasterize(&svg, w, h, scale)?;
    Some(MathImage { image, size: size(px(w), px(h)), ascent: px(list.height as f32 * em) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_inline_and_display_formulas() {
        for (src, display) in [("$x^2 + y^2$", false), ("\\[\\sum_{i=1}^n i\\]", true), ("$\\frac{a}{b}$", false)] {
            let t = std::time::Instant::now();
            let m = render(src, 16., 2.).expect(src);
            let took = t.elapsed();
            let (w, h, a) = (f32::from(m.size.width), f32::from(m.size.height), f32::from(m.ascent));
            assert!(w > 10. && h > 10. && a > 0. && a < h, "{src}: {w}x{h} ascent {a}");
            let px = m.image.size(0);
            assert_eq!(px.width.0, (w * 2.).ceil() as i32);
            if display {
                assert!(h > 30., "display sums are tall: {h}");
            }
            if let Ok(dir) = std::env::var("KALEM_DUMP") {
                let bytes = m.image.as_bytes(0).unwrap();
                let rgba: Vec<u8> = bytes.chunks(4).flat_map(|c| [c[2], c[1], c[0], c[3]]).collect();
                let img = image::RgbaImage::from_raw(px.width.0 as u32, px.height.0 as u32, rgba).unwrap();
                let name = format!("{dir}/formula-{}.raw", w as u32);
                std::fs::write(&name, img.as_raw()).unwrap();
                println!("{src}: {w}x{h} ascent {a}, {}x{} px, {took:?} -> {name}", px.width.0, px.height.0);
            }
        }
    }
}
