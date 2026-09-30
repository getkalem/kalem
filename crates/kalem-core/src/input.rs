//! Editing rules both frontends share (design §6.3): what Enter does where
//! the cursor is, completion menus while typing (`#+`, `[[`, `[fn:`), and
//! the formula under the cursor for a preview.

use org_edit::{EditError, Selection, Transaction};
use org_model::Document;
use org_syntax::SyntaxKind::{self, *};
use org_syntax::{SyntaxNode, TextSize};

fn bol(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

fn eol(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

/// The blanks at the start of the line holding `pos`.
fn indentation(text: &str, pos: usize) -> &str {
    let b = bol(text, pos);
    let line = &text[b..eol(text, pos)];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// A line break keeping the current line's indentation (when the cursor
/// is past it), replacing the selection.
pub fn newline(text: &str, point: usize, mark: Option<usize>) -> Transaction {
    let (a, b) = mark.map_or((point, point), |m| (m.min(point), m.max(point)));
    let indent = indentation(text, a);
    let keep = a - bol(text, a) >= indent.len();
    let insert = format!("\n{}", if keep { indent } else { "" });
    let mut tx = Transaction::new("New line");
    tx.replace(a..b, insert.clone()).expect("one edit");
    tx.select(Selection::caret(a + insert.len()))
}

fn ancestor(root: &SyntaxNode, pos: usize, kind: SyntaxKind) -> Option<SyntaxNode> {
    let el = org_edit::narrow::element_at(root, pos)?;
    el.ancestors().find(|a| a.kind() == kind)
}

/// Enter in the editor: a new item in a list (splitting it at the cursor),
/// or on an empty item, leaving the list (one level up for a nested item);
/// the next row in a table; elsewhere a line break that keeps the
/// indentation, in source blocks too.
pub fn enter(doc: &Document, point: usize, mark: Option<usize>) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    if mark.is_some_and(|m| m != point) {
        return Ok(newline(&text, point, mark));
    }
    let root = doc.parse().syntax();
    let line = bol(&text, point);
    if text[line..]
        .trim_start_matches([' ', '\t'])
        .starts_with('|')
        && ancestor(&root, line, TABLE).is_some()
    {
        return org_edit::table::next_row(doc, point);
    }
    if let Some(item) = ancestor(&root, point, ITEM) {
        let start = usize::from(item.text_range().start());
        let first_end = eol(&text, start);
        // After the bullet, counter and checkbox.
        let content = item
            .children_with_tokens()
            .find(|t| !matches!(t.kind(), BULLET | COUNTER | CHECKBOX | WHITESPACE))
            .map_or(first_end, |t| usize::from(t.text_range().start()))
            .min(first_end);
        let checkbox = item.children_with_tokens().any(|t| t.kind() == CHECKBOX);
        if point >= content && point <= first_end {
            let end = usize::from(item.text_range().end());
            if text[content..end].trim().is_empty() {
                let nested = item
                    .parent()
                    .and_then(|l| l.parent())
                    .is_some_and(|p| p.kind() == ITEM);
                if nested {
                    return org_edit::list::indent_item(doc, point, None, false, true);
                }
                // Leave the list: the empty item becomes an empty line.
                let mut tx = Transaction::new("End list");
                tx.replace(start..first_end, "").expect("one edit");
                return Ok(tx.select(Selection::caret(start)));
            }
            if let Some(tx) = org_edit::list::insert_item(doc, point, checkbox) {
                return Ok(tx);
            }
        }
    }
    Ok(newline(&text, point, None))
}

/// Where a link leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkAction {
    /// A web or mail address, for the system.
    Url(String),
    /// A file, relative to the document, with a search in it.
    File {
        /// The path as written.
        path: String,
        /// `::search` after it.
        search: Option<String>,
    },
    /// A place in this document.
    Jump(usize),
    /// An internal link without a target.
    Missing(String),
    /// A PDF for the system's print dialog ([`crate::print`]).
    Print(std::path::PathBuf),
    /// A file for its application ([`crate::system::open`]).
    System(std::path::PathBuf),
    /// A file to show in the system's file manager
    /// ([`crate::system::reveal`]).
    Reveal(std::path::PathBuf),
}

/// The link at `pos` and where it leads (`org-open-at-point`).
pub fn link_at(doc: &Document, pos: usize) -> Option<LinkAction> {
    let root = doc.parse().syntax();
    let len = usize::from(root.text_range().end());
    if len == 0 {
        return None;
    }
    let tok = root
        .token_at_offset(TextSize::from(pos.min(len - 1) as u32))
        .right_biased()?;
    let node = tok.parent_ancestors().find(|a| a.kind() == LINK)?;
    let link: org_syntax::ast::Link = org_syntax::ast::AstNode::cast(node.clone())?;
    let info = link.info(doc.parse().context());
    let found = |p: Option<usize>| {
        p.map_or_else(
            || LinkAction::Missing(info.raw_link.clone()),
            LinkAction::Jump,
        )
    };
    Some(match info.link_type.as_str() {
        "http" | "https" | "ftp" | "mailto" | "news" => LinkAction::Url(info.raw_link.clone()),
        "doi" => LinkAction::Url(format!("https://doi.org/{}", info.path)),
        "file" => LinkAction::File {
            path: info.path.clone(),
            search: info.search_option.clone(),
        },
        // In the heading's attachment folder.
        "attachment" => LinkAction::File {
            path: match org_export::attach::attachment_dir(&node, None) {
                Some(d) => format!("{}/{}", d.trim_end_matches('/'), info.path),
                None => info.path.clone(),
            },
            search: info.search_option.clone(),
        },
        "fuzzy" => found(doc.link_search(&info.path)),
        "custom-id" | "coderef" => found(doc.link_search(&info.raw_link)),
        "radio" => found(doc.radio_target(&info.path)),
        "id" => found(
            doc.entry_with_id(&info.path)
                .map(|e| usize::from(doc.entry(e).range.start())),
        ),
        _ => LinkAction::Url(info.raw_link.clone()),
    })
}

/// What a completion menu offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// Keywords and blocks after `#+`.
    Keyword,
    /// Link targets after `[[`.
    Link,
    /// Footnote labels after `[fn:`.
    Footnote,
    /// Tags after a colon at the end of a headline.
    Tag,
}

