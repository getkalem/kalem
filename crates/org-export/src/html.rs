//! The HTML back-end, as `ox-html.el` writes (XHTML strict, the default
//! settings of Emacs without customizations).

use org_syntax::SyntaxKind::{self, *};
use org_syntax::ast;

use crate::export::{Backend, Exporter, normalize_string, trim};
use crate::options::{Behavior, Value};
use crate::tree::{Id, Secondary};

/// The HTML back-end.
#[derive(Debug, Clone, Copy, Default)]
pub struct Html;

/// `org-html-encode-plain-text`.
pub fn encode(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// `org-html-convert-special-strings`.
pub fn special_strings(s: &str) -> String {
    let s = s.replace("\\-", "&#x00ad;");
    let s = dash(&s, "---", "&#x2014;");
    let s = dash(&s, "--", "&#x2013;");
    s.replace("...", "&#x2026;")
}

/// `DASHES\([^-]\)` replaced: dashes followed by something other than a
/// dash.
fn dash(s: &str, dashes: &str, with: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(dashes) {
        let after = &rest[i + dashes.len()..];
        match after.chars().next() {
            Some(c) if c != '-' => {
                out.push_str(&rest[..i]);
                out.push_str(with);
                rest = after;
            }
            _ => {
                // Not followed by a non-dash: skip one character.
                let c = rest[i..].chars().next().expect("a dash");
                out.push_str(&rest[..i + c.len_utf8()]);
                rest = &rest[i + c.len_utf8()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// `org-export-read-attribute`: `#+ATTR_HTML: :class x :id y` as pairs.
pub fn read_attribute(ex: &Exporter<'_>, id: Id, name: &str) -> Vec<(String, Option<String>)> {
    let Some(s) = ex.syntax(id) else {
        return Vec::new();
    };
    let values: Vec<String> = ast::affiliated_keywords(s)
        .filter(|k| k.key().eq_ignore_ascii_case(name))
        .map(|k| k.value())
        .collect();
    if values.is_empty() {
        return Vec::new();
    }
    let s = values.join(" ");
    parse_attributes(&s)
}

fn parse_attributes(s: &str) -> Vec<(String, Option<String>)> {
    // `\(?:^\|[ \t]+\)\(:[-a-zA-Z0-9_]+\)\([ \t]+\|$\)`
    let mut out: Vec<(String, Option<String>)> = Vec::new();
    let prepare = |v: &str| -> Option<String> {
        let v = v.trim();
        if v.is_empty() || v == "nil" {
            return None;
        }
        if v.len() >= 2
            && v.starts_with('"')
            && v.ends_with('"')
            && v[1..v.len() - 1].chars().all(|c| c == '"')
        {
            return Some(v[1..v.len() - 1].to_string());
        }
        Some(v.to_string())
    };
    let words: Vec<&str> = s.split([' ', '\t']).collect();
    let mut key: Option<String> = None;
    let mut value = String::new();
    for w in words {
        let is_key = w.len() > 1
            && w.starts_with(':')
            && w[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if is_key {
            if let Some(k) = key.take() {
                out.push((k, prepare(&value)));
            }
            key = Some(w.to_string());
            value.clear();
        } else if !w.is_empty() {
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(w);
        }
    }
    if let Some(k) = key {
        out.push((k, prepare(&value)));
    }
    out
}

/// `org-html--make-attribute-string`.
pub fn attribute_string(attrs: &[(String, Option<String>)]) -> String {
    let mut out: Vec<String> = Vec::new();
    for (k, v) in attrs {
        if let Some(v) = v {
            out.push(format!(
                "{}=\"{}\"",
                &k[1..],
                encode(v).replace('"', "&quot;")
            ));
        }
    }
    out.join(" ")
}

fn set_attr(attrs: &mut Vec<(String, Option<String>)>, key: &str, value: String) {
    match attrs.iter_mut().find(|(k, _)| k == key) {
        Some(a) => a.1 = Some(value),
        None => attrs.push((key.to_string(), Some(value))),
    }
}

fn has_attr(attrs: &[(String, Option<String>)], key: &str) -> bool {
    attrs.iter().any(|(k, _)| k == key)
}

fn get_attr<'a>(attrs: &'a [(String, Option<String>)], key: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| v.as_deref())
}

/// `org-html-fix-class-name`.
fn fix_class(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The inline image rules of `org-html-inline-image-rules`.
pub fn image_path(link_type: &str, path: &str) -> bool {
    let lower = path.to_lowercase();
    matches!(link_type, "file" | "http" | "https")
        && [".jpeg", ".jpg", ".png", ".gif", ".svg", ".webp", ".avif"]
            .iter()
            .any(|e| lower.contains(e))
}

impl Html {
    /// `org-html--reference`.
    pub fn reference(&self, ex: &mut Exporter<'_>, id: Id, named_only: bool) -> Option<String> {
        let k = ex.tree.kind(id)?;
        let custom = if matches!(k, HEADLINE | INLINETASK) {
            ex.node_property(id, "CUSTOM_ID", false)
        } else {
            None
        };
        if let Some(c) = custom {
            return Some(c);
        }
        let name = ex
            .syntax(id)
            .filter(|s| s.kind().is_element())
            .and_then(|s| {
                ast::affiliated_keywords(s)
                    .find(|k| k.key() == "NAME")
                    .map(|k| k.value())
            });
        let labeled = matches!(k, HEADLINE | INLINETASK | RADIO_TARGET | TARGET);
        if named_only && !labeled && name.is_none() {
            return None;
        }
        Some(ex.reference(id))
    }

    fn todo(&self, ex: &mut Exporter<'_>, id: Id) -> Option<String> {
        if !ex.flag("with-todo-keywords") {
            return None;
        }
        let kw = ex
            .headline(id)
            .and_then(|h| h.todo_keyword())
            .map(|t| t.text().to_string())
            .or_else(|| {
                ex.syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .and_then(|h: ast::Inlinetask| h.todo_keyword())
                    .map(|t| t.text().to_string())
            })?;
        let t = ex.tree.text_node(kw.clone(), None);
        let text = ex.data(t);
        let class = if ex.ctx.is_done_keyword(&kw) {
            "done"
        } else {
            "todo"
        };
        Some(format!(
            "<span class=\"{class} {}\">{text}</span>",
            fix_class(&text)
        ))
    }

    fn priority(&self, ex: &Exporter<'_>, id: Id) -> Option<String> {
        if !ex.flag("with-priority") {
            return None;
        }
        let p = ex.headline(id).and_then(|h| h.priority()).or_else(|| {
            ex.syntax(id)
                .and_then(|s| ast::AstNode::cast(s.clone()))
                .and_then(|h: ast::Inlinetask| h.priority())
        })?;
        Some(format!("<span class=\"priority\">[{p}]</span>"))
    }

    fn tags_html(tags: &[String]) -> Option<String> {
        if tags.is_empty() {
            return None;
        }
        Some(format!(
            "<span class=\"tag\">{}</span>",
            tags.iter()
                .map(|t| format!("<span class=\"{}\">{t}</span>", fix_class(t)))
                .collect::<Vec<_>>()
                .join("&#xa0;")
        ))
    }

    /// `org-html-format-headline-default-function`.
    fn format_headline(
        todo: Option<String>,
        priority: Option<String>,
        text: &str,
        tags: Option<String>,
    ) -> String {
        let mut out = String::new();
        if let Some(t) = todo {
            out.push_str(&t);
            out.push(' ');
        }
        if let Some(p) = priority {
            out.push_str(&p);
            out.push(' ');
        }
        out.push_str(text);
        if let Some(t) = tags {
            out.push_str("&#xa0;&#xa0;&#xa0;");
            out.push_str(&t);
        }
        out
    }

    fn title(&self, ex: &mut Exporter<'_>, id: Id) -> String {
        let ids = ex
            .tree
            .secondary(id, Secondary::Title)
            .map(<[Id]>::to_vec)
            .unwrap_or_default();
        crate::kalem::finish(&ex.data_list(&ids))
    }

    /// `org-html-toc`.
    pub fn toc(&self, ex: &mut Exporter<'_>, depth: Option<i64>) -> Option<String> {
        let headlines = collect_headlines(ex, depth);
        if headlines.is_empty() {
            return None;
        }
        let mut entries = Vec::new();
        for h in headlines {
            let level = ex.relative_level(h);
            let text = self.toc_headline(ex, h);
            entries.push((text, level));
        }
        let outer = if html5_fancy(ex) { "nav" } else { "div" };
        let mut out = format!(
            "<{outer} id=\"table-of-contents\" role=\"doc-toc\">\n<h2>{}</h2>\n<div id=\"text-table-of-contents\" role=\"doc-toc\">",
            ex.translate("Table of Contents", "html")
        );
        out.push_str(&toc_text(&entries));
        out.push_str(&format!("</div>\n</{outer}>\n"));
        Some(out)
    }

    /// `org-html-list-of-tables` and `org-html-list-of-listings`: the
    /// captioned elements of `kind`.
    fn list_of(
        &self,
        ex: &mut Exporter<'_>,
        kind: SyntaxKind,
        id: &str,
        title: &str,
        class: &str,
        number: &str,
    ) -> Option<String> {
        let entries: Vec<Id> = ex
            .tree
            .descendants(ex.tree.root)
            .into_iter()
            .filter(|&e| {
                ex.tree.kind(e) == Some(kind)
                    && ex.tree.secondary(e, Secondary::Caption(0)).is_some()
                    && reachable(ex, e)
            })
            .collect();
        if entries.is_empty() {
            return None;
        }
        let heading = ex.translate(&format!("List of {title}"), "html");
        let fmt = ex.translate(number, "html");
        let mut items = Vec::new();
        for (n, e) in entries.into_iter().enumerate() {
            let label = self.reference(ex, e, true);
            // The short caption, else the caption.
            let mut short: Vec<Id> = Vec::new();
            let mut i = 0;
            while let Some(sh) = ex.tree.secondary(e, Secondary::ShortCaption(i)) {
                short = sh.to_vec();
                i += 1;
            }
            let ids = if short.is_empty() {
                caption_ids(ex, e)
            } else {
                short
            };
            let text = ex.data_list(&ids);
            let text = crate::export::trim(&text).to_string();
            let num = format!(
                "<span class=\"{class}-number\">{}</span>",
                fmt.replace("%d", &(n + 1).to_string())
            );
            items.push(match label {
                Some(l) => format!("<li><a href=\"#{l}\">{num} {text}</a></li>"),
                None => format!("<li>{num} {text}</li>"),
            });
        }
        Some(format!(
            "<div id=\"list-of-{id}\">\n<h2>{heading}</h2>\n<div id=\"text-list-of-{id}\">\n<ul>\n{}\n</ul>\n</div>\n</div>",
            items.join("\n")
        ))
    }

    fn toc_headline(&self, ex: &mut Exporter<'_>, h: Id) -> String {
        let number = ex.headline_number(h);
        let todo = self.todo(ex, h);
        let priority = self.priority(ex, h);
        // The title (or alternative title), without footnote references
        // and links' targets (`org-export-toc-entry-backend`).
        let ids = ex.alt_title(h);
        let text = crate::kalem::finish(&ex.with_backend(&TocEntry, |ex| ex.data_list(&ids)));
        let tags = if ex.opt("with-tags") == Value::T {
            Self::tags_html(&ex.tags(h, &[], false))
        } else {
            None
        };
        let r = self.reference(ex, h, false).unwrap_or_default();
        let mut body = String::new();
        if ex.low_level_p(h).is_none()
            && ex.numbered_p(h)
            && let Some(n) = number
        {
            body.push_str(&n.iter().map(usize::to_string).collect::<Vec<_>>().join("."));
            body.push_str(". ");
        }
        body.push_str(&Self::format_headline(todo, priority, &text, tags));
        format!("<a href=\"#{r}\">{body}</a>")
    }

    /// `org-html-footnote-section`.
    fn footnote_section(&self, ex: &mut Exporter<'_>) -> Option<String> {
        let defs = ex.collect_footnote_definitions();
        if defs.is_empty() {
            return None;
        }
        let mut items = Vec::new();
        for (n, label, def) in defs {
            let label = label.filter(|l| {
                l.parse::<i64>()
                    .map(|v| v.to_string() != *l)
                    .unwrap_or(true)
            });
            let inline = !def.iter().any(|&d| {
                ex.tree
                    .descendants(d)
                    .into_iter()
                    .any(|x| ex.tree.kind(x).is_some_and(|k| k.is_element()))
            });
            let key = label.clone().unwrap_or_else(|| n.to_string());
            let anchor = format!(
                "<a id=\"fn.{key}\" class=\"footnum\" href=\"#fnr.{key}\" role=\"doc-backlink\">{n}</a>"
            );
            let contents = ex.data_list(&def);
            let contents = trim(&contents).to_string();
            items.push(format!(
                "<div class=\"footdef\"><sup>{anchor}</sup> <div class=\"footpara\" role=\"doc-footnote\">{}</div></div>\n",
                if inline {
                    format!("<p class=\"footpara\">{contents}</p>")
                } else {
                    contents
                }
            ));
        }
        Some(format!(
            "<div id=\"footnotes\">\n<h2 class=\"footnotes\">{}: </h2>\n<div id=\"text-footnotes\">\n\n{}\n\n</div>\n</div>",
            ex.translate("Footnotes", "html"),
            items.join("\n")
        ))
    }

    /// `org-html-format-code`: the code of a block, escaped, with line
    /// numbers and code references.
    fn format_code(&self, ex: &Exporter<'_>, id: Id) -> String {
        let (code, refs) = unravel_code(ex, id);
        let (numbers, retain) = number_lines(ex, id);
        let num_start = numbers.map(|_| get_loc(ex, id));
        let lines: Vec<&str> = code.split('\n').collect();
        let width = num_start.map(|n| (lines.len() + n).to_string().len());
        let mut out = String::new();
        for (i, l) in lines.iter().enumerate() {
            let line = i + 1;
            let r = refs
                .iter()
                .find(|(n, _)| *n == line)
                .map(|(_, r)| r.as_str());
            let mut loc = String::new();
            if let (Some(n), Some(w)) = (num_start, width) {
                loc.push_str(&format!("<span class=\"linenr\">{:>w$}: </span>", n + line));
            }
            loc.push_str(&encode(l));
            if let Some(r) = r
                && retain
            {
                loc.push_str(&format!(" ({r})"));
            }
            if let Some(r) = r {
                loc = format!("<span id=\"coderef-{r}\" class=\"coderef-off\">{loc}</span>");
            }
            out.push_str(&loc);
            out.push('\n');
        }
        out
    }

    fn standalone_image_p(&self, ex: &Exporter<'_>, id: Id) -> bool {
        let para = match ex.tree.kind(id) {
            Some(PARAGRAPH) => id,
            Some(LINK) => match ex.tree.parent(id) {
                Some(p) => p,
                None => return false,
            },
            _ => return false,
        };
        if ex.tree.kind(para) != Some(PARAGRAPH) {
            return false;
        }
        let mut links = 0;
        for &c in ex.tree.children(para) {
            if ex.tree.is_text(c) {
                if !ex.tree.nodes[c].text.trim().is_empty() {
                    return false;
                }
                continue;
            }
            if ex.tree.kind(c) == Some(LINK) {
                links += 1;
                if links > 1 || !self.inline_image_p(ex, c) {
                    return false;
                }
            } else {
                return false;
            }
        }
        links == 1
    }

    fn inline_image_p(&self, ex: &Exporter<'_>, id: Id) -> bool {
        if !ex.tree.children(id).is_empty() {
            // A description that is a single image link.
            let mut count = 0;
            for d in ex.tree.children(id) {
                for x in ex.tree.descendants(*d) {
                    if ex.tree.is_text(x) {
                        if !ex.tree.nodes[x].text.trim().is_empty() {
                            return false;
                        }
                    } else if ex.tree.kind(x) == Some(LINK) {
                        count += 1;
                        if count > 1 || !self.inline_image_p(ex, x) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
            }
            return true;
        }
        let Some(info) = ex.link_info(id) else {
            return false;
        };
        image_path(&info.link_type, &info.path)
    }

    fn format_image(
        &self,
        ex: &Exporter<'_>,
        source: &str,
        attrs: &[(String, Option<String>)],
    ) -> String {
        let mut a: Vec<(String, Option<String>)> = vec![(":src".into(), Some(source.to_string()))];
        let alt = source.rsplit('/').next().unwrap_or(source).to_string();
        a.push((":alt".into(), Some(alt)));
        if source.rsplit('.').next().is_some_and(|e| e == "svg") {
            a.push((":class".into(), Some("org-svg".into())));
        }
        for (k, v) in attrs {
            match a.iter_mut().find(|(x, _)| x == k) {
                Some(x) => x.1 = v.clone(),
                None => a.push((k.clone(), v.clone())),
            }
        }
        format!("<img {}{}", attribute_string(&a), close(ex))
    }

    fn link(&self, ex: &mut Exporter<'_>, id: Id, desc: Option<String>) -> Option<String> {
        let info = ex.link_info(id)?;
        let desc = desc.filter(|d| !d.trim().is_empty());
        let ty = info.link_type.as_str();
        let raw = info.path.clone();
        let path = if ty == "file" {
            let mut p = file_uri(&raw);
            // `html-link-use-abs-url`: relative names under
            // `#+HTML_LINK_HOME`.
            let home = option_string(ex, "html-link-home")
                .map(|h| h.trim().to_string())
                .filter(|h| !h.is_empty());
            if let Some(home) = home
                && ex.flag("html-link-use-abs-url")
                && !(p.starts_with('/') || p.starts_with('~'))
            {
                let sep = if home.ends_with('/') { "" } else { "/" };
                p = format!("{home}{sep}{p}");
            }
            // `.org` files become `.html`.
            let lower = p.to_lowercase();
            if let Some(stem) = lower
                .strip_suffix(".org")
                .or_else(|| lower.strip_suffix(".org.gpg"))
                .map(|s| p[..s.len()].to_string())
                .filter(|s| !s.is_empty())
            {
                p = format!("{stem}.html");
            }
            match &info.search_option {
                Some(opt) => match external_anchor(ex, &raw, opt) {
                    Ok(a) => format!("{p}#{a}"),
                    Err(message) => {
                        ex.broken_link(id, &message);
                        return None;
                    }
                },
                None => p,
            }
        } else {
            url_encode(&format!("{ty}:{raw}"))
        };
        // Link types with their own export function.
        if !matches!(ty, "coderef" | "custom-id" | "fuzzy" | "radio")
            && let Some(out) = ex.custom_protocol(ty, &raw, desc.as_deref(), "html")
        {
            return Some(out);
        }
        let attrs_plist = {
            // Attributes of the paragraph apply to its first link.
            let parent = ex
                .tree
                .ancestors(id)
                .find(|a| ex.tree.kind(*a).is_some_and(|k| k.is_element()));
            let first = parent.and_then(|p| {
                ex.tree
                    .descendants(p)
                    .into_iter()
                    .find(|&d| ex.tree.kind(d) == Some(LINK))
            });
            match (parent, first) {
                (Some(p), Some(f)) if f == id => read_attribute(ex, p, "ATTR_HTML"),
                _ => Vec::new(),
            }
        };
        let attrs = {
            let a = attribute_string(&attrs_plist);
            if a.is_empty() {
                String::new()
            } else {
                format!(" {a}")
            }
        };
        if desc.is_none() && image_path(ty, &raw) && ex.tree.children(id).is_empty() {
            return Some(self.format_image(ex, &path, &attrs_plist));
        }
        match ty {
            "radio" => {
                let dest = ex.resolve_radio(&raw);
                Some(match dest {
                    None => desc.unwrap_or_default(),
                    Some(d) => {
                        let r = ex.reference(d);
                        format!("<a href=\"#{r}\"{attrs}>{}</a>", desc.unwrap_or_default())
                    }
                })
            }
            "custom-id" | "fuzzy" | "id" => {
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
                    let href = self.reference(ex, dest, false).unwrap_or_default();
                    let d = match desc {
                        Some(d) => d,
                        None if ex.numbered_p(dest) => ex
                            .headline_number(dest)
                            .unwrap_or_default()
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join("."),
                        None => self.title(ex, dest),
                    };
                    return Some(format!("<a href=\"#{href}\"{attrs}>{d}</a>"));
                }
                // A math environment with MathJax: its label is in the
                // formula, referred to with `\eqref`.
                let with = ex.opt("with-latex");
                if (with == Value::T || with.sym() == Some("mathjax"))
                    && ex.tree.kind(dest) == Some(LATEX_ENVIRONMENT)
                    && crate::latex::math_environment(&remove_indentation(
                        &ex.syntax(dest)
                            .and_then(|s| ast::AstNode::cast(s.clone()))
                            .map(|l: ast::LatexEnvironment| l.value())
                            .unwrap_or_default(),
                    ))
                {
                    let r = self.reference(ex, dest, false).unwrap_or_default();
                    let format = option_string(ex, "html-equation-reference-format")
                        .unwrap_or_else(|| "\\eqref{%s}".into());
                    return Some(format.replacen("%s", &r, 1));
                }
                let r = self.reference(ex, dest, false).unwrap_or_default();
                let d = match desc {
                    Some(d) => d,
                    None => {
                        let number = self.ordinal(ex, dest);
                        match number {
                            Some(n) => n,
                            None => "No description for this link".to_string(),
                        }
                    }
                };
                Some(format!("<a href=\"#{r}\"{attrs}>{d}</a>"))
            }
            "coderef" => {
                let frag = format!("coderef-{}", encode(&raw));
                let Some(target) = resolve_coderef(ex, &raw) else {
                    ex.broken_link(id, &raw);
                    return None;
                };
                Some(format!(
                    "<a href=\"#{frag}\" class=\"coderef\" onmouseover=\"CodeHighlightOn(this, '{frag}');\" onmouseout=\"CodeHighlightOff(this, '{frag}');\"{attrs}>{}</a>",
                    coderef_format(&raw, desc.as_deref()).replace("%s", &target)
                ))
            }
            _ => Some(match desc {
                Some(d) => format!("<a href=\"{}\"{attrs}>{d}</a>", encode(&path)),
                None => {
                    let p = encode(&path);
                    format!("<a href=\"{p}\"{attrs}>{p}</a>")
                }
            }),
        }
    }

    /// The number a link to an element shows: its ordinal among captioned
    /// elements of its type (`org-export-get-ordinal`), or a target's
    /// container's number.
    fn ordinal(&self, ex: &mut Exporter<'_>, dest: Id) -> Option<String> {
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
            ITEM => Some(item_number(ex, el)),
            FOOTNOTE_DEFINITION | FOOTNOTE_REFERENCE => Some(ex.footnote_number(el).to_string()),
            k => {
                let has_caption = |ex: &Exporter<'_>, x: Id| {
                    ex.syntax(x)
                        .is_some_and(|s| ast::affiliated_keywords(s).any(|a| a.key() == "CAPTION"))
                };
                let mut n = 0;
                for d in ex.tree.descendants(ex.tree.root) {
                    if ex.tree.kind(d) != Some(k) {
                        continue;
                    }
                    let counts = has_caption(ex, d);
                    if d == el {
                        return counts.then(|| (n + 1).to_string());
                    }
                    if counts {
                        n += 1;
                    }
                }
                None
            }
        }
    }

    fn table(&self, ex: &mut Exporter<'_>, id: Id, contents: String) -> String {
        // A table.el table: its cells, as `table-generate-source` writes
        // them for HTML.
        if let Some(t) = ex
            .syntax(id)
            .and_then(|n| <ast::Table as ast::AstNode>::cast(n.clone()))
            && t.table_type() == ast::TableType::TableEl
        {
            return table_el_html(&t.table_el_value().unwrap_or_default());
        }
        let caption = caption_ids(ex, id);
        let mut attrs: Vec<(String, Option<String>)> = Vec::new();
        if let Some(r) = self.reference(ex, id, true) {
            attrs.push((":id".into(), Some(r)));
        }
        // `org-html-table-attributes`, which HTML5 leaves out.
        let presentational: &[(&str, &str)] = if html5(ex) {
            &[]
        } else {
            &[
                (":border", "2"),
                (":cellspacing", "0"),
                (":cellpadding", "6"),
                (":rules", "groups"),
                (":frame", "hsides"),
            ]
        };
        for (k, v) in presentational {
            attrs.push((k.to_string(), Some(v.to_string())));
        }
        for (k, v) in read_attribute(ex, id, "ATTR_HTML") {
            match attrs.iter_mut().find(|(x, _)| *x == k) {
                Some(x) => x.1 = v,
                None => attrs.push((k, v)),
            }
        }
        let attributes = attribute_string(&attrs);
        let caption_html = if caption.is_empty() {
            String::new()
        } else {
            let number = self.ordinal(ex, id).unwrap_or_default();
            let c = ex.data_list(&caption);
            let label = ex.translate("Table %d:", "html").replacen("%d", &number, 1);
            format!(
                "<caption class=\"t-above\"><span class=\"table-number\">{label}</span> {c}</caption>"
            )
        };
        // Column specifications from the first data row.
        let first = ex.table_rows(id).into_iter().find(|r| !ex.rule_row_p(*r));
        let mut cols = Vec::new();
        if let Some(row) = first {
            let special = ex.table_has_special_column(id);
            let cells: Vec<Id> = ex
                .row_cells(row)
                .into_iter()
                .skip(usize::from(special))
                .collect();
            for c in cells {
                let align = ex.cell_alignment(c);
                let starts = colgroup_starts(ex, c);
                let ends = colgroup_ends(ex, c);
                cols.push(format!(
                    "{}\n<col  class=\"org-{align}\"{}{}",
                    if starts { "\n<colgroup>" } else { "" },
                    close(ex),
                    if ends { "\n</colgroup>" } else { "" }
                ));
            }
        }
        format!(
            "<table{}>\n{}\n{}\n{}</table>",
            if attributes.is_empty() {
                String::new()
            } else {
                format!(" {attributes}")
            },
            caption_html,
            cols.join("\n"),
            contents
        )
    }

    fn table_row(&self, ex: &mut Exporter<'_>, id: Id, contents: String) -> Option<String> {
        if ex.rule_row_p(id) {
            return None;
        }
        let table = ex.tree.parent(id)?;
        let group = ex.row_group(id)?;
        let rows = ex.table_rows(table);
        let pos = rows.iter().position(|r| *r == id)?;
        let prev = pos.checked_sub(1).map(|i| rows[i]);
        let next = rows.get(pos + 1).copied();
        let starts = prev.is_none_or(|p| ex.rule_row_p(p))
            || prev.and_then(|p| ex.row_group(p)) != Some(group);
        let ends = next.is_none_or(|n| ex.rule_row_p(n))
            || next.and_then(|n| ex.row_group(n)) != Some(group);
        let tags = if group != 1 {
            ("<tbody>", "\n</tbody>")
        } else if ex.table_has_header(table) {
            ("<thead>", "\n</thead>")
        } else {
            ("<tbody>", "\n</tbody>")
        };
        Some(format!(
            "{}\n<tr>{contents}\n</tr>{}",
            if starts { tags.0 } else { "" },
            if ends { tags.1 } else { "" }
        ))
    }

    fn table_cell(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> String {
        let row = ex.tree.parent(id).expect("a row");
        let table = ex.tree.parent(row).expect("a table");
        let align = format!(" class=\"org-{}\"", ex.cell_alignment(id));
        let contents = match contents {
            Some(c) if !trim(&c).is_empty() => crate::kalem::finish(&c),
            _ => "&#xa0;".to_string(),
        };
        if ex.table_has_header(table) && ex.row_group(row) == Some(1) {
            format!("\n<th scope=\"col\"{align}>{contents}</th>")
        } else {
            format!("\n<td{align}>{contents}</td>")
        }
    }

    fn paragraph(&self, ex: &mut Exporter<'_>, id: Id, contents: String) -> String {
        let contents = crate::kalem::finish(&contents);
        let parent = ex.tree.parent(id);
        let parent_kind = parent.and_then(|p| ex.tree.kind(p));
        let mut attributes = read_attribute(ex, id, "ATTR_HTML");
        // Kalem's alignment and spacing: `#+ATTR_KALEM: :align right
        // :before 12 :after 6`.
        let kalem = read_attribute(ex, id, "ATTR_KALEM");
        let get = |key: &str| {
            kalem
                .iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, v)| v.clone())
                .map(|v| v.trim().to_ascii_lowercase())
        };
        let mut styles = Vec::new();
        if let Some(align) =
            get(":align").filter(|v| ["left", "right", "center", "justify"].contains(&v.as_str()))
        {
            styles.push(format!("text-align: {align}"));
        }
        for (key, prop) in [(":before", "margin-top"), (":after", "margin-bottom")] {
            if let Some(v) = get(key)
                .map(|v| v.trim_end_matches("pt").to_string())
                .filter(|v| v.parse::<f64>().is_ok_and(|x| (0.0..=1000.0).contains(&x)))
            {
                styles.push(format!("{prop}: {v}pt"));
            }
        }
        if !styles.is_empty() {
            let style = styles.join("; ");
            match attributes.iter_mut().find(|(k, _)| k == ":style") {
                Some((_, Some(v))) => *v = format!("{}; {style}", v.trim_end_matches(';')),
                Some((_, v)) => *v = Some(style),
                None => attributes.push((":style".into(), Some(style))),
            }
        }
        let attrs = attribute_string(&attributes);
        let extra = match parent_kind {
            Some(FOOTNOTE_DEFINITION) => " class=\"footpara\"",
            // A paragraph export made (a bibliography entry) has no parent,
            // unlike one parsed on its own (`org-data`).
            None if ex.tree.syntax(id).is_none() => "",
            None => " class=\"footpara\"",
            _ => "",
        };
        if parent_kind == Some(ITEM) && ex.previous_element(id).is_none() {
            let next = ex.next_element(id);
            let after = next.and_then(|n| ex.next_element(n));
            if after.is_none() && next.is_none_or(|n| ex.tree.kind(n) == Some(PLAIN_LIST)) {
                return contents;
            }
        }
        if self.standalone_image_p(ex, id) {
            let label = self.reference(ex, id, false).unwrap_or_default();
            let caption = caption_ids(ex, id);
            let raw = ex.data_list(&caption);
            let caption_html = if raw.trim().is_empty() {
                raw
            } else {
                let n = figure_number(self, ex, id);
                let label = ex
                    .translate("Figure %d:", "html")
                    .replacen("%d", &n.to_string(), 1);
                format!("<span class=\"figure-number\">{label} </span>{raw}")
            };
            let caption = !caption_html.trim().is_empty();
            if html5_fancy(ex) {
                return format!(
                    "\n<figure id=\"{label}\">\n{contents}{}\n</figure>",
                    if caption {
                        format!("\n<figcaption>{caption_html}</figcaption>")
                    } else {
                        String::new()
                    }
                );
            }
            return format!(
                "\n<div id=\"{label}\" class=\"figure\">\n<p>{contents}</p>{}\n</div>",
                if caption {
                    format!("\n<p>{caption_html}</p>")
                } else {
                    String::new()
                }
            );
        }
        format!(
            "<p{}{}>\n{}</p>",
            if attrs.is_empty() {
                String::new()
            } else {
                format!(" {attrs}")
            },
            extra,
            contents
        )
    }

    fn headline(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        if ex.footnote_section_p(id) {
            return None;
        }
        let numbered = ex.numbered_p(id);
        let numbers = ex.headline_number(id);
        let level = ex.relative_level(id) + 1;
        let todo = self.todo(ex, id);
        let priority = self.priority(ex, id);
        let text = self.title(ex, id);
        let tags = if ex.flag("with-tags") {
            Self::tags_html(&ex.tags(id, &[], false))
        } else {
            None
        };
        let full = Self::format_headline(todo, priority, &text, tags);
        let contents = contents.unwrap_or_default();
        let r = self.reference(ex, id, false).unwrap_or_default();
        if ex.low_level_p(id).is_some() {
            let html_type = if numbered { "ol" } else { "ul" };
            let mut out = String::new();
            if ex.first_sibling_p(id) {
                out.push_str(&format!("<{html_type} class=\"org-{html_type}\">\n"));
            }
            let headline = format!("<a id=\"{r}\"></a>{full}");
            out.push_str(&format_list_item(
                &contents,
                if numbered {
                    ListType::Ordered
                } else {
                    ListType::Unordered
                },
                None,
                None,
                Some(&headline),
                &br(ex),
            ));
            out.push('\n');
            if ex.last_sibling_p(id) {
                out.push_str(&format!("</{html_type}>\n"));
            }
            return Some(out);
        }
        let extra_class = ex.node_property(id, "HTML_CONTAINER_CLASS", false);
        let headline_class = ex.node_property(id, "HTML_HEADLINE_CLASS", false);
        let container = ex
            .node_property(id, "HTML_CONTAINER", false)
            .unwrap_or_else(|| {
                if ex.relative_level(id) == 1 {
                    option_string(ex, "html-container").unwrap_or_else(|| "div".into())
                } else {
                    "div".into()
                }
            });
        let first = ex.tree.children(id).first().copied();
        let body = match first {
            Some(f) if ex.tree.kind(f) == Some(SECTION) => contents,
            // A sub-headline first: an empty section of this headline
            // (for `org-info.js`); nothing at all: nothing.
            Some(_) => {
                let s = self.section(ex, id, true, String::new());
                format!("{s}{contents}")
            }
            None => contents,
        };
        let number = if numbered {
            format!(
                "<span class=\"section-number-{level}\">{}.</span> ",
                numbers
                    .unwrap_or_default()
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(".")
            )
        } else {
            String::new()
        };
        Some(format!(
            "<{container} id=\"outline-container-{r}\" class=\"outline-{level}{}\">\n<h{level} id=\"{r}\"{}>{number}{full}</h{level}>\n{body}</{container}>\n",
            extra_class.map(|c| format!(" {c}")).unwrap_or_default(),
            headline_class
                .map(|c| format!(" class=\"{c}\""))
                .unwrap_or_default(),
        ))
    }

    /// `org-html-section`; `of_headline`: an empty section made up for a
    /// headline without one.
    fn section(
        &self,
        ex: &mut Exporter<'_>,
        id: Id,
        of_headline: bool,
        contents: String,
    ) -> String {
        let parent = if of_headline {
            Some(id)
        } else {
            ex.tree.ancestor(id, HEADLINE)
        };
        let Some(parent) = parent else {
            return contents;
        };
        let class_num = ex.relative_level(parent) + 1;
        let number = if ex.numbered_p(parent) {
            ex.headline_number(parent)
                .map(|n| n.iter().map(usize::to_string).collect::<Vec<_>>().join("-"))
        } else {
            None
        };
        let id_text = ex
            .node_property(parent, "CUSTOM_ID", false)
            .or(number)
            .unwrap_or_else(|| ex.reference(parent));
        format!(
            "<div class=\"outline-text-{class_num}\" id=\"text-{id_text}\">\n{contents}</div>\n"
        )
    }

    fn item(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> String {
        let list = ex.tree.parent(id).expect("a list");
        let ty = list_type(ex, list);
        let item: Option<ast::Item> = ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()));
        let counter = item.as_ref().and_then(|i| i.counter());
        let checkbox = item.as_ref().and_then(|i| i.checkbox());
        let tag = ex.tree.secondary(id, Secondary::Tag).map(<[Id]>::to_vec);
        let tag = tag.map(|t| ex.data_list(&t));
        let term = tag.or(counter.map(|c| c.to_string()));
        format_list_item(
            &contents.unwrap_or_default(),
            ty,
            checkbox,
            term.as_deref(),
            None,
            &br(ex),
        )
    }
}

/// Plain list types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListType {
    /// `1.` items.
    Ordered,
    /// `-` items.
    Unordered,
    /// `- term ::` items.
    Descriptive,
}

/// A plain list's type.
pub fn list_type(ex: &Exporter<'_>, list: Id) -> ListType {
    let t = ex
        .syntax(list)
        .and_then(|s| ast::AstNode::cast(s.clone()))
        .map(|l: ast::PlainList| l.list_type());
    match t {
        Some(ast::ListType::Ordered) => ListType::Ordered,
        Some(ast::ListType::Descriptive) => ListType::Descriptive,
        _ => ListType::Unordered,
    }
}

/// `org-html-format-list-item`.
fn format_list_item(
    contents: &str,
    ty: ListType,
    checkbox: Option<ast::Checkbox>,
    term_counter: Option<&str>,
    headline: Option<&str>,
    br: &str,
) -> String {
    let (class, cb) = match checkbox {
        Some(ast::Checkbox::On) => (" class=\"on\"", "<code>[X]</code> "),
        Some(ast::Checkbox::Off) => (" class=\"off\"", "<code>[&#xa0;]</code> "),
        Some(ast::Checkbox::Partial) => (" class=\"trans\"", "<code>[-]</code> "),
        None => ("", ""),
    };
    let nonempty = !contents.trim().is_empty();
    let extra = if nonempty && headline.is_some() {
        "\n"
    } else {
        ""
    };
    let mut out = String::new();
    match ty {
        ListType::Ordered => {
            let extra_attr = term_counter
                .map(|c| format!(" value=\"{c}\""))
                .unwrap_or_default();
            out.push_str(&format!("<li{class}{extra_attr}>"));
            if let Some(h) = headline {
                out.push_str(h);
                out.push_str(br);
            }
        }
        ListType::Unordered => {
            let extra_attr = term_counter
                .map(|c| format!(" id=\"{c}\""))
                .unwrap_or_default();
            out.push_str(&format!("<li{class}{extra_attr}>"));
            if let Some(h) = headline {
                out.push_str(h);
                out.push_str(br);
            }
        }
        ListType::Descriptive => {
            let term = term_counter.unwrap_or("(no term)");
            out.push_str(&format!("<dt{class}>{cb}{term}</dt><dd>"));
        }
    }
    if ty != ListType::Descriptive {
        out.push_str(cb);
    }
    out.push_str(extra);
    if nonempty {
        out.push_str(trim(contents));
    }
    out.push_str(extra);
    out.push_str(match ty {
        ListType::Descriptive => "</dd>",
        _ => "</li>",
    });
    out
}

/// An item's number, as `1.2` (`org-list-get-item-number`).
fn item_number(ex: &Exporter<'_>, item: Id) -> String {
    let mut nums = Vec::new();
    let mut cur = item;
    while let Some(list) = ex.tree.parent(cur) {
        let siblings = ex.tree.children(list);
        let mut n = 0;
        for &s in siblings {
            let counter = ex
                .syntax(s)
                .and_then(|x| ast::AstNode::cast(x.clone()))
                .and_then(|i: ast::Item| i.counter());
            n = match counter {
                Some(c) => c as usize,
                None => n + 1,
            };
            if s == cur {
                break;
            }
        }
        nums.push(n);
        match ex.tree.parent(list) {
            Some(p) if ex.tree.kind(p) == Some(ITEM) => cur = p,
            _ => break,
        }
    }
    nums.reverse();
    nums.iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(".")
}

fn figure_number(h: &Html, ex: &Exporter<'_>, para: Id) -> usize {
    let mut n = 0;
    for d in ex.tree.descendants(ex.tree.root) {
        if ex.tree.kind(d) != Some(PARAGRAPH) {
            continue;
        }
        let has_caption = ex
            .syntax(d)
            .is_some_and(|s| ast::affiliated_keywords(s).any(|a| a.key() == "CAPTION"));
        if has_caption && h.standalone_image_p(ex, d) {
            n += 1;
        }
        if d == para {
            break;
        }
    }
    n
}

/// The caption of an element, lines joined with a space.
pub fn caption_ids(ex: &mut Exporter<'_>, id: Id) -> Vec<Id> {
    let mut out: Vec<Id> = Vec::new();
    let mut i = 0;
    while let Some(c) = ex
        .tree
        .secondary(id, Secondary::Caption(i))
        .map(<[Id]>::to_vec)
    {
        if !out.is_empty() {
            let sp = ex.tree.text_node(" ".into(), Some(id));
            out.push(sp);
        }
        out.extend(c);
        i += 1;
    }
    out
}

pub(crate) fn colgroup_starts(ex: &Exporter<'_>, cell: Id) -> bool {
    let row = ex.tree.parent(cell).expect("a row");
    let first = ex
        .tree
        .children(row)
        .iter()
        .copied()
        .find(|c| !ex.info.ignore.contains(c));
    first == Some(cell) || cell_borders(ex, cell).0
}

fn colgroup_ends(ex: &Exporter<'_>, cell: Id) -> bool {
    let row = ex.tree.parent(cell).expect("a row");
    ex.tree.children(row).last() == Some(&cell) || cell_borders(ex, cell).1
}

/// `org-export-table-cell-borders`, left and right: from the last `/`
/// row, a left border where the cell starts a group or the previous
/// one ends one, a right border where it ends one or the next starts
/// one.
pub(crate) fn cell_borders(ex: &Exporter<'_>, cell: Id) -> (bool, bool) {
    let row = ex.tree.parent(cell).expect("a row");
    let table = ex.tree.parent(row).expect("a table");
    let column = ex
        .tree
        .children(row)
        .iter()
        .position(|&c| c == cell)
        .unwrap_or(0);
    for &r in ex.tree.children(table).iter().rev() {
        let cells = ex.tree.children(r);
        let Some(&first) = cells.first() else {
            continue;
        };
        if ex.tree.source(first).trim().trim_matches('|').trim() != "/" {
            continue;
        }
        let groups: Vec<String> = cells
            .iter()
            .map(|&c| {
                ex.tree
                    .source(c)
                    .trim()
                    .trim_matches('|')
                    .trim()
                    .to_string()
            })
            .collect();
        let at = |i: usize| groups.get(i).map(String::as_str).unwrap_or("");
        let left = (column > 0 && matches!(at(column - 1), ">" | "<>"))
            || matches!(at(column), "<" | "<>");
        let right = (column + 1 != groups.len() && matches!(at(column + 1), "<" | "<>"))
            || matches!(at(column), ">" | "<>");
        return (left, right);
    }
    (false, false)
}

/// `org-export-collect-headlines`: headlines up to `depth` levels.
pub fn collect_headlines(ex: &Exporter<'_>, depth: Option<i64>) -> Vec<Id> {
    let limit = ex.opt("headline-levels").int().unwrap_or(3);
    let n = match depth {
        Some(d) => d.min(limit),
        None => limit,
    };
    ex.tree
        .descendants(ex.tree.root)
        .into_iter()
        .filter(|&h| {
            ex.tree.kind(h) == Some(HEADLINE)
                && !ex.footnote_section_p(h)
                && ex.node_property(h, "UNNUMBERED", false).as_deref() != Some("notoc")
                && ex.relative_level(h) <= n
                && reachable(ex, h)
        })
        .collect()
}

/// Whether `id` is still in the tree (not pruned).
pub fn reachable(ex: &Exporter<'_>, id: Id) -> bool {
    let mut cur = id;
    while let Some(p) = ex.tree.parent(cur) {
        let n = &ex.tree.nodes[p];
        if !n.children.contains(&cur) && !n.secondary.iter().any(|(_, v)| v.contains(&cur)) {
            return false;
        }
        cur = p;
    }
    cur == ex.tree.root
}

/// `org-html--toc-text`.
fn toc_text(entries: &[(String, i64)]) -> String {
    let mut prev = entries[0].1 - 1;
    let start = prev;
    let mut out = String::new();
    for (text, level) in entries {
        let cnt = level - prev;
        let times = if cnt > 0 { cnt - 1 } else { -cnt };
        prev = *level;
        let s = if cnt > 0 {
            "\n<ul>\n<li>"
        } else if cnt < 0 {
            "</li>\n</ul>\n"
        } else {
            ""
        };
        out.push_str(&s.repeat(times.max(0) as usize));
        out.push_str(if cnt > 0 {
            "\n<ul>\n<li>"
        } else {
            "</li>\n<li>"
        });
        out.push_str(text);
    }
    out.push_str(&"</li>\n</ul>\n".repeat((prev - start).max(0) as usize));
    out
}

/// `org-export-unravel-code`: a block's code without its common
/// indentation and final line feed, and its code references.
pub fn unravel_code(ex: &Exporter<'_>, id: Id) -> (String, Vec<(usize, String)>) {
    let Some(s) = ex.syntax(id) else {
        return (String::new(), Vec::new());
    };
    let (value, switches) = match s.kind() {
        SRC_BLOCK => {
            let b: Option<ast::SrcBlock> = ast::AstNode::cast(s.clone());
            (
                b.as_ref().map(|b| b.value()).unwrap_or_default(),
                b.and_then(|b| b.switches()),
            )
        }
        EXAMPLE_BLOCK => {
            let b: Option<ast::ExampleBlock> = ast::AstNode::cast(s.clone());
            (
                b.as_ref().map(|b| b.value()).unwrap_or_default(),
                b.and_then(|b| b.switches()),
            )
        }
        _ => (String::new(), None),
    };
    let preserve = switches
        .as_deref()
        .is_some_and(|sw| sw.split_whitespace().any(|w| w == "-i"));
    let code = if preserve {
        value
    } else {
        remove_indentation(&value)
    };
    let code = code.strip_suffix('\n').unwrap_or(&code).to_string();
    // Code references: `(ref:name)` (or the `-l` format) at line ends.
    let fmt = label_format(switches.as_deref());
    let (pre, post) = fmt.split_once("%s").unwrap_or((fmt.as_str(), ""));
    let mut refs = Vec::new();
    let mut lines = Vec::new();
    for (i, l) in code.split('\n').enumerate() {
        let t = l.trim_end_matches([' ', '\t']);
        let found = t.strip_suffix(post).and_then(|x| {
            let at = x.rfind(pre)?;
            let label = &x[at + pre.len()..];
            let ok = label
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ' ');
            ok.then(|| (at, label.to_string()))
        });
        match found {
            Some((at, label)) => {
                refs.push((i + 1, label));
                lines.push(l[..at].trim_end_matches([' ', '\t']).to_string());
            }
            None => lines.push(l.to_string()),
        }
    }
    (lines.join("\n"), refs)
}

/// The code reference format of a block's switches (`-l "…"`).
fn label_format(switches: Option<&str>) -> String {
    if let Some(sw) = switches
        && let Some(i) = sw.find("-l \"")
        && let Some(e) = sw[i + 4..].find('"')
    {
        return sw[i + 4..i + 4 + e].to_string();
    }
    "(ref:%s)".to_string()
}

/// `:number-lines` and `:retain-labels` of a block: `(new?, start)` and
/// whether labels stay in the code.
pub fn number_lines(ex: &Exporter<'_>, id: Id) -> (Option<(bool, usize)>, bool) {
    let switches = ex.syntax(id).and_then(|s| match s.kind() {
        SRC_BLOCK => ast::AstNode::cast(s.clone()).and_then(|b: ast::SrcBlock| b.switches()),
        EXAMPLE_BLOCK => {
            ast::AstNode::cast(s.clone()).and_then(|b: ast::ExampleBlock| b.switches())
        }
        _ => None,
    });
    let Some(sw) = switches else {
        return (None, true);
    };
    let mut numbers = None;
    let words: Vec<&str> = sw.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if *w == "-n" || *w == "+n" {
            let n = words
                .get(i + 1)
                .and_then(|x| x.parse::<usize>().ok())
                .map_or(0, |x| x.saturating_sub(1));
            numbers = Some((*w == "-n", n));
        }
    }
    let retain = !words.contains(&"-r") || (numbers.is_some() && words.contains(&"-k"));
    (numbers, retain)
}

