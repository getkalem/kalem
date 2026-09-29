//! Captions and names: the `#+CAPTION:` and `#+NAME:` lines above an
//! element (a table, a block, a paragraph holding a picture…), and the
//! targets a cross reference can point to.

use org_edit::{EditError, Selection, Transaction};
use org_model::Document;
use org_syntax::ast::{self, AstNode};
use org_syntax::{SyntaxKind, SyntaxNode, TextSize};

use crate::palette::PaletteItem;

/// The command that inserts a cross reference.
pub const REFERENCE: &str = "org.insert.reference";

/// Whether an element of `kind` takes affiliated keywords.
fn takes_keywords(kind: SyntaxKind) -> bool {
    use SyntaxKind::*;
    kind.is_element()
        && !matches!(
            kind,
            DOCUMENT
                | SECTION
                | HEADLINE
                | INLINETASK
                | ITEM
                | TABLE_ROW
                | NODE_PROPERTY
                | PROPERTY_DRAWER
                | PLANNING
                | KEYWORD
                | CLOCK
        )
}

/// The element at `point` that a caption or a name would belong to: the
/// innermost one that takes affiliated keywords (a table for a cell, a
/// paragraph in a list item).
pub fn element_at(root: &SyntaxNode, point: usize) -> Option<SyntaxNode> {
    let len = usize::from(root.text_range().end());
    if len == 0 {
        return None;
    }
    let at = TextSize::try_from(point.min(len - 1)).ok()?;
    let token = root.token_at_offset(at).right_biased()?;
    token.parent_ancestors().find(|n| takes_keywords(n.kind()))
}

/// A keyword's key as written (`caption`, `CAPTION`).
fn spelled(k: &ast::AffiliatedKeyword) -> String {
    let t = k.syntax().to_string();
    let t = t.trim_start().trim_start_matches("#+");
    t[..t.find([':', '[']).unwrap_or(t.len())].to_string()
}

/// The value of the element's `key` keyword (`CAPTION`, `NAME`): the
/// first one's.
pub fn value(element: &SyntaxNode, key: &str) -> Option<String> {
    ast::affiliated_keywords(element)
        .find(|k| k.key().eq_ignore_ascii_case(key))
        .map(|k| k.value())
}

/// The value of `key` for the element at `point`, for a prompt.
pub fn value_at(doc: &Document, point: usize, key: &str) -> Option<String> {
    value(&element_at(&doc.parse().syntax(), point)?, key)
}

