//! Formulas as images (design §9.2): rendered by `org-math` the first time
//! they are drawn, kept as gpui images so that the GPU keeps them too.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Hsla, Pixels, RenderImage, Size, px, size};

/// A formula ready to draw.
#[derive(Debug, Clone)]
pub enum Formula {
    /// The rendered formula: its image, size and baseline from the top.
    Image {
        /// The image, at the window's scale.
        image: Arc<RenderImage>,
        /// Its size in pixels.
        size: Size<Pixels>,
        /// Its baseline from the top.
        ascent: Pixels,
    },
    /// The engine's message: the source is drawn in a red frame (§9.2).
    Error(String),
}

type Key = (String, bool, u32, u32, [u8; 4]);

/// Rendered formulas, shared by the windows.
pub struct Formulas {
    cache: org_math::Cache,
    images: RefCell<HashMap<Key, Arc<RenderImage>>>,
}

impl std::fmt::Debug for Formulas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Formulas").finish_non_exhaustive()
    }
}

impl Default for Formulas {
    fn default() -> Formulas {
        Formulas {
            cache: org_math::Cache::new(Box::new(org_math::Ratex), 4096),
            images: RefCell::default(),
        }
    }
}

fn rgba(c: Hsla) -> [u8; 4] {
    let r = c.to_rgb();
    let b = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
    [b(r.r), b(r.g), b(r.b), b(r.a)]
}

/// An image of `org-math` as gpui takes it (BGRA).
fn render_image(img: &org_math::Image) -> Option<RenderImage> {
    let mut bgra = img.rgba.clone();
    for p in bgra.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    let buf = image::RgbaImage::from_raw(img.width, img.height, bgra)?;
    Some(RenderImage::new(vec![image::Frame::new(buf)]))
}

impl Formulas {
    /// The formula of a LaTeX fragment (`$x$`, `\[…\]`, an environment)
    /// at font size `font`, `scale` device pixels per pixel, in `color`,
    /// after the document's definitions `macros`.
    pub fn get(
        &self,
        fragment: &str,
        macros: &str,
        font: Pixels,
        scale: f32,
        color: Hsla,
    ) -> Formula {
        let (body, display) = org_math::source::body(fragment);
        let request = org_math::Request {
            latex: org_math::source::prepare(body, macros),
            display,
            size: f32::from(font),
            scale,
            color: rgba(color),
        };
        let rendered = self.cache.get(&request);
        let img = match &*rendered {
            Ok(img) => img,
            Err(e) => return Formula::Error(e.message.clone()),
        };
        let key = (
            request.latex,
            display,
            request.size.to_bits(),
            scale.to_bits(),
            request.color,
        );
        let mut images = self.images.borrow_mut();
        if images.len() > 4096 {
            images.clear();
        }
        let image = match images.get(&key) {
            Some(i) => i.clone(),
            None => {
                let Some(i) = render_image(img).map(Arc::new) else {
                    return Formula::Error("The formula is too large".into());
                };
                images.insert(key, i.clone());
                i
            }
        };
        Formula::Image {
            image,
            size: size(px(img.width as f32 / scale), px(img.height as f32 / scale)),
            ascent: px(img.baseline / scale),
        }
    }
}

/// The `\newcommand`s of a document's `#+LATEX_HEADER` lines.
pub fn macros(parse: &org_syntax::Parse) -> String {
    let headers: Vec<String> = parse
        .keywords()
        .into_iter()
        .filter(|(k, _)| {
            k.eq_ignore_ascii_case("LATEX_HEADER") || k.eq_ignore_ascii_case("LATEX_HEADER_EXTRA")
        })
        .map(|(_, v)| v)
        .collect();
    org_math::source::macros(&headers)
}
