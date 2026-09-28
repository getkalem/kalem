//! The view model's lines as terminal cells: styled graphemes with widths,
//! headline glyphs, checkbox glyphs, Unicode formulas and OSC 8 link
//! targets, wrapped into rows.

use kalem_core::view::{CheckState, LineView, Style as ViewStyle, Widget};
use org_syntax::{ParseContext, SyntaxKind, SyntaxNode, TextSize, ast};
use ratatui::style::{Color, Modifier, Style};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::caps::Caps;

/// A widget under a glyph, with its source range.
pub type WidgetAt = (Widget, usize, usize);

/// One grapheme on screen, with the widget it draws.
pub type Glyph = tui_rich_text::Glyph<WidgetAt>;

const LEVEL_GLYPHS: [&str; 4] = ["◉", "○", "◈", "◇"];
const LEVEL_ASCII: [&str; 4] = ["#", "=", "+", "-"];
const LEVEL_COLORS: [Color; 6] = [
    Color::Blue,
    Color::Cyan,
    Color::Green,
    Color::Yellow,
    Color::Magenta,
    Color::Red,
];

/// The background of code.
/// A theme color as a terminal color.
pub fn rgb(c: kalem_core::theme::Color) -> Color {
    let (r, g, b) = c.rgb();
    Color::Rgb(r, g, b)
}

/// A theme color that may be translucent, laid over the theme's
/// background.
pub fn solid(c: kalem_core::theme::Color, t: &kalem_core::theme::ThemeColors) -> Color {
    rgb(c.over(t.background))
}

pub fn code_bg(caps: &Caps) -> Color {
    if let Some(t) = &caps.colors {
        return solid(t.code_bg, t);
    }
    if caps.true_color {
        Color::Rgb(45, 45, 52)
    } else {
        Color::Indexed(236)
    }
}

/// A code glyph's style for its highlighting kind.
pub fn code_style(kind: kalem_highlight::Kind, base: Style, caps: &Caps) -> Style {
    use kalem_highlight::Kind as K;
    if caps.no_color {
        return match kind {
            K::Comment => base.add_modifier(Modifier::DIM),
            K::Keyword => base.add_modifier(Modifier::BOLD),
            _ => base,
        };
    }
    if let Some(t) = &caps.colors {
        let [keyword, string, comment, number, function, ty] = t.syntax.map(rgb);
        return match kind {
            K::Comment => base.fg(comment).add_modifier(if caps.italic {
                Modifier::ITALIC
            } else {
                Modifier::DIM
            }),
            K::String => base.fg(string),
            K::Number | K::Constant => base.fg(number),
            K::Keyword | K::Macro => base.fg(keyword),
            K::Function | K::Tag => base.fg(function),
            K::Type => base.fg(ty),
            K::Invalid => base.fg(rgb(t.todo)),
            K::Operator | K::Variable => base,
        };
    }
    match kind {
        K::Comment => base.fg(Color::DarkGray).add_modifier(if caps.italic {
            Modifier::ITALIC
        } else {
            Modifier::DIM
        }),
        K::String => base.fg(Color::Green),
        K::Number | K::Constant => base.fg(Color::LightMagenta),
        K::Keyword => base.fg(Color::LightRed),
        K::Function => base.fg(Color::LightBlue),
        K::Type => base.fg(Color::Yellow),
        K::Tag => base.fg(Color::LightBlue),
        K::Macro => base.fg(Color::Cyan),
        K::Invalid => base.fg(Color::Red),
        K::Operator | K::Variable => base,
    }
}

/// The style of a part of a file manager listing.
pub fn dir_style(style: kalem_core::dired::DirStyle, base: Style, caps: &Caps) -> Style {
    use kalem_core::dired::DirStyle as D;
    use kalem_highlight::Kind as K;
    match style {
        D::Header => code_style(K::Keyword, base, caps).add_modifier(Modifier::BOLD),
        D::Detail | D::Hidden => base.add_modifier(Modifier::DIM),
        D::Note => base.add_modifier(Modifier::DIM | Modifier::ITALIC),
        D::Dir => code_style(K::Function, base, caps).add_modifier(Modifier::BOLD),
        D::Link => code_style(K::Macro, base, caps),
        D::Broken => code_style(K::Invalid, base, caps),
        D::Executable => code_style(K::String, base, caps),
        D::Marked => code_style(K::Constant, base, caps).add_modifier(Modifier::BOLD),
        D::Flagged => code_style(K::Invalid, base, caps).add_modifier(Modifier::BOLD),
    }
}

