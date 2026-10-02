//! The view of a file a viewer plugin opened, in the terminal (design
//! §11.13, principle 7): the unit as an image through kitty, iTerm2 or
//! sixel where the terminal draws them, else in half-block cells; with
//! colors off, its text. The part of the bitmap in view is cut and scaled
//! here to the cells it covers, so zoom and pan work as in the graphical
//! editor.

use kalem_core::DocumentState;
use kalem_core::viewer::ViewerState;
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::Widget;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;

use crate::caps::Caps;

/// What the image drawn was made from: the bitmap, the cells it covers,
/// the part of it in view and the protocol.
type Key = (
    u64,
    usize,
    u8,
    Rect,
    [i32; 4],
    ratatui_image::picker::ProtocolType,
);

/// The image shown, kept between frames.
#[derive(Default)]
pub struct ViewerImage {
    shown: Option<(Key, Protocol)>,
}

impl std::fmt::Debug for ViewerImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewerImage")
            .field("shown", &self.shown.as_ref().map(|(k, _)| k.3))
            .finish()
    }
}

/// The picker for a viewer: the terminal's protocol, else half-blocks
/// when colors are on.
pub fn picker(images: Option<&Picker>, caps: &Caps) -> Option<Picker> {
    if let Some(p) = images {
        return Some(p.clone());
    }
    if caps.ascii || caps.no_color {
        return None;
    }
    Some(Picker::halfblocks())
}

/// Draws the viewer's document `doc` into `area`.
pub fn draw(
    doc: &mut DocumentState,
    image: &mut ViewerImage,
    images: Option<&Picker>,
    caps: &Caps,
    buf: &mut Buffer,
    area: Rect,
) {
    let Some(v) = doc.viewer.as_deref_mut() else {
        return;
    };
    // The bottom row says what is shown.
    let (pic, status) = if area.height > 2 {
        (
            Rect::new(area.x, area.y, area.width, area.height - 1),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        )
    } else {
        (area, Rect::default())
    };
    let (pic, info) = if v.info && pic.width > 40 {
        let w = 32.min(pic.width / 2);
        (
            Rect::new(pic.x, pic.y, pic.width - w - 1, pic.height),
            Some(Rect::new(pic.right() - w, pic.y, w, pic.height)),
        )
    } else {
        (pic, None)
    };
    match picker(images, caps) {
        Some(p) => draw_image(v, image, &p, buf, pic),
        None => text(&v.text(), buf, pic),
    }
    if let Some(r) = info {
        draw_info(v, caps, buf, r);
    }
    if status.height > 0 {
        let line = v.status();
        buf.set_stringn(
            status.x,
            status.y,
            line,
            status.width as usize,
            Style::default().add_modifier(Modifier::DIM),
        );
    }
}

fn text(s: &str, buf: &mut Buffer, area: Rect) {
    for (i, line) in s.lines().take(area.height as usize).enumerate() {
        buf.set_stringn(
            area.x,
            area.y + i as u16,
            line,
            area.width as usize,
            Style::default(),
        );
    }
}

fn draw_info(v: &ViewerState, caps: &Caps, buf: &mut Buffer, r: Rect) {
    let rule = crate::panels::panel_style(caps);
    for y in r.top()..r.bottom() {
        buf[(r.x - 1, y)]
            .set_symbol(if caps.ascii { "|" } else { "│" })
            .set_style(rule);
    }
    let mut y = r.y;
    for f in v.info_fields() {
        if y + 1 >= r.bottom() {
            break;
        }
        buf.set_stringn(
            r.x + 1,
            y,
            &f.label,
            r.width as usize - 1,
            Style::default().add_modifier(Modifier::DIM),
        );
        buf.set_stringn(
            r.x + 1,
            y + 1,
            &f.value,
            r.width as usize - 1,
            Style::default(),
        );
        y += 2;
    }
}

fn draw_image(
    v: &mut ViewerState,
    image: &mut ViewerImage,
    picker: &Picker,
    buf: &mut Buffer,
    area: Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let fs = picker.font_size();
    let (cw, ch) = (f32::from(fs.width.max(1)), f32::from(fs.height.max(1)));
    let (aw, ah) = (f32::from(area.width) * cw, f32::from(area.height) * ch);
    v.set_area(aw, ah);
    let p = v.placement();
    let (sx, sy, sw, sh) = p.visible(aw, ah);
    if sw <= 0.0 || sh <= 0.0 {
        return;
    }
    // The cells the visible part covers.
    let (x, y) = (p.x.max(0.0), p.y.max(0.0));
    let (w, h) = (sw * p.scale, sh * p.scale);
    let col = (x / cw).floor() as u16;
    let row = (y / ch).floor() as u16;
    let cols = ((w / cw).round() as u16).clamp(1, area.width - col.min(area.width - 1));
    let rows = ((h / ch).round() as u16).clamp(1, area.height - row.min(area.height - 1));
    let cells = Rect::new(area.x + col, area.y + row, cols, rows);
    let key = (
        v.generation(),
        v.unit,
        v.rotation,
        cells,
        [sx, sy, sw, sh].map(|n| n.round() as i32),
        picker.protocol_type(),
    );
    if image.shown.as_ref().is_none_or(|(k, _)| *k != key) {
        image.shown = make(v, picker, cells, (sx, sy, sw, sh), (cw, ch)).map(|p| (key, p));
    }
    if let Some((_, proto)) = &image.shown {
        ratatui_image::Image::new(proto).render(cells, buf);
    }
}

/// The part (`sx`, `sy`, `sw`, `sh`) of the bitmap, scaled to `cells`.
fn make(
    v: &mut ViewerState,
    picker: &Picker,
    cells: Rect,
    (sx, sy, sw, sh): (f32, f32, f32, f32),
    (cw, ch): (f32, f32),
) -> Option<Protocol> {
    let b = v.bitmap().ok()?;
    let img = image::RgbaImage::from_raw(b.width, b.height, b.rgba.to_vec())?;
    let (x, y) = (sx.floor() as u32, sy.floor() as u32);
    let w = (sw.ceil() as u32).clamp(1, b.width.saturating_sub(x).max(1));
    let h = (sh.ceil() as u32).clamp(1, b.height.saturating_sub(y).max(1));
    let part = image::imageops::crop_imm(&img, x, y, w, h).to_image();
    let (pw, ph) = (
        (f32::from(cells.width) * cw) as u32,
        (f32::from(cells.height) * ch) as u32,
    );
    let scaled = image::imageops::resize(
        &part,
        pw.max(1),
        ph.max(1),
        image::imageops::FilterType::Triangle,
    );
    picker
        .new_protocol(
            image::DynamicImage::ImageRgba8(scaled),
            Size::new(cells.width, cells.height),
            ratatui_image::Resize::Fit(None),
        )
        .ok()
}
