//! Kalem's formatting in exports (design §3.7): spans written as
//! `@@kalem:font="Georgia" size=14 color=#c00000 bg=#fff2a8@@` …
//! `@@kalem:end@@`, paragraphs aligned with `#+ATTR_KALEM: :align right`,
//! and a document's defaults in `#+KALEM: font="Georgia" size=12
//! spacing=1.5`. Emacs leaves them out of its exports; Kalem's back-ends
//! write them as the format allows.

/// Text colors by name, as the editor offers them.
pub const COLORS: &[(&str, &str)] = &[
    ("black", "#000000"),
    ("gray", "#7f7f7f"),
    ("red", "#c00000"),
    ("orange", "#e36c09"),
    ("gold", "#bf8f00"),
    ("green", "#00883a"),
    ("teal", "#1f8a8a"),
    ("blue", "#1f5fbf"),
    ("purple", "#7030a0"),
    ("brown", "#843c0c"),
];

/// Highlight colors by name, as the editor offers them.
pub const HIGHLIGHTS: &[(&str, &str)] = &[
    ("yellow", "#fff2a8"),
    ("lime", "#d8f5b0"),
    ("cyan", "#c5eef5"),
    ("pink", "#fcd3e6"),
    ("lavender", "#e2d8f5"),
    ("peach", "#fde0c5"),
    ("silver", "#e3e3e3"),
];

/// A span's formatting, as written: a font family, a size in points, and
/// colors as `#rrggbb`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Format {
    /// The font family.
    pub font: Option<String>,
    /// The size in points (`14`, `10.5`).
    pub size: Option<String>,
    /// The text color.
    pub color: Option<String>,
    /// The highlight color.
    pub background: Option<String>,
    /// The line spacing (`#+KALEM:` only), in lines.
    pub spacing: Option<String>,
}

/// `#rrggbb` for a color written as `#rrggbb`, `#rgb` or a name of
/// [`COLORS`] and [`HIGHLIGHTS`].
pub fn color(s: &str) -> Option<String> {
    let s = s.trim();
    if let Some(h) = s.strip_prefix('#') {
        let hex = |t: &str| t.chars().all(|c| c.is_ascii_hexdigit());
        return match h.len() {
            6 if hex(h) => Some(format!("#{}", h.to_ascii_lowercase())),
            3 if hex(h) => Some(h.chars().flat_map(|c| [c, c]).fold(
                "#".to_string(),
                |mut a, c| {
                    a.push(c.to_ascii_lowercase());
                    a
                },
            )),
            _ => None,
        };
    }
    COLORS
        .iter()
        .chain(HIGHLIGHTS)
        .find(|(n, _)| n.eq_ignore_ascii_case(s))
        .map(|(_, h)| h.to_string())
}

/// A number of points or lines as written (`14`, `10.5`, `14pt`), if it
/// is one.
fn amount(s: &str, max: f64) -> Option<String> {
    let t = s.trim().trim_end_matches("pt");
    let v: f64 = t.parse().ok()?;
    (v > 0.0 && v <= max).then(|| t.to_string())
}

impl Format {
    /// The format of a snippet value; `None` for `end`. Unknown keys and
    /// values that do not read are left out.
    pub fn parse(value: &str) -> Option<Format> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("end") || value == "/" {
            return None;
        }
        let mut f = Format::default();
        let mut rest = value;
        while let Some(eq) = rest.find('=') {
            let key = rest[..eq].trim();
            let after = rest[eq + 1..].trim_start();
            let (val, next) = if let Some(q) = after.strip_prefix('"') {
                match q.find('"') {
                    Some(e) => (&q[..e], &q[e + 1..]),
                    None => (q, ""),
                }
            } else {
                let e = after.find(char::is_whitespace).unwrap_or(after.len());
                (&after[..e], &after[e..])
            };
            match key.to_ascii_lowercase().as_str() {
                "font" if !val.trim().is_empty() => f.font = Some(val.trim().to_string()),
                "size" => f.size = amount(val, 1638.0),
                "color" => f.color = color(val),
                "bg" | "highlight" => f.background = color(val),
                "spacing" => f.spacing = amount(val, 5.0),
                _ => {}
            }
            rest = next;
        }
        Some(f)
    }

    /// As CSS declarations: `font-family: "Georgia"; font-size: 14pt`.
    pub fn css(&self) -> String {
        let mut out = Vec::new();
        if let Some(f) = &self.font {
            out.push(format!(
                "font-family: \"{}\"",
                f.replace(['"', '<', '>'], "")
            ));
        }
        if let Some(s) = &self.size {
            out.push(format!("font-size: {s}pt"));
        }
        if let Some(c) = &self.color {
            out.push(format!("color: {c}"));
        }
        if let Some(c) = &self.background {
            out.push(format!("background-color: {c}"));
        }
        if let Some(s) = &self.spacing {
            out.push(format!("line-height: {s}"));
        }
        out.join("; ")
    }
}

