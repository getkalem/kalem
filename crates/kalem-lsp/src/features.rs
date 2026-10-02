//! The protocol's answers read into plain values with byte ranges: what
//! the editor shows (hover, diagnostics, completion items, locations) and
//! the edits it applies (formatting, completion's text edits).

use std::ops::Range;
use std::path::PathBuf;

use serde_json::Value;

use crate::position::{Encoding, Position, byte_range};

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// An error.
    Error,
    /// A warning.
    Warning,
    /// Information.
    Information,
    /// A hint.
    Hint,
}

/// A diagnostic on bytes of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The bytes.
    pub range: Range<usize>,
    /// How serious.
    pub severity: Severity,
    /// The message.
    pub message: String,
    /// Who says so: `Elixir`, `Credo`, `dialyzer`.
    pub source: Option<String>,
    /// Its code, when it has one.
    pub code: Option<String>,
}

/// Reads published diagnostics against the document's text.
pub fn diagnostics(text: &str, list: &[Value], enc: Encoding) -> Vec<Diagnostic> {
    let mut out: Vec<Diagnostic> = list
        .iter()
        .filter_map(|d| {
            Some(Diagnostic {
                range: byte_range(text, d.get("range")?, enc)?,
                severity: match d["severity"].as_u64() {
                    Some(2) => Severity::Warning,
                    Some(3) => Severity::Information,
                    Some(4) => Severity::Hint,
                    _ => Severity::Error,
                },
                message: d["message"].as_str()?.to_string(),
                source: d["source"].as_str().map(str::to_string),
                code: match &d["code"] {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                },
            })
        })
        .collect();
    out.sort_by_key(|d| (d.range.start, d.severity));
    out
}

/// The text of a hover answer: Markdown or plain text, the parts of a
/// list joined by blank lines; `None` when there is nothing to show.
pub fn hover_text(answer: &Value) -> Option<String> {
    fn part(v: &Value) -> Option<String> {
        match v {
            Value::String(s) => Some(s.clone()),
            Value::Object(o) => {
                let value = o.get("value")?.as_str()?;
                // A `MarkedString` with a language is code.
                match o.get("language").and_then(Value::as_str) {
                    Some(lang) => Some(format!("```{lang}\n{value}\n```")),
                    None => Some(value.to_string()),
                }
            }
            Value::Array(a) => {
                let parts: Vec<String> = a.iter().filter_map(part).collect();
                (!parts.is_empty()).then(|| parts.join("\n\n"))
            }
            _ => None,
        }
    }
    let text = part(answer.get("contents")?)?;
    let text = text.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// A place in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The file.
    pub path: PathBuf,
    /// The start of the range.
    pub start: Position,
    /// Its end.
    pub end: Position,
}

/// The locations of a definition or references answer: a location, a
/// list of locations, or a list of location links.
pub fn locations(answer: &Value) -> Vec<Location> {
    fn one(v: &Value) -> Option<Location> {
        let uri = v.get("targetUri").or_else(|| v.get("uri"))?.as_str()?;
        let range = v.get("targetSelectionRange").or_else(|| v.get("range"))?;
        Some(Location {
            path: crate::uri::to_path(uri)?,
            start: Position::from_json(range.get("start")?)?,
            end: Position::from_json(range.get("end")?)?,
        })
    }
    match answer {
        Value::Array(a) => a.iter().filter_map(one).collect(),
        Value::Object(_) => one(answer).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// A completion item.
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionItem {
    /// What the list shows.
    pub label: String,
    /// The detail beside it (a signature, a module).
    pub detail: Option<String>,
    /// The protocol's kind (3 function, 9 module…).
    pub kind: Option<u8>,
    /// The documentation, when given at once.
    pub documentation: Option<String>,
    /// The text inserted, when there is no edit.
    pub insert_text: String,
    /// The replacement, with the range the server gave.
    pub edit: Option<(Value, String)>,
    /// The server's sort key.
    pub sort_text: String,
    /// The server's filter key.
    pub filter_text: String,
    /// The item as sent, for `completionItem/resolve`.
    pub raw: Value,
}

/// Removes snippet syntax (`$1`, `${1:x}`, `$0`), keeping placeholders'
/// text, until the snippet engine of T3.8.3 exists.
pub fn strip_snippet(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            '$' => match chars.peek() {
                Some(d) if d.is_ascii_digit() => {
                    while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                        chars.next();
                    }
                }
                Some('{') => {
                    chars.next();
                    while chars
                        .peek()
                        .is_some_and(|d| d.is_ascii_alphanumeric() || *d == '_')
                    {
                        chars.next();
                    }
                    let mut depth = 1;
                    let mut inner = String::new();
                    match chars.next() {
                        Some(':') => {
                            for c in chars.by_ref() {
                                match c {
                                    '{' => depth += 1,
                                    '}' => {
                                        depth -= 1;
                                        if depth == 0 {
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                                inner.push(c);
                            }
                            out.push_str(&strip_snippet(&inner));
                        }
                        Some('|') => {
                            // A choice: its first option.
                            let rest: String = chars.by_ref().take_while(|c| *c != '}').collect();
                            out.push_str(
                                rest.trim_end_matches('|').split(',').next().unwrap_or(""),
                            );
                        }
                        _ => {}
                    }
                }
                _ => out.push('$'),
            },
            c => out.push(c),
        }
    }
    out
}

fn doc_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Object(o) => o.get("value")?.as_str().map(str::to_string),
        _ => None,
    }
}