/// `org-export-get-coderef-format`: the description with `(REF)` as the
/// place of the reference, or just the reference.
pub fn coderef_format(path: &str, desc: Option<&str>) -> String {
    match desc {
        None => "%s".into(),
        Some(d) => d.replace(&format!("({path})"), "%s"),
    }
}

/// `org-export-resolve-coderef`: the line number of the label `r` in the
/// first block that has it, or the label when the block keeps labels.
pub fn resolve_coderef(ex: &Exporter<'_>, r: &str) -> Option<String> {
    for d in ex.tree.descendants(ex.tree.root) {
        let Some(s) = ex.syntax(d) else { continue };
        let (switches, value) = match s.kind() {
            SRC_BLOCK => {
                let b: ast::SrcBlock = ast::AstNode::cast(s.clone())?;
                (b.switches(), b.value())
            }
            EXAMPLE_BLOCK => {
                let b: ast::ExampleBlock = ast::AstNode::cast(s.clone())?;
                (b.switches(), b.value())
            }
            _ => continue,
        };
        let label = label_format(switches.as_deref()).replace("%s", r);
        let value = value.trim_matches([' ', '\t', '\n', '\r']);
        // The last line ending with the label, blanks around it allowed.
        let found = value
            .split('\n')
            .enumerate()
            .filter(|(_, l)| l.trim_end_matches([' ', '\t']).ends_with(&label))
            .last();
        let Some((line, _)) = found else { continue };
        let words: Vec<&str> = switches
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .collect();
        let (numbers, retain) = number_lines(ex, d);
        let _ = numbers;
        let use_labels = switches.is_none() || (retain && !words.contains(&"-k"));
        return Some(if use_labels {
            r.to_string()
        } else {
            (get_loc(ex, d) + line + 1).to_string()
        });
    }
    None
}

