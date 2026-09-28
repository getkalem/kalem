//! The Markdown back-end, as `ox-md.el` writes (derived from HTML: what
//! Markdown cannot express goes out as HTML).

use org_syntax::SyntaxKind::{self, *};
use org_syntax::ast;

use crate::export::{Backend, Exporter, trim};
use crate::html::{self, Html, ListType};
use crate::options::Value;
use crate::tree::{Id, Secondary};

/// The Markdown back-end.
#[derive(Debug, Clone, Copy, Default)]
pub struct Markdown;

/// `(replace-regexp-in-string "^" PREFIX s)`: `prefix` at the start of
/// every line, the empty one after a final line feed included.
pub fn prefix_lines(s: &str, prefix: &str) -> String {
    let mut out = String::from(prefix);
    for c in s.chars() {
        out.push(c);
        if c == '\n' {
            out.push_str(prefix);
        }
    }
    out
}

/// `org-remove-blank-lines`.
fn remove_blank_lines(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('\n') {
        out.push_str(&rest[..=i]);
        rest = &rest[i + 1..];
        // Blank lines after this line feed go.
        loop {
            let blanks = rest.len() - rest.trim_start_matches([' ', '\t']).len();
            if rest[blanks..].starts_with('\n') {
                rest = &rest[blanks + 1..];
            } else {
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `org-make-tag-string`.
fn tag_string(tags: &[String]) -> String {
    if tags.is_empty() {
        String::new()
    } else {
        format!(":{}:", tags.join(":"))
    }
}

/// `org-md--headline-title` in the `atx` style.
fn headline_title(level: i64, title: &str, anchor: Option<&str>, tags: &str) -> String {
    let anchor = anchor.map(|a| format!("{a}\n\n")).unwrap_or_default();
    format!(
        "\n{anchor}{} {title}{tags}\n\n",
        "#".repeat(level.max(1) as usize)
    )
}

/// `org-export-format-code-default`.
pub fn format_code_default(ex: &Exporter<'_>, id: Id) -> String {
    let (code, refs) = html::unravel_code(ex, id);
    let (numbers, retain) = html::number_lines(ex, id);
    let lines: Vec<&str> = code.split('\n').collect();
    let num_start = numbers.map(|_| html::get_loc(ex, id));
    let width = num_start.map(|n| (lines.len() + n).to_string().len());
    let num_len = width.map_or(0, |w| w + 2);
    let max_width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) + num_len;
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        let line = i + 1;
        if let (Some(n), Some(w)) = (num_start, width) {
            out.push_str(&format!("{:>w$}  ", n + line));
        }
        out.push_str(l);
        if retain && let Some((_, r)) = refs.iter().find(|(n, _)| *n == line) {
            let pad = (6 + max_width).saturating_sub(l.chars().count() + num_len);
            out.push_str(&" ".repeat(pad));
            out.push_str(&format!("({r})"));
        }
        out.push('\n');
    }
    out
}

impl Markdown {
    fn headline_referred(&self, ex: &mut Exporter<'_>, id: Id) -> bool {
        if ex.footnote_section_p(id) {
            return false;
        }
        let toc = ex.opt("with-toc");
        if toc.truthy() && html::collect_headlines(ex, toc.int()).contains(&id) {
            return true;
        }
        // A link points to it.
        let links: Vec<Id> = ex
            .tree
            .descendants(ex.tree.root)
            .into_iter()
            .filter(|&d| ex.tree.kind(d) == Some(LINK))
            .collect();
        for l in links {
            let Some(info) = ex.link_info(l) else {
                continue;
            };
            let dest = match info.link_type.as_str() {
                "fuzzy" => ex.resolve_fuzzy(&info.path),
                "custom-id" | "id" => ex.resolve_id(&info.path),
                _ => None,
            };
            // `org-md--headline-referred-p` only checks id links; fuzzy
            // links resolve through `org-export-resolve-id-link` too, which
            // fails on them.
            if dest == Some(id) && matches!(info.link_type.as_str(), "custom-id" | "id") {
                return true;
            }
        }
        false
    }

    fn build_toc(&self, ex: &mut Exporter<'_>, depth: Option<i64>) -> String {
        let title = ex.translate("Table of Contents", "html");
        let mut out = headline_title(1, &title, None, "");
        let heads = html::collect_headlines(ex, depth);
        let mut lines = Vec::new();
        for h in heads {
            let indent = " ".repeat((4 * (ex.relative_level(h) - 1)).max(0) as usize);
            let bullet = if !ex.numbered_p(h) {
                "-   ".to_string()
            } else {
                let last = ex
                    .headline_number(h)
                    .and_then(|n| n.last().copied())
                    .unwrap_or(0);
                let prefix = format!("{last}.");
                let pad = 4usize.saturating_sub(prefix.len()).max(1);
                format!("{prefix}{}", " ".repeat(pad))
            };
            let ids = ex
                .tree
                .secondary(h, Secondary::Title)
                .map(<[Id]>::to_vec)
                .unwrap_or_default();
            let text = ex.with_backend(&MdToc, |ex| ex.data_list(&ids));
            let r = ex
                .node_property(h, "CUSTOM_ID", false)
                .unwrap_or_else(|| ex.reference(h));
            let tags = match ex.opt("with-tags") {
                Value::Nil => String::new(),
                Value::Sym(s) if s == "not-in-toc" => String::new(),
                _ => tag_string(&ex.tags(h, &[], false)),
            };
            lines.push(format!("{indent}{bullet}[{text}](#{r}){tags}"));
        }
        out.push_str(&lines.join("\n"));
        out.push('\n');
        out
    }

    fn footnote_section(&self, ex: &mut Exporter<'_>) -> String {
        let defs = ex.collect_footnote_definitions();
        if defs.is_empty() {
            return String::new();
        }
        let mut items = Vec::new();
        for (n, _, raw) in defs {
            let text = ex.data_list(&raw);
            let text = trim(&text).to_string();
            items.push(format!(
                "<sup><a id=\"fn.{n}\" href=\"#fnr.{n}\">{n}</a></sup> {text}\n"
            ));
        }
        let title = ex.translate("Footnotes", "html");
        format!(
            "{}{}",
            headline_title(1, &title, None, ""),
            items.join("\n")
        )
    }

    fn html_of(self, ex: &mut Exporter<'_>, id: Id) -> String {
        ex.with_backend(&Html, |ex| ex.data(id))
    }

    fn link(&self, ex: &mut Exporter<'_>, id: Id, desc: Option<String>) -> Option<String> {
        let info = ex.link_info(id)?;
        let ty = info.link_type.as_str();
        let raw = info.path.clone();
        let desc = desc.filter(|d| !d.trim().is_empty());
        let as_md = |p: &str| -> String {
            let lower = p.to_lowercase();
            match lower.strip_suffix(".org") {
                Some(stem) if !stem.is_empty() => format!("{}.md", &p[..stem.len()]),
                _ => p.to_string(),
            }
        };
        let path = if ty == "file" {
            html::file_uri(&as_md(&raw))
        } else {
            format!("{ty}:{raw}")
        };
        if !matches!(ty, "coderef" | "custom-id" | "fuzzy" | "radio")
            && let Some(out) = ex.custom_protocol(ty, &raw, desc.as_deref(), "md")
        {
            return Some(out);
        }
        match ty {
            "custom-id" | "id" | "fuzzy" => {
                let dest = if ty == "fuzzy" {
                    ex.resolve_fuzzy(&raw)
                } else {
                    ex.resolve_id(&raw)
                };
                let Some(dest) = dest else {
                    ex.broken_link(id, &raw);
                    return None;
                };
                if ex.tree.kind(dest) == Some(HEADLINE) {
                    let d = match desc {
                        Some(d) => d,
                        None if ex.numbered_p(dest) => ex
                            .headline_number(dest)
                            .unwrap_or_default()
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join("."),
                        None => {
                            let ids = ex
                                .tree
                                .secondary(dest, Secondary::Title)
                                .map(<[Id]>::to_vec)
                                .unwrap_or_default();
                            ex.data_list(&ids)
                        }
                    };
                    let r = ex
                        .node_property(dest, "CUSTOM_ID", false)
                        .unwrap_or_else(|| ex.reference(dest));
                    return Some(format!("[{d}](#{r})"));
                }
                let d = desc.or_else(|| ordinal(ex, dest))?;
                let r = ex.reference(dest);
                Some(format!("[{d}](#{r})"))
            }
            _ if desc.is_none()
                && html::image_path(ty, &raw)
                && ex.tree.children(id).is_empty() =>
            {
                let p = if ty != "file" {
                    format!("{ty}:{raw}")
                } else {
                    raw.clone()
                };
                let parent = ex
                    .tree
                    .ancestors(id)
                    .find(|a| ex.tree.kind(*a).is_some_and(|k| k.is_element()));
                let caption = match parent {
                    Some(p) => {
                        let c = html::caption_ids(ex, p);
                        ex.data_list(&c)
                    }
                    None => String::new(),
                };
                Some(if caption.trim().is_empty() {
                    format!("![img]({p})")
                } else {
                    format!("![img]({p} \"{caption}\")")
                })
            }
            "coderef" => {
                // ox-md resolves `coderef:REF`, the path it builds for
                // links that are not files, which never matches.
                let Some(target) = html::resolve_coderef(ex, &path) else {
                    ex.broken_link(id, &path);
                    return None;
                };
                Some(html::coderef_format(&path, desc.as_deref()).replace("%s", &target))
            }
            "radio" => {
                let dest = ex.resolve_radio(&raw);
                Some(match dest {
                    None => desc.unwrap_or_default(),
                    Some(d) => format!(
                        "<a href=\"#{}\">{}</a>",
                        ex.reference(d),
                        desc.unwrap_or_default()
                    ),
                })
            }
            _ => Some(match desc {
                None => format!("<{path}>"),
                Some(d) => format!("[{d}]({path})"),
            }),
        }
    }

    fn item(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> String {
        let list = ex.tree.parent(id).expect("a list");
        let bullet = if html::list_type(ex, list) != ListType::Ordered {
            "-".to_string()
        } else {
            format!("{}.", item_last_number(ex, id))
        };
        let pad = 4usize.saturating_sub(bullet.len()).max(1);
        let item: Option<ast::Item> = ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()));
        let cb = match item.as_ref().and_then(|i| i.checkbox()) {
            Some(ast::Checkbox::On) => "[X] ",
            Some(ast::Checkbox::Partial) => "[-] ",
            Some(ast::Checkbox::Off) => "[ ] ",
            None => "",
        };
        let tag = ex.tree.secondary(id, Secondary::Tag).map(<[Id]>::to_vec);
        let tag = tag
            .map(|t| format!("**{}:** ", ex.data_list(&t)))
            .unwrap_or_default();
        let body = contents
            .map(|c| trim(&prefix_lines(&c, "    ")).to_string())
            .unwrap_or_default();
        format!("{bullet}{}{cb}{tag}{body}", " ".repeat(pad))
    }
}

/// The last number of an item (`org-list-get-item-number`).
fn item_last_number(ex: &Exporter<'_>, item: Id) -> usize {
    let Some(list) = ex.tree.parent(item) else {
        return 1;
    };
    let mut n = 0;
    for &s in ex.tree.children(list) {
        let counter = ex
            .syntax(s)
            .and_then(|x| ast::AstNode::cast(x.clone()))
            .and_then(|i: ast::Item| i.counter());
        n = match counter {
            Some(c) => c as usize,
            None => n + 1,
        };
        if s == item {
            break;
        }
    }
    n
}

/// `org-export-get-ordinal` as a string, for link descriptions.
fn ordinal(ex: &mut Exporter<'_>, dest: Id) -> Option<String> {
    let mut el = dest;
    if ex.tree.kind(el) == Some(TARGET) {
        el = ex.tree.ancestors(el).find(|a| {
            matches!(
                ex.tree.kind(*a),
                Some(FOOTNOTE_DEFINITION | FOOTNOTE_REFERENCE | HEADLINE | ITEM | TABLE)
            )
        })?;
    }
    match ex.tree.kind(el)? {
        HEADLINE => ex
            .headline_number(el)
            .map(|n| n.iter().map(usize::to_string).collect::<Vec<_>>().join(".")),
        ITEM => Some(item_last_number(ex, el).to_string()),
        FOOTNOTE_DEFINITION | FOOTNOTE_REFERENCE => Some(ex.footnote_number(el).to_string()),
        k => {
            let mut n = 0;
            for d in ex.tree.descendants(ex.tree.root) {
                if ex.tree.kind(d) == Some(k) {
                    n += 1;
                }
                if d == el {
                    return Some(n.to_string());
                }
            }
            None
        }
    }
}

/// Titles in the table of contents.
#[derive(Debug, Clone, Copy)]
struct MdToc;

impl Backend for MdToc {
    fn name(&self) -> &'static str {
        "md-toc"
    }

    fn parents(&self) -> &'static [&'static str] {
        &["md", "html"]
    }

    fn has_transcoder(&self, kind: SyntaxKind) -> bool {
        Markdown.has_transcoder(kind)
    }

    fn transcode(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        match ex.tree.kind(id)? {
            FOOTNOTE_REFERENCE | TARGET => None,
            RADIO_TARGET => contents,
            LINK => Some(match contents {
                Some(c) => c,
                None => {
                    let raw = ex.link_info(id).map(|i| i.raw_link).unwrap_or_default();
                    let t = ex.tree.text_node(raw, None);
                    ex.data(t)
                }
            }),
            _ => Markdown.transcode(ex, id, contents),
        }
    }

    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        Markdown.plain_text(ex, text)
    }
}