/// The items of a completion answer and whether the list is incomplete
/// (to be asked again as the word grows).
pub fn completion_items(answer: &Value) -> (Vec<CompletionItem>, bool) {
    let (list, incomplete) = match answer {
        Value::Array(a) => (a.as_slice(), false),
        Value::Object(o) => (
            o.get("items")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
            o.get("isIncomplete")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
        _ => (&[][..], false),
    };
    let items = list
        .iter()
        .filter_map(|i| {
            let label = i["label"].as_str()?.to_string();
            let snippet = i["insertTextFormat"].as_u64() == Some(2);
            let fix = |s: &str| {
                if snippet {
                    strip_snippet(s)
                } else {
                    s.to_string()
                }
            };
            let edit = i.get("textEdit").and_then(|e| {
                let range = e.get("range").or_else(|| e.get("replace"))?.clone();
                Some((range, fix(e["newText"].as_str()?)))
            });
            Some(CompletionItem {
                insert_text: fix(i["insertText"].as_str().unwrap_or(&label)),
                detail: i["detail"].as_str().map(str::to_string),
                kind: i["kind"].as_u64().map(|k| k as u8),
                documentation: i.get("documentation").and_then(doc_text),
                sort_text: i["sortText"].as_str().unwrap_or(&label).to_string(),
                filter_text: i["filterText"].as_str().unwrap_or(&label).to_string(),
                edit,
                label,
                raw: i.clone(),
            })
        })
        .collect();
    (items, incomplete)
}

/// The text edits of a formatting answer, as byte ranges of `text`,
/// sorted and checked not to overlap.
pub fn text_edits(
    text: &str,
    answer: &Value,
    enc: Encoding,
) -> Option<Vec<(Range<usize>, String)>> {
    let list = answer.as_array()?;
    let mut edits: Vec<(Range<usize>, String)> = list
        .iter()
        .filter_map(|e| {
            Some((
                byte_range(text, e.get("range")?, enc)?,
                e["newText"].as_str()?.to_string(),
            ))
        })
        .collect();
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    if edits.windows(2).any(|w| w[0].0.end > w[1].0.start) {
        return None;
    }
    Some(edits)
}

/// `text` with sorted, non-overlapping `edits` applied.
pub fn apply(text: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (r, t) in edits {
        out.push_str(&text[at..r.start]);
        out.push_str(t);
        at = r.end;
    }
    out.push_str(&text[at..]);
    out
}

/// The symbols of a document symbol answer, flattened in text order:
/// (depth, name, kind, start position).
pub fn symbols(answer: &Value) -> Vec<(usize, String, u8, Position)> {
    fn walk(v: &Value, depth: usize, out: &mut Vec<(usize, String, u8, Position)>) {
        for s in v.as_array().map_or(&[][..], Vec::as_slice) {
            let Some(name) = s["name"].as_str() else {
                continue;
            };
            let range = s
                .get("selectionRange")
                .or_else(|| s.get("location").and_then(|l| l.get("range")))
                .or_else(|| s.get("range"));
            let Some(start) = range.and_then(|r| Position::from_json(&r["start"])) else {
                continue;
            };
            out.push((
                depth,
                name.to_string(),
                s["kind"].as_u64().unwrap_or(0) as u8,
                start,
            ));
            if let Some(c) = s.get("children") {
                walk(c, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(answer, 0, &mut out);
    out.sort_by_key(|s| s.3);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hover_forms() {
        assert_eq!(
            hover_text(&json!({"contents": {"kind": "markdown", "value": " *x* "}})).unwrap(),
            "*x*"
        );
        assert_eq!(
            hover_text(&json!({"contents": [{"language": "elixir", "value": "def f"}, "doc"]}))
                .unwrap(),
            "```elixir\ndef f\n```\n\ndoc"
        );
        assert!(hover_text(&json!({"contents": ""})).is_none());
    }

    #[test]
    fn snippets_stripped() {
        assert_eq!(strip_snippet("foo(${1:a}, ${2:b})$0"), "foo(a, b)");
        assert_eq!(strip_snippet("x ${1|one,two|} \\$y $"), "x one $y $");
        assert_eq!(strip_snippet("${1:a ${2:b}}"), "a b");
    }

    #[test]
    fn edits_applied() {
        let text = "a  =  1\n";
        let ans = json!([{"range": {"start": {"line": 0, "character": 1}, "end": {"line": 0, "character": 3}}, "newText": " "},
                         {"range": {"start": {"line": 0, "character": 4}, "end": {"line": 0, "character": 6}}, "newText": " "}]);
        let e = text_edits(text, &ans, Encoding::Utf16).unwrap();
        assert_eq!(apply(text, &e), "a = 1\n");
    }

    #[test]
    fn location_forms() {
        let l = locations(
            &json!([{"targetUri": "file:///a.ex", "targetRange": {}, "targetSelectionRange": {"start": {"line": 1, "character": 2}, "end": {"line": 1, "character": 3}}}]),
        );
        assert_eq!(l[0].path, PathBuf::from("/a.ex"));
        assert_eq!(l[0].start.line, 1);
        let l = locations(
            &json!({"uri": "file:///b.ex", "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}}),
        );
        assert_eq!(l.len(), 1);
    }
}
