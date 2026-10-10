//! Themes (§7.5): light and dark colors by role, in TOML, for both
//! frontends.
//!
//! The built-in themes are `themes/light.toml` and `themes/dark.toml`. A
//! file of the same name in the `themes` directory of the user's settings
//! changes any of their colors. `editor.theme` picks light, dark, or what
//! the system uses (the window's appearance; in a terminal, its
//! background color).

use std::path::Path;

/// A color as `0xRRGGBBAA`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(pub u32);

impl Color {
    /// `#rrggbb` or `#rrggbbaa`.
    pub fn parse(s: &str) -> Option<Color> {
        let hex = s.strip_prefix('#')?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        match hex.len() {
            6 => Some(Color((u32::from_str_radix(hex, 16).ok()? << 8) | 0xff)),
            8 => Some(Color(u32::from_str_radix(hex, 16).ok()?)),
            _ => None,
        }
    }

    /// Red, green and blue.
    pub fn rgb(self) -> (u8, u8, u8) {
        (
            (self.0 >> 24) as u8,
            (self.0 >> 16) as u8,
            (self.0 >> 8) as u8,
        )
    }

    /// Opacity, 255 for opaque.
    pub fn alpha(self) -> u8 {
        self.0 as u8
    }

    /// The opaque color of this one laid over `below`.
    pub fn over(self, below: Color) -> Color {
        let a = u32::from(self.alpha());
        let mix = |x: u8, y: u8| (u32::from(x) * a + u32::from(y) * (255 - a)) / 255;
        let ((r, g, b), (r2, g2, b2)) = (self.rgb(), below.rgb());
        Color((mix(r, r2) << 24) | (mix(g, g2) << 16) | (mix(b, b2) << 8) | 0xff)
    }
}

/// The colors of a theme, by role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeColors {
    /// Its name.
    pub name: String,
    /// Light text on a dark background.
    pub dark: bool,
    /// The page.
    pub background: Color,
    /// Text.
    pub foreground: Color,
    /// Dimmed text: markup, drawers, keywords, tags.
    pub muted: Color,
    /// Links.
    pub link: Color,
    /// The caret.
    pub caret: Color,
    /// The selection.
    pub selection: Color,
    /// Search matches.
    pub mark: Color,
    /// Backgrounds of code.
    pub code_bg: Color,
    /// TODO keywords that are not done.
    pub todo: Color,
    /// Done keywords.
    pub done: Color,
    /// Timestamps.
    pub timestamp: Color,
    /// Priorities.
    pub priority: Color,
    /// Toolbars, status bars and panels.
    pub bar: Color,
    /// Lines between areas.
    pub border: Color,
    /// Headlines, by level.
    pub levels: [Color; 6],
    /// Source code: keyword, string, comment, number, function, type;
    /// then a diff's inserted and deleted lines.
    pub syntax: [Color; 8],
}

const LIGHT: &str = include_str!("../themes/light.toml");
const DARK: &str = include_str!("../themes/dark.toml");

const SYNTAX: [&str; 8] = [
    "keyword", "string", "comment", "number", "function", "type", "inserted", "deleted",
];

impl ThemeColors {
    /// The theme's shade of a color a plugin names for its document's text
    /// ([`crate::StyleColor`]): red its TODO keywords', green its done
    /// ones', the others its source code's; none for the text's own.
    pub fn style_color(&self, c: crate::StyleColor) -> Option<Color> {
        use crate::StyleColor as S;
        Some(match c {
            S::Default => return None,
            S::Muted => self.muted,
            S::Red => self.todo,
            S::Green => self.done,
            S::Yellow => self.syntax[5],
            S::Blue => self.syntax[4],
            S::Magenta => self.syntax[0],
            S::Cyan => self.levels[1],
            S::Accent => self.link,
        })
    }

    fn blank() -> ThemeColors {
        let c = Color(0x000000ff);
        ThemeColors {
            name: String::new(),
            dark: false,
            background: c,
            foreground: c,
            muted: c,
            link: c,
            caret: c,
            selection: c,
            mark: c,
            code_bg: c,
            todo: c,
            done: c,
            timestamp: c,
            priority: c,
            bar: c,
            border: c,
            levels: [c; 6],
            syntax: [c; 8],
        }
    }

    fn role(&mut self, key: &str) -> Option<&mut Color> {
        Some(match key {
            "background" => &mut self.background,
            "foreground" => &mut self.foreground,
            "muted" => &mut self.muted,
            "link" => &mut self.link,
            "caret" => &mut self.caret,
            "selection" => &mut self.selection,
            "mark" => &mut self.mark,
            "code_bg" => &mut self.code_bg,
            "todo" => &mut self.todo,
            "done" => &mut self.done,
            "timestamp" => &mut self.timestamp,
            "priority" => &mut self.priority,
            "bar" => &mut self.bar,
            "border" => &mut self.border,
            _ => return None,
        })
    }