/// `org-md-separate-elements`: a blank line after every element,
/// but items, rows, and a first paragraph before an item's last
/// sub-list.
fn separate_elements(ex: &mut Exporter<'_>) {
    for id in ex.tree.descendants(ex.tree.root) {
        let Some(k) = ex.tree.kind(id) else { continue };
        if !k.is_element() || matches!(k, ITEM | TABLE_ROW | DOCUMENT) {
            continue;
        }
        let tight = k == PARAGRAPH
            && ex
                .tree
                .parent(id)
                .is_some_and(|p| ex.tree.kind(p) == Some(ITEM))
            && ex.first_sibling_p(id)
            && ex.next_element(id).is_some_and(|n| {
                ex.tree.kind(n) == Some(PLAIN_LIST) && ex.next_element(n).is_none()
            });
        ex.tree.nodes[id].post_blank = if tight { 0 } else { 1 };
    }
}

impl Backend for Markdown {
    fn name(&self) -> &'static str {
        "md"
    }

    fn parents(&self) -> &'static [&'static str] {
        &["html"]
    }

    fn has_transcoder(&self, kind: SyntaxKind) -> bool {
        Html.has_transcoder(kind)
    }

    fn options(&self) -> Vec<crate::export::BackendOption> {
        Html.options()
    }

    fn filter_final_output(&self, _: &mut Exporter<'_>, out: String) -> String {
        out.replace(crate::kalem::END_MARK, "")
    }

    fn filter_parse_tree(&self, ex: &mut Exporter<'_>) {
        // The parent's filter, `org-html-image-link-filter`, runs after.
        separate_elements(ex);
        ex.insert_image_links(html::image_path);
    }

    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        let text = if ex.flag("with-smart-quotes") {
            ex.smart_quotes(ex.current_text, text, crate::quotes::Encoding::Html)
        } else {
            text.to_string()
        };
        let mut out = String::with_capacity(text.len());
        for c in text.chars() {
            if matches!(c, '`' | '*' | '_' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        let mut out = out.replace("\n#", "\n\\#");
        out = out.replace("![", "\\![");
        if ex.flag("with-special-strings") {
            out = html::special_strings(&out);
        }
        if ex.flag("preserve-breaks") {
            let mut s = String::new();
            let mut rest = out.as_str();
            while let Some(i) = rest.find('\n') {
                s.push_str(rest[..i].trim_end_matches([' ', '\t']));
                s.push_str("  \n");
                rest = &rest[i + 1..];
            }
            s.push_str(rest);
            out = s;
        }
        out
    }

    fn transcode(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        let kind = ex.tree.kind(id)?;
        let c = || contents.clone().unwrap_or_default();
        Some(match kind {
            BOLD => format!("**{}**", c()),
            ITALIC => format!("*{}*", c()),
            CODE | VERBATIM | INLINE_SRC_BLOCK => {
                let v = match kind {
                    CODE => ex
                        .syntax(id)
                        .and_then(|s| ast::AstNode::cast(s.clone()))
                        .map(|x: ast::Code| x.value()),
                    VERBATIM => ex
                        .syntax(id)
                        .and_then(|s| ast::AstNode::cast(s.clone()))
                        .map(|x: ast::Verbatim| x.value()),
                    _ => ex
                        .syntax(id)
                        .and_then(|s| ast::AstNode::cast(s.clone()))
                        .map(|x: ast::InlineSrcBlock| x.value()),
                }
                .unwrap_or_default();
                if !v.contains('`') {
                    format!("`{v}`")
                } else if v.starts_with('`') || v.ends_with('`') {
                    format!("`` {v} ``")
                } else {
                    format!("``{v}``")
                }
            }
            CENTER_BLOCK | INLINETASK | SPECIAL_BLOCK | TABLE => {
                let h = self.html_of(ex, id);
                // The HTML export added its own trailing blank lines.
                let blank = ex.tree.nodes[id].post_blank;
                let t = h.trim_end_matches('\n');
                let _ = blank;
                t.to_string()
            }
            DRAWER | DYNAMIC_BLOCK | PLAIN_LIST | SECTION => return contents,
            EXAMPLE_BLOCK | SRC_BLOCK | FIXED_WIDTH => {
                let code = if kind == FIXED_WIDTH {
                    let v = ex
                        .syntax(id)
                        .and_then(|s| ast::AstNode::cast(s.clone()))
                        .map(|f: ast::FixedWidth| f.value())
                        .unwrap_or_default();
                    let v = html::remove_indentation(&v);
                    // `org-export-format-code-default` on the value.
                    let v = v.strip_suffix('\n').unwrap_or(&v).to_string();
                    format!("{v}\n")
                } else {
                    format_code_default(ex, id)
                };
                prefix_lines(&html::remove_indentation(&code), "    ")
            }
            EXPORT_BLOCK => {
                let b: ast::ExportBlock =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                match b.backend().as_deref() {
                    Some("MARKDOWN" | "MD") => html::remove_indentation(&b.value()),
                    _ => return Html.transcode(ex, id, contents),
                }
            }
            EXPORT_SNIPPET => {
                let s: ast::ExportSnippet =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                match s.backend().as_str() {
                    "md" | "html" => s.value(),
                    _ => return None,
                }
            }
            HEADLINE => {
                if ex.footnote_section_p(id) {
                    return None;
                }
                let level = ex.relative_level(id);
                let ids = ex
                    .tree
                    .secondary(id, Secondary::Title)
                    .map(<[Id]>::to_vec)
                    .unwrap_or_default();
                let title = ex.data_list(&ids);
                let todo = if ex.flag("with-todo-keywords") {
                    ex.headline(id).and_then(|h| h.todo_keyword()).map(|t| {
                        let t = ex.tree.text_node(t.text().to_string(), None);
                        format!("{} ", ex.data(t))
                    })
                } else {
                    None
                };
                let tags = if ex.flag("with-tags") {
                    let t = ex.tags(id, &[], false);
                    (!t.is_empty()).then(|| format!("     {}", tag_string(&t)))
                } else {
                    None
                };
                let priority = if ex.flag("with-priority") {
                    ex.headline(id)
                        .and_then(|h| h.priority())
                        .map(|p| format!("[#{p}] "))
                } else {
                    None
                };
                let heading = format!(
                    "{}{}{title}",
                    todo.unwrap_or_default(),
                    priority.unwrap_or_default()
                );
                let tags = tags.unwrap_or_default();
                if ex.low_level_p(id).is_some() || level > 6 {
                    let bullet = if !ex.numbered_p(id) {
                        "-".to_string()
                    } else {
                        format!(
                            "{}.",
                            ex.headline_number(id)
                                .and_then(|n| n.last().copied())
                                .unwrap_or(0)
                        )
                    };
                    let pad = 4usize.saturating_sub(bullet.len());
                    let body = contents
                        .map(|c| prefix_lines(&c, "    "))
                        .unwrap_or_default();
                    format!("{bullet}{}{heading}{tags}\n\n{body}", " ".repeat(pad))
                } else {
                    let anchor = if self.headline_referred(ex, id) {
                        let r = ex
                            .node_property(id, "CUSTOM_ID", false)
                            .unwrap_or_else(|| ex.reference(id));
                        Some(format!("<a id=\"{r}\"></a>"))
                    } else {
                        None
                    };
                    format!(
                        "{}{}",
                        headline_title(level, &heading, anchor.as_deref(), &tags),
                        c()
                    )
                }
            }
            HORIZONTAL_RULE => "---".to_string(),
            ITEM => self.item(ex, id, contents),
            KEYWORD => {
                let k: ast::Keyword = ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                match k.key().as_str() {
                    "MARKDOWN" | "MD" => k.value(),
                    "TOC" => {
                        let value = k.value();
                        if value
                            .to_lowercase()
                            .split_whitespace()
                            .any(|w| w == "headlines")
                        {
                            let depth =
                                value.split_whitespace().find_map(|w| w.parse::<i64>().ok());
                            html::remove_indentation(&self.build_toc(ex, depth))
                        } else {
                            return None;
                        }
                    }
                    _ => return Html.transcode(ex, id, contents),
                }
            }
            LATEX_ENVIRONMENT => {
                if !ex.flag("with-latex") {
                    return None;
                }
                let v = ex
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|l: ast::LatexEnvironment| l.value())
                    .unwrap_or_default();
                let v = html::remove_indentation(&v);
                match Html.reference(ex, id, true) {
                    Some(l) => match v.split_once('\n') {
                        Some((first, rest)) => format!("{first}\n\\label{{{l}}}\n{rest}"),
                        None => format!("{v}\n\\label{{{l}}}"),
                    },
                    None => v,
                }
            }
            LATEX_FRAGMENT => {
                if !ex.flag("with-latex") {
                    return None;
                }
                let v = ex
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|l: ast::LatexFragment| l.value())
                    .unwrap_or_default();
                if v.starts_with("\\(") && v.len() >= 4 {
                    format!("${}$", &v[2..v.len() - 2])
                } else if v.starts_with("\\[") && v.len() >= 4 {
                    format!("$${}$$", &v[2..v.len() - 2])
                } else {
                    v
                }
            }
            LINE_BREAK => "  \n".to_string(),
            LINK => return self.link(ex, id, contents),
            NODE_PROPERTY => return Html.transcode(ex, id, contents),
            PARAGRAPH => {
                let c = remove_blank_lines(&c());
                let first = ex.tree.children(id).first().copied();
                if first
                    .is_some_and(|f| ex.tree.is_text(f) && ex.tree.nodes[f].text.starts_with('#'))
                {
                    format!("\\{c}")
                } else {
                    c
                }
            }
            PROPERTY_DRAWER => {
                let c = c();
                if c.trim().is_empty() {
                    return None;
                }
                prefix_lines(&c, "    ")
            }
            QUOTE_BLOCK => {
                let c = c();
                let c = c.strip_suffix('\n').unwrap_or(&c);
                prefix_lines(c, "> ")
            }
            _ => return Html.transcode(ex, id, contents),
        })
    }

    fn inner_template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let toc = match ex.opt("with-toc") {
            Value::Nil => String::new(),
            Value::Int(n) => format!("{}\n", self.build_toc(ex, Some(n))),
            _ => format!("{}\n", self.build_toc(ex, None)),
        };
        let foot = self.footnote_section(ex);
        format!("{toc}{body}\n{foot}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(prefix_lines("a\nb\n", "    "), "    a\n    b\n    ");
        assert_eq!(remove_blank_lines("a\n\n  \nb"), "a\nb");
    }
}
