//! Documents drawn as a canvas of shapes where their plugin places them
//! (plugin API 0.2.16): Logseq's whiteboards, Obsidian's canvases, a
//! diagram. The plugin gives each shape's place and size, its kind, text
//! and colors, and the connectors between shapes; the editors draw them as
//! placed, and [`CanvasView`] keeps the window's view of them (where it
//! looks, how near, the shape chosen). Opening a shape that leads
//! somewhere runs the canvas's command with `{"shape": target}`.

use std::path::PathBuf;
use std::sync::Arc;

/// A color a shape or a connector is drawn in: one of the theme's names,
/// or one given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Color {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Gray,
    /// `#rrggbb`.
    Rgb(u8, u8, u8),
}

impl Color {
    /// A color's name (`red`, `orange`, `yellow`, `green`, `cyan`, `blue`,
    /// `purple`, `gray`) or `#rrggbb` (`#rgb`); none for anything else,
    /// the theme's own.
    pub fn parse(s: &str) -> Option<Color> {
        let s = s.trim();
        if let Some(hex) = s.strip_prefix('#') {
            let v = |h: &str| u8::from_str_radix(h, 16).ok();
            return match hex.len() {
                6 => Some(Color::Rgb(v(&hex[0..2])?, v(&hex[2..4])?, v(&hex[4..6])?)),
                3 => {
                    let d = |i: usize| v(&hex[i..=i]).map(|x| x * 17);
                    Some(Color::Rgb(d(0)?, d(1)?, d(2)?))
                }
                _ => None,
            };
        }
        Some(match s.to_ascii_lowercase().as_str() {
            "red" => Color::Red,
            "orange" => Color::Orange,
            "yellow" => Color::Yellow,
            "green" => Color::Green,
            "cyan" => Color::Cyan,
            "blue" => Color::Blue,
            "purple" => Color::Purple,
            "gray" | "grey" => Color::Gray,
            _ => return None,
        })
    }

    /// Its red, green and blue: a name's, soft and readable on light and
    /// dark backgrounds.
    pub fn rgb(self) -> (u8, u8, u8) {
        match self {
            Color::Red => (0xE1, 0x57, 0x59),
            Color::Orange => (0xF2, 0x8E, 0x2B),
            Color::Yellow => (0xED, 0xC9, 0x48),
            Color::Green => (0x59, 0xA1, 0x4F),
            Color::Cyan => (0x76, 0xB7, 0xB2),
            Color::Blue => (0x4E, 0x79, 0xA7),
            Color::Purple => (0xB0, 0x7A, 0xA1),
            Color::Gray => (0x9C, 0x9C, 0x9C),
            Color::Rgb(r, g, b) => (r, g, b),
        }
    }
}

/// What a shape is drawn as.
#[derive(Debug, Clone, PartialEq)]
pub enum ShapeKind {
    /// A rectangle, its corners rounded by so many units.
    Box(f32),
    /// An ellipse in its bounds.
    Ellipse,
    /// Text alone, with no outline.
    Text,
    /// A frame around other shapes, its text at its top.
    Group,
    /// A picture: an image file, drawn in its bounds.
    Image(PathBuf),
    /// A line through points, from the shape's corner (a pen's stroke).
    Stroke(Vec<[f32; 2]>),
}

/// A shape where its plugin placed it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    /// The plugin's name for it.
    pub id: String,
    /// What it is drawn as.
    pub kind: ShapeKind,
    /// Its top left corner, its width and its height.
    pub rect: [f32; 4],
    /// What is written in it, wrapped to its width.
    pub text: String,
    /// Its fill and its line; none for the theme's (no fill).
    pub fill: Option<Color>,
    pub stroke: Option<Color>,
    /// Its line dashed.
    pub dashed: bool,
    /// Where opening it leads, handed to the canvas's command.
    pub target: Option<String>,
    /// The size its text is written in, in the canvas's units (API
    /// 0.2.19, `canvas-view-2`); none for the window's own.
    pub text_size: Option<f32>,
}

impl Shape {
    /// The size its text is drawn at in the window at zoom `zoom`, in
    /// pixels: its own size, else the window's 16, times the zoom, and no
    /// more than three times that size however far the canvas is zoomed
    /// in. The terminal writes a character a cell whatever the size.
    pub fn text_px(&self, zoom: f32) -> f32 {
        self.text_size.unwrap_or(16.0) * zoom.min(3.0)
    }

