//! Document modes (design §2.6): which editor a file opens in.

use std::path::Path;

/// How a document is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentMode {
    /// Org files, shown and edited as documents.
    Org,
    /// The Markdown editor.
    Markdown,
    /// The CSV grid; the delimiter is detected later.
    Csv,
    /// The LaTeX editor (design §9.5): the document rendered, the file
    /// kept as it is.
    Latex,
    /// Plain text, with a language for highlighting when one is known
    /// (an extension or an interpreter name).
    Text {
        /// The language hint, such as `rs` or `python`.
        language: Option<String>,
    },
    /// Not text: not opened for editing.
    Binary,
    /// A file that is not text, opened by a viewer plugin
    /// (`crate::viewer`, design §11.13): no text, the plugin's units.
    Viewer,
    /// A folder in the file manager (`crate::dired`), or the projects.
    Directory,
}

/// The line Emacs reads a `-*-` line from: the first, or the second
/// when the first is a `#!` line.
fn mode_line_of(text: &str) -> &str {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if first.starts_with("#!") {
        let second = lines.next().unwrap_or("");
        if second.contains("-*-") {
            return second;
        }
    }
    first
}

/// A variable of the `-*-` line (`-*- mode: org; coding: utf-8 -*-`).
pub fn mode_line_variable(text: &str, name: &str) -> Option<String> {
    let line = mode_line_of(text);
    let start = line.find("-*-")? + 3;
    let end = start + line[start..].find("-*-")?;
    line[start..end].split(';').find_map(|part| {
        let (k, v) = part.split_once(':')?;
        k.trim()
            .eq_ignore_ascii_case(name)
            .then(|| v.trim().to_string())
    })
}