/// A completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    /// What the menu shows.
    pub label: String,
    /// What replaces the text from the trigger to the cursor.
    pub insert: String,
    /// Where the cursor goes in `insert`.
    pub cursor: usize,
}

/// A completion menu at the cursor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// Which.
    pub kind: CompletionKind,
    /// The start of the text a choice replaces (the trigger included for
    /// keywords, after it otherwise).
    pub start: usize,
    /// What was typed after the trigger.
    pub prefix: String,
    /// The choices matching it.
    pub items: Vec<CompletionItem>,
}

const KEYWORDS: &[&str] = &[
    "TITLE",
    "AUTHOR",
    "DATE",
    "EMAIL",
    "SUBTITLE",
    "DESCRIPTION",
    "KEYWORDS",
    "LANGUAGE",
    "OPTIONS",
    "STARTUP",
    "TODO",
    "TAGS",
    "FILETAGS",
    "CATEGORY",
    "PROPERTY",
    "SETUPFILE",
    "INCLUDE",
    "NAME",
    "CAPTION",
    "ATTR_ORG",
    "ATTR_HTML",
    "ATTR_LATEX",
    "RESULTS",
    "CALL",
    "LINK",
    "MACRO",
    "LATEX_CLASS",
    "LATEX_HEADER",
    "HTML_HEAD",
    "BIBLIOGRAPHY",
    "CITE_EXPORT",
    "PRINT_BIBLIOGRAPHY",
    "TOC",
    "EXPORT_FILE_NAME",
    "ARCHIVE",
    "COLUMNS",
    "PRIORITIES",
    "SEQ_TODO",
    "TYP_TODO",
];

const BLOCKS: &[&str] = &[
    "src", "example", "quote", "center", "verse", "comment", "export",
];