    /// Changes the colors that theme file `text` gives; returns what is
    /// wrong in it (unknown keys, colors that do not read). Other values
    /// stay.
    pub fn apply(&mut self, text: &str) -> Vec<String> {
        let mut issues = Vec::new();
        let doc: toml_edit::DocumentMut = match text.parse() {
            Ok(d) => d,
            Err(e) => return vec![format!("Not valid TOML: {e}")],
        };
        let color = |v: &toml_edit::Item, key: &str, issues: &mut Vec<String>| {
            let c = v.as_str().and_then(Color::parse);
            if c.is_none() {
                issues.push(format!("`{key}` is not a color like \"#1f2328\""));
            }
            c
        };
        for (k, v) in doc.iter() {
            match k {
                "name" => {
                    if let Some(n) = v.as_str() {
                        self.name = n.to_string();
                    }
                }
                "dark" => match v.as_bool() {
                    Some(b) => self.dark = b,
                    None => issues.push("`dark` must be true or false".into()),
                },
                "colors" => {
                    let Some(t) = v.as_table_like() else {
                        issues.push("`colors` must be a table".into());
                        continue;
                    };
                    for (k, v) in t.iter() {
                        if k == "levels" {
                            let list: Vec<Option<Color>> = v
                                .as_array()
                                .map(|a| {
                                    a.iter()
                                        .map(|x| x.as_str().and_then(Color::parse))
                                        .collect()
                                })
                                .unwrap_or_default();
                            if list.len() != 6 || list.iter().any(Option::is_none) {
                                issues.push("`colors.levels` must be six colors".into());
                            } else {
                                for (d, c) in self.levels.iter_mut().zip(list.into_iter().flatten())
                                {
                                    *d = c;
                                }
                            }
                            continue;
                        }
                        let key = format!("colors.{k}");
                        if self.role(k).is_none() {
                            issues.push(format!("Unknown color `{key}`"));
                        } else if let Some(c) = color(v, &key, &mut issues)
                            && let Some(role) = self.role(k)
                        {
                            *role = c;
                        }
                    }
                }
                "syntax" => {
                    let Some(t) = v.as_table_like() else {
                        issues.push("`syntax` must be a table".into());
                        continue;
                    };
                    for (k, v) in t.iter() {
                        let key = format!("syntax.{k}");
                        match SYNTAX.iter().position(|s| *s == k) {
                            Some(i) => {
                                if let Some(c) = color(v, &key, &mut issues) {
                                    self.syntax[i] = c;
                                }
                            }
                            None => issues.push(format!("Unknown color `{key}`")),
                        }
                    }
                }
                other => issues.push(format!("Unknown key `{other}`")),
            }
        }
        issues
    }

    /// A built-in theme.
    pub fn builtin(dark: bool) -> ThemeColors {
        let mut t = ThemeColors::blank();
        let issues = t.apply(if dark { DARK } else { LIGHT });
        debug_assert!(issues.is_empty(), "{issues:?}");
        t
    }

    /// The light or dark theme with the user's changes from
    /// `dir/light.toml` or `dir/dark.toml`; problems go to the log.
    pub fn load(dark: bool, dir: Option<&Path>) -> ThemeColors {
        let mut t = ThemeColors::builtin(dark);
        let Some(path) = dir.map(|d| d.join(if dark { "dark.toml" } else { "light.toml" })) else {
            return t;
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                for i in t.apply(&text) {
                    tracing::warn!(path = %path.display(), "{i}");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => tracing::warn!(path = %path.display(), "cannot read the theme: {e}"),
        }
        t
    }

    /// A headline's color.
    pub fn level(&self, level: u8) -> Color {
        self.levels[(level.max(1) as usize - 1) % self.levels.len()]
    }
}

/// Whether `editor.theme` asks for the dark theme, `system_dark` being
/// what the system (or the terminal) uses.
pub fn wants_dark(setting: &str, system_dark: bool) -> bool {
    match setting {
        "light" => false,
        "dark" => true,
        _ => system_dark,
    }
}

/// The color a grid cell's text is drawn in under a theme, given the
/// cell's own `color` and whether the cell has a fill: `None` is the
/// theme's foreground. A workbook's automatic text color ("Text 1",
/// which Excel writes as black) has to read on the dark theme, and an
/// explicit white one on the light theme: black text on no fill under
/// the dark theme, and white on no fill under the light theme, are drawn
/// in the foreground. A filled cell keeps its color, chosen against its
/// fill.
pub fn cell_text_color(color: Option<[u8; 3]>, filled: bool, dark: bool) -> Option<[u8; 3]> {
    let [r, g, b] = color?;
    let blackish = r < 0x20 && g < 0x20 && b < 0x20;
    let whitish = r > 0xdf && g > 0xdf && b > 0xdf;
    if !filled && ((dark && blackish) || (!dark && whitish)) {
        return None;
    }
    Some([r, g, b])
}

/// The relative luminance of a color (WCAG 2).
fn luminance([r, g, b]: [u8; 3]) -> f32 {
    let c = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * c(r) + 0.7152 * c(g) + 0.0722 * c(b)
}

/// The contrast ratio of two colors (WCAG 2): 1 to 21.
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// A color with its lightness turned over (HSL's L to 1 − L), its hue
/// and saturation kept: dark blue to light blue, dark gray to light gray.
fn lightness_turned([r, g, b]: [u8; 3]) -> [u8; 3] {
    let (r, g, b) = (
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    );
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    let d = max - min;
    let (h, s) = if d == 0.0 {
        (0.0, 0.0)
    } else {
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        (h / 6.0, s)
    };
    let l = 1.0 - l;
    let to = |p: f32, q: f32, t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let (r, g, b) = if s == 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        (
            to(p, q, h + 1.0 / 3.0),
            to(p, q, h),
            to(p, q, h - 1.0 / 3.0),
        )
    };
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [byte(r), byte(g), byte(b)]
}

