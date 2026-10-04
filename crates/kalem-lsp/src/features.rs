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

/// The text of a signature help answer: the active signature's label,
/// then its active parameter's documentation, then its own; `None` when
/// there is none.
pub fn signature_text(answer: &Value) -> Option<String> {
    let sigs = answer.get("signatures")?.as_array()?;
    let active = answer["activeSignature"].as_u64().unwrap_or(0) as usize;
    let sig = sigs.get(active).or_else(|| sigs.first())?;
    let label = sig["label"].as_str()?;
    let mut out = format!("```\n{label}\n```");
    let param = sig["activeParameter"]
        .as_u64()
        .or_else(|| answer["activeParameter"].as_u64())
        .map(|i| i as usize);
    if let Some(p) = param.and_then(|i| sig["parameters"].as_array()?.get(i))
        && let Some(d) = p
            .get("documentation")
            .and_then(doc_text)
            .filter(|d| !d.trim().is_empty())
    {
        out.push_str("\n\n");
        out.push_str(d.trim());
    }
    if let Some(d) = sig
        .get("documentation")
        .and_then(doc_text)
        .filter(|d| !d.trim().is_empty())
    {
        out.push_str("\n\n");
        out.push_str(d.trim());
    }
    Some(out)
}

/// The text edits of a `WorkspaceEdit`, by document URI, in the order
/// given: its `documentChanges` (text edits; creating, renaming and
/// deleting files are not supported and make the edit refused, `None`),
/// else its `changes`.
pub fn workspace_edit(edit: &Value) -> Option<Vec<(String, Vec<Value>)>> {
    if let Some(changes) = edit.get("documentChanges").and_then(Value::as_array) {
        let mut out = Vec::new();
        for c in changes {
            if c.get("kind").is_some() {
                return None;
            }
            let uri = c["textDocument"]["uri"].as_str()?.to_string();
            let edits = c["edits"].as_array().cloned().unwrap_or_default();
            out.push((uri, edits));
        }
        return Some(out);
    }
    let map = edit.get("changes").and_then(Value::as_object);
    Some(
        map.map(|m| {
            m.iter()
                .map(|(uri, edits)| (uri.clone(), edits.as_array().cloned().unwrap_or_default()))
                .collect()
        })
        .unwrap_or_default(),
    )
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
    /// Where the cursor goes in `insert_text` (a snippet's first place
    /// to fill); the end when `None`.
    pub cursor: Option<usize>,
    /// The replacement, with the range the server gave, and where the
    /// cursor goes in it.
    pub edit: Option<(Value, String, Option<usize>)>,
    /// Edits elsewhere that come with the item (an alias or an import
    /// added at the top), with the ranges the server gave.
    pub additional: Vec<(Value, String)>,
    /// The server's sort key.
    pub sort_text: String,
    /// The server's filter key.
    pub filter_text: String,
    /// The item as sent, for `completionItem/resolve`: kept only when
    /// asked ([`completion_items`]' `keep_raw`), null otherwise.
    pub raw: Value,
}

/// A snippet as plain text to insert, until the snippet engine of T3.8.3
/// exists: the places to fill left empty, brackets left with only
/// separators in them emptied (`all?(${1:enumerable})` is `all?()`,
/// `reduce(${1:e}, ${2:acc})` is `reduce()`), and the cursor at the first
/// place (`$1`, else `$0`, else the end).
pub fn snippet_insert(s: &str) -> (String, usize) {
    // The cursor is a marker character in the text until the end.
    const MARK: char = '\u{1}';
    let mut out = String::new();
    let mut first: Option<(u32, usize)> = None;
    let mut chars = s.chars().peekable();
    let place = |n: u32, at: usize, first: &mut Option<(u32, usize)>| {
        let better = match *first {
            None => true,
            Some((m, _)) => n != 0 && (m == 0 || n < m),
        };
        if better {
            *first = Some((n, at));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            '$' => match chars.peek().copied() {
                Some(d) if d.is_ascii_digit() => {
                    let mut n = 0u32;
                    while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                        n = n * 10 + d;
                        chars.next();
                    }
                    place(n, out.len(), &mut first);
                }
                Some('{') => {
                    chars.next();
                    let mut n: Option<u32> = None;
                    while let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                        n = Some(n.unwrap_or(0) * 10 + d);
                        chars.next();
                    }
                    if let Some(n) = n {
                        place(n, out.len(), &mut first);
                    }
                    // Skip the rest of the place, nested ones included.
                    let mut depth = 1;
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
                    }
                }
                _ => out.push('$'),
            },
            c => out.push(c),
        }
    }
    let at = first.map_or(out.len(), |(_, at)| at);
    out.insert(at, MARK);
    // Brackets holding only separators (and the cursor) are emptied.
    loop {
        let mut changed = false;
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            let mut i = 0;
            while let Some(o) = out[i..].find(open).map(|k| i + k) {
                let rest = &out[o + 1..];
                let Some(c) = rest.find(close) else { break };
                let inner = &rest[..c];
                if !inner.is_empty()
                    && inner
                        .chars()
                        .all(|ch| ch == ',' || ch == MARK || ch.is_whitespace())
                    && inner.chars().any(|ch| ch != MARK)
                {
                    let keep = if inner.contains(MARK) {
                        MARK.to_string()
                    } else {
                        String::new()
                    };
                    out.replace_range(o + 1..o + 1 + c, &keep);
                    changed = true;
                }
                i = o + 1;
            }
        }
        if !changed {
            break;
        }
    }
    let cursor = out.find(MARK).unwrap_or(out.len());
    out.remove(cursor);
    (out, cursor)
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