/// `org-export-get-loc`: the line number before the block's first line.
pub fn get_loc(ex: &Exporter<'_>, id: Id) -> usize {
    let (numbers, _) = number_lines(ex, id);
    let Some((new, n)) = numbers else {
        return 0;
    };
    if new {
        return n;
    }
    let mut loc = 0;
    for d in ex.tree.descendants(ex.tree.root) {
        if !matches!(ex.tree.kind(d), Some(SRC_BLOCK | EXAMPLE_BLOCK)) {
            continue;
        }
        if d == id {
            return loc + n;
        }
        if let (Some((new_d, nd)), _) = number_lines(ex, d) {
            let lines = unravel_code(ex, d).0.split('\n').count();
            loc = if new_d { nd + lines } else { loc + nd + lines };
        }
    }
    loc + n
}

/// `org-remove-indentation`: the smallest indentation of non-blank lines
/// taken away, column by column (a tab across the cut stays, the
/// characters before it go); blank lines lose their blanks.
pub fn remove_indentation(s: &str) -> String {
    let column_of = |l: &str| {
        let mut w = 0;
        for c in l.chars() {
            match c {
                ' ' => w += 1,
                '\t' => w = (w / 8 + 1) * 8,
                _ => break,
            }
        }
        w
    };
    let mut min = usize::MAX;
    for l in s.split('\n') {
        if l.trim().is_empty() {
            continue;
        }
        let ind = column_of(l);
        if ind == 0 {
            return s.to_string();
        }
        min = min.min(ind);
    }
    let n = if min == usize::MAX { s.len() + 1 } else { min };
    let mut out = String::with_capacity(s.len());
    let lines: Vec<&str> = s.split('\n').collect();
    for (k, l) in lines.iter().enumerate() {
        if k > 0 {
            out.push('\n');
        }
        let ind = column_of(l);
        if ind < n {
            // A blank line: its blanks go (others cannot be shorter).
            if l.trim().is_empty() {
                continue;
            }
            out.push_str(l);
            continue;
        }
        let mut col = 0;
        let mut rest = *l;
        while col < n {
            let Some(c) = rest.chars().next() else { break };
            match c {
                ' ' => {
                    col += 1;
                    rest = &rest[1..];
                }
                '\t' => {
                    let next = (col / 8 + 1) * 8;
                    if next > n {
                        // `move-to-column` with `indent-tabs-mode`: spaces
                        // go before the tab, which stays.
                        break;
                    }
                    rest = &rest[1..];
                    col = next;
                }
                _ => break,
            }
        }
        out.push_str(rest);
    }
    out
}