/// A document's text color as it reads on `background` (a color chosen
/// for a white page, drawn on a dark theme or on a highlight): kept when
/// it stands out enough (a contrast of 4.5, WCAG's for text); else its
/// lightness turned over, its hue kept, as a word processor's dark mode
/// does; else black or white, whichever reads.
pub fn legible(color: [u8; 3], background: [u8; 3]) -> [u8; 3] {
    const ENOUGH: f32 = 4.5;
    if contrast(color, background) >= ENOUGH {
        return color;
    }
    let turned = lightness_turned(color);
    if contrast(turned, background) >= ENOUGH {
        return turned;
    }
    let (black, white) = ([0, 0, 0], [0xff, 0xff, 0xff]);
    if contrast(black, background) >= contrast(white, background) {
        black
    } else {
        white
    }
}

/// The user's theme directory: `themes` in the settings directory.
pub fn user_dir() -> Option<std::path::PathBuf> {
    crate::settings::config_dir().map(|d| d.join("themes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn themes() {
        let light = ThemeColors::builtin(false);
        let dark = ThemeColors::builtin(true);
        assert!(!light.dark && dark.dark);
        assert_eq!(light.background, Color(0xffffffff));
        assert_eq!(dark.selection.alpha(), 0x4c);
        assert_eq!(Color::parse("#1f2328").unwrap().rgb(), (0x1f, 0x23, 0x28));
        assert!(Color::parse("1f2328").is_none() && Color::parse("#12345").is_none());
        // A user's file changes some colors and reports what is wrong.
        let mut t = light.clone();
        let issues = t.apply(
            "[colors]\ntodo = \"#ff0000\"\nlink = \"blue\"\nshade = \"#000000\"\n[syntax]\nstring = \"#00ff00\"\n",
        );
        assert_eq!(t.todo, Color(0xff0000ff));
        assert_eq!(t.syntax[1], Color(0x00ff00ff));
        assert_eq!(t.link, light.link);
        assert_eq!(issues.len(), 2, "{issues:?}");
        assert!(wants_dark("system", true) && !wants_dark("light", true));
        // A workbook's automatic (black) text reads on the dark theme, white on the light one.
        let (black, white, red) = (
            Some([0, 0, 0]),
            Some([0xff, 0xff, 0xff]),
            Some([0xc0, 0, 0]),
        );
        assert_eq!(cell_text_color(black, false, true), None);
        // A document's dark text on the dark theme: its lightness turned
        // over, its hue kept; a color that reads, kept.
        let dark_bg = [0x1e, 0x1e, 0x1e];
        assert_eq!(super::legible([0, 0, 0], dark_bg), [0xff, 0xff, 0xff]);
        assert_eq!(
            super::legible([0x40, 0x40, 0x40], dark_bg),
            [0xbf, 0xbf, 0xbf]
        );
        let blue = super::legible([0x2f, 0x54, 0x96], dark_bg);
        assert!(
            blue[2] > blue[0] && super::contrast(blue, dark_bg) >= 4.5,
            "{blue:?}"
        );
        assert_eq!(super::legible([0xff, 0xc0, 0], dark_bg), [0xff, 0xc0, 0]);
        // White text on a white page, and dark text on a yellow highlight.
        assert_eq!(
            super::legible([0xff, 0xff, 0xff], [0xff, 0xff, 0xff]),
            [0, 0, 0]
        );
        assert_eq!(super::legible([0, 0, 0], [0xff, 0xff, 0]), [0, 0, 0]);
        assert_eq!(cell_text_color(black, false, false), black);
        assert_eq!(cell_text_color(black, true, true), black);
        assert_eq!(cell_text_color(white, false, false), None);
        assert_eq!(cell_text_color(white, false, true), white);
        assert_eq!(cell_text_color(red, false, true), red);
        assert_eq!(cell_text_color(None, false, true), None);
        assert_eq!(Color(0xff000080).over(Color(0x0000ffff)), Color(0x80007fff));
        let dir = std::env::temp_dir().join(format!("kalem-themes-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("dark.toml"), "[colors]\nmark = \"#11223344\"\n").unwrap();
        assert_eq!(ThemeColors::load(true, Some(&dir)).mark, Color(0x11223344));
        assert_eq!(ThemeColors::load(false, Some(&dir)), light);
        let _ = std::fs::remove_dir_all(dir);
    }
}
