//! Document modes (design §2.6): which editor a file opens in.

use std::path::Path;

/// How a document is edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentMode {
    /// The Org editor.
    Org,
    /// The Markdown editor.
    Markdown,
    /// The CSV grid; the delimiter is detected later.
    Csv,
    /// Plain text, with a language for highlighting when one is known
    /// (an extension or an interpreter name).
    Text {
        /// The language hint, such as `rs` or `python`.
        language: Option<String>,
    },
    /// Not text: not opened for editing.
    Binary,
    /// A folder in the file manager (`crate::dired`), or the projects.
    Directory,
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
        // `.klm`: a Kalem document, Org with Kalem's additions (§3.7).
        "org" | "org_archive" | "klm" => DocumentMode::Org,
        "md" | "markdown" | "mdown" | "mkd" | "gfm" => DocumentMode::Markdown,
        "csv" | "tsv" | "tab" => DocumentMode::Csv,
        _ => return None,
    })
}

/// Whether the start of a file looks binary: a NUL byte, or bytes that are
/// not UTF-8 with many control characters among them (text in a legacy
/// encoding such as Windows-1254 has none).
pub fn looks_binary(sample: &[u8]) -> bool {
    if sample.contains(&0) {
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
            DocumentMode::Text { .. } => "text",
            DocumentMode::Binary => "binary",
            DocumentMode::Directory => "directory",
        }
    }

    /// The mode a user names: `org`, `markdown`, `csv` or `text` (also
    /// `plain`), or a language, which is text with that highlighting.
    pub fn from_name(name: &str) -> Option<DocumentMode> {
        let name = name.trim().to_lowercase();
        if name.is_empty() || name == "binary" {
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
            DocumentMode::Text { language: Some(l) } => l.clone(),
            DocumentMode::Text { language: None } | DocumentMode::Binary => {
                crate::l10n::tr("mode-text")
            }
            DocumentMode::Directory => crate::l10n::tr("mode-directory"),
        }
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
        if let Some(m) = mode_line(first) {
            return by_name(m.trim_end_matches("-mode"))
                .unwrap_or(DocumentMode::Text { language: Some(m) });
        }
        let ext = path
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .map(str::to_lowercase);
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
    fn detection() {
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
}