/// `org-export-file-uri`.
pub fn file_uri(path: &str) -> String {
    if path.starts_with("//") {
        return format!("file:{path}");
    }
    let absolute = path.starts_with('/') || path.starts_with('~');
    if !absolute {
        return path.to_string();
    }
    let full = if let Some(rest) = path.strip_prefix('~') {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}{rest}")
    } else {
        path.to_string()
    };
    format!("file://{full}")
}

/// `org-publish-resolve-external-link` without a publishing project, as
/// Emacs runs it for HTML: `#id` is the custom ID; otherwise the search
/// runs in the file (`org-link-search`, headings matched exactly) and a
/// heading found gives its `CUSTOM_ID`, anything else found
/// `MissingReference`. `Err` is the message of a broken link.
fn external_anchor(ex: &Exporter<'_>, path: &str, opt: &str) -> Result<String, String> {
    if let Some(id) = opt.strip_prefix('#') {
        return Ok(id.to_string());
    }
    let dir = ex
        .info
        .input_file
        .as_deref()
        .and_then(std::path::Path::parent)
        .unwrap_or(std::path::Path::new("."));
    let file = crate::export::expand_file_name(path, dir);
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let normalized = opt.replace('\n', " ");
    let starred = normalized.starts_with('*');
    let words: Vec<String> = opt
        .strip_prefix('*')
        .unwrap_or(opt)
        .split_whitespace()
        .map(str::to_uppercase)
        .collect();
    if words.is_empty() {
        return Err(format!("Invalid search string \"{opt}\""));
    }
    // Another kind of file: a plain text search for the words.
    let org = file.ends_with(".org")
        || file.ends_with(".org_archive")
        || text
            .lines()
            .next()
            .is_some_and(|l| l.contains("-*-") && l.to_lowercase().contains("org"));
    if !org {
        let hay: Vec<String> = text.split_whitespace().map(str::to_uppercase).collect();
        let found = hay.windows(words.len()).any(|w| {
            w.iter().zip(&words).enumerate().all(|(i, (h, want))| {
                if words.len() == 1 {
                    h.contains(want.as_str())
                } else if i == 0 {
                    h.ends_with(want.as_str())
                } else if i + 1 == words.len() {
                    h.starts_with(want.as_str())
                } else {
                    h == want
                }
            })
        });
        return if found {
            Ok("MissingReference".into())
        } else {
            Err(format!("No match for fuzzy expression: {normalized}"))
        };
    }
    let p = org_syntax::parse(&text);
    let root = p.syntax();
    let same = |s: &str| {
        s.split_whitespace()
            .map(str::to_uppercase)
            .collect::<Vec<_>>()
            == words
    };
    // Coderefs, regular expressions: somewhere, not a heading.
    if normalized.starts_with('(') && normalized.ends_with(')') {
        let label = &normalized[1..normalized.len() - 1];
        let found = root.descendants().any(|n| {
            matches!(n.kind(), SRC_BLOCK | EXAMPLE_BLOCK)
                && n.text().to_string().contains(&format!("(ref:{label})"))
        });
        return if found {
            Ok("MissingReference".into())
        } else {
            Err(format!("No match for coderef: {label}"))
        };
    }
    if normalized.len() > 1 && normalized.starts_with('/') && normalized.ends_with('/') {
        return Ok("MissingReference".into());
    }
    if !starred {
        let target = root.descendants().any(|n| {
            n.kind() == TARGET
                && ast::AstNode::cast(n.clone()).is_some_and(|t: ast::Target| same(&t.value()))
        });
        let named = root
            .descendants()
            .any(|n| ast::affiliated_keywords(&n).any(|k| k.key() == "NAME" && same(&k.value())));
        if target || named {
            return Ok("MissingReference".into());
        }
    }
    // A heading: TODO keyword, priority, COMMENT, tags and statistics
    // cookies do not count.
    for n in root.descendants().filter(|n| n.kind() == HEADLINE) {
        let Some(h): Option<ast::Headline> = ast::AstNode::cast(n.clone()) else {
            continue;
        };
        if !same(&without_cookies(&h.raw_value())) {
            continue;
        }
        let custom = h
            .properties()
            .into_iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("CUSTOM_ID"))
            .map(|(_, v)| v.trim().to_string())
            .filter(|v| !v.is_empty());
        return Ok(custom.unwrap_or_else(|| "MissingReference".into()));
    }
    Err(format!("No match for fuzzy expression: {normalized}"))
}