fn keyword_items(prefix: &str, indent: &str) -> Vec<CompletionItem> {
    let upper = !prefix.is_empty() && prefix.chars().all(|c| !c.is_lowercase());
    let lower_prefix = prefix.to_ascii_lowercase();
    let mut out = Vec::new();
    for b in BLOCKS {
        let name = format!("begin_{b}");
        if !name.starts_with(&lower_prefix) {
            continue;
        }
        let (begin, end) = if upper {
            ("BEGIN", "END")
        } else {
            ("begin", "end")
        };
        let b = if upper {
            b.to_ascii_uppercase()
        } else {
            b.to_string()
        };
        let head = format!(
            "#+{begin}_{b}{}",
            if b.eq_ignore_ascii_case("src") || b.eq_ignore_ascii_case("export") {
                " "
            } else {
                ""
            }
        );
        let insert = format!("{head}\n{indent}\n{indent}#+{end}_{b}");
        let cursor = if head.ends_with(' ') {
            head.len()
        } else {
            head.len() + 1 + indent.len()
        };
        out.push(CompletionItem {
            label: format!("#+{begin}_{b}"),
            insert,
            cursor,
        });
    }
    for k in KEYWORDS {
        if !k.to_ascii_lowercase().starts_with(&lower_prefix) {
            continue;
        }
        let k = if prefix.is_empty() || upper {
            k.to_string()
        } else {
            k.to_ascii_lowercase()
        };
        let insert = format!("#+{k}: ");
        out.push(CompletionItem {
            label: format!("#+{k}:"),
            cursor: insert.len(),
            insert,
        });
    }
    out
}

fn link_items(doc: &Document, root: &SyntaxNode, prefix: &str) -> Vec<CompletionItem> {
    let p = prefix.to_lowercase();
    let mut out = Vec::new();
    let mut add = |label: String, target: String| {
        if label.to_lowercase().contains(&p)
            && !out.iter().any(|i: &CompletionItem| i.insert == target)
        {
            let insert = format!("{target}]]");
            out.push(CompletionItem {
                label,
                cursor: insert.len(),
                insert,
            });
        }
    };
    for e in &doc.outline().entries {
        if let Some((_, id)) = e
            .drawer
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("CUSTOM_ID"))
        {
            add(format!("#{id}"), format!("#{id}"));
        }
        let title = e.raw_title.trim();
        if !title.is_empty() {
            add(format!("*{title}"), format!("*{title}"));
        }
    }
    // Named elements (`#+NAME:`), which internal links reach by name.
    for (target, _, kind) in crate::affiliated::references(doc) {
        if !matches!(kind, "heading" | "target") {
            add(target.clone(), target);
        }
    }
    for t in root.descendants().filter(|n| n.kind() == TARGET) {
        let s = t.text().to_string();
        let inner = s
            .trim()
            .trim_start_matches("<<")
            .trim_end_matches(">>")
            .to_string();
        add(inner.clone(), inner);
    }
    for scheme in ["file:", "https://", "id:", "mailto:"] {
        if scheme.starts_with(&p) || p.is_empty() {
            out.push(CompletionItem {
                label: scheme.to_string(),
                insert: scheme.to_string(),
                cursor: scheme.len(),
            });
        }
    }
    out.truncate(50);
    out
}

fn footnote_items(root: &SyntaxNode, prefix: &str) -> Vec<CompletionItem> {
    let mut labels: Vec<String> = Vec::new();
    for n in root
        .descendants()
        .filter(|n| matches!(n.kind(), FOOTNOTE_DEFINITION | FOOTNOTE_REFERENCE))
    {
        if let Some(k) = n.children_with_tokens().find(|t| t.kind() == KEY) {
            let l = k.to_string();
            if !labels.contains(&l) {
                labels.push(l);
            }
        }
    }
    let next = labels
        .iter()
        .filter_map(|l| l.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n + 1);
    let mut out: Vec<CompletionItem> = labels
        .iter()
        .filter(|l| l.starts_with(prefix))
        .map(|l| {
            let insert = format!("{l}]");
            CompletionItem {
                label: l.clone(),
                cursor: insert.len(),
                insert,
            }
        })
        .collect();
    if prefix.is_empty() || next.to_string().starts_with(prefix) {
        let insert = format!("{next}]");
        out.push(CompletionItem {
            label: format!("{next} (new)"),
            cursor: insert.len(),
            insert,
        });
    }
    out
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%')
}