    /// Its middle.
    pub fn center(&self) -> [f32; 2] {
        let [x, y, w, h] = self.rect;
        [x + w / 2.0, y + h / 2.0]
    }

    /// Whether point `p` is in its bounds.
    pub fn contains(&self, p: [f32; 2]) -> bool {
        let [x, y, w, h] = self.rect;
        p[0] >= x && p[0] <= x + w && p[1] >= y && p[1] <= y + h
    }

    /// Where a line from its middle towards `toward` leaves it.
    pub fn exit(&self, toward: [f32; 2]) -> [f32; 2] {
        let c = self.center();
        let (dx, dy) = (toward[0] - c[0], toward[1] - c[1]);
        let (hw, hh) = (self.rect[2] / 2.0, self.rect[3] / 2.0);
        if dx.abs() < 1e-6 && dy.abs() < 1e-6 {
            return c;
        }
        let t = match self.kind {
            ShapeKind::Ellipse if hw > 0.0 && hh > 0.0 => {
                1.0 / ((dx / hw).powi(2) + (dy / hh).powi(2)).sqrt()
            }
            _ => {
                let tx = if dx.abs() > 1e-6 {
                    hw / dx.abs()
                } else {
                    f32::MAX
                };
                let ty = if dy.abs() > 1e-6 {
                    hh / dy.abs()
                } else {
                    f32::MAX
                };
                tx.min(ty)
            }
        };
        let t = t.min(1.0);
        [c[0] + dx * t, c[1] + dy * t]
    }
}

/// A line between two shapes, or two points.
#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    /// The shapes it joins, by place; none for an end at a point.
    pub from: Option<usize>,
    pub to: Option<usize>,
    /// Its ends where they are not at shapes.
    pub start: [f32; 2],
    pub end: [f32; 2],
    /// What is written at its middle.
    pub label: String,
    /// Which ends have an arrow.
    pub arrow_start: bool,
    pub arrow_end: bool,
    /// Its color; none for the theme's.
    pub stroke: Option<Color>,
    /// Dashed.
    pub dashed: bool,
}

/// What a plugin's document is drawn as: shapes in the order they are
/// drawn, the first under the others, the connectors over them.
#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    /// The shapes.
    pub shapes: Vec<Shape>,
    /// The connectors.
    pub connectors: Vec<Connector>,
    /// The plugin's command run with `{"shape": target}` to open one.
    pub command: String,
}

// A canvas is compared to tell a new one from the same written again.
impl Eq for Canvas {}

impl Canvas {
    /// The canvas of `shapes` and the connectors, whose ends name shapes
    /// by id (a connector's end naming no shape is at its point).
    pub fn new(
        shapes: Vec<Shape>,
        connectors: Vec<(Option<String>, Option<String>, Connector)>,
        command: String,
    ) -> Canvas {
        let ids: std::collections::HashMap<&str, usize> = shapes
            .iter()
            .enumerate()
            .map(|(i, s)| (s.id.as_str(), i))
            .collect();
        let find = |id: &Option<String>| id.as_deref().and_then(|id| ids.get(id).copied());
        let connectors = connectors
            .into_iter()
            .map(|(a, b, mut c)| {
                c.from = find(&a);
                c.to = find(&b);
                c
            })
            .collect();
        Canvas {
            shapes,
            connectors,
            command,
        }
    }

    /// Connector `c`'s ends: at its shapes' edges facing each other, else
    /// at its points.
    pub fn ends(&self, c: &Connector) -> ([f32; 2], [f32; 2]) {
        let shape = |i: Option<usize>| i.and_then(|i| self.shapes.get(i));
        let (a, b) = (shape(c.from), shape(c.to));
        let toward_b = b.map_or(c.end, Shape::center);
        let toward_a = a.map_or(c.start, Shape::center);
        (
            a.map_or(c.start, |s| s.exit(toward_b)),
            b.map_or(c.end, |s| s.exit(toward_a)),
        )
    }

