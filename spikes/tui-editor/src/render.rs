//! Turns the frontend-neutral line view (shared with the gpui spike) into
//! terminal cells: styled graphemes wrapped into rows, with headline glyphs,
//! checkbox glyphs, Unicode formulas and OSC 8 link targets.

use ratatui::style::{Color, Modifier, Style};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::caps::Caps;
use crate::view::{BlockKind, LineView, Sty, Widget};
use org_syntax::{SyntaxKind, SyntaxNode, TextSize};

/// One grapheme on screen.
#[derive(Clone, Debug)]
pub struct Glyph {
    pub text: String,
    pub width: u16,
    pub style: Style,
    /// Source offset of the grapheme (for non-source text: of its segment).
    pub src: usize,
    pub src_end: usize,
    pub link: Option<String>,
    /// The widget this glyph draws, with its source range.
    pub widget: Option<(Widget, usize, usize)>,
}

const LEVEL_GLYPHS: [&str; 4] = ["◉", "○", "◈", "◇"];
const LEVEL_COLORS: [Color; 6] = [Color::Blue, Color::Cyan, Color::Green, Color::Yellow, Color::Magenta, Color::Red];

pub fn code_bg(caps: &Caps) -> Color {
    if caps.true_color { Color::Rgb(45, 45, 52) } else { Color::Indexed(236) }
}

fn style_for(s: &Sty, heading: u8, caps: &Caps) -> Style {
    let mut st = Style::default();
    if caps.no_color {
        if s.bold || s.title || heading > 0 {
            st = st.add_modifier(Modifier::BOLD);
        }
        if s.italic {
            st = st.add_modifier(Modifier::ITALIC);
        }
        if s.underline || s.link {
            st = st.add_modifier(Modifier::UNDERLINED);
        }
        return st;
    }
    if heading > 0 {
        st = st.fg(LEVEL_COLORS[(heading as usize - 1) % LEVEL_COLORS.len()]).add_modifier(Modifier::BOLD);
    }
    if s.bold || s.title {
        st = st.add_modifier(Modifier::BOLD);
    }
    if s.title {
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    if s.italic {
        st = st.add_modifier(if caps.italic { Modifier::ITALIC } else { Modifier::DIM });
    }
    if s.underline {
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    if s.strike && caps.strikethrough {
        st = st.add_modifier(Modifier::CROSSED_OUT);
    }
    if s.code_bg {
        st = st.bg(code_bg(caps)).fg(Color::LightYellow);
    }
    if s.link {
        st = st.fg(Color::LightBlue).add_modifier(Modifier::UNDERLINED);
    }
    if let Some(done) = s.todo {
        st = st.fg(if done { Color::Green } else { Color::Red }).add_modifier(Modifier::BOLD);
    }
    if s.tag || s.dim {
        st = st.fg(Color::DarkGray);
    }
    if s.timestamp {
        st = st.fg(Color::Magenta);
    }
    if s.priority {
        st = st.fg(Color::LightRed);
    }
    st
}

/// The raw target of the link containing `offset`.
fn link_target(root: &SyntaxNode, offset: usize) -> Option<String> {
    let tok = root.token_at_offset(TextSize::from(offset as u32)).right_biased()?;
    let link = tok.parent_ancestors().find(|a| a.kind() == SyntaxKind::LINK)?;
    let text = link.text().to_string();
    let text = text.trim_end();
    if let Some(inner) = text.strip_prefix("[[") {
        return Some(inner.split(']').next().unwrap_or(inner).to_string());
    }
    Some(text.trim_start_matches('<').trim_end_matches('>').to_string())
}

/// The graphemes of one source line.
pub fn glyphs(view: &LineView, root: &SyntaxNode, ls: usize, on_line: bool, caps: &Caps) -> Vec<Glyph> {
    let mut out = Vec::new();
    // Headline stars are hidden away from the cursor; show a level glyph.
    if view.heading > 0 && !on_line {
        let level = view.heading as usize;
        let style = style_for(&Sty::default(), view.heading, caps);
        let indent = "  ".repeat(level.saturating_sub(1).min(4));
        let glyph = LEVEL_GLYPHS[(level - 1) % LEVEL_GLYPHS.len()];
        for g in format!("{indent}{glyph} ").graphemes(true) {
            out.push(Glyph { text: g.into(), width: g.width() as u16, style, src: ls, src_end: ls, link: None, widget: None });
        }
    }
    for seg in &view.segs {
        let style = style_for(&seg.sty, view.heading, caps);
        let link = if seg.sty.link { link_target(root, seg.src_start) } else { None };
        let (shown, verbatim) = match &seg.widget {
            Some(Widget::Checkbox(s)) => (match s { b'X' => "☑", b'-' => "◐", _ => "☐" }.to_string(), false),
            Some(Widget::Math(src)) => (crate::math::unicode(src), false),
            None => (seg.text.clone(), seg.verbatim),
        };
        let style = if matches!(seg.widget, Some(Widget::Math(_))) && !caps.no_color {
            style.fg(Color::LightCyan).add_modifier(Modifier::ITALIC)
        } else {
            style
        };
        for (i, g) in shown.grapheme_indices(true) {
            let (src, src_end) = if verbatim {
                (seg.src_start + i, seg.src_start + i + g.len())
            } else {
                (seg.src_start, seg.src_end)
            };
            let width = if g == "\t" { 1 } else { g.width() as u16 };
            out.push(Glyph {
                text: if g == "\t" { " ".into() } else { g.into() },
                width,
                style,
                src,
                src_end,
                link: link.clone(),
                widget: seg.widget.clone().map(|w| (w, seg.src_start, seg.src_end)),
            });
        }
    }
    out
}

/// Greedy wrapping at spaces; long words break anywhere.
pub fn wrap(glyphs: Vec<Glyph>, width: u16) -> Vec<Vec<Glyph>> {
    let mut rows: Vec<Vec<Glyph>> = vec![Vec::new()];
    let mut w = 0u16;
    let mut last_space: Option<usize> = None;
    for g in glyphs {
        let row = rows.last_mut().unwrap();
        if w + g.width > width && !row.is_empty() && g.text != " " {
            let carry: Vec<Glyph> = match last_space {
                Some(i) if i + 1 < row.len() => row.split_off(i + 1),
                Some(_) => Vec::new(),
                None => Vec::new(),
            };
            w = carry.iter().map(|g| g.width).sum();
            rows.push(carry);
            last_space = None;
        }
        let row = rows.last_mut().unwrap();
        if g.text == " " {
            last_space = Some(row.len());
        }
        w += g.width;
        row.push(g);
    }
    rows
}

/// Whether the view is a code block row (background across the width).
pub fn is_code(view: &LineView) -> bool {
    view.block == BlockKind::Code
}