/// `s` with statistics cookies (`[1/3]`, `[50%]`) replaced by spaces.
fn without_cookies(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'[' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let close = match b.get(j) {
                Some(b'%') => Some(j + 1),
                Some(b'/') => {
                    let mut k = j + 1;
                    while k < b.len() && b[k].is_ascii_digit() {
                        k += 1;
                    }
                    Some(k)
                }
                _ => None,
            };
            if let Some(c) = close
                && b.get(c) == Some(&b']')
            {
                out.push(' ');
                i = c + 1;
                continue;
            }
        }
        let ch = s[i..].chars().next().expect("a char");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// `url-encode-url` for the characters Org links carry.
fn url_encode(s: &str) -> String {
    let s = url_normalize(s);
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii()
            && !c.is_ascii_control()
            && c != ' '
            && c != '"'
            && c != '<'
            && c != '>'
            && c != '`'
            && c != '{'
            && c != '}'
            && c != '|'
            && c != '\\'
            && c != '^'
        {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

/// The normalization of `url-generic-parse-url` and `url-recreate-url`:
/// the scheme and the host in lower case, a default port left out.
fn url_normalize(s: &str) -> String {
    let scheme_len = s
        .char_indices()
        .take_while(|(i, c)| {
            if *i == 0 {
                c.is_ascii_alphabetic()
            } else {
                c.is_ascii_alphanumeric() || matches!(c, '-' | '+' | '.')
            }
        })
        .count();
    if scheme_len == 0 || s.as_bytes().get(scheme_len) != Some(&b':') {
        return s.to_string();
    }
    let scheme = s[..scheme_len].to_ascii_lowercase();
    let rest = &s[scheme_len + 1..];
    let Some(after) = rest.strip_prefix("//") else {
        return format!("{scheme}:{rest}");
    };
    let end = after.find(['/', '?', '#']).unwrap_or(after.len());
    let (authority, tail) = after.split_at(end);
    let (user, hostport) = match authority.find('@') {
        Some(i) => (&authority[..=i], &authority[i + 1..]),
        None => ("", authority),
    };
    let (host, port) = if hostport.starts_with('[') {
        match hostport.find(']') {
            Some(i) => (&hostport[..=i], &hostport[i + 1..]),
            None => (hostport, ""),
        }
    } else {
        match hostport.rfind(':') {
            Some(i) if hostport[i + 1..].bytes().all(|b| b.is_ascii_digit()) => {
                (&hostport[..i], &hostport[i..])
            }
            _ => (hostport, ""),
        }
    };
    let default = match scheme.as_str() {
        "http" => ":80",
        "https" => ":443",
        "ftp" => ":21",
        _ => "",
    };
    let port = if port == ":" || (!default.is_empty() && port == default) {
        ""
    } else {
        port
    };
    format!("{scheme}://{user}{}{port}{tail}", host.to_lowercase())
}

/// The back-end the table of contents uses for titles: no footnote
/// references, links as their description.
#[derive(Debug, Clone, Copy)]
struct TocEntry;

impl Backend for TocEntry {
    fn name(&self) -> &'static str {
        "toc"
    }

    fn has_transcoder(&self, kind: SyntaxKind) -> bool {
        Html.has_transcoder(kind)
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
            _ => Html.transcode(ex, id, contents),
        }
    }

    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        Html.plain_text(ex, text)
    }
}