    /// The bounds of all it draws (left, top, right, bottom); none for an
    /// empty canvas.
    pub fn bounds(&self) -> Option<[f32; 4]> {
        let mut b: Option<[f32; 4]> = None;
        let mut add = |x0: f32, y0: f32, x1: f32, y1: f32| {
            b = Some(match b {
                Some(o) => [o[0].min(x0), o[1].min(y0), o[2].max(x1), o[3].max(y1)],
                None => [x0, y0, x1, y1],
            });
        };
        for s in &self.shapes {
            let [x, y, w, h] = s.rect;
            add(x, y, x + w, y + h);
        }
        for c in &self.connectors {
            let (a, z) = self.ends(c);
            add(
                a[0].min(z[0]),
                a[1].min(z[1]),
                a[0].max(z[0]),
                a[1].max(z[1]),
            );
        }
        b
    }
}

/// The two ends of an arrow's head at `tip` of a line from `tail`, `len`
/// long.
pub fn arrow_head(tip: [f32; 2], tail: [f32; 2], len: f32) -> [[f32; 2]; 2] {
    let (dx, dy) = (tip[0] - tail[0], tip[1] - tail[1]);
    let d = dx.hypot(dy);
    if d < 1e-6 {
        return [tip, tip];
    }
    let (ux, uy) = (dx / d, dy / d);
    let (cos, sin) = (0.89_f32, 0.45_f32);
    [
        [
            tip[0] - len * (ux * cos - uy * sin),
            tip[1] - len * (uy * cos + ux * sin),
        ],
        [
            tip[0] - len * (ux * cos + uy * sin),
            tip[1] - len * (uy * cos - ux * sin),
        ],
    ]
}

/// `text` in lines of at most `width` characters, broken at spaces where
/// it can, its own line feeds kept.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    for para in text.lines() {
        let mut line = String::new();
        for word in para.split(' ') {
            let mut word = word.to_string();
            loop {
                let have = line.chars().count();
                let need = word.chars().count() + usize::from(have > 0);
                if have + need <= width {
                    if have > 0 {
                        line.push(' ');
                    }
                    line.push_str(&word);
                    break;
                }
                if have > 0 {
                    out.push(std::mem::take(&mut line));
                    continue;
                }
                // A word longer than a line: cut.
                let cut: String = word.chars().take(width).collect();
                word = word.chars().skip(width).collect();
                out.push(cut);
                if word.is_empty() {
                    break;
                }
            }
        }
        out.push(line);
    }
    out
}

/// A canvas seen through a window: where it looks, how near, the shape
/// chosen.
#[derive(Debug, Clone)]
pub struct CanvasView {
    canvas: Arc<Canvas>,
    /// The point of the canvas in the middle of the window.
    pub center: [f32; 2],
    /// Pixels per unit of the canvas.
    pub zoom: f32,
    /// Whether the user moved the view: until then it is fitted.
    moved: bool,
    /// The shape under the mouse.
    pub hovered: Option<usize>,
    /// The shape chosen.
    pub selected: Option<usize>,
}

impl CanvasView {
    /// A view of `canvas`, fitted once drawn.
    pub fn new(canvas: Arc<Canvas>) -> CanvasView {
        CanvasView {
            canvas,
            center: [0.0, 0.0],
            zoom: 1.0,
            moved: false,
            hovered: None,
            selected: None,
        }
    }

    /// The canvas written again: the view stays, the shape chosen by its
    /// id.
    pub fn update(&mut self, canvas: Arc<Canvas>) {
        if canvas == self.canvas {
            return;
        }
        let id = self.selected.map(|i| self.canvas.shapes[i].id.clone());
        self.canvas = canvas;
        self.selected = id.and_then(|id| self.canvas.shapes.iter().position(|s| s.id == id));
        self.hovered = None;
    }

    /// The canvas.
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    /// Where a point of the canvas is in a window of `size` pixels.
    pub fn to_screen(&self, p: [f32; 2], size: [f32; 2]) -> [f32; 2] {
        [
            (p[0] - self.center[0]) * self.zoom + size[0] / 2.0,
            (p[1] - self.center[1]) * self.zoom + size[1] / 2.0,
        ]
    }

    /// Which point of the canvas a pixel of a window of `size` shows.
    pub fn to_canvas(&self, s: [f32; 2], size: [f32; 2]) -> [f32; 2] {
        [
            (s[0] - size[0] / 2.0) / self.zoom + self.center[0],
            (s[1] - size[1] / 2.0) / self.zoom + self.center[1],
        ]
    }

