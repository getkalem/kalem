//! The RaTeX engine: parse, layout, an SVG of glyph outlines, pixels.

use ratex_layout::LayoutOptions;
use ratex_types::color::Color;
use ratex_types::math_style::MathStyle;

use crate::{Image, MathEngine, MathError, Request};

/// RaTeX: KaTeX-compatible LaTeX math in Rust, with the KaTeX fonts.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ratex;

/// Blank room around a formula, in ems, so that antialiased edges are not
/// cut.
const PAD: f64 = 0.05;

impl Ratex {
    /// The formula as an SVG document of glyph outlines, at `r.size`
    /// pixels to the em (`r.scale` is not applied).
    pub fn svg(&self, r: &Request) -> Result<crate::Svg, MathError> {
        let (svg, _, depth, em) = self.svg_text(r, f64::from(r.size))?;
        // The view box is in pixels at `em` to the em (the width and height
        // attributes say points).
        let view_box = svg
            .split("viewBox=\"")
            .nth(1)
            .and_then(|v| v.split('"').next())
            .map(|v| {
                v.split_whitespace()
                    .filter_map(|n| n.parse::<f64>().ok())
                    .collect::<Vec<_>>()
            })
            .filter(|v| v.len() == 4)
            .ok_or_else(|| MathError {
                message: "SVG without a view box".into(),
            })?;
        Ok(crate::Svg {
            svg,
            width: view_box[2] / em,
            height: view_box[3] / em,
            depth: depth + PAD,
        })
    }

    /// The SVG, the height and depth in ems (without padding), and the em
    /// in pixels.
    fn svg_text(&self, r: &Request, em: f64) -> Result<(String, f64, f64, f64), MathError> {
        let nodes = ratex_parser::parse(&r.latex).map_err(|e| MathError {
            message: format!("{e}"),
        })?;
        let [cr, cg, cb, ca] = r.color;
        let opts = LayoutOptions {
            style: if r.display {
                MathStyle::Display
            } else {
                MathStyle::Text
            },
            color: Color {
                r: f32::from(cr) / 255.,
                g: f32::from(cg) / 255.,
                b: f32::from(cb) / 255.,
                a: f32::from(ca) / 255.,
            },
            ..Default::default()
        };
        let layout = ratex_layout::layout(&nodes, &opts);
        let list = ratex_layout::to_display_list(&layout);
        let svg = ratex_svg::render_to_svg(
            &list,
            &ratex_svg::SvgOptions {
                font_size: em,
                padding: PAD * em,
                embed_glyphs: true,
                ..Default::default()
            },
        );
        Ok((svg, list.height, list.depth, em))
    }
}

impl MathEngine for Ratex {
    fn render(&self, r: &Request) -> Result<Image, MathError> {
        let em = f64::from(r.size) * f64::from(r.scale);
        let (svg, height, _, _) = self.svg_text(r, em)?;
        let tree =
            resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).map_err(|e| {
                MathError {
                    message: format!("SVG: {e}"),
                }
            })?;
        let size = tree.size();
        let (w, h) = (
            size.width().ceil().max(1.) as u32,
            size.height().ceil().max(1.) as u32,
        );
        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| MathError {
            message: "The formula is too large".into(),
        })?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        // tiny-skia keeps premultiplied pixels.
        let mut rgba = pixmap.take();
        for px in rgba.as_chunks_mut::<4>().0 {
            let a = u16::from(px[3]);
            if a > 0 && a < 255 {
                for c in &mut px[..3] {
                    *c = ((u16::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Ok(Image {
            width: w,
            height: h,
            rgba,
            baseline: ((PAD + height) * em) as f32,
            scale: r.scale,
        })
    }
}