impl Backend for Html {
    fn name(&self) -> &'static str {
        "html"
    }

    fn filter_parse_tree(&self, ex: &mut Exporter<'_>) {
        infojs_install_script(ex);
        // `org-html-image-link-filter`.
        ex.insert_image_links(image_path);
    }

    fn has_transcoder(&self, kind: SyntaxKind) -> bool {
        matches!(
            kind,
            BOLD | CENTER_BLOCK
                | CLOCK
                | CODE
                | DRAWER
                | DYNAMIC_BLOCK
                | ENTITY
                | EXAMPLE_BLOCK
                | EXPORT_BLOCK
                | EXPORT_SNIPPET
                | FIXED_WIDTH
                | FOOTNOTE_REFERENCE
                | HEADLINE
                | HORIZONTAL_RULE
                | INLINE_SRC_BLOCK
                | INLINETASK
                | ITALIC
                | ITEM
                | KEYWORD
                | LATEX_ENVIRONMENT
                | LATEX_FRAGMENT
                | LINE_BREAK
                | LINK
                | NODE_PROPERTY
                | PARAGRAPH
                | PLAIN_LIST
                | PLANNING
                | PROPERTY_DRAWER
                | QUOTE_BLOCK
                | RADIO_TARGET
                | SECTION
                | SPECIAL_BLOCK
                | SRC_BLOCK
                | STATISTICS_COOKIE
                | STRIKE_THROUGH
                | SUBSCRIPT
                | SUPERSCRIPT
                | TABLE
                | TABLE_CELL
                | TABLE_ROW
                | TARGET
                | TIMESTAMP
                | UNDERLINE
                | VERBATIM
                | VERSE_BLOCK
        )
    }

    fn options(&self) -> Vec<crate::export::BackendOption> {
        vec![
            // `org-html-creator-string`.
            (
                "creator",
                Some("CREATOR"),
                None,
                Behavior::First,
                Value::Str(
                    "<a href=\"https://www.gnu.org/software/emacs/\">Emacs</a> 30.1 (<a href=\"https://orgmode.org\">Org</a> mode 9.7.11)"
                        .into(),
                ),
            ),
            (
                "html-doctype",
                Some("HTML_DOCTYPE"),
                None,
                Behavior::First,
                Value::Str("xhtml-strict".into()),
            ),
            (
                "description",
                Some("DESCRIPTION"),
                None,
                Behavior::Newline,
                Value::Nil,
            ),
            (
                "keywords",
                Some("KEYWORDS"),
                None,
                Behavior::Space,
                Value::Nil,
            ),
            (
                "html-head",
                Some("HTML_HEAD"),
                None,
                Behavior::Newline,
                Value::Nil,
            ),
            (
                "html-head-extra",
                Some("HTML_HEAD_EXTRA"),
                None,
                Behavior::Newline,
                Value::Nil,
            ),
            (
                "subtitle",
                Some("SUBTITLE"),
                None,
                Behavior::Parse,
                Value::Nil,
            ),
            (
                "html-postamble",
                None,
                Some("html-postamble"),
                Behavior::First,
                Value::Sym("auto".into()),
            ),
            (
                "html-preamble",
                None,
                Some("html-preamble"),
                Behavior::First,
                Value::T,
            ),
            (
                "html-head-include-default-style",
                None,
                Some("html-style"),
                Behavior::First,
                Value::T,
            ),
            (
                "html-head-include-scripts",
                None,
                Some("html-scripts"),
                Behavior::First,
                Value::Nil,
            ),
            (
                "html5-fancy",
                None,
                Some("html5-fancy"),
                Behavior::First,
                Value::Nil,
            ),
            (
                "html-container",
                Some("HTML_CONTAINER"),
                None,
                Behavior::First,
                Value::Str("div".into()),
            ),
            (
                "html-content-class",
                Some("HTML_CONTENT_CLASS"),
                None,
                Behavior::First,
                Value::Str("content".into()),
            ),
            (
                "infojs-opt",
                Some("INFOJS_OPT"),
                None,
                Behavior::First,
                Value::Nil,
            ),
            (
                "html-link-use-abs-url",
                None,
                Some("html-link-use-abs-url"),
                Behavior::First,
                Value::Nil,
            ),
            (
                "html-link-home",
                Some("HTML_LINK_HOME"),
                None,
                Behavior::First,
                Value::Str(String::new()),
            ),
            (
                "html-link-up",
                Some("HTML_LINK_UP"),
                None,
                Behavior::First,
                Value::Str(String::new()),
            ),
            (
                "html-mathjax",
                Some("HTML_MATHJAX"),
                None,
                Behavior::Space,
                Value::Str(String::new()),
            ),
            (
                "html-equation-reference-format",
                Some("HTML_EQUATION_REFERENCE_FORMAT"),
                None,
                Behavior::Last,
                Value::Str("\\eqref{%s}".into()),
            ),
        ]
    }

    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        let mut out = encode(text);
        if ex.flag("with-smart-quotes") {
            out = ex.smart_quotes(ex.current_text, &out, crate::quotes::Encoding::Html);
        }
        if ex.flag("with-special-strings") {
            out = special_strings(&out);
        }
        if ex.flag("preserve-breaks") {
            out = preserve_breaks(&out, &format!("{}\n", br(ex)));
        }
        out
    }

    fn transcode(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        let kind = ex.tree.kind(id)?;
        let c = || contents.clone().unwrap_or_default();
        Some(match kind {
            BOLD => format!("<b>{}</b>", c()),
            ITALIC => format!("<i>{}</i>", c()),
            UNDERLINE => format!("<span class=\"underline\">{}</span>", c()),
            STRIKE_THROUGH => format!("<del>{}</del>", c()),
            CODE => {
                let v = ex
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|x: ast::Code| x.value())
                    .unwrap_or_default();
                format!("<code>{}</code>", encode(&v))
            }
            VERBATIM => {
                let v = ex
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|x: ast::Verbatim| x.value())
                    .unwrap_or_default();
                format!("<code>{}</code>", encode(&v))
            }
            CENTER_BLOCK => format!("<div class=\"org-center\">\n{}</div>", c()),
            DYNAMIC_BLOCK | DRAWER => return contents,
            ENTITY => {
                let e: Option<ast::Entity> =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()));
                e.and_then(|e| e.html()).unwrap_or("").to_string()
            }
            EXAMPLE_BLOCK => {
                let mut attrs = read_attribute(ex, id, "ATTR_HTML");
                let class = match get_attr(&attrs, ":class") {
                    Some(cl) => format!("example {cl}"),
                    None => "example".to_string(),
                };
                set_attr(&mut attrs, ":class", class);
                if let Some(r) = self.reference(ex, id, false)
                    && !has_attr(&attrs, ":id")
                {
                    set_attr(&mut attrs, ":id", r);
                }
                let a = attribute_string(&attrs);
                format!(
                    "<pre{}>\n{}</pre>",
                    if a.is_empty() {
                        String::new()
                    } else {
                        format!(" {a}")
                    },
                    self.format_code(ex, id)
                )
            }
            EXPORT_SNIPPET => {
                let s: ast::ExportSnippet =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                if s.backend() == "html" {
                    s.value()
                } else if s.backend() == "kalem" {
                    // Kalem's formatting; a span ends where its container
                    // does at the latest (`kalem::finish`).
                    match crate::kalem::Format::parse(&s.value()) {
                        Some(f) => crate::kalem::open_tag(&f),
                        None => crate::kalem::END_MARK.to_string(),
                    }
                } else {
                    return None;
                }
            }
            EXPORT_BLOCK => {
                let b: ast::ExportBlock =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                if b.backend().as_deref() == Some("HTML") {
                    remove_indentation(&b.value())
                } else {
                    return None;
                }
            }
            FIXED_WIDTH => {
                let v = ex
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|f: ast::FixedWidth| f.value())
                    .unwrap_or_default();
                let code = remove_indentation(&v);
                let mut out = String::new();
                for l in code.split('\n') {
                    out.push_str(&encode(l));
                    out.push('\n');
                }
                format!("<pre class=\"example\">\n{out}</pre>")
            }
            FOOTNOTE_REFERENCE => {
                let sep = match ex.previous_element(id) {
                    Some(p) if ex.tree.kind(p) == Some(FOOTNOTE_REFERENCE) => "<sup>, </sup>",
                    _ => "",
                };
                let n = ex.footnote_number(id);
                let label = ex
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .and_then(|f: ast::FootnoteReference| f.label())
                    .filter(|l| {
                        l.parse::<i64>()
                            .map(|v| v.to_string() != *l)
                            .unwrap_or(true)
                    });
                let key = label.clone().unwrap_or_else(|| n.to_string());
                let first = ex.footnote_first_reference_p(id);
                let rid = if first {
                    format!("fnr.{key}")
                } else {
                    // `org-export-get-ordinal` of a footnote reference is
                    // its footnote number.
                    let k = n;
                    format!("fnr.{key}.{k}")
                };
                format!(
                    "{sep}<sup><a id=\"{rid}\" class=\"footref\" href=\"#fn.{key}\" role=\"doc-backlink\">{n}</a></sup>"
                )
            }
            HEADLINE => return self.headline(ex, id, contents),
            HORIZONTAL_RULE => format!("<hr{}", close(ex)),
            INLINE_SRC_BLOCK => {
                let b: ast::InlineSrcBlock =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                let label = self
                    .reference(ex, id, true)
                    .map(|l| format!(" id=\"{l}\""))
                    .unwrap_or_default();
                format!(
                    "<code class=\"src src-{}\"{label}>{}</code>",
                    b.language(),
                    encode(&b.value())
                )
            }
            INLINETASK => {
                let todo = self.todo(ex, id);
                let priority = self.priority(ex, id);
                let text = self.title(ex, id);
                let tags = if ex.flag("with-tags") {
                    Self::tags_html(&ex.tags(id, &[], false))
                } else {
                    None
                };
                format!(
                    "<div class=\"inlinetask\">\n<b>{}</b>{}\n{}</div>",
                    Self::format_headline(todo, priority, &text, tags),
                    br(ex),
                    c()
                )
            }
            ITEM => self.item(ex, id, contents),
            KEYWORD => {
                let k: ast::Keyword = ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                let key = k.key();
                let value = k.value();
                if key == "HTML" {
                    value
                } else if key == "TOC" {
                    let lower = value.to_lowercase();
                    if lower.split_whitespace().any(|w| w == "headlines") {
                        let depth = value.split_whitespace().find_map(|w| w.parse::<i64>().ok());
                        return self.toc(ex, depth);
                    }
                    if lower.split_whitespace().any(|w| w == "tables") {
                        return self.list_of(ex, TABLE, "tables", "Tables", "table", "Table %d:");
                    }
                    if lower.split_whitespace().any(|w| w == "listings") {
                        return self.list_of(
                            ex,
                            SRC_BLOCK,
                            "listings",
                            "Listings",
                            "listing",
                            "Listing %d:",
                        );
                    }
                    return None;
                } else {
                    return None;
                }
            }
            LATEX_ENVIRONMENT => {
                let v = ex
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|l: ast::LatexEnvironment| l.value())
                    .unwrap_or_default();
                let v = remove_indentation(&v);
                if let Some(img) = svg_formula(ex, &v) {
                    let id_attr = self
                        .reference(ex, id, true)
                        .map(|l| format!(" id=\"{l}\""))
                        .unwrap_or_default();
                    return Some(format!(
                        "<div{id_attr} class=\"equation-container\">\n<span class=\"equation\">\n{img}\n</span>\n</div>"
                    ));
                }
                // MathJax: the environment as it is, a label after its
                // first line.
                match self.reference(ex, id, true) {
                    Some(l) => match v.split_once('\n') {
                        Some((first, rest)) => format!("{first}\n\\label{{{l}}}\n{rest}"),
                        None => format!("{v}\n\\label{{{l}}}"),
                    },
                    None => v,
                }
            }
            LATEX_FRAGMENT => {
                let v = ex
                    .syntax(id)
                    .and_then(|s| ast::AstNode::cast(s.clone()))
                    .map(|l: ast::LatexFragment| l.value())
                    .unwrap_or_default();
                if let Some(img) = svg_formula(ex, &v) {
                    return Some(img);
                }
                // MathJax: `$…$` as `\(…\)`, `$$…$$` as `\[…\]`.
                if let Some(inner) = v.strip_prefix("$$").and_then(|x| x.strip_suffix("$$")) {
                    format!("\\[{inner}\\]")
                } else if let Some(inner) = v.strip_prefix('$').and_then(|x| x.strip_suffix('$')) {
                    format!("\\({inner}\\)")
                } else {
                    v
                }
            }
            LINE_BREAK => format!("{}\n", br(ex)),
            LINK => return self.link(ex, id, contents),
            NODE_PROPERTY => {
                let p: ast::NodeProperty =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                let v = p.value();
                format!(
                    "{}:{}",
                    p.key(),
                    if v.is_empty() {
                        String::new()
                    } else {
                        format!(" {v}")
                    }
                )
            }
            PARAGRAPH => self.paragraph(ex, id, c()),
            PLAIN_LIST => {
                let ty = match list_type(ex, id) {
                    ListType::Ordered => "ol",
                    ListType::Unordered => "ul",
                    ListType::Descriptive => "dl",
                };
                let mut attrs = read_attribute(ex, id, "ATTR_HTML");
                let class = match get_attr(&attrs, ":class") {
                    Some(cl) => format!("org-{ty} {cl}"),
                    None => format!("org-{ty}"),
                };
                set_attr(&mut attrs, ":class", class.trim().to_string());
                format!("<{ty} {}>\n{}</{ty}>", attribute_string(&attrs), c())
            }
            PLANNING => {
                let p: ast::Planning = ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                let mut parts = String::new();
                for (label, ts) in [
                    ("CLOSED:", p.closed()),
                    ("DEADLINE:", p.deadline()),
                    ("SCHEDULED:", p.scheduled()),
                ] {
                    if let Some(t) = ts {
                        let raw = crate::timestamps::interpret(ast::AstNode::syntax(&t));
                        let v = self.plain_text(ex, &raw);
                        parts.push_str(&format!(
                            "<span class=\"timestamp-kwd\">{label}</span> <span class=\"timestamp\">{v}</span> "
                        ));
                    }
                }
                format!(
                    "<p><span class=\"timestamp-wrapper\">{}</span></p>",
                    trim(&parts)
                )
            }
            PROPERTY_DRAWER => {
                let c = c();
                if c.trim().is_empty() {
                    return None;
                }
                format!("<pre class=\"example\">\n{c}</pre>")
            }
            QUOTE_BLOCK => {
                let mut attrs = read_attribute(ex, id, "ATTR_HTML");
                if let Some(r) = self.reference(ex, id, true)
                    && !has_attr(&attrs, ":id")
                {
                    set_attr(&mut attrs, ":id", r);
                }
                let a = attribute_string(&attrs);
                format!(
                    "<blockquote{}>\n{}</blockquote>",
                    if a.is_empty() {
                        String::new()
                    } else {
                        format!(" {a}")
                    },
                    c()
                )
            }
            RADIO_TARGET => {
                let r = self.reference(ex, id, false).unwrap_or_default();
                format!("<a id=\"{r}\">{}</a>", c())
            }
            SECTION => {
                if ex.tree.ancestor(id, HEADLINE).is_none() {
                    return contents;
                }
                self.section(ex, id, false, c())
            }
            SPECIAL_BLOCK => {
                let b: ast::SpecialBlock =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                let ty = b.block_type();
                let mut attrs = read_attribute(ex, id, "ATTR_HTML");
                let class = match get_attr(&attrs, ":class") {
                    Some(cl) => format!("{cl} {ty}"),
                    None => ty.clone(),
                };
                set_attr(&mut attrs, ":class", class);
                if let Some(r) = self.reference(ex, id, false)
                    && !has_attr(&attrs, ":id")
                {
                    set_attr(&mut attrs, ":id", r);
                }
                // HTML5 elements are written as themselves.
                let fancy = html5_fancy(ex) && HTML5_ELEMENTS.contains(&ty.as_str());
                if fancy {
                    attrs = read_attribute(ex, id, "ATTR_HTML");
                    if let Some(r) = self.reference(ex, id, false)
                        && !has_attr(&attrs, ":id")
                    {
                        set_attr(&mut attrs, ":id", r);
                    }
                }
                let a = attribute_string(&attrs);
                let a = if a.is_empty() {
                    String::new()
                } else {
                    format!(" {a}")
                };
                if fancy {
                    format!("<{ty}{a}>\n{}</{ty}>", c())
                } else {
                    format!("<div{a}>\n{}\n</div>", c())
                }
            }
            SRC_BLOCK => {
                let b: ast::SrcBlock = ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                let lang = b.language().unwrap_or_else(|| "nil".into());
                let code = self.format_code(ex, id);
                let label = self
                    .reference(ex, id, true)
                    .map(|l| format!(" id=\"{l}\""))
                    .unwrap_or_default();
                let caption = caption_ids(ex, id);
                let caption_html = if caption.is_empty() {
                    String::new()
                } else {
                    let n = self.ordinal(ex, id).unwrap_or_default();
                    let c = ex.data_list(&caption);
                    format!(
                        "<label class=\"org-src-name\"><span class=\"listing-number\">Listing {n}: </span>{}</label>",
                        trim(&c)
                    )
                };
                format!(
                    "<div class=\"org-src-container\">\n{caption_html}<pre class=\"src src-{lang}\"{label}>{code}</pre>\n</div>"
                )
            }
            STATISTICS_COOKIE => {
                let s: ast::StatisticsCookie =
                    ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                format!("<code>{}</code>", s.value())
            }
            SUBSCRIPT => format!("<sub>{}</sub>", c()),
            SUPERSCRIPT => format!("<sup>{}</sup>", c()),
            TABLE => self.table(ex, id, c()),
            TABLE_ROW => return self.table_row(ex, id, c()),
            TABLE_CELL => self.table_cell(ex, id, contents),
            TARGET => {
                let r = self.reference(ex, id, false).unwrap_or_default();
                format!("<a id=\"{r}\"></a>")
            }
            TIMESTAMP => {
                // `org-timestamp-translate`: the timestamp written
                // again, with the blanks after it.
                let raw = crate::timestamps::interpret(ex.syntax(id)?);
                let v = self.plain_text(ex, &raw);
                format!(
                    "<span class=\"timestamp-wrapper\"><span class=\"timestamp\">{}</span></span>",
                    v.replace("--", "&#x2013;")
                )
            }
            VERSE_BLOCK => {
                let c = c();
                let br = br(ex);
                // Line feeds become `<br />`, leading blanks
                // non-breaking spaces.
                let mut out = String::new();
                for l in c.split_inclusive('\n') {
                    let (body, nl) = match l.strip_suffix('\n') {
                        Some(b) => (b, true),
                        None => (l, false),
                    };
                    let body = if nl {
                        let b = body.strip_suffix(br.as_str()).unwrap_or(body);
                        b.trim_end_matches([' ', '\t'])
                    } else {
                        body
                    };
                    let lead = body.len() - body.trim_start_matches([' ', '\t']).len();
                    out.push_str(&"&#xa0;".repeat(lead));
                    out.push_str(&body[lead..]);
                    if nl {
                        out.push_str(&br);
                        out.push('\n');
                    }
                }
                format!("<p class=\"verse\">\n{out}</p>")
            }
            CLOCK => {
                let cl: ast::Clock = ex.syntax(id).and_then(|s| ast::AstNode::cast(s.clone()))?;
                let ts = cl
                    .timestamp()
                    .map(|t| crate::timestamps::interpret(ast::AstNode::syntax(&t)))
                    .unwrap_or_default();
                let dur = cl
                    .duration()
                    .map(|d| format!(" <span class=\"timestamp\">({d})</span>"))
                    .unwrap_or_default();
                format!(
                    "<p>\n<span class=\"timestamp-wrapper\">\n<span class=\"timestamp-kwd\">CLOCK:</span> <span class=\"timestamp\">{ts}</span>{dur}\n</span>\n</p>"
                )
            }
            _ => return contents,
        })
    }

    fn filter_final_output(&self, _: &mut Exporter<'_>, out: String) -> String {
        // A span end outside every paragraph, title and cell is dropped.
        out.replace(crate::kalem::END_MARK, "")
    }

    fn inner_template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let toc = match ex.opt("with-toc") {
            Value::Nil => None,
            Value::Int(n) => self.toc(ex, Some(n)),
            _ => self.toc(ex, None),
        };
        let foot = self.footnote_section(ex);
        format!(
            "{}{}{}",
            toc.unwrap_or_default(),
            body,
            foot.unwrap_or_default()
        )
    }

    fn template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let lang = option_string(ex, "language").unwrap_or_else(|| "en".into());
        let doctype = doctype(ex);
        let mut out = String::new();
        if !html5(ex) && xhtml(ex) {
            out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n");
        }
        out.push_str(&doctype_declaration(&doctype));
        out.push('\n');
        if xhtml(ex) {
            out.push_str(&format!(
                "<html xmlns=\"http://www.w3.org/1999/xhtml\" lang=\"{lang}\" xml:lang=\"{lang}\">\n"
            ));
        } else if html5(ex) {
            out.push_str(&format!("<html lang=\"{lang}\">\n"));
        } else {
            out.push_str("<html>\n");
        }
        out.push_str("<head>\n");
        out.push_str(&meta_info(ex));
        out.push_str(&head(ex));
        out.push_str(&mathjax(ex));
        out.push_str("</head>\n<body>\n");
        let up = option_string(ex, "html-link-up").unwrap_or_default();
        let home = option_string(ex, "html-link-home").unwrap_or_default();
        let (up, home) = (up.trim(), home.trim());
        if !(up.is_empty() && home.is_empty()) {
            // `(or link-up link-home)`: an empty string is not nil.
            let (u, h) = (up, home);
            out.push_str(&format!(
                "<div id=\"org-div-home-and-up\">\n <a accesskey=\"h\" href=\"{u}\"> UP </a>\n |\n <a accesskey=\"H\" href=\"{h}\"> HOME </a>\n</div>"
            ));
        }
        out.push_str(&amble(ex, false));
        let class = option_string(ex, "html-content-class").unwrap_or_else(|| "content".into());
        out.push_str(&format!("<div id=\"content\" class=\"{class}\">\n"));
        if ex.flag("with-title") {
            let title = ex.info.parsed.get("title").cloned();
            if let Some(title) = title {
                let t = ex.data_list(&title);
                let subtitle = ex.info.parsed.get("subtitle").cloned();
                let fancy = html5_fancy(ex);
                let sub = match subtitle {
                    Some(s) => {
                        let s = ex.data_list(&s);
                        if fancy {
                            format!("<p class=\"subtitle\" role=\"doc-subtitle\">{s}</p>\n")
                        } else {
                            format!("\n{}\n<span class=\"subtitle\">{s}</span>\n", br(ex))
                        }
                    }
                    None => String::new(),
                };
                if fancy {
                    out.push_str(&format!(
                        "<header>\n<h1 class=\"title\">{t}</h1>\n{sub}</header>"
                    ));
                } else {
                    out.push_str(&format!("<h1 class=\"title\">{t}{sub}</h1>\n"));
                }
            }
        }
        out.push_str(&body);
        out.push_str("</div>\n");
        out.push_str(&amble(ex, true));
        out.push_str("</body>\n</html>");
        out
    }
}