    /// Fits the canvas into a window of `size` until the user moves the
    /// view; true when it did.
    pub fn follow(&mut self, size: [f32; 2]) -> bool {
        if self.moved || size[0] < 1.0 || size[1] < 1.0 {
            return false;
        }
        self.fit(size);
        true
    }

    /// Fits the whole canvas into a window of `size`.
    pub fn fit(&mut self, size: [f32; 2]) {
        let Some([x0, y0, x1, y1]) = self.canvas.bounds() else {
            return;
        };
        let (w, h) = ((x1 - x0).max(1.0), (y1 - y0).max(1.0));
        self.center = [(x0 + x1) / 2.0, (y0 + y1) / 2.0];
        self.zoom = ((size[0] * 0.9) / w)
            .min((size[1] * 0.9) / h)
            .clamp(0.02, 4.0);
    }

    /// Zooms by `factor` keeping pixel `s` over the same point.
    pub fn zoom_at(&mut self, factor: f32, s: [f32; 2], size: [f32; 2]) {
        self.moved = true;
        let before = self.to_canvas(s, size);
        self.zoom = (self.zoom * factor).clamp(0.02, 16.0);
        let after = self.to_canvas(s, size);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Moves the view by `d` pixels, the canvas following the mouse.
    pub fn pan_by(&mut self, d: [f32; 2]) {
        self.moved = true;
        self.center[0] -= d[0] / self.zoom;
        self.center[1] -= d[1] / self.zoom;
    }

    /// The shape at pixel `s` of a window of `size`: the uppermost of
    /// those it is in, a frame only where no other shape is.
    pub fn shape_at(&self, s: [f32; 2], size: [f32; 2]) -> Option<usize> {
        let p = self.to_canvas(s, size);
        let shapes = &self.canvas.shapes;
        let group = |i: &usize| matches!(shapes[*i].kind, ShapeKind::Group);
        (0..shapes.len())
            .rev()
            .filter(|i| !group(i) && shapes[*i].contains(p))
            .chain(
                (0..shapes.len())
                    .rev()
                    .filter(|i| group(i) && shapes[*i].contains(p)),
            )
            .next()
    }

    /// Chooses the next shape that leads somewhere (`back`: the one
    /// before), in reading order: top to bottom, then left to right.
    pub fn select_next(&mut self, back: bool) {
        let shapes = &self.canvas.shapes;
        let mut order: Vec<usize> = (0..shapes.len())
            .filter(|&i| shapes[i].target.is_some())
            .collect();
        if order.is_empty() {
            return;
        }
        order.sort_by(|&a, &b| {
            let (ra, rb) = (shapes[a].rect, shapes[b].rect);
            ra[1].total_cmp(&rb[1]).then(ra[0].total_cmp(&rb[0]))
        });
        let at = self
            .selected
            .and_then(|s| order.iter().position(|&i| i == s));
        let n = order.len();
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => n - 1,
            (Some(k), false) => (k + 1) % n,
            (Some(k), true) => (k + n - 1) % n,
        };
        self.selected = Some(order[next]);
    }

    /// Brings the shape chosen to the middle of the window.
    pub fn center_on_selected(&mut self) {
        if let Some(s) = self.selected.and_then(|i| self.canvas.shapes.get(i)) {
            self.center = s.center();
            self.moved = true;
        }
    }