/// The terminal style of a view style on a heading of `heading` level.
pub fn style_for(s: &ViewStyle, heading: u8, caps: &Caps) -> Style {
    let st = style_base(s, heading, caps);
    // Kalem's colors: 24-bit where the terminal has it, else the nearest
    // of 256.
    if caps.no_color {
        return st;
    }
    let to = |c: kalem_core::theme::Color| {
        let (r, g, b) = c.rgb();
        if caps.true_color {
            Color::Rgb(r, g, b)
        } else {
            Color::Indexed(ansi256(r, g, b))
        }
    };
    let mut st = st;
    if let Some(c) = s.rich.color {
        st = st.fg(to(c));
    }
    if let Some(c) = s.rich.highlight {
        st = st.bg(to(c));
        if s.rich.color.is_none() {
            // Dark text on a light highlight.
            st = st.fg(Color::Black);
        }
    }
    st
}

/// The nearest color of the 256-color palette's 6×6×6 cube.
fn ansi256(r: u8, g: u8, b: u8) -> u8 {
    let q = |v: u8| ((u16::from(v) * 5 + 127) / 255) as u8;
    16 + 36 * q(r) + 6 * q(g) + q(b)
}

fn style_base(s: &ViewStyle, heading: u8, caps: &Caps) -> Style {
    let mut st = Style::default();
    if s.bold || s.title || heading > 0 {
        st = st.add_modifier(Modifier::BOLD);
    }
    if s.underline || s.title {
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    if s.byline {
        st = st.add_modifier(if caps.italic {
            Modifier::ITALIC
        } else {
            Modifier::DIM
        });
    }
    if s.italic {
        st = st.add_modifier(if caps.italic {
            Modifier::ITALIC
        } else {
            Modifier::DIM
        });
    }
    if s.strike && caps.strikethrough {
        st = st.add_modifier(Modifier::CROSSED_OUT);
    }
    if s.dim || s.target {
        st = st.add_modifier(Modifier::DIM);
    }
    if caps.no_color {
        if s.link {
            st = st.add_modifier(Modifier::UNDERLINED);
        }
        if s.todo.is_some() {
            st = st.add_modifier(Modifier::REVERSED);
        }
        return st;
    }
    if let Some(t) = &caps.colors {
        return themed(st, s, heading, t, caps);
    }
    if heading > 0 {
        st = st.fg(LEVEL_COLORS[(heading as usize - 1) % LEVEL_COLORS.len()]);
    }
    if s.code {
        st = st.bg(code_bg(caps)).fg(Color::LightYellow);
    }
    if s.link {
        st = st.fg(Color::LightBlue).add_modifier(Modifier::UNDERLINED);
    }
    if let Some(done) = s.todo {
        st = st
            .fg(if done { Color::Green } else { Color::Red })
            .add_modifier(Modifier::BOLD);
    }
    if s.tag || s.dim || s.cookie {
        st = st.fg(Color::DarkGray).remove_modifier(Modifier::BOLD);
    }
    if s.timestamp {
        st = st.fg(Color::Magenta);
    }
    if s.priority {
        st = st.fg(Color::LightRed);
    }
    if s.footnote || s.superscript || s.subscript {
        st = st.fg(Color::LightCyan);
    }
    st
}

/// [`style_for`]'s colors from the theme.
fn themed(
    mut st: Style,
    s: &ViewStyle,
    heading: u8,
    t: &kalem_core::theme::ThemeColors,
    caps: &Caps,
) -> Style {
    if heading > 0 {
        st = st.fg(rgb(t.level(heading)));
    }
    if s.code {
        st = st.bg(code_bg(caps));
    }
    if s.link || s.footnote || s.superscript || s.subscript {
        st = st.fg(rgb(t.link));
    }
    if s.link {
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    if let Some(done) = s.todo {
        st = st
            .fg(rgb(if done { t.done } else { t.todo }))
            .add_modifier(Modifier::BOLD);
    }
    if s.tag || s.dim || s.cookie {
        st = st.fg(rgb(t.muted)).remove_modifier(Modifier::BOLD);
    }
    if s.timestamp {
        st = st.fg(rgb(t.timestamp));
    }
    if s.priority {
        st = st.fg(rgb(t.priority));
    }
    st
}

/// The OSC 8 target of the link at `offset`: web and mail links only.
fn link_target(root: &SyntaxNode, ctx: &ParseContext, offset: usize) -> Option<String> {
    if offset >= usize::from(root.text_range().end()) {
        return None;
    }
    let tok = root
        .token_at_offset(TextSize::from(offset as u32))
        .right_biased()?;
    let node = tok
        .parent_ancestors()
        .find(|a| a.kind() == SyntaxKind::LINK)?;
    let link: ast::Link = ast::AstNode::cast(node)?;
    let info = link.info(ctx);
    matches!(
        info.link_type.as_str(),
        "http" | "https" | "mailto" | "ftp" | "news"
    )
    .then(|| info.raw_link.replace(['\x1b', '\x07'], ""))
}

/// Superscript and subscript digits and signs, where Unicode has them.
fn script(text: &str, sup: bool) -> Option<String> {
    let table: &[(char, char)] = if sup {
        &[
            ('0', '⁰'),
            ('1', '¹'),
            ('2', '²'),
            ('3', '³'),
            ('4', '⁴'),
            ('5', '⁵'),
            ('6', '⁶'),
            ('7', '⁷'),
            ('8', '⁸'),
            ('9', '⁹'),
            ('+', '⁺'),
            ('-', '⁻'),
            ('=', '⁼'),
            ('(', '⁽'),
            (')', '⁾'),
            ('n', 'ⁿ'),
            ('i', 'ⁱ'),
        ]
    } else {
        &[
            ('0', '₀'),
            ('1', '₁'),
            ('2', '₂'),
            ('3', '₃'),
            ('4', '₄'),
            ('5', '₅'),
            ('6', '₆'),
            ('7', '₇'),
            ('8', '₈'),
            ('9', '₉'),
            ('+', '₊'),
            ('-', '₋'),
            ('=', '₌'),
            ('(', '₍'),
            (')', '₎'),
        ]
    };
    text.chars()
        .map(|c| table.iter().find(|(a, _)| *a == c).map(|(_, b)| *b))
        .collect()
}

/// The glyphs of a line and the width to indent its wrapped rows by.
#[derive(Debug, Clone)]
pub struct LineGlyphs {
    /// The glyphs.
    pub glyphs: Vec<Glyph>,
    /// The hanging indent of continuation rows.
    pub hang: u16,
}

/// The glyphs of one source line. `on_line`: the cursor is on it;
/// `folded`: it is the heading of a folded headline.
pub fn glyphs(
    view: &LineView,
    root: &SyntaxNode,
    ctx: &ParseContext,
    on_line: bool,
    folded: bool,
    caps: &Caps,
    raw_math: bool,
) -> LineGlyphs {
    let mut out = Vec::new();
    let ls = view.range.start;
    let push_deco = |out: &mut Vec<Glyph>, text: &str, style: Style, at: usize| {
        for g in text.graphemes(true) {
            out.push(Glyph {
                text: g.into(),
                width: g.width() as u16,
                style,
                src: at,
                src_end: at,
                link: None,
                data: None,
            });
        }
    };
    // Headline stars away from the cursor: a level glyph, indented.
    if view.heading > 0 && !on_line {
        let level = view.heading as usize;
        let style = style_for(&ViewStyle::default(), view.heading, caps);
        let indent = "  ".repeat(level.saturating_sub(1).min(4));
        let glyphs = if caps.ascii {
            LEVEL_ASCII
        } else {
            LEVEL_GLYPHS
        };
        let glyph = glyphs[(level - 1) % glyphs.len()];
        push_deco(&mut out, &format!("{indent}{glyph} "), style, ls);
    }
    let mut hang_at: Option<usize> = (view.heading > 0 && !on_line).then_some(out.len());
    let mut seen_text = false;
    for run in &view.runs {
        let mut style = style_for(&run.style, view.heading, caps);
        let link = if run.style.link && caps.hyperlinks {
            link_target(root, ctx, run.src.start)
        } else {
            None
        };
        let (shown, verbatim) = match &run.widget {
            Some(Widget::Checkbox(s)) => {
                let g = match (s, caps.ascii) {
                    (CheckState::Checked, false) => "☑",
                    (CheckState::Partial, false) => "◐",
                    (CheckState::Unchecked, false) => "☐",
                    (CheckState::Checked, true) => "[X]",
                    (CheckState::Partial, true) => "[-]",
                    (CheckState::Unchecked, true) => "[ ]",
                };
                (g.to_string(), false)
            }
            Some(Widget::Math { source, .. }) if raw_math => (source.clone(), true),
            Some(Widget::Math { source, .. }) => {
                if !caps.no_color {
                    style = style.fg(Color::LightCyan);
                }
                if caps.italic {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                (kalem_core::math::unicode(source), false)
            }
            Some(Widget::Image { path, .. }) => {
                style = style.add_modifier(Modifier::DIM);
                (format!("[image: {path}]"), false)
            }
            None if run.text == "•" && caps.ascii => ("-".to_string(), false),
            None if (run.style.superscript || run.style.subscript) && !run.verbatim => {
                (run.text.clone(), false)
            }
            None if run.style.superscript || run.style.subscript => {
                match script(&run.text, run.style.superscript) {
                    Some(s) => (s, false),
                    None => (run.text.clone(), true),
                }
            }
            None => (run.text.clone(), run.verbatim),
        };
        // Wrapped rows of list items line up after the bullet (and box).
        let blank = shown.trim().is_empty();
        if !seen_text && !blank && hang_at.is_none() {
            let is_marker = run.text == "•"
                || run
                    .widget
                    .as_ref()
                    .is_some_and(|w| matches!(w, Widget::Checkbox(_)))
                || tok_is_bullet(root, run.src.start);
            if !is_marker {
                hang_at = Some(out.len());
                seen_text = true;
            }
        }
        for (i, g) in shown.grapheme_indices(true) {
            let (src, src_end) = if verbatim {
                (run.src.start + i, run.src.start + i + g.len())
            } else {
                (run.src.start, run.src.end)
            };
            let (text, width) = match g {
                "\t" => ("\t".to_string(), 1),
                g if g.chars().all(char::is_control) => {
                    let c = g.chars().next().unwrap_or('?') as u32;
                    (
                        format!("^{}", char::from_u32((c + 64) & 0x7f).unwrap_or('?')),
                        2,
                    )
                }
                g => (g.to_string(), g.width() as u16),
            };
            out.push(Glyph {
                text,
                width,
                style,
                src,
                src_end,
                link: link.clone(),
                data: run.widget.clone().map(|w| (w, run.src.start, run.src.end)),
            });
        }
    }
    if folded {
        let at = view.range.end;
        push_deco(
            &mut out,
            if caps.ascii { " ..." } else { " …" },
            Style::default().add_modifier(Modifier::DIM),
            at,
        );
    }
    let hang = hang_at.map_or(0, |n| out[..n].iter().map(|g| g.width).sum());
    LineGlyphs { glyphs: out, hang }
}

fn tok_is_bullet(root: &SyntaxNode, at: usize) -> bool {
    at < usize::from(root.text_range().end())
        && root
            .token_at_offset(TextSize::from(at as u32))
            .right_biased()
            .is_some_and(|t| {
                matches!(
                    t.kind(),
                    SyntaxKind::BULLET | SyntaxKind::CHECKBOX | SyntaxKind::WHITESPACE
                )
            })
}