/// The document's defaults from its `#+KALEM:` keywords (the last one
/// wins, key by key).
pub fn defaults(keywords: &[(String, String)]) -> Format {
    let mut d = Format::default();
    for (k, v) in keywords {
        if !k.eq_ignore_ascii_case("KALEM") {
            continue;
        }
        let Some(n) = Format::parse(v) else { continue };
        d.font = n.font.or(d.font);
        d.size = n.size.or(d.size);
        d.spacing = n.spacing.or(d.spacing);
    }
    d
}

/// What a span's end becomes until the container it is in is finished.
pub(crate) const END_MARK: char = '\u{E000}';

/// The opening tag of a span with `format`.
pub(crate) fn open_tag(format: &Format) -> String {
    let css = format.css();
    if css.is_empty() {
        "<span class=\"kalem-format\">".to_string()
    } else {
        format!(
            "<span class=\"kalem-format\" style=\"{}\">",
            css.replace('"', "&quot;")
        )
    }
}

/// `html` with each span end closing the innermost open span (an end
/// without one goes) and the spans still open closed at the end: a
/// paragraph, title or cell holds its spans, as in the editor.
pub(crate) fn finish(html: &str) -> String {
    if !html.contains("kalem-format") && !html.contains(END_MARK) {
        return html.to_string();
    }
    let open = "<span class=\"kalem-format\"";
    let mut out = String::with_capacity(html.len());
    let mut depth = 0usize;
    let mut rest = html;
    while let Some(c) = rest.chars().next() {
        if rest.starts_with(open) {
            depth += 1;
            out.push_str(open);
            rest = &rest[open.len()..];
            continue;
        }
        if c == END_MARK {
            if depth > 0 {
                depth -= 1;
                out.push_str("</span>");
            }
        } else {
            out.push(c);
        }
        rest = &rest[c.len_utf8()..];
    }
    // Before the blanks and line feeds that end the container.
    let body = out.trim_end().len();
    let tail = out.split_off(body);
    for _ in 0..depth {
        out.push_str("</span>");
    }
    out.push_str(&tail);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let f =
            Format::parse(r#"font="Times New Roman" size=10.5 color=red bg=#FFF other=1"#).unwrap();
        assert_eq!(
            f.css(),
            "font-family: \"Times New Roman\"; font-size: 10.5pt; color: #c00000; background-color: #ffffff"
        );
        assert_eq!(Format::parse("end"), None);
        assert_eq!(Format::parse("size=big").unwrap(), Format::default());
        let d = defaults(&[
            ("KALEM".into(), "font=Georgia size=12".into()),
            ("KALEM".into(), "spacing=1.5".into()),
        ]);
        assert_eq!(
            d.css(),
            "font-family: \"Georgia\"; font-size: 12pt; line-height: 1.5"
        );
    }

    #[test]
    fn finishing() {
        let o = open_tag(&Format::parse("color=red").unwrap());
        let e = END_MARK;
        assert_eq!(
            finish(&format!("a {o}b {o}c{e} d{e} e{e}")),
            format!("a {o}b {o}c</span> d</span> e")
        );
        assert_eq!(finish(&format!("{o}open")), format!("{o}open</span>"));
        assert_eq!(finish(&format!("{o}open\n")), format!("{o}open</span>\n"));
        assert_eq!(finish("plain"), "plain");
    }
}