    /// The target of the shape chosen, if it leads somewhere.
    pub fn target(&self) -> Option<&str> {
        self.selected
            .and_then(|i| self.canvas.shapes.get(i))
            .and_then(|s| s.target.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(id: &str, kind: ShapeKind, rect: [f32; 4], target: Option<&str>) -> Shape {
        Shape {
            id: id.into(),
            kind,
            rect,
            text: String::new(),
            fill: None,
            stroke: None,
            dashed: false,
            target: target.map(str::to_string),
            text_size: None,
        }
    }

    fn connector() -> Connector {
        Connector {
            from: None,
            to: None,
            start: [0.0, 0.0],
            end: [0.0, 0.0],
            label: String::new(),
            arrow_start: false,
            arrow_end: true,
            stroke: None,
            dashed: false,
        }
    }

    fn canvas() -> Canvas {
        Canvas::new(
            vec![
                shape(
                    "frame",
                    ShapeKind::Group,
                    [-50.0, -50.0, 400.0, 300.0],
                    None,
                ),
                shape("a", ShapeKind::Box(4.0), [0.0, 0.0, 100.0, 50.0], Some("A")),
                shape(
                    "b",
                    ShapeKind::Ellipse,
                    [200.0, 0.0, 100.0, 50.0],
                    Some("B"),
                ),
                shape("c", ShapeKind::Text, [0.0, 150.0, 100.0, 20.0], Some("C")),
            ],
            vec![
                (Some("a".into()), Some("b".into()), connector()),
                (
                    Some("a".into()),
                    Some("nowhere".into()),
                    Connector {
                        end: [50.0, 400.0],
                        ..connector()
                    },
                ),
            ],
            "notes.open".into(),
        )
    }

    #[test]
    fn connectors_end_at_their_shapes_edges() {
        let c = canvas();
        assert_eq!(
            (c.connectors[0].from, c.connectors[0].to),
            (Some(1), Some(2))
        );
        // From the box's right side to the ellipse's left.
        assert_eq!(c.ends(&c.connectors[0]), ([100.0, 25.0], [200.0, 25.0]));
        // An end naming no shape: at its point; the box's side towards it.
        assert_eq!(c.connectors[1].to, None);
        assert_eq!(c.ends(&c.connectors[1]), ([50.0, 50.0], [50.0, 400.0]));
        assert_eq!(c.bounds(), Some([-50.0, -50.0, 350.0, 400.0]));
        let head = arrow_head([10.0, 0.0], [0.0, 0.0], 5.0);
        assert!(head.iter().all(|p| p[0] < 10.0) && head[0][1] * head[1][1] < 0.0);
    }

    #[test]
    fn a_view_fitted_its_shapes_hit_and_chosen_in_reading_order() {
        let mut v = CanvasView::new(Arc::new(canvas()));
        let size = [800.0, 900.0];
        assert!(v.follow(size));
        assert_eq!(v.center, [150.0, 175.0]);
        // The box over its frame; the frame where nothing else is.
        let at = |v: &CanvasView, p: [f32; 2]| v.to_screen(p, size);
        assert_eq!(v.shape_at(at(&v, [50.0, 25.0]), size), Some(1));
        assert_eq!(v.shape_at(at(&v, [150.0, 100.0]), size), Some(0));
        assert_eq!(v.shape_at(at(&v, [900.0, 900.0]), size), None);
        // Tab: A, B, C, then A again; Shift+Tab back.
        v.select_next(false);
        assert_eq!(v.target(), Some("A"));
        v.select_next(false);
        v.select_next(false);
        assert_eq!(v.target(), Some("C"));
        v.select_next(false);
        assert_eq!(v.target(), Some("A"));
        v.select_next(true);
        assert_eq!(v.target(), Some("C"));
        // Moved by the user: no longer fitted; written again, the shape
        // chosen kept by its id.
        v.pan_by([10.0, 0.0]);
        assert!(!v.follow(size));
        let mut fewer = canvas();
        fewer.shapes.remove(1);
        v.update(Arc::new(Canvas::new(
            fewer.shapes,
            Vec::new(),
            fewer.command,
        )));
        assert_eq!(v.target(), Some("C"));
    }

    #[test]
    fn colors_and_text_in_lines() {
        assert_eq!(Color::parse("Red"), Some(Color::Red));
        assert_eq!(Color::parse("#0a0B0c"), Some(Color::Rgb(10, 11, 12)));
        assert_eq!(Color::parse("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(Color::parse("var(--ls-x)"), None);
        assert_eq!(
            wrap("a sketch of the graph plugin\nnext", 10),
            ["a sketch", "of the", "graph", "plugin", "next"]
        );
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn a_shapes_text_at_its_own_size() {
        let mut s = shape("a", ShapeKind::Text, [0.0, 0.0, 100.0, 40.0], None);
        // The window's own, growing with the zoom up to three times.
        assert_eq!(
            (s.text_px(1.0), s.text_px(2.0), s.text_px(5.0)),
            (16.0, 32.0, 48.0)
        );
        // Its own size (Logseq's xl), and a small one (its xs).
        s.text_size = Some(48.0);
        assert_eq!(
            (s.text_px(0.5), s.text_px(1.0), s.text_px(4.0)),
            (24.0, 48.0, 144.0)
        );
        s.text_size = Some(10.0);
        assert_eq!(s.text_px(1.0), 10.0);
    }
}