/// Sets the `key` keyword (`CAPTION` or `NAME`) of the element at `point`
/// to `value`: its lines replaced by one, or a new line above the element
/// (a name first, a caption after the other keywords), indented as the
/// element. An empty value removes the keyword.
pub fn set(doc: &Document, point: usize, key: &str, value: &str) -> Result<Transaction, EditError> {
    let fail = |m: &str| EditError {
        message: m.into(),
        point: None,
    };
    let root = doc.parse().syntax();
    let text = root.to_string();
    let element = element_at(&root, point).ok_or_else(|| fail("No element here"))?;
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let existing: Vec<ast::AffiliatedKeyword> = ast::affiliated_keywords(&element)
        .filter(|k| k.key().eq_ignore_ascii_case(key))
        .collect();
    let body = usize::from(ast::post_affiliated(&element));
    let bol = text[..body].rfind('\n').map_or(0, |i| i + 1);
    let indent: String = text[bol..]
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let mut tx = Transaction::new(if key.eq_ignore_ascii_case("CAPTION") {
        "Set Caption"
    } else {
        "Set Name"
    });
    let edit = |tx: &mut Transaction, r: std::ops::Range<usize>, s: &str| {
        tx.replace(r, s)
            .map(|_| ())
            .map_err(|_| fail("cannot edit here"))
    };
    // The whole lines of the keywords.
    let line = |k: &ast::AffiliatedKeyword| {
        let r = k.syntax().text_range();
        let s = usize::from(r.start());
        let s = text[..s].rfind('\n').map_or(0, |i| i + 1);
        let e = usize::from(r.end());
        let e = if text[..e].ends_with('\n') {
            e
        } else {
            text[e..].find('\n').map_or(text.len(), |i| e + i + 1)
        };
        (s..e, spelled(k))
    };
    let lines: Vec<_> = existing.iter().map(line).collect();
    let written = |raw: &str| format!("{indent}#+{raw}: {value}\n");
    let cursor = match lines.first() {
        Some((first, raw)) => {
            let new = if value.is_empty() {
                String::new()
            } else {
                written(raw)
            };
            edit(&mut tx, first.clone(), &new)?;
            for (r, _) in &lines[1..] {
                edit(&mut tx, r.clone(), "")?;
            }
            first.start + new.len().saturating_sub(1)
        }
        None if value.is_empty() => return Err(fail(&format!("No {key} to remove"))),
        None => {
            let lower = text[bol..].trim_start().starts_with("#+begin")
                || ast::affiliated_keywords(&element)
                    .any(|k| spelled(&k).starts_with(char::is_lowercase));
            let raw = if lower {
                key.to_lowercase()
            } else {
                key.to_uppercase()
            };
            let first = ast::affiliated_keywords(&element)
                .next()
                .map(|k| line(&k).0.start);
            let at = if key.eq_ignore_ascii_case("NAME") {
                first.unwrap_or(bol)
            } else {
                bol
            };
            let new = written(&raw);
            edit(&mut tx, at..at, &new)?;
            // The cursor stays on the element's line.
            point + new.len()
        }
    };
    let cursor = if lines.is_empty() {
        cursor
    } else {
        // Replaced lines: the cursor keeps its place when it was below
        // them, else goes to the end of the new line.
        let removed: usize = lines.iter().map(|(r, _)| r.len()).sum();
        let added = if value.is_empty() {
            0
        } else {
            written(&lines[0].1).len()
        };
        if point >= lines.last().map_or(0, |(r, _)| r.end) {
            point + added - removed
        } else {
            cursor
        }
    };
    Ok(tx.select(Selection::caret(cursor)))
}

/// What a cross reference can point to in `doc`: named elements, headings
/// by their `CUSTOM_ID` and title, and targets, as (target, description,
/// kind).
pub fn references(doc: &Document) -> Vec<(String, String, &'static str)> {
    let root = doc.parse().syntax();
    let mut out: Vec<(String, String, &'static str)> = Vec::new();
    let mut add = |target: String, desc: String, kind: &'static str| {
        if !target.is_empty() && !out.iter().any(|(t, ..)| *t == target) {
            out.push((target, desc, kind));
        }
    };
    for n in root.descendants().filter(|n| takes_keywords(n.kind())) {
        if let Some(name) = value(&n, "NAME") {
            let caption = value(&n, "CAPTION").unwrap_or_default();
            add(name, caption, kind_name(n.kind()));
        }
    }
    for e in doc.outline().entries.iter().filter(|e| !e.inlinetask) {
        let title = e.raw_title.trim().to_string();
        if let Some((_, id)) = e
            .drawer
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("CUSTOM_ID"))
        {
            add(format!("#{id}"), title.clone(), "heading");
        }
        if !title.is_empty() {
            add(format!("*{title}"), String::new(), "heading");
        }
    }
    for t in root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::TARGET)
    {
        let s = t.text().to_string();
        let inner = s.trim().trim_start_matches("<<").trim_end_matches(">>");
        add(inner.to_string(), String::new(), "target");
    }
    out
}

/// What an element of `kind` is called in the reference picker.
fn kind_name(kind: SyntaxKind) -> &'static str {
    use SyntaxKind::*;
    match kind {
        TABLE => "table",
        SRC_BLOCK => "code",
        EXAMPLE_BLOCK => "example",
        LATEX_ENVIRONMENT => "equation",
        PARAGRAPH => "paragraph",
        PLAIN_LIST => "list",
        QUOTE_BLOCK => "quote",
        _ => "element",
    }
}