/// On a headline, the tag being typed at the end of `before` (the line up
/// to the cursor): where it starts in `before`, what is typed of it, and
/// the tags before it in its group (`:a:b:pre`).
fn tag_prefix(before: &str) -> Option<(usize, &str, Vec<&str>)> {
    let stars = before.len() - before.trim_start_matches('*').len();
    if stars == 0 || !before[stars..].starts_with(' ') {
        return None;
    }
    let word_start = before.rfind([' ', '\t']).map_or(0, |i| i + 1);
    let word = &before[word_start..];
    if word_start <= stars
        || !word.starts_with(':')
        || !word.chars().all(|c| c == ':' || is_tag_char(c))
    {
        return None;
    }
    let last = word.rfind(':')?;
    let already = word[..last].split(':').filter(|t| !t.is_empty()).collect();
    Some((word_start + last + 1, &word[last + 1..], already))
}

/// Whether the text of a line before the cursor may open a completion
/// menu (see [`completion`]).
pub fn completion_trigger(before: &str) -> bool {
    before.trim_start().starts_with("#+")
        || before.contains("[[")
        || before.contains("[fn:")
        || tag_prefix(before).is_some()
}

/// The tags to offer: the tag table's, then the others used in the
/// document, those starting with `prefix` (in any case) and not in
/// `already`.
fn tag_items(doc: &Document, prefix: &str, already: &[&str]) -> Vec<CompletionItem> {
    let mut tags: Vec<String> = doc.tag_table().tags().map(|(t, _)| t.to_string()).collect();
    let mut used: Vec<String> = doc
        .outline()
        .entries
        .iter()
        .flat_map(|e| e.local_tags.iter().cloned())
        .filter(|t| !tags.contains(t))
        .collect();
    used.sort();
    used.dedup();
    tags.extend(used);
    let lower = prefix.to_lowercase();
    tags.into_iter()
        .filter(|t| t.to_lowercase().starts_with(&lower) && !already.contains(&t.as_str()))
        .map(|t| {
            let insert = format!("{t}:");
            CompletionItem {
                label: t,
                cursor: insert.len(),
                insert,
            }
        })
        .collect()
}

/// The edit that choosing `item` of the menu `c` makes, the cursor being
/// at `point`: the text from the menu's start to the cursor replaced, and
/// a headline's tags aligned again.
pub fn apply_completion(
    doc: &Document,
    point: usize,
    c: &Completion,
    item: &CompletionItem,
) -> Transaction {
    let mut tx = Transaction::new("Complete");
    tx.replace(c.start..point, item.insert.clone())
        .expect("one edit");
    let caret = c.start + item.cursor;
    let tx = tx.select(Selection::caret(caret));
    if c.kind != CompletionKind::Tag {
        return tx;
    }
    let text = doc.parse().syntax().to_string();
    let new_text = tx.apply(&text);
    let new_doc = Document::with_settings(
        org_syntax::parse_with(&new_text, doc.parse().context()),
        std::sync::Arc::new(doc.settings().clone()),
        None,
    );
    let tags: Vec<String> = new_doc
        .outline()
        .entries
        .iter()
        .rev()
        .find(|e| usize::from(e.range.start()) <= caret)
        .map(|e| e.local_tags.clone())
        .unwrap_or_default();
    let Ok(align) = org_edit::tags::set_tags(&new_doc, caret, &tags) else {
        return tx;
    };
    let aligned = align.apply(&new_text);
    // One step from the old text to the aligned one, the cursor at the end
    // of the tags.
    let pre = text
        .bytes()
        .zip(aligned.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    let mut pre = pre.min(c.start);
    while !text.is_char_boundary(pre) {
        pre -= 1;
    }
    let max_suf = text.len().min(aligned.len()) - pre;
    let mut suf = text
        .bytes()
        .rev()
        .zip(aligned.bytes().rev())
        .take(max_suf)
        .take_while(|(a, b)| a == b)
        .count();
    while !text.is_char_boundary(text.len() - suf) || !aligned.is_char_boundary(aligned.len() - suf)
    {
        suf -= 1;
    }
    let line = bol(&aligned, caret.min(aligned.len()));
    let eol = aligned[line..]
        .find('\n')
        .map_or(aligned.len(), |i| line + i);
    let mut out = Transaction::new("Complete");
    out.replace(pre..text.len() - suf, &aligned[pre..aligned.len() - suf])
        .expect("one edit");
    out.select(Selection::caret(eol))
}

/// The completion menu for the text before `point`, if a trigger is there:
/// `#+` at the start of a line, an unclosed `[[`, an unclosed `[fn:`, a
/// colon starting tags at the end of a headline.
pub fn completion(doc: &Document, point: usize) -> Option<Completion> {
    let root = doc.parse().syntax();
    let text = root.to_string();
    let b = bol(&text, point);
    let before = &text[b..point];
    let indent = indentation(&text, point).to_string();
    // `#+keyword` at the start of the line.
    let trimmed = before.trim_start_matches([' ', '\t']);
    if let Some(prefix) = trimmed.strip_prefix("#+")
        && prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        let items = keyword_items(prefix, &indent);
        return (!items.is_empty()).then(|| Completion {
            kind: CompletionKind::Keyword,
            start: point - trimmed.len(),
            prefix: prefix.to_string(),
            items,
        });
    }
    if let Some(i) = before.rfind("[fn:") {
        let prefix = &before[i + 4..];
        if prefix
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
        {
            let items = footnote_items(&root, prefix);
            return (!items.is_empty()).then(|| Completion {
                kind: CompletionKind::Footnote,
                start: b + i + 4,
                prefix: prefix.to_string(),
                items,
            });
        }
    }
    if let Some(i) = before.rfind("[[") {
        let prefix = &before[i + 2..];
        if !prefix.contains(']') {
            let items = link_items(doc, &root, prefix);
            return (!items.is_empty()).then(|| Completion {
                kind: CompletionKind::Link,
                start: b + i + 2,
                prefix: prefix.to_string(),
                items,
            });
        }
    }
    if let Some((at, prefix, already)) = tag_prefix(before) {
        let items = tag_items(doc, prefix, &already);
        return (!items.is_empty()).then(|| Completion {
            kind: CompletionKind::Tag,
            start: b + at,
            prefix: prefix.to_string(),
            items,
        });
    }
    None
}

