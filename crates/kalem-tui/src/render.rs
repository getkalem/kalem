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

/// The background of a line a plugin marked as added or changed (the
/// git plugin's), under its text: the mark's color over the theme's
/// background where the terminal has the theme's colors, else a shade
/// for its light or dark background; none for a removal or without color.
pub fn change_tint(mark: kalem_core::GutterMark, caps: &Caps) -> Option<Color> {
    use kalem_core::GutterMark as M;
    if caps.no_color || !matches!(mark, M::Added | M::Changed) {
        return None;
    }
    let added = mark == M::Added;
    if let Some(t) = &caps.colors {
        let tint = kalem_core::theme::Color(if added { 0x4caf5033 } else { 0xe9a23b2e });
        return Some(rgb(tint.over(t.background)));
    }
    let dark = caps.dark_background().unwrap_or(true);
    Some(match (caps.true_color, dark, added) {
        (true, true, true) => Color::Rgb(30, 52, 36),
        (true, true, false) => Color::Rgb(58, 46, 30),
        (true, false, true) => Color::Rgb(225, 245, 228),
        (true, false, false) => Color::Rgb(252, 240, 215),
        (false, true, true) => Color::Indexed(22),
        (false, true, false) => Color::Indexed(58),
        (false, false, true) => Color::Indexed(194),
        (false, false, false) => Color::Indexed(230),
    })
}

/// The background of a whole line of a diff ([`kalem_highlight::line_kind`]):
/// an added or removed line in a wash of its color over the theme's
/// background, a hunk's line in the link color's, as magit shows them;
/// shades of the palette where the theme's colors are not shown.
pub fn diff_bg(kind: kalem_highlight::Kind, caps: &Caps) -> Option<Color> {
    use kalem_highlight::Kind as K;
    if caps.no_color || !matches!(kind, K::Inserted | K::Deleted | K::Hunk) {
        return None;
    }
    if let Some(t) = &caps.colors {
        let c = match kind {
            K::Inserted => t.syntax[6],
            K::Deleted => t.syntax[7],
            _ => t.link,
        };
        let wash = kalem_core::theme::Color((c.0 & 0xffffff00) | 0x33);
        return Some(rgb(wash.over(t.background)));
    }
    let dark = caps.dark_background().unwrap_or(true);
    Some(match (caps.true_color, dark, kind) {
        (true, true, K::Inserted) => Color::Rgb(30, 52, 36),
        (true, true, K::Deleted) => Color::Rgb(62, 32, 32),
        (true, true, _) => Color::Rgb(34, 42, 62),
        (true, false, K::Inserted) => Color::Rgb(225, 245, 228),
        (true, false, K::Deleted) => Color::Rgb(252, 226, 226),
        (true, false, _) => Color::Rgb(226, 234, 250),
        (false, true, K::Inserted) => Color::Indexed(22),
        (false, true, K::Deleted) => Color::Indexed(52),
        (false, true, _) => Color::Indexed(17),
        (false, false, K::Inserted) => Color::Indexed(194),
        (false, false, K::Deleted) => Color::Indexed(224),
        (false, false, _) => Color::Indexed(189),
    })
}

/// A code glyph's style for its highlighting kind.
/// A plugin document's style over `base` (`styled-documents`): the
/// theme's shade of its color on a true-color terminal, the terminal's own
/// color otherwise; only the emphasis without colors.
pub fn span_style(s: kalem_core::SpanStyle, base: Style, caps: &Caps) -> Style {
    use kalem_core::StyleColor as C;
    let mut st = base;
    if s.bold {
        st = st.add_modifier(Modifier::BOLD);
    }
    if s.italic && caps.italic {
        st = st.add_modifier(Modifier::ITALIC);
    }
    if s.underline {
        st = st.add_modifier(Modifier::UNDERLINED);
    }
    if caps.no_color {
        if s.color == C::Muted {
            st = st.add_modifier(Modifier::DIM);
        }
        return st;
    }
    if let Some(t) = &caps.colors {
        return match t.style_color(s.color) {
            Some(c) => st.fg(rgb(c)),
            None => st,
        };
    }
    match s.color {
        C::Default => st,
        C::Muted => st.fg(Color::DarkGray),
        C::Red => st.fg(Color::Red),
        C::Green => st.fg(Color::Green),
        C::Yellow => st.fg(Color::Yellow),
        C::Blue => st.fg(Color::Blue),
        C::Magenta => st.fg(Color::Magenta),
        C::Cyan => st.fg(Color::Cyan),
        C::Accent => st.fg(Color::LightBlue),
    }
}

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
        let [
            keyword,
            string,
            comment,
            number,
            function,
            ty,
            inserted,
            deleted,
        ] = t.syntax.map(rgb);
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
            K::Inserted => base.fg(inserted),
            K::Deleted => base.fg(deleted),
            K::Hunk => base.fg(function),
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
        // Bright, to read on the dark washes of `diff_bg`.
        K::Inserted => base.fg(Color::LightGreen),
        K::Deleted => base.fg(Color::LightRed),
        K::Hunk => base.fg(Color::LightBlue),
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
    let rgb = |c: kalem_core::theme::Color| {
        let (r, g, b) = c.rgb();
        [r, g, b]
    };
    let color = |c: [u8; 3]| {
        kalem_core::theme::Color(
            (u32::from(c[0]) << 24) | (u32::from(c[1]) << 16) | (u32::from(c[2]) << 8) | 0xff,
        )
    };
    // A document's colors (a Word document's), chosen for a white page:
    // kept legible on the highlight or the terminal's background (dark
    // when the terminal does not say).
    let under = match s.rich.highlight {
        Some(h) => rgb(h),
        None => caps.background.map_or([0, 0, 0], |(r, g, b)| [r, g, b]),
    };
    if let Some(c) = s.rich.color {
        let c = if s.rich.paper && !s.link {
            color(kalem_core::theme::legible(rgb(c), under))
        } else {
            c
        };
        st = st.fg(to(c));
    }
    if let Some(c) = s.rich.highlight {
        st = st.bg(to(c));
        if s.rich.color.is_none() {
            // Dark text on a light highlight, light on a dark one.
            st = st.fg(to(color(kalem_core::theme::legible([0, 0, 0], rgb(c)))));
        }
    }
    st
}

/// The nearest color of the 256-color palette's 6×6×6 cube.
fn ansi256(r: u8, g: u8, b: u8) -> u8 {
    let q = |v: u8| ((u16::from(v) * 5 + 127) / 255) as u8;
    16 + 36 * q(r) + 6 * q(g) + q(b)
}

pub(crate) fn style_base(s: &ViewStyle, heading: u8, caps: &Caps) -> Style {
    let mut st = Style::default();
    // Under a diagnostic: underlined, red for a warning, blue for style
    // (terminals that know underline colors show them).
    if let Some(warning) = s.flagged {
        st = st.add_modifier(Modifier::UNDERLINED);
        if !caps.no_color {
            st = st.underline_color(if warning {
                Color::LightRed
            } else {
                Color::LightBlue
            });
        }
    }
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
    if s.italic || s.expansion {
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
    if s.footnote || s.superscript || s.subscript || s.expansion {
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
    if s.link || s.footnote || s.superscript || s.subscript || s.expansion {
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
                (kalem_core::view::image_label(&run.text, path), false)
            }
            Some(Widget::TocRow { .. }) => (run.text.clone(), false),
            Some(Widget::Picture { .. }) => {
                style = style.add_modifier(Modifier::DIM);
                (run.text.clone(), false)
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
