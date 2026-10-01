//! A Markdown document's front matter as a form (T2.7c.9): its fields
//! listed, each edited on its own, as Obsidian's properties are. YAML
//! (`---`) with `key: value` lines and lists, written `[a, b]` or as
//! `- item` lines, and TOML (`+++`) with `key = value` lines. Only the
//! line or lines of the field changed are rewritten; the rest stays as
//! written.

use std::ops::Range;

/// A field of the front matter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The key.
    pub key: String,
    /// The value as one line: a list's items joined by `, `.
    pub value: String,
    /// Whether it is a list.
    pub list: bool,
    /// Its lines, with their line feeds.
    pub lines: Range<usize>,
}

/// The front matter of `text`: its fields, where its closing fence
/// starts, and whether it is TOML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatter {
    /// The fields in order.
    pub fields: Vec<Field>,
    /// The start of the closing fence's line.
    pub end: usize,
    /// `+++` and `key = value`.
    pub toml: bool,
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    for q in ['"', '\''] {
        if v.len() >= 2 && v.starts_with(q) && v.ends_with(q) {
            return v[1..v.len() - 1].to_string();
        }
    }
    v.to_string()
}

fn flow_list(v: &str) -> Option<Vec<String>> {
    let v = v.trim();
    let inner = v.strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .map(unquote)
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

/// Reads the front matter at the start of `text`.
pub fn read(text: &str) -> Option<FrontMatter> {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let skip = text.len() - body.len();
    let first = body.split_inclusive('\n').next()?;
    let toml = match first.trim_end() {
        "---" => false,
        "+++" => true,
        _ => return None,
    };
    let fence = if toml { "+++" } else { "---" };
    let mut at = skip + first.len();
    let mut fields: Vec<Field> = Vec::new();
    for line in text[at..].split_inclusive('\n') {
        let start = at;
        at += line.len();
        let t = line.trim_end();
        if t == fence || (!toml && t == "...") {
            return Some(FrontMatter {
                fields,
                end: start,
                toml,
            });
        }
        // A `- item` line of the list above.
        if !toml
            && let Some(item) = t
                .trim_start()
                .strip_prefix("- ")
                .or_else(|| (t.trim() == "-").then_some(""))
            && t.starts_with([' ', '-'])
            && let Some(f) = fields.last_mut()
            && (f.list || f.value.is_empty())
        {
            f.list = true;
            let item = unquote(item);
            if !f.value.is_empty() {
                f.value.push_str(", ");
            }
            f.value.push_str(&item);
            f.lines.end = at;
            continue;
        }
        if t.starts_with([' ', '\t', '#']) || t.is_empty() {
            if let Some(f) = fields.last_mut().filter(|_| t.starts_with([' ', '\t'])) {
                f.lines.end = at;
            }
            continue;
        }
        let sep = if toml { '=' } else { ':' };
        let Some((k, v)) = t.split_once(sep) else {
            continue;
        };
        let (value, list) = match flow_list(v) {
            Some(items) => (items.join(", "), true),
            None => (unquote(v), false),
        };
        fields.push(Field {
            key: k.trim().to_string(),
            value,
            list,
            lines: start..at,
        });
    }
    None
}

/// `v` as a YAML or TOML scalar: quoted when it needs it.
fn scalar(v: &str, toml: bool) -> String {
    let plain = !v.is_empty()
        && !v.starts_with([
            ' ', '[', '{', '"', '\'', '-', '!', '&', '*', '#', '|', '>', '%', '@', '`',
        ])
        && !v.ends_with(' ')
        && !v.contains(": ")
        && !v.contains(" #")
        && !v.ends_with(':');
    let number = v.parse::<f64>().is_ok() || matches!(v, "true" | "false");
    if (toml && !number) || (!plain && !number) {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_string()
    }
}

/// The line of a field `key` with `value` (a list when `list`, its items
/// separated by commas).
fn line_of(key: &str, value: &str, list: bool, toml: bool) -> String {
    let v = if list {
        let items: Vec<String> = value
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| scalar(s, toml))
            .collect();
        format!("[{}]", items.join(", "))
    } else {
        scalar(value.trim(), toml)
    };
    if toml {
        format!("{key} = {v}\n")
    } else {
        format!("{key}: {v}\n")
    }
}

