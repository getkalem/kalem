//! The plain text back-end, as `ox-ascii.el` writes in its `ascii`
//! charset (or `utf-8` with [`Text::utf8`]): paragraphs filled to 72
//! columns, underlined titles, boxed code, tables drawn with `|` and `-`,
//! and links and footnotes as notes.

use org_syntax::SyntaxKind::{self, *};
use org_syntax::ast;

use crate::export::{Backend, Exporter, trim};
use crate::fill::{self, HARD, Justify, width};
use crate::html;
use crate::options::Value;
use crate::tree::{Id, Secondary};

/// The plain text back-end.
#[derive(Debug, Clone, Copy, Default)]
pub struct Text {
    /// UTF-8 characters for lines, bullets, quotes and dashes, rather
    /// than ASCII.
    pub utf8: bool,
}

/// `org-ascii-text-width`.
const TEXT_WIDTH: usize = 72;
/// `org-ascii-inner-margin`.
const INNER_MARGIN: usize = 2;
/// `org-ascii-quote-margin`.
const QUOTE_MARGIN: usize = 6;
/// `org-ascii-inlinetask-width`.
const INLINETASK_WIDTH: usize = 30;

fn cast<T: ast::AstNode>(ex: &Exporter<'_>, id: Id) -> Option<T> {
    ex.syntax(id).and_then(|s| T::cast(s.clone()))
}