/// Kalem's style sheet, written in the head unless `html-style:nil`.
const STYLE: &str = include_str!("html.css");

/// The value of option `prop` as a string, if it has one.
/// `org-html-infojs-template`.
const INFOJS_TEMPLATE: &str = "<script src=\"%SCRIPT_PATH\">
// @license magnet:?xt=urn:btih:1f739d935676111cfff4b4693e3816e664797050&amp;dn=gpl-3.0.txt GPL-v3-or-Later
// @license-end
</script>

<script>
// @license magnet:?xt=urn:btih:1f739d935676111cfff4b4693e3816e664797050&amp;dn=gpl-3.0.txt GPL-v3-or-Later
%MANAGER_OPTIONS
org_html_manager.setup();  // activate after the parameters are set
// @license-end
</script>";

/// `org-html-infojs-install-script` (`org-html-use-infojs` at its
/// default, `when-configured`): with a `#+INFOJS_OPT:` line, org-info.js
/// and its settings go into the page's head, and the table of contents
/// follows its section depth.
fn infojs_install_script(ex: &mut Exporter<'_>) {
    let options = option_string(ex, "infojs-opt").unwrap_or_default();
    // `\<name:\(\S-+\)`.
    let find = |name: &str| -> Option<String> {
        let key = format!("{name}:");
        options.match_indices(&key).find_map(|(i, _)| {
            let word_start = options[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
            let val: String = options[i + key.len()..]
                .chars()
                .take_while(|c| !c.is_whitespace())
                .collect();
            (word_start && !val.is_empty()).then_some(val)
        })
    };
    if ex.info.body_only || options.is_empty() || find("view").as_deref() == Some("nil") {
        return;
    }
    let as_str = |v: Value| match v {
        Value::T => "1".to_string(),
        Value::Nil => "0".to_string(),
        Value::Int(n) => n.to_string(),
        Value::Str(s) | Value::Sym(s) => s,
        Value::List(_) => String::new(),
    };
    let hlevels = match ex.opt("headline-levels") {
        Value::Int(n) => n,
        _ => 3,
    };
    let ptoc = ex.opt("with-toc");
    let mut sdepth = hlevels;
    let mut tdepth = match ptoc {
        Value::Int(n) => n.min(hlevels),
        _ => hlevels,
    };
    let mut template = INFOJS_TEMPLATE.to_string();
    let mut style: Vec<(String, String)> = Vec::new();
    let table: [(&str, &str, Option<&str>); 11] = [
        ("path", "PATH", Some("https://orgmode.org/org-info.js")),
        ("view", "VIEW", Some("info")),
        ("toc", "TOC", None),
        ("ftoc", "FIXED_TOC", Some("0")),
        ("tdepth", "TOC_DEPTH", Some("max")),
        ("sdepth", "SECTION_DEPTH", Some("max")),
        ("mouse", "MOUSE_HINT", Some("underline")),
        ("buttons", "VIEW_BUTTONS", Some("0")),
        ("ltoc", "LOCAL_TOC", Some("1")),
        ("up", "LINK_UP", None),
        ("home", "LINK_HOME", None),
    ];
    for (opt, var, default) in table {
        let default = match (opt, default) {
            (_, Some(d)) => d.to_string(),
            ("toc", None) => as_str(ptoc.clone()),
            ("up", None) => as_str(ex.opt("html-link-up")),
            _ => as_str(ex.opt("html-link-home")),
        };
        let val = find(opt).unwrap_or(default);
        match opt {
            "path" => template = template.replace("%SCRIPT_PATH", &val),
            "sdepth" => {
                if let Ok(n) = val.parse::<i64>() {
                    sdepth = sdepth.min(n);
                }
            }
            "tdepth" => {
                if let Ok(n) = val.parse::<i64>() {
                    tdepth = tdepth.min(n);
                }
            }
            _ => {
                let val = match val.as_str() {
                    "t" => "1".to_string(),
                    "nil" => "0".to_string(),
                    _ => val,
                };
                style.insert(0, (var.to_string(), val));
            }
        }
    }
    ex.info.values.insert("with-toc".into(), Value::Int(sdepth));
    style.insert(0, ("TOC_DEPTH".into(), tdepth.min(sdepth).to_string()));
    let style = style
        .iter()
        .map(|(k, v)| format!("org_html_manager.set(\"{k}\", \"{v}\");"))
        .collect::<Vec<_>>()
        .join("\n");
    let script = template.replacen("%MANAGER_OPTIONS", &style, 1);
    let extra = option_string(ex, "html-head-extra").unwrap_or_default();
    ex.info.values.insert(
        "html-head-extra".into(),
        Value::Str(format!("{extra}\n{script}")),
    );
}

fn option_string(ex: &Exporter<'_>, prop: &str) -> Option<String> {
    match ex.opt(prop) {
        Value::Str(s) | Value::Sym(s) => Some(s),
        Value::Int(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `org-html--build-meta-entry`.
fn meta_entry(label: &str, identity: &str, content: Option<&str>) -> String {
    match content {
        Some(c) => format!(
            "<meta {label}=\"{identity}\" content=\"{}\" />\n",
            encode(c).replace('"', "&quot;")
        ),
        None => format!("<meta {label}=\"{identity}\" />\n"),
    }
}

/// The export time formatted as the HTML metadata time stamps are.
fn metadata_time(ex: &Exporter<'_>) -> String {
    let now = ex.info.now.clone().unwrap_or_else(jiff::Zoned::now);
    crate::macros::format_time("%Y-%m-%d %a %H:%M", &now)
}

/// `org-html--build-meta-info`: the time stamp, character set, viewport,
/// title and meta tags. The generator is Kalem.
fn meta_info(ex: &Exporter<'_>) -> String {
    let mut out = String::new();
    if ex.flag("time-stamp-file") {
        out.push_str(&format!("<!-- {} -->\n", metadata_time(ex)));
    }
    if html5(ex) {
        out.push_str(&meta_entry("charset", "utf-8", None));
    } else {
        out.push_str(&meta_entry(
            "http-equiv",
            "Content-Type",
            Some("text/html;charset=utf-8"),
        ));
    }
    out.push_str(&meta_entry(
        "name",
        "viewport",
        Some("width=device-width, initial-scale=1"),
    ));
    let title = ex.info.strings.get("title").cloned().unwrap_or_default();
    let title = encode(title.trim());
    let title = if title.trim().is_empty() {
        "&lrm;".to_string()
    } else {
        title
    };
    out.push_str(&format!("<title>{title}</title>\n"));
    if ex.flag("with-author")
        && let Some(a) = ex
            .info
            .strings
            .get("author")
            .filter(|a| !a.trim().is_empty())
    {
        out.push_str(&meta_entry("name", "author", Some(a.trim())));
    }
    for (prop, name) in [("description", "description"), ("keywords", "keywords")] {
        if let Some(v) = option_string(ex, prop).filter(|v| !v.trim().is_empty()) {
            out.push_str(&meta_entry("name", name, Some(&v)));
        }
    }
    out.push_str(&meta_entry("name", "generator", Some("Kalem")));
    out
}

/// `org-html--build-head`: the style sheet, `#+HTML_HEAD` and
/// `#+HTML_HEAD_EXTRA`.
fn head(ex: &Exporter<'_>) -> String {
    let mut out = String::new();
    if ex.flag("html-head-include-default-style") {
        out.push_str(&normalize_string(STYLE));
    }
    // Kalem's document defaults (`#+KALEM: font=… size=… spacing=…`).
    let defaults = crate::kalem::defaults(&ex.info.keywords).css();
    if !defaults.is_empty() {
        out.push_str(&format!("<style>\n#content {{ {defaults}; }}\n</style>\n"));
    }
    for prop in ["html-head", "html-head-extra"] {
        if let Some(v) = option_string(ex, prop) {
            out.push_str(&normalize_string(&v));
        }
    }
    normalize_string(&out)
}

/// MathJax, when the document has LaTeX and `tex:` asks for it: the
/// options of `#+HTML_MATHJAX` (`scale:`, `align:`, `indent:`, `tags:`,
/// `tagside:`, `tagindent:`, `multlinewidth:`, `font:`, `overflow:`,
/// `path:`) over the defaults.
fn mathjax(ex: &mut Exporter<'_>) -> String {
    let with = ex.opt("with-latex");
    if !(with == Value::T || with.sym() == Some("mathjax")) {
        return String::new();
    }
    let root = ex.tree.root;
    let has_math = ex
        .tree
        .descendants(root)
        .into_iter()
        .any(|n| matches!(ex.tree.kind(n), Some(LATEX_FRAGMENT | LATEX_ENVIRONMENT)));
    if !has_math {
        return String::new();
    }
    let in_buffer = option_string(ex, "html-mathjax").unwrap_or_default();
    let get = |key: &str, default: &str| -> String {
        let pat = format!("{key}:");
        in_buffer
            .split_whitespace()
            .find_map(|w| w.strip_prefix(pat.as_str()))
            .map(str::to_string)
            .unwrap_or_else(|| default.to_string())
    };
    let scale = get("scale", "1.0");
    let scale = match scale.parse::<f64>() {
        Ok(v) if v >= 10.0 => format!("{}", v / 100.0),
        Ok(_) => scale,
        Err(_) => "1.0".into(),
    };
    let font = match get("font", "mathjax-modern").as_str() {
        "TeX" => "mathjax-tex".to_string(),
        "STIX-Web" => "mathjax-stix2".to_string(),
        "Asana-Math" => "mathjax-asana".to_string(),
        "Neo-Euler" => "mathjax-euler".to_string(),
        "Gyre-Pagella" => "mathjax-pagella".to_string(),
        "Gyre-Termes" => "mathjax-termes".to_string(),
        "Latin-Modern" => "mathjax-modern".to_string(),
        f => f.to_string(),
    };
    let tags = match in_buffer.contains("autonumber:") {
        true => get("autonumber", "ams").to_lowercase(),
        false => get("tags", "ams"),
    };
    let overflow = match in_buffer.contains("linebreaks:") {
        true if get("linebreaks", "false") == "true" => "linebreak".to_string(),
        true => "overflow".to_string(),
        false => get("overflow", "overflow"),
    };
    format!(
        "<script>\n  window.MathJax = {{\n    tex: {{\n      ams: {{ multlineWidth: '{}' }},\n      tags: '{tags}',\n      tagSide: '{}',\n      tagIndent: '{}'\n    }},\n    chtml: {{ scale: {scale}, displayAlign: '{align}', displayIndent: '{indent}' }},\n    svg: {{ scale: {scale}, displayAlign: '{align}', displayIndent: '{indent}' }},\n    output: {{ font: '{font}', displayOverflow: '{overflow}' }}\n  }};\n</script>\n<script id=\"MathJax-script\" async src=\"{}\"></script>\n",
        get("multlinewidth", "85%"),
        get("tagside", "right"),
        get("tagindent", ".8em"),
        get(
            "path",
            "https://cdn.jsdelivr.net/npm/mathjax@3/es5/tex-mml-chtml.js"
        ),
        align = get("align", "center"),
        indent = get("indent", "0em"),
    )
}

/// `org-export-get-date` with the metadata format: a date that is one
/// timestamp, formatted; else the date exported.
fn date(ex: &mut Exporter<'_>) -> String {
    let Some(ids) = ex.info.parsed.get("date").cloned() else {
        return String::new();
    };
    let lone: Vec<Id> = ids
        .iter()
        .copied()
        .filter(|&i| !(ex.tree.is_text(i) && ex.tree.nodes[i].text.trim().is_empty()))
        .collect();
    if let [only] = lone.as_slice()
        && ex.tree.kind(*only) == Some(TIMESTAMP)
        && let Some(ts) = ex
            .syntax(*only)
            .and_then(|s| <ast::Timestamp as ast::AstNode>::cast(s.clone()))
        && let Some(start) = ts.start()
    {
        let (h, m) = start.time.unwrap_or((0, 0));
        let dt = jiff::civil::DateTime::new(
            start.year as i16,
            start.month as i8,
            start.day as i8,
            h as i8,
            m as i8,
            0,
            0,
        );
        if let Ok(dt) = dt {
            let tz = ex
                .info
                .now
                .as_ref()
                .map(|n| n.time_zone().clone())
                .unwrap_or_else(jiff::tz::TimeZone::system);
            if let Ok(z) = dt.to_zoned(tz) {
                return crate::macros::format_time("%Y-%m-%d %a %H:%M", &z);
            }
        }
    }
    ex.data_list(&ids)
}

/// `org-html--build-pre/postamble`: the preamble (empty unless
/// `html-preamble` is a format string) or the postamble (`auto`: date,
/// author, email, creation time, creator and validation link).
fn amble(ex: &mut Exporter<'_>, post: bool) -> String {
    let setting = ex.opt(if post {
        "html-postamble"
    } else {
        "html-preamble"
    });
    let spec = |ex: &mut Exporter<'_>, c: char| -> String {
        match c {
            't' => {
                let ids = ex.info.parsed.get("title").cloned().unwrap_or_default();
                ex.data_list(&ids)
            }
            's' => {
                let ids = ex.info.parsed.get("subtitle").cloned().unwrap_or_default();
                ex.data_list(&ids)
            }
            'd' => date(ex),
            'T' | 'C' => metadata_time(ex),
            'a' => {
                let ids = ex.info.parsed.get("author").cloned().unwrap_or_default();
                ex.data_list(&ids)
            }
            'e' => option_string(ex, "email")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .map(|e| format!("<a href=\"mailto:{e}\">{e}</a>"))
                .collect::<Vec<_>>()
                .join(", "),
            'c' => option_string(ex, "creator").unwrap_or_default(),
            'v' => VALIDATION.to_string(),
            '%' => "%".into(),
            other => format!("%{other}"),
        }
    };
    let format_spec = |ex: &mut Exporter<'_>, f: &str| -> String {
        let mut out = String::new();
        let mut chars = f.chars();
        while let Some(c) = chars.next() {
            if c == '%'
                && let Some(n) = chars.next()
            {
                out.push_str(&spec(ex, n));
            } else {
                out.push(c);
            }
        }
        out
    };
    let contents = match setting {
        Value::Nil => return String::new(),
        Value::Str(f) => format_spec(ex, &f),
        Value::Sym(s) if s == "auto" && post => {
            let mut out = String::new();
            let d = spec(ex, 'd');
            if ex.flag("with-date") && !d.trim().is_empty() {
                let l = ex.translate("Date", "html");
                out.push_str(&format!("<p class=\"date\">{l}: {d}</p>\n"));
            }
            let a = spec(ex, 'a');
            if ex.flag("with-author") && !a.trim().is_empty() {
                let l = ex.translate("Author", "html");
                out.push_str(&format!("<p class=\"author\">{l}: {a}</p>\n"));
            }
            let e = spec(ex, 'e');
            if ex.flag("with-email")
                && option_string(ex, "email").is_some_and(|e| !e.trim().is_empty())
            {
                let l = ex.translate("Email", "html");
                out.push_str(&format!("<p class=\"email\">{l}: {e}</p>\n"));
            }
            if ex.flag("time-stamp-file") {
                let l = ex.translate("Created", "html");
                out.push_str(&format!(
                    "<p class=\"date\">{l}: {}</p>\n",
                    metadata_time(ex)
                ));
            }
            let c = spec(ex, 'c');
            if ex.flag("with-creator") && !c.trim().is_empty() {
                out.push_str(&format!("<p class=\"creator\">{c}</p>\n"));
            }
            out.push_str(&format!("<p class=\"validation\">{VALIDATION}</p>\n"));
            out
        }
        // `t`: the format for the language (the preamble's is empty).
        _ if post => format_spec(
            ex,
            "<p class=\"author\">Author: %a (%e)</p>\n<p class=\"date\">Date: %d</p>\n<p class=\"creator\">%c</p>\n<p class=\"validation\">%v</p>",
        ),
        _ => String::new(),
    };
    if contents.trim().is_empty() {
        return String::new();
    }
    let id = if post { "postamble" } else { "preamble" };
    format!(
        "<div id=\"{id}\" class=\"status\">\n{}</div>\n",
        normalize_string(&contents)
    )
}

/// Formula `latex` drawn as an SVG image, when `tex:` asks for images
/// (`svg`, or Emacs's `dvisvgm`, `dvipng`, `imagemagick`) and the export
/// has a renderer: an `<img>` with the SVG inline, sized in ems and
/// lowered to the baseline.
fn svg_formula(ex: &Exporter<'_>, latex: &str) -> Option<String> {
    let mode = ex.opt("with-latex");
    if !matches!(
        mode.sym(),
        Some("svg" | "dvisvgm" | "dvipng" | "imagemagick")
    ) {
        return None;
    }
    let renderer = ex.info.math.clone()?;
    let headers: Vec<String> = ex
        .info
        .keywords
        .iter()
        .filter(|(k, _)| {
            k.eq_ignore_ascii_case("LATEX_HEADER") || k.eq_ignore_ascii_case("LATEX_HEADER_EXTRA")
        })
        .map(|(_, v)| v.clone())
        .collect();
    let f = renderer.0.render(latex, &headers)?;
    let mut uri = String::from("data:image/svg+xml,");
    for b in f.svg.bytes() {
        if b.is_ascii_alphanumeric() || b" -_.~:/=;,'()!*".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    let alt = encode(latex.trim()).replace('"', "&quot;");
    Some(format!(
        "<img src=\"{uri}\" alt=\"{alt}\" class=\"org-latex org-latex-{}\" style=\"width: {:.3}em; height: {:.3}em; vertical-align: -{:.3}em\"{}",
        if f.display { "display" } else { "inline" },
        f.width,
        f.height,
        f.depth,
        close(ex)
    ))
}

/// `org-html-validation-link`.
const VALIDATION: &str = "<a href=\"https://validator.w3.org/check?uri=referer\">Validate</a>";

/// The HTML5 elements a special block is written as, with `html5-fancy`.
const HTML5_ELEMENTS: &[&str] = &[
    "article",
    "aside",
    "audio",
    "canvas",
    "details",
    "figcaption",
    "figure",
    "footer",
    "header",
    "menu",
    "meter",
    "nav",
    "output",
    "progress",
    "section",
    "summary",
    "video",
];

/// The `HTML_DOCTYPE`: a name of [`doctype_declaration`] or a literal
/// declaration.
fn doctype(ex: &Exporter<'_>) -> String {
    match ex.opt("html-doctype") {
        Value::Str(s) | Value::Sym(s) => s,
        _ => "xhtml-strict".into(),
    }
}

/// The declaration of doctype `name` (`org-html-doctype-alist`).
fn doctype_declaration(name: &str) -> String {
    let (id, dtd) = match name {
        "html4-strict" => ("HTML 4.01", "html4/strict.dtd"),
        "html4-transitional" => ("HTML 4.01 Transitional", "html4/loose.dtd"),
        "html4-frameset" => ("HTML 4.01 Frameset", "html4/frameset.dtd"),
        "xhtml-strict" => ("XHTML 1.0 Strict", "xhtml1/DTD/xhtml1-strict.dtd"),
        "xhtml-transitional" => (
            "XHTML 1.0 Transitional",
            "xhtml1/DTD/xhtml1-transitional.dtd",
        ),
        "xhtml-frameset" => ("XHTML 1.0 Frameset", "xhtml1/DTD/xhtml1-frameset.dtd"),
        "xhtml-11" => ("XHTML 1.1", "xhtml11/DTD/xhtml11.dtd"),
        "html5" | "xhtml5" => return "<!DOCTYPE html>".into(),
        other => return other.to_string(),
    };
    format!("<!DOCTYPE html PUBLIC \"-//W3C//DTD {id}//EN\"\n\"http://www.w3.org/TR/{dtd}\">")
}

/// `org-html-xhtml-p`.
fn xhtml(ex: &Exporter<'_>) -> bool {
    doctype(ex).to_lowercase().contains("xhtml")
}

/// `org-html-html5-p`.
fn html5(ex: &Exporter<'_>) -> bool {
    matches!(
        doctype(ex).to_lowercase().as_str(),
        "html5" | "xhtml5" | "<!doctype html>"
    )
}

/// `org-html--html5-fancy-p`.
fn html5_fancy(ex: &Exporter<'_>) -> bool {
    ex.flag("html5-fancy") && html5(ex)
}

/// The end of an empty element: ` />` in XHTML, `>` otherwise.
fn close(ex: &Exporter<'_>) -> &'static str {
    if xhtml(ex) { " />" } else { ">" }
}

/// A line break element.
fn br(ex: &Exporter<'_>) -> String {
    format!("<br{}", close(ex))
}

/// `\(\\\\\)?[ \t]*\n` replaced by `with`: each line feed, with the
/// blanks and a `\\` before it.
pub fn preserve_breaks(s: &str, with: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('\n') {
        let line = rest[..i].trim_end_matches([' ', '\t']);
        let line = line.strip_suffix("\\\\").unwrap_or(line);
        out.push_str(line);
        out.push_str(with);
        rest = &rest[i + 1..];
    }
    out.push_str(rest);
    out
}

/// A table.el table as HTML, as `table-generate-source` writes it (Emacs
/// 30.1): each cell's lines with their blanks as no-break spaces, the
/// text as it is (table.el does not escape it). Cells spanning several
/// columns or rows are split at the columns of the first rule.
pub(crate) fn table_el_html(value: &str) -> String {
    let text = remove_indentation(value);
    let lines: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
    let cols: Vec<usize> = lines
        .iter()
        .find(|l| l.first() == Some(&'+'))
        .map(|l| {
            l.iter()
                .enumerate()
                .filter(|(_, c)| **c == '+')
                .map(|(i, _)| i)
                .collect()
        })
        .unwrap_or_default();
    let mut out = String::from(
        "<!-- This HTML table template is generated by emacs 30.1 -->\n<table border=\"1\">\n",
    );
    let mut row: Vec<&Vec<char>> = Vec::new();
    let flush = |row: &mut Vec<&Vec<char>>, out: &mut String| {
        if row.is_empty() {
            return;
        }
        out.push_str("  <tr>\n");
        for w in cols.windows(2) {
            let cell: Vec<String> = row
                .iter()
                .map(|l| {
                    let end = w[1].min(l.len());
                    let start = (w[0] + 1).min(end);
                    l[start..end]
                        .iter()
                        .collect::<String>()
                        .replace(' ', "&nbsp;")
                })
                .collect();
            out.push_str("    <td align=\"left\" valign=\"top\">\n      ");
            out.push_str(&cell.join("<br />\n      "));
            out.push_str("\n    </td>\n");
        }
        out.push_str("  </tr>\n");
        row.clear();
    };
    for l in &lines {
        if l.first() == Some(&'+') {
            flush(&mut row, &mut out);
        } else {
            row.push(l);
        }
    }
    flush(&mut row, &mut out);
    out.push_str("</table>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_el_as_emacs_writes_it() {
        // `org-export-as` with ox-html on Emacs (29.3, version renamed).
        let value =
            "+-----+----+\n| a   | b  |\n| two | &< |\n+-----+----+\n| 1   |    |\n+-----+----+\n";
        assert_eq!(
            table_el_html(value),
            "<!-- This HTML table template is generated by emacs 30.1 -->\n<table border=\"1\">\n  <tr>\n    <td align=\"left\" valign=\"top\">\n      &nbsp;a&nbsp;&nbsp;&nbsp;<br />\n      &nbsp;two&nbsp;\n    </td>\n    <td align=\"left\" valign=\"top\">\n      &nbsp;b&nbsp;&nbsp;<br />\n      &nbsp;&<&nbsp;\n    </td>\n  </tr>\n  <tr>\n    <td align=\"left\" valign=\"top\">\n      &nbsp;1&nbsp;&nbsp;&nbsp;\n    </td>\n    <td align=\"left\" valign=\"top\">\n      &nbsp;&nbsp;&nbsp;&nbsp;\n    </td>\n  </tr>\n</table>"
        );
    }

    #[test]
    fn strings() {
        assert_eq!(
            special_strings("a -- b --- c ... d \\- e"),
            "a &#x2013; b &#x2014; c &#x2026; d &#x00ad; e"
        );
        assert_eq!(
            parse_attributes(":class a b :id x"),
            vec![
                (":class".into(), Some("a b".into())),
                (":id".into(), Some("x".into()))
            ]
        );
        assert_eq!(remove_indentation("  a\n    b\n"), "a\n  b\n");
    }
}