/// A variable of the `Local Variables:` block at the end of a file (its
/// last 3,000 characters, as Emacs looks): each line between
/// `PREFIX Local Variables: SUFFIX` and `PREFIX End: SUFFIX` is
/// `PREFIX NAME: VALUE SUFFIX`.
pub fn local_variable(text: &str, name: &str) -> Option<String> {
    let mut from = text.len().saturating_sub(3000);
    while !text.is_char_boundary(from) {
        from += 1;
    }
    let tail = &text[from..];
    let at = tail.rfind("Local Variables:")?;
    let line_start = tail[..at].rfind('\n').map_or(0, |i| i + 1);
    let prefix = &tail[line_start..at];
    let line_end = tail[at..].find('\n').map_or(tail.len(), |i| at + i);
    let suffix = tail[at + "Local Variables:".len()..line_end].trim_end_matches('\r');
    let body = tail.get(line_end + 1..)?;
    for line in body.lines() {
        let line = line.trim_end_matches('\r');
        let line = line.strip_prefix(prefix).unwrap_or(line);
        let line = line.strip_suffix(suffix).unwrap_or(line).trim();
        if line == "End:" {
            break;
        }
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case(name)
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

/// Whether text in a `.txt` file is tab-separated values (Excel's "Unicode
/// Text" export): at least two lines, each with the same number of tabs,
/// at least one, and some field before the first tab.
fn looks_like_tsv(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim_end_matches('\r'))
        .filter(|l| !l.is_empty())
        .take(20)
        .collect();
    if lines.len() < 2 {
        return false;
    }
    let tabs = lines[0].matches('\t').count();
    tabs >= 1
        && lines.iter().all(|l| l.matches('\t').count() == tabs)
        && lines.iter().filter(|l| !l.starts_with('\t')).count() * 2 > lines.len()
}

/// `-*- mode: NAME -*-` or `-*- NAME -*-` on the first line.
fn mode_line(first_line: &str) -> Option<String> {
    let start = first_line.find("-*-")? + 3;
    let end = start + first_line[start..].find("-*-")?;
    let inner = first_line[start..end].trim();
    let value = inner
        .split(';')
        .find_map(|part| {
            let (k, v) = part.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("mode")
                .then(|| v.trim().to_string())
        })
        .or_else(|| (!inner.contains(':')).then(|| inner.to_string()))?;
    (!value.is_empty()).then(|| value.to_lowercase())
}

fn by_name(name: &str) -> Option<DocumentMode> {
    Some(match name {
        // `.klm`: the Kalem format's extension, read as strict Org until
        // its parser exists (T2.13.3).
        "org" | "org_archive" | "klm" => DocumentMode::Org,
        "md" | "markdown" | "mdown" | "mkd" | "gfm" => DocumentMode::Markdown,
        "csv" | "tsv" | "tab" => DocumentMode::Csv,
        "tex" | "latex" | "ltx" => DocumentMode::Latex,
        _ => return None,
    })
}

/// Whether the start of a file looks binary: a NUL byte, or bytes that are
/// not UTF-8 with many control characters among them (text in a legacy
/// encoding such as Windows-1254 has none). A PDF file is binary even
/// when its streams are not compressed and its bytes read as text (ISO
/// 32000-2, 7.5.2: byte offsets make it so).
pub fn looks_binary(sample: &[u8]) -> bool {
    if sample.contains(&0) || sample.starts_with(b"%PDF-") {
        return true;
    }
    match std::str::from_utf8(sample) {
        Ok(_) => false,
        Err(e) if e.error_len().is_none() => false,
        Err(_) => {
            let control = sample
                .iter()
                .filter(|b| matches!(b, 0x01..=0x08 | 0x0B | 0x0E..=0x1A | 0x1C..=0x1F | 0x7F))
                .count();
            control * 20 > sample.len()
        }
    }
}

impl DocumentMode {
    /// The mode's name in settings and when-clauses (`editorMode`).
    pub fn name(&self) -> &'static str {
        match self {
            DocumentMode::Org => "org",
            DocumentMode::Markdown => "markdown",
            DocumentMode::Csv => "csv",
            DocumentMode::Latex => "latex",
            DocumentMode::Text { .. } => "text",
            DocumentMode::Binary => "binary",
            DocumentMode::Viewer => "viewer",
            DocumentMode::Directory => "directory",
        }
    }

    /// The name `files.modes` keeps for the mode: [`DocumentMode::name`],
    /// or the language of a text file that has one.
    pub fn setting_name(&self) -> String {
        match self {
            DocumentMode::Text { language: Some(l) } => l.clone(),
            m => m.name().to_string(),
        }
    }

    /// The mode a user names: `org`, `markdown`, `csv`, `latex` or `text`
    /// (also `plain`), or a language, which is text with that
    /// highlighting.
    pub fn from_name(name: &str) -> Option<DocumentMode> {
        let name = name.trim().to_lowercase();
        if name.is_empty() || name == "binary" || name == "viewer" {
            return None;
        }
        Some(by_name(&name).unwrap_or(match name.as_str() {
            "text" | "plain" => DocumentMode::Text { language: None },
            _ => DocumentMode::Text {
                language: Some(name),
            },
        }))
    }

    /// The mode's name for the status bar: `Org`, `Markdown`, `CSV`, the
    /// language of a text file, or `Text` (in the interface language).
    pub fn title(&self) -> String {
        match self {
            DocumentMode::Org => "Org".into(),
            DocumentMode::Markdown => "Markdown".into(),
            DocumentMode::Csv => "CSV".into(),
            DocumentMode::Latex => "LaTeX".into(),
            DocumentMode::Text { language: Some(l) } => l.clone(),
            DocumentMode::Text { language: None } | DocumentMode::Binary => {
                crate::l10n::tr("mode-text")
            }
            DocumentMode::Directory => crate::l10n::tr("mode-directory"),
            DocumentMode::Viewer => crate::l10n::tr("mode-viewer"),
        }
    }

    /// The mode for a whole decoded text: [`DocumentMode::detect`] on its
    /// first 8 KiB, and its `Local Variables:` block, which is at the end.
    pub fn detect_text(path: Option<&Path>, text: &str) -> DocumentMode {
        let mut cut = text.len().min(8192);
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        let head = &text[..cut];
        let mode = DocumentMode::detect(path, head.as_bytes());
        if mode == DocumentMode::Binary {
            return DocumentMode::detect(path, b"");
        }
        if cut < text.len()
            && mode_line(mode_line_of(head.strip_prefix('\u{feff}').unwrap_or(head))).is_none()
            && let Some(m) = local_variable(text, "mode").map(|m| m.to_lowercase())
            && !m.is_empty()
        {
            return by_name(m.trim_end_matches("-mode"))
                .unwrap_or(DocumentMode::Text { language: Some(m) });
        }
        mode
    }

    /// The mode for a file, from its start (the first few kilobytes are
    /// enough), in the order of §2.6: a mode line, the extension, a shebang
    /// line, plain text. An explicit choice by the user is applied by the
    /// caller before this.
    pub fn detect(path: Option<&Path>, start: &[u8]) -> DocumentMode {
        if looks_binary(start) {
            return DocumentMode::Binary;
        }
        let text = String::from_utf8_lossy(start);
        let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        let first = text.lines().next().unwrap_or("");
        // As Emacs's `set-auto-mode`: the `-*-` line (the second after a
        // `#!` line), then the `Local Variables:` block, then the file
        // name, then the interpreter.
        let named = mode_line(mode_line_of(text)).or_else(|| {
            local_variable(text, "mode")
                .map(|m| m.to_lowercase())
                .filter(|m| !m.is_empty())
        });
        if let Some(m) = named {
            return by_name(m.trim_end_matches("-mode"))
                .unwrap_or(DocumentMode::Text { language: Some(m) });
        }
        let ext = path
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map(str::to_lowercase);
        if ext.as_deref() == Some("txt") && looks_like_tsv(text) {
            return DocumentMode::Csv;
        }
        if let Some(e) = &ext {
            return by_name(e).unwrap_or_else(|| DocumentMode::Text {
                language: Some(e.clone()),
            });
        }
        if let Some(rest) = first.strip_prefix("#!") {
            // `#!/usr/bin/env python3` or `#!/bin/sh`.
            let mut words = rest.split_whitespace();
            let prog = words.next().unwrap_or("");
            let prog = if prog.ends_with("/env") {
                words.next().unwrap_or("")
            } else {
                prog
            };
            let name = prog.rsplit('/').next().unwrap_or("");
            let lang = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.');
            if !lang.is_empty() {
                return DocumentMode::Text {
                    language: Some(lang.to_string()),
                };
            }
        }
        DocumentMode::Text { language: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect(path: &str, text: &str) -> DocumentMode {
        DocumentMode::detect(Some(Path::new(path)), text.as_bytes())
    }

    #[test]
    fn setting_names_round_trip() {
        for m in [
            DocumentMode::Org,
            DocumentMode::Latex,
            DocumentMode::Csv,
            DocumentMode::Text { language: None },
            DocumentMode::Text {
                language: Some("python".into()),
            },
        ] {
            assert_eq!(DocumentMode::from_name(&m.setting_name()), Some(m));
        }
    }

    #[test]
    fn detection() {
        // A PDF whose bytes read as text is still not text.
        assert_eq!(
            detect("a.pdf", "%PDF-1.7\n1 0 obj << >> endobj"),
            DocumentMode::Binary
        );
        assert_eq!(detect("a.org", "* x"), DocumentMode::Org);
        assert_eq!(detect("README.md", "# x"), DocumentMode::Markdown);
        assert_eq!(detect("data.TSV", "a\tb"), DocumentMode::Csv);
        assert_eq!(
            detect("notes.txt", "-*- mode: org -*-\n* x"),
            DocumentMode::Org
        );
        assert_eq!(detect("x.txt", "# -*- org -*-"), DocumentMode::Org);
        assert_eq!(
            detect("main.rs", "fn main() {}"),
            DocumentMode::Text {
                language: Some("rs".into())
            }
        );
        assert_eq!(
            DocumentMode::detect(Some(Path::new("script")), b"#!/usr/bin/env python3\n"),
            DocumentMode::Text {
                language: Some("python".into())
            }
        );
        assert_eq!(
            DocumentMode::detect(None, b"\x00\x01"),
            DocumentMode::Binary
        );
        assert_eq!(
            DocumentMode::detect(None, "é".as_bytes()),
            DocumentMode::Text { language: None }
        );
        assert_eq!(DocumentMode::from_name("Org"), Some(DocumentMode::Org));
        assert_eq!(
            DocumentMode::from_name("plain"),
            Some(DocumentMode::Text { language: None })
        );
        assert_eq!(
            DocumentMode::from_name("python").map(|m| m.title()),
            Some("python".to_string())
        );
        assert_eq!(DocumentMode::Csv.name(), "csv");
    }

    #[test]
    fn file_variables() {
        // A `-*-` line on the second line after `#!`.
        assert_eq!(
            detect("run", "#!/bin/sh\n# -*- mode: org -*-\n"),
            DocumentMode::Org
        );
        // A `Local Variables:` block beats the file name.
        let text = "text\n\n# Local Variables:\n# fill-column: 70\n# mode: org\n# End:\n";
        assert_eq!(detect("notes.txt", text), DocumentMode::Org);
        assert_eq!(local_variable(text, "fill-column").as_deref(), Some("70"));
        let text = "x\n/* Local Variables: */\n/* mode: markdown */\n/* End: */\n";
        assert_eq!(detect("a.c", text), DocumentMode::Markdown);
        assert_eq!(
            mode_line_variable("-*- mode: org; coding: latin-1 -*-\n", "coding").as_deref(),
            Some("latin-1")
        );
        // Tab-separated text saved as `.txt`.
        assert_eq!(
            detect("export.txt", "Ad\tYaş\r\nAyşe\t30\r\n"),
            DocumentMode::Csv
        );
        assert_eq!(
            detect("notes.txt", "\tindented\n\tagain\n"),
            DocumentMode::Text {
                language: Some("txt".into())
            }
        );
    }
}