/// `org-ascii--indent-string`: `n` spaces before each line with text.
fn indent(s: &str, n: usize) -> String {
    if n == 0 {
        return s.to_string();
    }
    let pad = " ".repeat(n);
    s.split('\n')
        .map(|l| {
            if l.trim_matches([' ', '\t', HARD]).is_empty() {
                l.to_string()
            } else {
                format!("{pad}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `org-ascii--fill-string`: `preserve-breaks` makes each line feed a
/// hard one.
fn fill_str(ex: &Exporter<'_>, s: &str, w: usize, how: Justify) -> String {
    if !ex.flag("preserve-breaks") {
        return fill::fill(s, w, how);
    }
    let mut t = String::with_capacity(s.len() + 8);
    let mut prev = None;
    for c in s.chars() {
        if c == '\n' && prev != Some(HARD) {
            t.push(HARD);
        }
        t.push(c);
        prev = Some(c);
    }
    fill::fill(&t, w, how)
}

/// `org-remove-blank-lines`.
fn remove_blank_lines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        if line.trim_matches([' ', '\t', '\n']).is_empty() && line.ends_with('\n') {
            continue;
        }
        out.push_str(line);
    }
    out
}

impl Text {
    fn charset(&self) -> &'static str {
        if self.utf8 { "utf-8" } else { "ascii" }
    }

    /// `org-ascii--box-string`.
    fn boxed(&self, s: &str) -> String {
        let s = s.trim_end_matches([' ', '\t']);
        let s = s.strip_suffix('\n').unwrap_or(s);
        let (top, side, bottom) = if self.utf8 {
            ("┌────", "│ ", "└────")
        } else {
            (",----", "| ", "`----")
        };
        // (`replace-regexp-in-string` does not touch an empty string.)
        let body: Vec<String> = if s.is_empty() {
            Vec::new()
        } else {
            s.split('\n').map(|l| format!("{side}{l}")).collect()
        };
        format!("{top}\n{}\n{bottom}", body.join("\n"))
    }

    fn checkbox(&self, ex: &Exporter<'_>, item: Id) -> &'static str {
        let cb = cast::<ast::Item>(ex, item).and_then(|i| i.checkbox());
        match (cb, self.utf8) {
            (Some(ast::Checkbox::On), false) => "[X] ",
            (Some(ast::Checkbox::Off), false) => "[ ] ",
            (Some(ast::Checkbox::Partial), false) => "[-] ",
            (Some(ast::Checkbox::On), true) => "☑ ",
            (Some(ast::Checkbox::Off), true) => "☐ ",
            (Some(ast::Checkbox::Partial), true) => "☒ ",
            (None, _) => "",
        }
    }

    /// `org-ascii--current-text-width`.
    fn text_width(&self, ex: &Exporter<'_>, id: Id) -> usize {
        match ex.tree.kind(id) {
            Some(INLINETASK) => return INLINETASK_WIDTH,
            Some(HEADLINE) => {
                return TEXT_WIDTH - ex.low_level_p(id).map_or(0, |r| r as usize * 2);
            }
            _ => {}
        }
        let lineage: Vec<Id> = std::iter::once(id).chain(ex.tree.ancestors(id)).collect();
        let total = if lineage.iter().any(|&a| ex.tree.kind(a) == Some(INLINETASK)) {
            INLINETASK_WIDTH
        } else {
            let margin = match lineage.iter().find(|&&a| ex.tree.kind(a) == Some(HEADLINE)) {
                None => 0,
                Some(&h) => match ex.low_level_p(h) {
                    Some(r) => r as usize * 2,
                    None => INNER_MARGIN,
                },
            };
            TEXT_WIDTH - margin
        };
        let quotes = lineage
            .iter()
            .filter(|&&a| matches!(ex.tree.kind(a), Some(QUOTE_BLOCK | VERSE_BLOCK)))
            .count();
        let mut indentation = 0;
        for &a in &lineage {
            if ex.tree.kind(a) != Some(ITEM) {
                continue;
            }
            let list = ex.tree.parent(a);
            if list.is_some_and(|l| html::list_type(ex, l) == html::ListType::Descriptive) {
                indentation += QUOTE_MARGIN;
            } else {
                let bullet = cast::<ast::Item>(ex, a)
                    .map(|i| i.bullet())
                    .unwrap_or_default();
                indentation += width(self.checkbox(ex, a)) + width(&bullet);
            }
        }
        total.saturating_sub(quotes * 2 * QUOTE_MARGIN + indentation)
    }

    /// `org-ascii--current-justification`.
    fn justification(ex: &Exporter<'_>, id: Id) -> Justify {
        for a in ex.tree.ancestors(id) {
            match ex.tree.kind(a) {
                Some(CENTER_BLOCK) => return Justify::Center,
                Some(SPECIAL_BLOCK) => {
                    let ty = cast::<ast::SpecialBlock>(ex, a)
                        .map(|b| b.block_type())
                        .unwrap_or_default();
                    match ty.as_str() {
                        "JUSTIFYRIGHT" => return Justify::Right,
                        "JUSTIFYLEFT" => return Justify::Left,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Justify::Left
    }

    /// `org-ascii--justify-element`.
    fn justify_element(&self, ex: &Exporter<'_>, id: Id, contents: String) -> String {
        if contents.trim().is_empty() {
            return contents;
        }
        let w = self.text_width(ex, id);
        let how = Self::justification(ex, id);
        if ex.tree.kind(id) == Some(PARAGRAPH) {
            return fill_str(ex, &contents, w, how);
        }
        if how == Justify::Left {
            return contents;
        }
        let mut max = 0;
        for l in contents.split('\n') {
            if l.trim().is_empty() {
                continue;
            }
            let c = width(l.trim_end_matches(['\n']));
            if c >= w {
                return contents;
            }
            max = max.max(c);
        }
        let offset = (w - max) / if how == Justify::Right { 1 } else { 2 };
        if offset == 0 {
            return contents;
        }
        contents
            .split('\n')
            .map(|l| {
                if l.trim().is_empty() {
                    l.to_string()
                } else {
                    // `indent-to-column` at the start of the line.
                    format!("{}{l}", fill::indentation(0, offset, true))
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn underline_char(&self, level: i64) -> Option<char> {
        let chars: &[char] = if self.utf8 {
            &['═', '─', '╌', '┄', '┈']
        } else {
            &['=', '~', '-']
        };
        chars.get((level - 1).max(0) as usize).copied()
    }

    fn line_char(&self) -> char {
        if self.utf8 { '─' } else { '_' }
    }

    /// `org-ascii--build-title`.
    fn build_title(
        &self,
        ex: &mut Exporter<'_>,
        id: Id,
        text_width: usize,
        underline: bool,
        notags: bool,
        toc: bool,
    ) -> String {
        let headline = ex.tree.kind(id) == Some(HEADLINE);
        let numbers = if headline && ex.numbered_p(id) {
            ex.headline_number(id).map(|n| {
                if toc {
                    format!("{}. ", n.last().copied().unwrap_or(0))
                } else {
                    format!(
                        "{} ",
                        n.iter().map(usize::to_string).collect::<Vec<_>>().join(".")
                    )
                }
            })
        } else {
            None
        };
        let ids = if toc && headline {
            ex.alt_title(id)
        } else {
            ex.tree
                .secondary(id, Secondary::Title)
                .map(<[Id]>::to_vec)
                .unwrap_or_default()
        };
        let text = trim(&ex.data_list(&ids)).to_string();
        let todo = if ex.flag("with-todo-keywords") {
            keyword(ex, id).map(|k| {
                let t = ex.tree.text_node(k, None);
                format!("{} ", ex.data(t))
            })
        } else {
            None
        };
        let tags = if !notags && ex.flag("with-tags") {
            let t = ex.tags(id, &[], false);
            (!t.is_empty()).then(|| format!(":{}:", t.join(":")))
        } else {
            None
        };
        let priority = if ex.flag("with-priority") {
            priority(ex, id).map(|c| format!("(#{c}) "))
        } else {
            None
        };
        let first = format!(
            "{}{}{}{text}",
            numbers.unwrap_or_default(),
            todo.unwrap_or_default(),
            priority.unwrap_or_default()
        );
        let mut out = first.clone();
        if let Some(t) = tags {
            let w = text_width.saturating_sub(1 + width(&first)).max(width(&t));
            out.push_str(&format!(" {t:>w$}"));
        }
        if underline
            && headline
            && let Some(c) = self.underline_char(ex.relative_level(id))
        {
            let n = width(&first) / fill::char_width(c).max(1);
            out.push('\n');
            out.extend(std::iter::repeat_n(c, n));
        }
        out
    }

    /// `org-ascii--build-caption`: `Table N:` or `Listing N:` and the
    /// caption, filled.
    fn build_caption(&self, ex: &mut Exporter<'_>, id: Id) -> Option<String> {
        ex.tree.secondary(id, Secondary::Caption(0))?;
        let kind = ex.tree.kind(id)?;
        let n = ordinal_captioned(ex, id);
        let fmt = match kind {
            TABLE => "Table %d:",
            _ => "Listing %d:",
        };
        let label = ex
            .translate(fmt, self.charset())
            .replace("%d", &n.to_string());
        let caption = html::caption_ids(ex, id);
        let text = ex.data_list(&caption);
        Some(fill_str(
            ex,
            &format!("{label} {text}"),
            self.text_width(ex, id),
            Justify::Left,
        ))
    }

    /// `org-ascii--build-toc`.
    fn build_toc(
        &self,
        ex: &mut Exporter<'_>,
        n: Option<i64>,
        keyword: Option<Id>,
        scope: Option<Id>,
    ) -> String {
        let mut out = String::new();
        if scope.is_none() {
            let title = ex.translate("Table of Contents", self.charset());
            out.push_str(&format!(
                "{title}\n{}\n\n",
                self.line_char().to_string().repeat(width(&title))
            ));
        }
        let text_width = match keyword {
            Some(k) => self.text_width(ex, k),
            None => TEXT_WIDTH,
        };
        let heads = html::collect_headlines_in(ex, n, scope);
        let notags = !ex.flag("with-tags") || ex.opt("with-tags").sym() == Some("not-in-toc");
        let mut lines = Vec::new();
        for h in heads {
            let level = ex.relative_level(h) as usize;
            let ind = (level.saturating_sub(1)) * 3;
            let mut line = String::new();
            if ind > 0 {
                line.push_str(&".".repeat(ind - 1));
                line.push(' ');
            }
            line.push_str(&self.build_title(
                ex,
                h,
                text_width.saturating_sub(ind),
                false,
                notags,
                true,
            ));
            lines.push(line);
        }
        out.push_str(&lines.join("\n"));
        out
    }

    /// `org-ascii--list-tables` and `org-ascii--list-listings`.
    fn list_of(&self, ex: &mut Exporter<'_>, keyword: Id, kind: SyntaxKind) -> String {
        let (title, fmt) = if kind == TABLE {
            ("List of Tables", "Table %d:")
        } else {
            ("List of Listings", "Listing %d:")
        };
        let title = ex.translate(title, self.charset());
        let mut out = format!(
            "{title}\n{}\n\n",
            self.line_char().to_string().repeat(width(&title))
        );
        let text_width = self.text_width(ex, keyword);
        let entries: Vec<Id> = ex
            .tree
            .descendants(ex.tree.root)
            .into_iter()
            .filter(|&e| {
                ex.tree.kind(e) == Some(kind)
                    && ex.tree.secondary(e, Secondary::Caption(0)).is_some()
                    && html::reachable(ex, e)
            })
            .collect();
        let mut lines = Vec::new();
        for (i, e) in entries.into_iter().enumerate() {
            let initial = ex
                .translate(fmt, self.charset())
                .replace("%d", &(i + 1).to_string());
            let iw = width(&initial);
            let mut short: Vec<Id> = Vec::new();
            let mut k = 0;
            while let Some(s) = ex.tree.secondary(e, Secondary::ShortCaption(k)) {
                short = s.to_vec();
                k += 1;
            }
            let ids = if short.is_empty() {
                html::caption_ids(ex, e)
            } else {
                short
            };
            let caption = ex.data_list(&ids);
            let filled = fill_str(ex, &caption, text_width.saturating_sub(iw), Justify::Left);
            lines.push(format!("{initial} {}", trim(&indent(&filled, iw))));
        }
        out.push_str(&lines.join("\n"));
        out
    }

    /// `org-ascii--unique-links`: the links of a section, or of a
    /// headline's title and section, first of their kind.
    fn unique_links(&self, ex: &mut Exporter<'_>, id: Id) -> Vec<Id> {
        let mut roots: Vec<Id> = Vec::new();
        if ex.tree.kind(id) == Some(SECTION) {
            roots.push(id);
        } else {
            if let Some(t) = ex.tree.secondary(id, Secondary::Title) {
                roots.extend(t);
            }
            if let Some(&first) = ex.tree.children(id).first()
                && ex.tree.kind(first) == Some(SECTION)
            {
                roots.push(first);
            }
        }
        let mut seen: Vec<(String, Option<String>)> = Vec::new();
        let mut out = Vec::new();
        for r in roots {
            let mut stack = vec![r];
            while let Some(x) = stack.pop() {
                if ex.info.ignore.contains(&x) || ex.tree.kind(x) == Some(HEADLINE) {
                    continue;
                }
                if ex.tree.kind(x) == Some(LINK) {
                    let exported = !ex.data(x).trim().is_empty();
                    let raw = ex.link_info(x).map(|i| i.raw_link).unwrap_or_default();
                    let children = ex.tree.children(x).to_vec();
                    let contents = (!children.is_empty()).then(|| {
                        let s: String = children.iter().map(|c| ex.tree.source(*c)).collect();
                        s.split_whitespace().collect::<Vec<_>>().join(" ")
                    });
                    let footprint = (raw, contents);
                    if exported && !seen.contains(&footprint) {
                        seen.push(footprint);
                        out.push(x);
                    }
                }
                let n = &ex.tree.nodes[x];
                let mut next: Vec<Id> = Vec::new();
                for (_, v) in &n.secondary {
                    next.extend(v);
                }
                next.extend(&n.children);
                stack.extend(next.into_iter().rev());
            }
        }
        out
    }

    /// `org-ascii--describe-datum`.
    fn describe(&self, ex: &mut Exporter<'_>, dest: Id) -> String {
        let cs = self.charset();
        let section = |ex: &mut Exporter<'_>, h: Id| {
            let s = if ex.numbered_p(h) {
                ex.headline_number(h)
                    .unwrap_or_default()
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(".")
            } else {
                let ids = ex
                    .tree
                    .secondary(h, Secondary::Title)
                    .map(<[Id]>::to_vec)
                    .unwrap_or_default();
                ex.data_list(&ids)
            };
            ex.translate("See section %s", cs).replace("%s", &s)
        };
        if ex.tree.kind(dest) == Some(HEADLINE) {
            return section(ex, dest);
        }
        let number = ordinal(ex, dest);
        let enumerable = std::iter::once(dest)
            .chain(ex.tree.ancestors(dest))
            .find(|a| {
                matches!(
                    ex.tree.kind(*a),
                    Some(HEADLINE | PARAGRAPH | SRC_BLOCK | TABLE)
                )
            });
        match enumerable.and_then(|e| ex.tree.kind(e).map(|k| (e, k))) {
            Some((e, HEADLINE)) => {
                if ex.numbered_p(e) {
                    let n = number.unwrap_or_default();
                    ex.translate("See section %s", cs).replace("%s", &n)
                } else {
                    section(ex, e)
                }
            }
            _ if number.is_none() => ex.translate("Unknown reference", cs),
            Some((_, PARAGRAPH)) => ex
                .translate("See figure %s", cs)
                .replace("%s", &number.unwrap_or_default()),
            Some((_, SRC_BLOCK)) => ex
                .translate("See listing %s", cs)
                .replace("%s", &number.unwrap_or_default()),
            Some((_, TABLE)) => ex
                .translate("See table %s", cs)
                .replace("%s", &number.unwrap_or_default()),
            _ => ex.translate("Unknown reference", cs),
        }
    }

    /// `org-ascii--describe-links`.
    fn describe_links(&self, ex: &mut Exporter<'_>, links: &[Id], w: usize) -> String {
        let mut out = String::new();
        for &l in links {
            let Some(info) = ex.link_info(l) else {
                continue;
            };
            let ty = info.link_type.clone();
            let children = ex.tree.children(l).to_vec();
            let anchor = if children.is_empty() {
                let t = ex.tree.text_node(info.raw_link.clone(), None);
                ex.data(t)
            } else {
                ex.data_list(&children)
            };
            if matches!(ty.as_str(), "coderef" | "radio") {
                continue;
            }
            if matches!(ty.as_str(), "custom-id" | "fuzzy" | "id") {
                if children.is_empty() {
                    continue;
                }
                let dest = if ty == "fuzzy" {
                    ex.resolve_fuzzy(&info.path)
                } else {
                    ex.resolve_id(&info.path)
                };
                if let Some(d) = dest {
                    let desc = self.describe(ex, d);
                    out.push_str(&fill_str(
                        ex,
                        &format!("[{anchor}] {desc}"),
                        w,
                        Justify::Left,
                    ));
                    out.push_str("\n\n");
                }
                continue;
            }
            if children.is_empty() {
                continue;
            }
            if ex
                .custom_protocol(&ty, &info.path, Some(&anchor), "ascii")
                .is_some()
            {
                continue;
            }
            out.push_str(&fill_str(
                ex,
                &format!("[{anchor}] <{}>", info.raw_link),
                w,
                Justify::Left,
            ));
            out.push_str("\n\n");
        }
        out
    }

    fn headline(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        if ex.footnote_section_p(id) {
            return None;
        }
        let low = ex.low_level_p(id);
        let w = self.text_width(ex, id);
        let title = self.build_title(ex, id, w, low.is_none(), false, false);
        // `org-ascii-headline-spacing`: one blank line before the body.
        let pre = "\n";
        let links = self.unique_links(ex, id);
        let links = self.describe_links(ex, &links, w);
        let body = if links.trim().is_empty() {
            contents.unwrap_or_default()
        } else {
            let children = ex.tree.children(id).to_vec();
            let section = children
                .first()
                .copied()
                .filter(|&f| ex.tree.kind(f) == Some(SECTION));
            let mut b = String::new();
            if let Some(s) = section {
                b.push_str(&crate::export::normalize_string(&ex.data(s)));
                b.push_str("\n\n");
            }
            b.push_str(&links);
            let rest: Vec<Id> = if section.is_some() {
                children[1..].to_vec()
            } else {
                children
            };
            for r in rest {
                b.push_str(&ex.data(r));
            }
            b
        };
        Some(match low {
            Some(rank) => {
                let bullets: &[char] = if self.utf8 {
                    &['◊']
                } else {
                    &['*', '+', '-']
                };
                let bullet = format!("{} ", bullets[(rank as usize - 1) % bullets.len()]);
                format!("{bullet}{title}\n{pre}{}", indent(&body, width(&bullet)))
            }
            None => format!("{title}\n{pre}{body}"),
        })
    }

    fn item(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> String {
        let list = ex.tree.parent(id);
        let ty = list.map_or(html::ListType::Unordered, |l| html::list_type(ex, l));
        let checkbox = self.checkbox(ex, id);
        let item: Option<ast::Item> = cast(ex, id);
        let raw_bullet = item.as_ref().map(|i| i.bullet()).unwrap_or_default();
        // `org-list-bullet-string`: one space after the bullet.
        let bul = format!("{} ", raw_bullet.trim());
        let bullet = match ty {
            html::ListType::Descriptive => {
                let tag = ex.tree.secondary(id, Secondary::Tag).map(<[Id]>::to_vec);
                let t = tag.map(|t| ex.data_list(&t)).unwrap_or_default();
                format!("{checkbox}{t}")
            }
            html::ListType::Ordered => {
                let n = item_number(ex, id).to_string();
                replace_counter(&bul, &n)
            }
            html::ListType::Unordered => {
                if self.utf8 {
                    bul.replace('*', "‣").replace('+', "⁃").replace('-', "•")
                } else {
                    bul
                }
            }
        };
        let ind = if ty == html::ListType::Descriptive {
            QUOTE_MARGIN
        } else {
            width(&bullet)
        };
        let c = indent(&contents.unwrap_or_default(), ind);
        let first_is_paragraph = ex
            .tree
            .children(id)
            .to_vec()
            .into_iter()
            .find(|&e| !ex.data(e).trim().is_empty())
            .is_some_and(|e| ex.tree.kind(e) == Some(PARAGRAPH));
        let body =
            if ty != html::ListType::Descriptive && !c.trim().is_empty() && first_is_paragraph {
                trim(&c).to_string()
            } else {
                format!("\n{c}")
            };
        // (A descriptive item's checkbox comes twice, as in Emacs.)
        format!("{bullet}{checkbox}{body}")
    }

    fn link(&self, ex: &mut Exporter<'_>, id: Id, desc: Option<String>) -> Option<String> {
        let info = ex.link_info(id)?;
        let ty = info.link_type.clone();
        let desc = desc.filter(|d| !d.is_empty());
        if let Some(out) = ex.custom_protocol(&ty, &info.path, desc.as_deref(), "ascii") {
            return Some(out);
        }
        match ty.as_str() {
            "coderef" => {
                let Some(target) = html::resolve_coderef(ex, &info.path) else {
                    ex.broken_link(id, &info.path);
                    return None;
                };
                Some(html::coderef_format(&info.path, desc.as_deref()).replace("%s", &target))
            }
            "radio" => Some(desc.unwrap_or_default()),
            "custom-id" | "fuzzy" | "id" => {
                let dest = if ty == "fuzzy" {
                    ex.resolve_fuzzy(&info.path)
                } else {
                    ex.resolve_id(&info.path)
                };
                let Some(dest) = dest else {
                    ex.broken_link(id, &info.path);
                    return None;
                };
                if let Some(d) = desc {
                    return Some(format!("[{d}]"));
                }
                if ex.tree.kind(dest) == Some(HEADLINE) {
                    return Some(if ex.numbered_p(dest) {
                        ex.headline_number(dest)
                            .unwrap_or_default()
                            .iter()
                            .map(usize::to_string)
                            .collect::<Vec<_>>()
                            .join(".")
                    } else {
                        let ids = ex
                            .tree
                            .secondary(dest, Secondary::Title)
                            .map(<[Id]>::to_vec)
                            .unwrap_or_default();
                        ex.data_list(&ids)
                    });
                }
                Some(ordinal(ex, dest).unwrap_or_else(|| "???".into()))
            }
            _ => Some(match desc {
                Some(d) if !d.trim().is_empty() => format!("[{d}]"),
                _ => format!("<{}>", info.raw_link),
            }),
        }
    }

    /// `org-ascii--table-cell-width`.
    fn cell_width(&self, ex: &mut Exporter<'_>, cell: Id) -> usize {
        if let Some(w) = ex.tree.nodes[cell].props.get("ascii-width") {
            return w.parse().unwrap_or(0);
        }
        let row = ex.tree.parent(cell).unwrap_or(cell);
        let table = ex.tree.parent(row).unwrap_or(row);
        let col = ex
            .tree
            .children(row)
            .iter()
            .position(|&c| c == cell)
            .unwrap_or(0);
        let cookie = ex.cell_cookie_width(cell);
        let mut max = 0;
        for r in ex.tree.children(table).to_vec() {
            if ex.info.ignore.contains(&r) || ex.rule_row_p(r) {
                continue;
            }
            if let Some(&c) = ex.tree.children(r).get(col) {
                let kids = ex.tree.children(c).to_vec();
                max = max.max(width(&ex.data_list(&kids)));
            }
        }
        let w = cookie.map_or(max, |c| c.max(max));
        // The same width for every cell of the column.
        for r in ex.tree.children(table).to_vec() {
            if let Some(&c) = ex.tree.children(r).get(col) {
                ex.tree.nodes[c].props.insert("ascii-width", w.to_string());
            }
        }
        w
    }

    fn table_cell(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> String {
        let w = self.cell_width(ex, id);
        let how = match ex.cell_alignment(id) {
            "right" => Justify::Right,
            "center" => Justify::Center,
            _ => Justify::Left,
        };
        let data = contents
            .map(|c| fill::justify_lines(&c, w, how))
            .unwrap_or_default();
        let pad = w.saturating_sub(width(&data));
        let (_, right) = html::cell_borders(ex, id);
        format!(
            " {data}{} {}",
            " ".repeat(pad),
            if right {
                if self.utf8 { "│" } else { "|" }
            } else {
                ""
            }
        )
    }

    fn hline(&self, ex: &mut Exporter<'_>, row: Id, corners: (char, char, char, char)) -> String {
        let (l, h, v, r) = corners;
        let cells: Vec<Id> = ex
            .row_cells(row)
            .into_iter()
            .filter(|c| !ex.info.ignore.contains(c))
            .collect();
        let last = ex.tree.children(row).last().copied();
        let mut out = String::new();
        for (i, &c) in cells.iter().enumerate() {
            let w = self.cell_width(ex, c);
            let (left, right) = html::cell_borders(ex, c);
            if left && i == 0 {
                out.push(l);
            }
            out.extend(std::iter::repeat_n(h, w + 2));
            if right {
                out.push(if Some(c) == last { r } else { v });
            }
        }
        out.push('\n');
        out
    }

    fn table_row(&self, ex: &mut Exporter<'_>, id: Id, contents: String) -> Option<String> {
        if ex.rule_row_p(id) {
            return None;
        }
        let table = ex.tree.parent(id)?;
        let rows: Vec<Id> = ex.tree.children(table).to_vec();
        let pos = rows.iter().position(|&r| r == id)?;
        // `org-export-table-cell-borders` above and below.
        let special =
            |ex: &Exporter<'_>, r: Id| ex.table_row_is_special(r) || ex.info.ignore.contains(&r);
        let (mut above, mut top) = (false, false);
        {
            let mut rule = false;
            let mut found = false;
            for &r in rows[..pos].iter().rev() {
                if ex.rule_row_p(r) {
                    rule = true;
                } else if !special(ex, r) {
                    if rule {
                        above = true;
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                if rule {
                    above = true;
                }
                top = true;
            }
        }
        let (mut below, mut bottom) = (false, false);
        {
            let mut rule = false;
            let mut found = false;
            for &r in &rows[pos + 1..] {
                if ex.rule_row_p(r) {
                    rule = true;
                } else if !special(ex, r) {
                    if rule {
                        below = true;
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                if rule {
                    below = true;
                }
                bottom = true;
            }
        }
        let first = ex
            .row_cells(id)
            .into_iter()
            .find(|c| !ex.info.ignore.contains(c));
        let left = first.is_some_and(|c| html::cell_borders(ex, c).0);
        let mut out = String::new();
        if top && (self.utf8 || above) {
            let corners = if self.utf8 {
                ('┍', '━', '┯', '┑')
            } else {
                ('+', '-', '+', '+')
            };
            out.push_str(&self.hline(ex, id, corners));
        } else if above {
            let corners = if self.utf8 {
                ('├', '─', '┼', '┤')
            } else {
                ('+', '-', '+', '+')
            };
            out.push_str(&self.hline(ex, id, corners));
        }
        if left {
            out.push_str(if self.utf8 { "│" } else { "|" });
        }
        out.push_str(&contents);
        out.push('\n');
        if bottom && (self.utf8 || below) {
            let corners = if self.utf8 {
                ('┕', '━', '┷', '┙')
            } else {
                ('+', '-', '+', '+')
            };
            out.push_str(&self.hline(ex, id, corners));
        }
        Some(out)
    }

    /// The document's title block (`org-ascii-template--document-title`).
    fn title_block(&self, ex: &mut Exporter<'_>) -> String {
        let with_title = ex.flag("with-title");
        let title = if with_title {
            ex.info
                .parsed
                .get("title")
                .cloned()
                .map(|t| ex.data_list(&t))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let subtitle = if with_title {
            ex.info
                .parsed
                .get("subtitle")
                .cloned()
                .map(|t| ex.data_list(&t))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let author = if ex.flag("with-author") {
            ex.info
                .parsed
                .get("author")
                .cloned()
                .filter(|a| !a.is_empty())
                .map(|a| ex.data_list(&a))
        } else {
            None
        }
        .filter(|a| !a.trim().is_empty());
        let email = if ex.flag("with-email") {
            ex.string("email").map(str::to_string)
        } else {
            None
        }
        .filter(|e| !e.trim().is_empty());
        let date = if ex.flag("with-date") {
            ex.info
                .parsed
                .get("date")
                .cloned()
                .map(|d| ex.data_list(&d))
        } else {
            None
        }
        .filter(|d| !d.trim().is_empty());
        let tw = TEXT_WIDTH;
        if title.is_empty() {
            return match (&author, &email, &date) {
                (Some(a), e, Some(d)) => format!(
                    "{a}{}{d}{}\n\n\n",
                    " ".repeat(tw.saturating_sub(width(d) + width(a))),
                    e.as_ref().map(|e| format!("\n{e}")).unwrap_or_default()
                ),
                (None, Some(e), Some(d)) => format!(
                    "{e}{}{d}\n\n\n",
                    " ".repeat(tw.saturating_sub(width(d) + width(e)))
                ),
                (None, None, Some(d)) => {
                    format!("{}\n\n\n", fill::justify_lines(d, tw, Justify::Right))
                }
                (Some(a), Some(e), None) => format!("{a}\n{e}\n\n\n"),
                (Some(a), None, None) => format!("{a}\n\n\n"),
                (None, Some(e), None) => format!("{e}\n\n\n"),
                (None, None, None) => String::new(),
            };
        }
        let title_len = format!("{title}\n{subtitle}")
            .split('\n')
            .map(width)
            .max()
            .unwrap_or(0)
            .min(2 * tw / 3);
        let formatted = fill_str(ex, &title, title_len, Justify::Left);
        let formatted_sub = (!subtitle.trim().is_empty())
            .then(|| fill_str(ex, &subtitle, title_len, Justify::Left));
        let line_len = (title_len
            .max(author.as_deref().map_or(0, width))
            .max(email.as_deref().map_or(0, width))
            + 2)
        .min(tw);
        let line: String =
            std::iter::repeat_n(if self.utf8 { '━' } else { '_' }, line_len).collect();
        let mut s = format!("{line}\n");
        if !self.utf8 {
            s.push('\n');
        }
        s.push_str(&formatted.to_uppercase());
        if let Some(sub) = formatted_sub {
            s.push('\n');
            s.push_str(&sub);
        }
        match (&author, &email) {
            (Some(a), Some(e)) => s.push_str(&format!("\n\n{a}\n{e}")),
            (Some(a), None) => s.push_str(&format!("\n\n{a}")),
            (None, Some(e)) => s.push_str(&format!("\n\n{e}")),
            (None, None) => {}
        }
        s.push('\n');
        s.push_str(&line);
        if let Some(d) = &date {
            s.push_str(&format!("\n\n\n{d}"));
        }
        s.push_str("\n\n\n");
        fill::justify_lines(&s, tw, Justify::Center)
    }

    fn keyword(&self, ex: &mut Exporter<'_>, id: Id) -> Option<String> {
        let k: ast::Keyword = cast(ex, id)?;
        let value = k.value();
        let out = match k.key().as_str() {
            "ASCII" => value,
            "TOC" => {
                let lower = value.to_lowercase();
                let words: Vec<&str> = lower
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|w| !w.is_empty())
                    .collect();
                if words.contains(&"headlines") {
                    let req = html::toc_request(&value);
                    let scope = html::toc_scope(ex, id, &req).ok()?;
                    self.build_toc(ex, req.depth, Some(id), scope)
                } else if words.contains(&"tables") {
                    self.list_of(ex, id, TABLE)
                } else if words.contains(&"listings") {
                    self.list_of(ex, id, SRC_BLOCK)
                } else {
                    return None;
                }
            }
            _ => return None,
        };
        Some(self.justify_element(ex, id, out))
    }
}

/// The TODO keyword of a headline or an inlinetask.
fn keyword(ex: &Exporter<'_>, id: Id) -> Option<String> {
    ex.headline(id)
        .and_then(|h| h.todo_keyword())
        .map(|t| t.text().to_string())
        .or_else(|| {
            cast::<ast::Inlinetask>(ex, id)
                .and_then(|h| h.todo_keyword())
                .map(|t| t.text().to_string())
        })
}

fn priority(ex: &Exporter<'_>, id: Id) -> Option<char> {
    ex.headline(id)
        .and_then(|h| h.priority())
        .or_else(|| cast::<ast::Inlinetask>(ex, id).and_then(|h| h.priority()))
}

/// The number of an item in its list (`org-list-get-item-number`, the
/// last part).
fn item_number(ex: &Exporter<'_>, item: Id) -> usize {
    let Some(list) = ex.tree.parent(item) else {
        return 1;
    };
    let mut n = 0;
    for &s in ex.tree.children(list) {
        let counter = cast::<ast::Item>(ex, s).and_then(|i| i.counter());
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

/// A bullet (`1.`, `a)`) with its number replaced.
fn replace_counter(bullet: &str, n: &str) -> String {
    let start = bullet.find(|c: char| c.is_ascii_alphanumeric());
    match start {
        Some(s) => {
            let end = bullet[s..]
                .find(|c: char| !c.is_ascii_alphanumeric())
                .map_or(bullet.len(), |e| s + e);
            format!("{}{n}{}", &bullet[..s], &bullet[end..])
        }
        None => bullet.to_string(),
    }
}

/// `org-export-get-ordinal` with `org-ascii--has-caption-p`.
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
        ITEM => Some(item_number(ex, el).to_string()),
        FOOTNOTE_DEFINITION | FOOTNOTE_REFERENCE => Some(ex.footnote_number(el).to_string()),
        // Only a captioned element has a number.
        _ => ex
            .tree
            .secondary(el, Secondary::Caption(0))
            .is_some()
            .then(|| ordinal_captioned(ex, el).to_string()),
    }
}

/// The position of an element among the captioned ones of its type.
fn ordinal_captioned(ex: &Exporter<'_>, el: Id) -> usize {
    let kind = ex.tree.kind(el);
    let mut n = 0;
    for d in ex.tree.descendants(ex.tree.root) {
        if d == el {
            return n + 1;
        }
        if ex.tree.kind(d) == kind
            && ex.tree.secondary(d, Secondary::Caption(0)).is_some()
            && !ex.info.ignore.contains(&d)
        {
            n += 1;
        }
    }
    n + 1
}

impl Backend for Text {
    fn name(&self) -> &'static str {
        "ascii"
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
                | TIMESTAMP
                | UNDERLINE
                | VERBATIM
                | VERSE_BLOCK
        )
    }

    fn options(&self) -> Vec<crate::export::BackendOption> {
        vec![(
            "subtitle",
            Some("SUBTITLE"),
            None,
            crate::options::Behavior::Parse,
            Value::Nil,
        )]
    }

    fn plain_text(&self, ex: &mut Exporter<'_>, text: &str) -> String {
        let mut out = text.to_string();
        if self.utf8 && ex.flag("with-smart-quotes") {
            out = ex.smart_quotes(ex.current_text, &out, crate::quotes::Encoding::Utf8);
        }
        if !ex.flag("with-special-strings") {
            return out;
        }
        out = out.replace("\\-", "");
        if self.utf8 {
            out = out
                .replace("---", "—")
                .replace("--", "–")
                .replace("...", "…");
        }
        out
    }

    fn filter_output(&self, ex: &mut Exporter<'_>, id: Id, out: String) -> String {
        // `org-ascii-filter-headline-blank-lines`: two blank lines after a
        // headline or a section.
        // (`\n\(?:\n[ \t]*\)*\'` becomes three line feeds).
        if matches!(ex.tree.kind(id), Some(HEADLINE | SECTION)) {
            let body = out.trim_end_matches(['\n', ' ', '\t']).len();
            let tail = &out[body..];
            let start = tail
                .char_indices()
                .find(|&(i, c)| {
                    c == '\n' && matches!(tail[i + 1..].chars().next(), None | Some('\n'))
                })
                .map(|(i, _)| body + i);
            if let Some(i) = start {
                return format!("{}\n\n\n", &out[..i]);
            }
        }
        out
    }

    fn filter_final_output(&self, _: &mut Exporter<'_>, out: String) -> String {
        out.replace(HARD, "")
    }

    fn transcode(&self, ex: &mut Exporter<'_>, id: Id, contents: Option<String>) -> Option<String> {
        let kind = ex.tree.kind(id)?;
        let c = || contents.clone().unwrap_or_default();
        let verbatim = |v: String| format!("`{v}'");
        Some(match kind {
            BOLD => format!("*{}*", c()),
            ITALIC => format!("/{}/", c()),
            UNDERLINE => format!("_{}_", c()),
            STRIKE_THROUGH => format!("+{}+", c()),
            CODE => verbatim(cast::<ast::Code>(ex, id)?.value()),
            VERBATIM => verbatim(cast::<ast::Verbatim>(ex, id)?.value()),
            INLINE_SRC_BLOCK => verbatim(cast::<ast::InlineSrcBlock>(ex, id)?.value()),
            CENTER_BLOCK | DYNAMIC_BLOCK | SPECIAL_BLOCK | DRAWER => return contents,
            RADIO_TARGET => return contents,
            CLOCK => {
                let cl: ast::Clock = cast(ex, id)?;
                let ts = cl
                    .timestamp()
                    .map(|t| crate::timestamps::interpret(ast::AstNode::syntax(&t)))
                    .unwrap_or_default();
                let dur = cl
                    .duration()
                    .map(|d| {
                        let (h, m) = d.split_once(':').unwrap_or((&d, "0"));
                        format!(" => {h:>2}:{:0>2}", m)
                    })
                    .unwrap_or_default();
                self.justify_element(ex, id, format!("CLOCK: {ts}{dur}"))
            }
            ENTITY => {
                let e: ast::Entity = cast(ex, id)?;
                if self.utf8 {
                    e.utf8()?.to_string()
                } else {
                    e.ascii()?.to_string()
                }
            }
            EXAMPLE_BLOCK => {
                let code = crate::md::format_code_default(ex, id);
                let b = self.boxed(&code);
                self.justify_element(ex, id, b)
            }
            EXPORT_BLOCK => {
                let b: ast::ExportBlock = cast(ex, id)?;
                if b.backend().as_deref() != Some("ASCII") {
                    return None;
                }
                self.justify_element(ex, id, b.value())
            }
            EXPORT_SNIPPET => {
                let s: ast::ExportSnippet = cast(ex, id)?;
                if s.backend() != "ascii" {
                    return None;
                }
                s.value()
            }
            FIXED_WIDTH => {
                let v = cast::<ast::FixedWidth>(ex, id)?.value();
                let v = html::remove_indentation(&v);
                let b = self.boxed(&v);
                self.justify_element(ex, id, b)
            }
            FOOTNOTE_REFERENCE => format!("[{}]", ex.footnote_number(id)),
            HEADLINE => return self.headline(ex, id, contents),
            HORIZONTAL_RULE => {
                let w = self.text_width(ex, id);
                let spec = html::read_attribute(ex, id, "ATTR_ASCII")
                    .into_iter()
                    .find(|(k, _)| k == ":width")
                    .and_then(|(_, v)| v)
                    .and_then(|v| v.parse::<usize>().ok());
                let ch = if self.utf8 { '―' } else { '-' };
                let line: String = std::iter::repeat_n(ch, spec.unwrap_or(w)).collect();
                fill::justify_lines(&line, w, Justify::Center)
            }
            INLINETASK => {
                let w = self.text_width(ex, id);
                let title = self.build_title(ex, id, w, false, false, false);
                let title = if width(&title) <= w {
                    title
                } else {
                    fill_str(ex, &title, w, Justify::Left)
                };
                let heavy: String =
                    std::iter::repeat_n(if self.utf8 { '━' } else { '_' }, w).collect();
                let mut s = format!("{heavy}\n");
                if !self.utf8 {
                    s.push_str(&" ".repeat(w));
                    s.push('\n');
                }
                s.push_str(&title);
                s.push('\n');
                let c = c();
                if !c.trim().is_empty() {
                    let light: String =
                        std::iter::repeat_n(if self.utf8 { '─' } else { '-' }, w).collect();
                    s.push_str(&format!("{light}\n{c}"));
                }
                s.push_str(&heavy);
                let in_headline = ex
                    .tree
                    .ancestors(id)
                    .any(|a| ex.tree.kind(a) == Some(HEADLINE));
                let margin = TEXT_WIDTH
                    .saturating_sub(if in_headline { INNER_MARGIN } else { 0 })
                    .saturating_sub(w);
                indent(&s, margin)
            }
            ITEM => self.item(ex, id, contents),
            KEYWORD => return self.keyword(ex, id),
            LATEX_ENVIRONMENT => {
                if !ex.flag("with-latex") {
                    return None;
                }
                let v = cast::<ast::LatexEnvironment>(ex, id)?.value();
                self.justify_element(ex, id, html::remove_indentation(&v))
            }
            LATEX_FRAGMENT => {
                if !ex.flag("with-latex") {
                    return None;
                }
                cast::<ast::LatexFragment>(ex, id)?.value()
            }
            LINE_BREAK => format!("{HARD}\n"),
            LINK => return self.link(ex, id, contents),
            NODE_PROPERTY => {
                let p: ast::NodeProperty = cast(ex, id)?;
                let v = p.value();
                if v.is_empty() {
                    format!("{}:", p.key())
                } else {
                    format!("{}: {v}", p.key())
                }
            }
            PARAGRAPH => {
                let c = remove_blank_lines(&c());
                self.justify_element(ex, id, c)
            }
            PLAIN_LIST => return contents,
            PLANNING => {
                let p: ast::Planning = cast(ex, id)?;
                let mut parts = Vec::new();
                for (label, ts) in [
                    ("CLOSED:", p.closed()),
                    ("DEADLINE:", p.deadline()),
                    ("SCHEDULED:", p.scheduled()),
                ] {
                    if let Some(t) = ts {
                        let raw = crate::timestamps::interpret(ast::AstNode::syntax(&t));
                        parts.push(format!("{label} {raw}"));
                    }
                }
                self.justify_element(ex, id, parts.join(" "))
            }
            PROPERTY_DRAWER => {
                let c = c();
                if c.trim().is_empty() {
                    return None;
                }
                self.justify_element(ex, id, c)
            }
            QUOTE_BLOCK => indent(&c(), QUOTE_MARGIN),
            SECTION => {
                let top = !ex
                    .tree
                    .ancestors(id)
                    .any(|a| ex.tree.kind(a) == Some(HEADLINE));
                let mut c = c();
                if top {
                    let links = self.unique_links(ex, id);
                    let w = self.text_width(ex, id);
                    let links = self.describe_links(ex, &links, w);
                    if !links.trim().is_empty() {
                        c = format!("{}\n\n{links}", crate::export::normalize_string(&c));
                    }
                }
                let headline = ex
                    .tree
                    .ancestors(id)
                    .find(|a| ex.tree.kind(*a) == Some(HEADLINE));
                let margin = match headline {
                    Some(h) if ex.low_level_p(h).is_none() => INNER_MARGIN,
                    _ => 0,
                };
                indent(&c, margin)
            }
            SRC_BLOCK => {
                let code = crate::md::format_code_default(ex, id);
                if code.is_empty() {
                    return Some(String::new());
                }
                let caption = self.build_caption(ex, id);
                let mut s = self.boxed(&code);
                if let Some(cap) = caption {
                    s = format!("{s}\n{cap}");
                }
                self.justify_element(ex, id, s)
            }
            STATISTICS_COOKIE => ex.tree.source(id).trim_end().to_string(),
            SUBSCRIPT | SUPERSCRIPT => {
                let brackets = ex.tree.source(id).contains('{');
                let mark = if kind == SUBSCRIPT { '_' } else { '^' };
                if brackets {
                    format!("{mark}{{{}}}", c())
                } else {
                    format!("{mark}{}", c())
                }
            }
            TABLE => {
                let t: ast::Table = cast(ex, id)?;
                let body = if t.table_type() == ast::TableType::Org {
                    c()
                } else {
                    html::remove_indentation(&t.table_el_value().unwrap_or_default())
                };
                let caption = self.build_caption(ex, id);
                let s = match caption {
                    Some(cap) => format!("{body}{cap}"),
                    None => body,
                };
                self.justify_element(ex, id, s)
            }
            TABLE_CELL => self.table_cell(ex, id, contents),
            TABLE_ROW => return self.table_row(ex, id, c()),
            TIMESTAMP => {
                let raw = crate::timestamps::interpret(ex.syntax(id)?);
                self.plain_text(ex, &raw)
            }
            VERSE_BLOCK => {
                let j = self.justify_element(ex, id, c());
                indent(&j, QUOTE_MARGIN)
            }
            _ => return None,
        })
    }

    fn inner_template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let defs = ex.collect_footnote_definitions();
        let mut out = body;
        if !defs.is_empty() {
            let title = ex.translate("Footnotes", self.charset());
            out.push_str(&format!(
                "\n\n\n{title}\n{}\n\n",
                self.line_char().to_string().repeat(width(&title))
            ));
            let mut notes = Vec::new();
            for (n, _, def) in defs {
                let id = format!("[{n}] ");
                let has_elements = def
                    .iter()
                    .any(|&d| ex.tree.kind(d).is_some_and(|k| k.is_element()));
                let text = if has_elements {
                    let first = def[0];
                    if ex.tree.kind(first) != Some(PARAGRAPH) {
                        format!("{id}\n{}", ex.data_list(&def))
                    } else {
                        // The number goes in front of the first paragraph.
                        let t = ex.tree.text_node(id.clone(), Some(first));
                        ex.tree.nodes[first].children.insert(0, t);
                        ex.forget(first);
                        ex.data_list(&def)
                    }
                } else {
                    let d = ex.data_list(&def);
                    fill_str(ex, &format!("{id}{d}"), TEXT_WIDTH, Justify::Left)
                };
                notes.push(trim(&text).to_string());
            }
            out.push_str(&notes.join("\n\n"));
        }
        crate::export::normalize_string(&out)
    }

    fn template(&self, ex: &mut Exporter<'_>, body: String) -> String {
        let mut out = self.title_block(ex);
        match ex.opt("with-toc") {
            Value::Nil => {}
            v => {
                out.push_str(&self.build_toc(ex, v.int(), None, None));
                out.push_str("\n\n\n");
            }
        }
        out.push_str(&body);
        if ex.flag("with-creator") {
            let creator = match ex.opt("creator") {
                Value::Str(s) => s,
                _ => String::new(),
            };
            out.push_str("\n\n\n");
            out.push_str(&fill_str(ex, &creator, TEXT_WIDTH, Justify::Right));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn export(text: &str) -> String {
        crate::export(
            text,
            &Text::default(),
            &crate::Settings {
                body_only: true,
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn sections_end_with_two_blank_lines() {
        let out = export("* A\n| a | b |\n\n* B\nx\n");
        assert!(
            out.contains("| a | b |") || out.contains(" a  b "),
            "{out:?}"
        );
        assert!(out.contains("   a  b \n\n\n2 B\n"), "{out:?}");
    }
}