/// The source of the LaTeX fragment or environment at `point`, for a
/// preview.
pub fn formula_at(root: &SyntaxNode, point: usize) -> Option<String> {
    let len = usize::from(root.text_range().end());
    if len == 0 {
        return None;
    }
    let at = TextSize::from(point.min(len - 1) as u32);
    let tok = root.token_at_offset(at).left_biased()?;
    let n = tok
        .parent_ancestors()
        .find(|a| matches!(a.kind(), LATEX_FRAGMENT | LATEX_ENVIRONMENT))?;
    Some(n.text().to_string().trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enter_in(text: &str, point: usize) -> (String, usize) {
        let doc = Document::new(org_syntax::parse(text));
        let t = enter(&doc, point, None).unwrap();
        (t.apply(text), t.selection_after.map_or(0, |s| s.head))
    }

    #[test]
    fn enter_rules() {
        // A new item, with a box when the item has one.
        assert_eq!(enter_in("- a\n", 3).0, "- a\n- \n");
        assert_eq!(enter_in("- [ ] a\n", 7).0, "- [ ] a\n- [ ] \n");
        // An empty item ends the list; a nested one goes up a level.
        assert_eq!(enter_in("- a\n- \n", 6), ("- a\n\n".into(), 4));
        assert_eq!(enter_in("- a\n  - \n", 8).0, "- a\n- \n");
        // Indentation stays; tables go to the next row.
        assert_eq!(enter_in("  text\n", 6), ("  text\n  \n".into(), 9));
        assert_eq!(
            enter_in("#+begin_src sh\n  ls\n#+end_src\n", 19).0,
            "#+begin_src sh\n  ls\n  \n#+end_src\n"
        );
        assert_eq!(enter_in("* H\n", 3).0, "* H\n\n");
        let (t, p) = enter_in("| a |\n| b |\n", 3);
        assert_eq!((t.as_str(), p), ("| a |\n| b |\n", 8));
    }

    #[test]
    fn tag_completion() {
        let t = "#+TAGS: work home\n* A :home:\n* B :tool:\n* C :\n* D :w\n";
        let doc = Document::new(org_syntax::parse(t));
        let at = |s: &str| t.find(s).unwrap() + s.len();
        let labels = |p: usize| {
            completion(&doc, p).map(|c| {
                (
                    c.kind,
                    c.items.iter().map(|i| i.label.clone()).collect::<Vec<_>>(),
                )
            })
        };
        assert_eq!(
            labels(at("* C :")),
            Some((
                CompletionKind::Tag,
                vec!["work".into(), "home".into(), "tool".into()]
            ))
        );
        assert_eq!(
            labels(at("* D :w")).map(|l| l.1),
            Some(vec!["work".to_string()])
        );
        // Not the tags already in the group, and not in a title.
        assert_eq!(
            labels(at("* A :home:")).map(|l| l.1),
            Some(vec!["work".to_string(), "tool".to_string()])
        );
        assert!(!completion_trigger("* Note: x"));
        assert!(completion_trigger("* Title :a:b"));
        // Choosing one aligns the tags at the right edge.
        let p = at("* D :w");
        let c = completion(&doc, p).unwrap();
        let tx = apply_completion(&doc, p, &c, &c.items[0]);
        let out = tx.apply(t);
        let line = out.lines().find(|l| l.starts_with("* D")).unwrap();
        assert!(line.ends_with(" :work:") && line.len() == 77, "{line:?}");
        let head = tx.selection_after.unwrap().head;
        assert_eq!(&out[head - 6..head], ":work:");
    }

    #[test]
    fn completions() {
        let text = "* Intro\n:PROPERTIES:\n:CUSTOM_ID: intro\n:END:\n<<here>> text[fn:1] and [fn:note]\n#+ti\nsee [[in\nnote [fn:\n";
        let doc = Document::new(org_syntax::parse(text));
        let at = |s: &str| text.find(s).unwrap() + s.len();
        let c = completion(&doc, at("#+ti")).unwrap();
        assert_eq!(c.kind, CompletionKind::Keyword);
        assert_eq!(c.items[0].insert, "#+title: ");
        let c = completion(&doc, at("[[in")).unwrap();
        let labels: Vec<&str> = c.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["#intro", "*Intro"]);
        assert_eq!(c.items[1].insert, "*Intro]]");
        let c = completion(&doc, at("note [fn:")).unwrap();
        let labels: Vec<&str> = c.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["1", "note", "2 (new)"]);
        // Named elements.
        let text = "#+NAME: tab:int\n| 1 |\n\nsee [[int\n";
        let doc = Document::new(org_syntax::parse(text));
        let c = completion(&doc, text.len() - 1).unwrap();
        assert_eq!(c.items[0].insert, "tab:int]]");
        // A block template puts the cursor after `src `.
        let doc = Document::new(org_syntax::parse("#+b\n"));
        let c = completion(&doc, 3).unwrap();
        assert_eq!(
            (c.items[0].insert.as_str(), c.items[0].cursor),
            ("#+begin_src \n\n#+end_src", 12)
        );
        let doc = Document::new(org_syntax::parse(
            "* Intro\nSee [[*Intro]], [[https://x.org][web]] and [[file:a.org::*B]].\n",
        ));
        assert_eq!(link_at(&doc, 12), Some(LinkAction::Jump(0)));
        assert_eq!(
            link_at(&doc, 26),
            Some(LinkAction::Url("https://x.org".into()))
        );
        assert_eq!(
            link_at(&doc, 52),
            Some(LinkAction::File {
                path: "a.org".into(),
                search: Some("*B".into())
            })
        );
        assert_eq!(link_at(&doc, 2), None);
        // An attachment, in its heading's folder.
        let doc = Document::new(org_syntax::parse(
            "* A\n:PROPERTIES:\n:ID: abcd\n:END:\n[[attachment:p.pdf]]\n",
        ));
        assert_eq!(
            link_at(&doc, 35),
            Some(LinkAction::File {
                path: "data/ab/cd/p.pdf".into(),
                search: None
            })
        );
        let doc = Document::new(org_syntax::parse("x $a^2$ y\n"));
        assert_eq!(
            formula_at(&doc.parse().syntax(), 4).as_deref(),
            Some("$a^2$")
        );
        assert_eq!(formula_at(&doc.parse().syntax(), 0), None);
    }
}