/// The reference picker's items: choosing one inserts a link to it.
pub fn picker_items(doc: &Document) -> Vec<PaletteItem> {
    references(doc)
        .into_iter()
        .map(|(target, desc, kind)| PaletteItem {
            id: crate::palette::invocation(REFERENCE, &serde_json::json!({ "target": target })),
            title: if desc.is_empty() {
                target
            } else {
                format!("{target}  {desc}")
            },
            category: kind.to_string(),
            keys: String::new(),
            also: String::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, point: usize, key: &str, value: &str) -> (String, usize) {
        let d = Document::new(org_syntax::parse(text));
        let tx = set(&d, point, key, value).unwrap();
        let head = tx.selection_after.map_or(0, |s| s.head);
        (tx.apply(text), head)
    }

    #[test]
    fn captions_and_names() {
        let t = "Text.\n\n| a | b |\n| 1 | 2 |\n";
        // A new caption, then a name above it, for a table cell.
        let (t1, p) = run(t, 12, "caption", "Numbers");
        assert_eq!(t1, "Text.\n\n#+CAPTION: Numbers\n| a | b |\n| 1 | 2 |\n");
        assert_eq!(p, 12 + "#+CAPTION: Numbers\n".len());
        let (t2, _) = run(&t1, 30, "name", "tab:n");
        assert_eq!(
            t2,
            "Text.\n\n#+NAME: tab:n\n#+CAPTION: Numbers\n| a | b |\n| 1 | 2 |\n"
        );
        let d = Document::new(org_syntax::parse(&t2));
        assert_eq!(value_at(&d, 45, "caption").as_deref(), Some("Numbers"));
        // Replaced, keeping its spelling; several caption lines become one.
        let (t3, _) = run(&t2, 45, "CAPTION", "New  one");
        assert_eq!(
            t3,
            "Text.\n\n#+NAME: tab:n\n#+CAPTION: New one\n| a | b |\n| 1 | 2 |\n"
        );
        let two = "#+caption: a\n#+caption: b\n#+begin_src sh\nls\n#+end_src\n";
        assert_eq!(
            run(two, 30, "caption", "c").0,
            "#+caption: c\n#+begin_src sh\nls\n#+end_src\n"
        );
        // Removed with an empty value.
        assert_eq!(
            run(&t2, 45, "name", "").0,
            "Text.\n\n#+CAPTION: Numbers\n| a | b |\n| 1 | 2 |\n"
        );
        // Lower case beside a lower-case block, indented as the element.
        assert_eq!(
            run(
                "- item\n\n  #+begin_src sh\n  ls\n  #+end_src\n",
                20,
                "name",
                "x"
            )
            .0,
            "- item\n\n  #+name: x\n  #+begin_src sh\n  ls\n  #+end_src\n"
        );
        // From the keyword line itself.
        assert_eq!(
            run(&t2, 9, "name", "t2").0.lines().nth(2),
            Some("#+NAME: t2")
        );
        // Nothing to name in a heading.
        let d = Document::new(org_syntax::parse("* H\n"));
        assert!(set(&d, 1, "name", "x").is_err());
    }

    #[test]
    fn reference_targets() {
        let t = "* Intro\n:PROPERTIES:\n:CUSTOM_ID: intro\n:END:\n#+NAME: tab:n\n#+CAPTION: Numbers\n| 1 |\n\nSee <<here>>.\n";
        let d = Document::new(org_syntax::parse(t));
        assert_eq!(
            references(&d),
            [
                ("tab:n".into(), "Numbers".into(), "table"),
                ("#intro".into(), "Intro".into(), "heading"),
                ("*Intro".into(), String::new(), "heading"),
                ("here".into(), String::new(), "target"),
            ]
        );
    }
}