/// The documentation of a completion item (as sent, or resolved).
pub fn item_documentation(item: &Value) -> Option<String> {
    item.get("documentation")
        .and_then(doc_text)
        .filter(|d| !d.trim().is_empty())
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
pub fn completion_items(answer: &Value, keep_raw: bool) -> (Vec<CompletionItem>, bool) {
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
                    let (text, at) = snippet_insert(s);
                    (text, Some(at))
                } else {
                    (s.to_string(), None)
                }
            };
            let edit = i.get("textEdit").and_then(|e| {
                let range = e.get("range").or_else(|| e.get("replace"))?.clone();
                let (text, at) = fix(e["newText"].as_str()?);
                Some((range, text, at))
            });
            let (insert_text, cursor) = fix(i["insertText"].as_str().unwrap_or(&label));
            let additional = i["additionalTextEdits"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default()
                .iter()
                .filter_map(|e| Some((e.get("range")?.clone(), e["newText"].as_str()?.to_string())))
                .collect();
            Some(CompletionItem {
                additional,
                insert_text,
                cursor,
                detail: i["detail"].as_str().map(str::to_string),
                kind: i["kind"].as_u64().map(|k| k as u8),
                documentation: i.get("documentation").and_then(doc_text),
                sort_text: i["sortText"].as_str().unwrap_or(&label).to_string(),
                filter_text: i["filterText"].as_str().unwrap_or(&label).to_string(),
                edit,
                label,
                raw: if keep_raw { i.clone() } else { Value::Null },
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
    // `null` is no edit, as the protocol allows.
    let empty = Vec::new();
    let list = match answer {
        Value::Null => &empty,
        v => v.as_array()?,
    };
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
    fn snippets_inserted() {
        assert_eq!(
            snippet_insert("all?(${1:enumerable})"),
            ("all?()".into(), 5)
        );
        assert_eq!(
            snippet_insert("reduce(${1:e}, ${2:acc}, ${3:fun})$0"),
            ("reduce()".into(), 7)
        );
        assert_eq!(snippet_insert("now()"), ("now()".into(), 5));
        assert_eq!(
            snippet_insert("if ${1:cond} do\n  $0\nend"),
            ("if  do\n  \nend".into(), 3)
        );
        assert_eq!(snippet_insert("x$0 ${2:b} ${1:a}"), ("x  ".into(), 3));
        assert_eq!(snippet_insert("\\$1 é$1"), ("$1 é".into(), 5));
        assert_eq!(snippet_insert("%{${1:k}: ${2:v}}"), ("%{: }".into(), 2));
    }

    #[test]
    fn snippets_stripped() {
        assert_eq!(strip_snippet("foo(${1:a}, ${2:b})$0"), "foo(a, b)");
        assert_eq!(strip_snippet("x ${1|one,two|} \\$y $"), "x one $y $");
        assert_eq!(strip_snippet("${1:a ${2:b}}"), "a b");
    }

    #[test]
    fn no_edits() {
        assert_eq!(
            text_edits("x", &Value::Null, Encoding::Utf16),
            Some(Vec::new())
        );
        assert_eq!(
            text_edits("x", &json!([]), Encoding::Utf16),
            Some(Vec::new())
        );
        assert_eq!(text_edits("x", &json!({}), Encoding::Utf16), None);
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
