//! Colors and fonts: the light and dark themes of `kalem_core::theme` as
//! gpui colors.

use gpui::{Hsla, WindowAppearance};
use kalem_core::theme::{self, Color, ThemeColors};

/// The colors of the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    /// Dark or light.
    pub dark: bool,
    /// The page.
    pub background: Hsla,
    /// Text.
    pub foreground: Hsla,
    /// Dimmed text: markup, drawers, keywords, tags.
    pub muted: Hsla,
    /// Links.
    pub link: Hsla,
    /// The caret.
    pub caret: Hsla,
    /// The selection.
    pub selection: Hsla,
    /// Search matches.
    pub mark: Hsla,
    /// Code backgrounds.
    pub code_bg: Hsla,
    /// TODO keywords that are not done.
    pub todo: Hsla,
    /// Done keywords.
    pub done: Hsla,
    /// Timestamps.
    pub timestamp: Hsla,
    /// Priorities.
    pub priority: Hsla,
    /// Headline colors by level.
    pub levels: [Hsla; 6],
    /// Bars: toolbar and status bar.
    pub bar: Hsla,
    /// Lines between areas.
    pub border: Hsla,
    /// Syntax colors: keyword, string, comment, number, function, type.
    pub syntax: [Hsla; 8],
    /// The colors plugins name for their documents' text, as
    /// [`STYLE_COLORS`] lists them.
    pub styles: [Option<Hsla>; 9],
    /// The body font family.
    pub font: String,
    /// The code font family.
    pub mono: String,
    /// The body font size in pixels.
    pub size: f32,
}

fn fonts() -> (String, String) {
    if cfg!(target_os = "macos") {
        (".SystemUIFont".into(), "Menlo".into())
    } else if cfg!(windows) {
        ("Segoe UI".into(), "Consolas".into())
    } else {
        ("Noto Sans".into(), "DejaVu Sans Mono".into())
    }
}

/// The colors a plugin names for its document's text, in [`Theme::styles`]'s
/// order.
pub const STYLE_COLORS: [kalem_core::StyleColor; 9] = [
    kalem_core::StyleColor::Default,
    kalem_core::StyleColor::Muted,
    kalem_core::StyleColor::Red,
    kalem_core::StyleColor::Green,
    kalem_core::StyleColor::Yellow,
    kalem_core::StyleColor::Blue,
    kalem_core::StyleColor::Magenta,
    kalem_core::StyleColor::Cyan,
    kalem_core::StyleColor::Accent,
];

pub fn color(c: Color) -> Hsla {
    gpui::rgba(c.0).into()
}

impl Theme {
    /// The theme of `colors`, with the default fonts.
    pub fn from_colors(c: &ThemeColors) -> Theme {
        let (font, mono) = fonts();
        Theme {
            dark: c.dark,
            background: color(c.background),
            foreground: color(c.foreground),
            muted: color(c.muted),
            link: color(c.link),
            caret: color(c.caret),
            selection: color(c.selection),
            mark: color(c.mark),
            code_bg: color(c.code_bg),
            todo: color(c.todo),
            done: color(c.done),
            timestamp: color(c.timestamp),
            priority: color(c.priority),
            levels: c.levels.map(color),
            bar: color(c.bar),
            border: color(c.border),
            syntax: c.syntax.map(color),
            styles: STYLE_COLORS.map(|s| c.style_color(s).map(color)),
            font,
            mono,
            size: 16.,
        }
    }

    /// The shade of a color a plugin names for its document's text; none
    /// for the text's own.
    pub fn style_color(&self, c: kalem_core::StyleColor) -> Option<Hsla> {
        let i = STYLE_COLORS.iter().position(|s| *s == c)?;
        self.styles[i]
    }

    /// The built-in light theme.
    pub fn light() -> Theme {
        Theme::from_colors(&ThemeColors::builtin(false))
    }

    /// The built-in dark theme.
    pub fn dark() -> Theme {
        Theme::from_colors(&ThemeColors::builtin(true))
    }

    /// The theme the settings ask for: `editor.theme` (the system's
    /// appearance for `system`) with the user's theme files, and
    /// `editor.font_family` and `editor.font_size`.
    pub fn from_config(config: &kalem_core::Config, a: WindowAppearance) -> Theme {
        let system_dark = matches!(a, WindowAppearance::Dark | WindowAppearance::VibrantDark);
        let dark = theme::wants_dark(config.str("editor.theme"), system_dark);
        let colors = ThemeColors::load(dark, theme::user_dir().as_deref());
        let mut t = Theme::from_colors(&colors);
        let family = config.str("editor.font_family");
        if !family.is_empty() {
            t.font = family.to_string();
        }
        let code = config.str("editor.code_font_family");
        if !code.is_empty() {
            t.mono = code.to_string();
        }
        t.size = config.int("editor.font_size") as f32;
        t
    }

    /// The built-in theme for the window's appearance.
    pub fn for_appearance(a: WindowAppearance) -> Theme {
        match a {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Theme::dark(),
            WindowAppearance::Light | WindowAppearance::VibrantLight => Theme::light(),
        }
    }

    /// A headline's color.
    pub fn level(&self, level: u8) -> Hsla {
        self.levels[(level.max(1) as usize - 1) % self.levels.len()]
    }

    /// A syntax color for a highlighting kind.
    pub fn code(&self, kind: kalem_highlight::Kind) -> Option<Hsla> {
        use kalem_highlight::Kind as K;
        Some(match kind {
            K::Keyword | K::Macro => self.syntax[0],
            K::String => self.syntax[1],
            K::Comment => self.syntax[2],
            K::Number | K::Constant => self.syntax[3],
            K::Function | K::Tag => self.syntax[4],
            K::Type => self.syntax[5],
            K::Inserted => self.syntax[6],
            K::Deleted => self.syntax[7],
            K::Invalid => self.todo,
            K::Operator | K::Variable => return None,
        })
    }
}