/// Sets field `key` of `text`'s front matter to `value` (a field that is
/// a list stays one, its items separated by commas), adding the field
/// before the closing fence, or the front matter at the top.
pub fn set(text: &str, key: &str, value: &str) -> Option<org_edit::Transaction> {
    let key = key.trim();
    if key.is_empty() {
        return None;
    }
    let mut tx = org_edit::Transaction::new("Set Field");
    match read(text) {
        Some(fm) => match fm.fields.iter().find(|f| f.key == key) {
            Some(f) => {
                let line = line_of(key, value, f.list, fm.toml);
                if text[f.lines.clone()] == line {
                    return None;
                }
                tx.replace(f.lines.clone(), line).ok()?;
            }
            None => {
                tx.replace(fm.end..fm.end, line_of(key, value, false, fm.toml))
                    .ok()?;
            }
        },
        None => {
            let line = line_of(key, value, false, false);
            tx.replace(0..0, format!("---\n{line}---\n")).ok()?;
        }
    }
    Some(tx)
}

/// Deletes field `key`.
pub fn delete(text: &str, key: &str) -> Option<org_edit::Transaction> {
    let fm = read(text)?;
    let f = fm.fields.iter().find(|f| f.key == key)?;
    let mut tx = org_edit::Transaction::new("Delete Field");
    tx.replace(f.lines.clone(), "").ok()?;
    Some(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, tx: &org_edit::Transaction) -> String {
        let mut s = text.to_string();
        for e in tx.edits.iter().rev() {
            s.replace_range(e.range.clone(), &e.insert);
        }
        s
    }

    #[test]
    fn yaml_fields() {
        let t = "---\ntitle: \"My: notes\"\ntags:\n  - one\n  - two\ndraft: false\naliases: [a, b]\n---\n# Body\n";
        let fm = read(t).unwrap();
        let got: Vec<(&str, &str, bool)> = fm
            .fields
            .iter()
            .map(|f| (f.key.as_str(), f.value.as_str(), f.list))
            .collect();
        assert_eq!(
            got,
            [
                ("title", "My: notes", false),
                ("tags", "one, two", true),
                ("draft", "false", false),
                ("aliases", "a, b", true),
            ]
        );
        // A list written as lines becomes one line; the rest stays.
        let s = apply(t, &set(t, "tags", "one, two, three").unwrap());
        assert_eq!(
            s,
            "---\ntitle: \"My: notes\"\ntags: [one, two, three]\ndraft: false\naliases: [a, b]\n---\n# Body\n"
        );
        let s = apply(&s, &set(&s, "title", "Plain").unwrap());
        assert!(s.contains("\ntitle: Plain\n"));
        let s = apply(&s, &set(&s, "date", "2026-10-01").unwrap());
        assert!(s.contains("aliases: [a, b]\ndate: 2026-10-01\n---\n"));
        let s = apply(&s, &delete(&s, "draft").unwrap());
        assert!(!s.contains("draft"));
        assert!(set(&s, "title", "Plain").is_none(), "unchanged");
        assert!(s.ends_with("---\n# Body\n"));
    }

    #[test]
    fn toml_and_none() {
        let t = "+++\ntitle = \"T\"\ntags = [\"x\"]\n+++\ntext\n";
        let fm = read(t).unwrap();
        assert!(fm.toml);
        assert_eq!(fm.fields[1].value, "x");
        let s = apply(t, &set(t, "title", "New").unwrap());
        assert!(s.starts_with("+++\ntitle = \"New\"\n"));
        let plain = "# Hi\n";
        assert!(read(plain).is_none());
        let s = apply(plain, &set(plain, "title", "Hi").unwrap());
        assert_eq!(s, "---\ntitle: Hi\n---\n# Hi\n");
    }
}
