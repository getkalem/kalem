//! Statistics cookies (`[2/5]`, `[40%]`), computed as
//! `(org-update-statistics-cookies 'all)` computes them: first from
//! checkboxes (`org-update-checkbox-count`), then from the TODO states of
//! child headlines (`org-update-parent-todo-statistics`), which wins.

use std::collections::HashMap;

use org_syntax::{SyntaxKind, SyntaxNode};

use crate::{Document, EntryId, Inherit};

/// A heading line as `outline-regexp` sees it: headlines, inlinetasks and
/// the `END` lines of inlinetasks.
struct HeadingLine {
    start: usize,
    end: usize,
    stars: usize,
    todo: Option<String>,
}

/// Whether `s` contains `word` as a whole word (`\<word\>`).
fn has_word(s: &str, word: &str) -> bool {
    s.match_indices(word).any(|(i, _)| {
        let before = s[..i].chars().next_back();
        let after = s[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

fn format_cookie(percent: bool, done: usize, all: usize) -> String {
    if percent {
        format!("[{}%]", (100 * done) / all.max(1))
    } else {
        format!("[{done}/{all}]")
    }
}

impl Document {
    fn heading_lines(&self) -> Vec<HeadingLine> {
        let text = self.parse.syntax().to_string();
        let ctx = self.parse.context();
        let mut out = Vec::new();
        let mut pos = 0;
        for raw in text.split_inclusive('\n') {
            let line = raw.trim_end_matches(['\n', '\r']);
            let stars = line.bytes().take_while(|b| *b == b'*').count();
            if stars > 0 && line.as_bytes().get(stars) == Some(&b' ') {
                let todo = crate::properties::complex_heading_todo(line, ctx);
                out.push(HeadingLine {
                    start: pos,
                    end: pos + line.len(),
                    stars,
                    todo,
                });
            }
            pos += raw.len();
        }
        out
    }

    /// Heading lines (start, number of stars), inlinetask `END` lines
    /// included.
    pub(crate) fn heading_lines_pub(&self) -> Vec<(usize, usize)> {
        self.heading_lines()
            .into_iter()
            .map(|l| (l.start, l.stars))
            .collect()
    }

    /// The statistics cookies of the document and their updated text, in
    /// document order.
    pub fn statistics_cookies(&self) -> Vec<(usize, String)> {
        let root = self.parse.syntax();
        let cookies: Vec<SyntaxNode> = root
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::STATISTICS_COOKIE)
            .collect();
        let text_of = |n: &SyntaxNode| {
            let t = n.text().to_string();
            t.trim_end_matches([' ', '\t']).to_string()
        };
        let mut values: HashMap<usize, String> = cookies
            .iter()
            .map(|c| (usize::from(c.text_range().start()), text_of(c)))
            .collect();

        // The COOKIE_DATA at the start of the buffer applies to every
        // checkbox cookie.
        let first = self
            .outline()
            .entries
            .first()
            .filter(|e| usize::from(e.range.start()) == 0)
            .map(|_| EntryId(0));
        let global = self
            .entry_get(first, "COOKIE_DATA", Inherit::No, false)
            .unwrap_or_default();

        if !has_word(&global, "todo") {
            let recursive =
                !self.settings.checkbox_hierarchical_statistics || has_word(&global, "recursive");
            let mut cache: HashMap<usize, (usize, usize)> = HashMap::new();
            for c in &cookies {
                let (done, all) = self.checkbox_count(c, recursive, &mut cache);
                let begin = usize::from(c.text_range().start());
                let percent = text_of(c).contains('%');
                values.insert(begin, format_cookie(percent, done, all));
            }
        }

        // TODO statistics, heading by heading.
        let lines = self.heading_lines();
        let parent = |i: usize| (0..i).rev().find(|&j| lines[j].stars < lines[i].stars);
        let done_keywords = &self.parse.context().done_keywords;
        for i in 0..lines.len() {
            let Some(p0) = parent(i) else { continue };
            let p_entry = self.outline().entry_at(lines[p0].start);
            let (prop, lim) = self.entry_get_with_source(p_entry, "COOKIE_DATA");
            let recursive = !self.settings.hierarchical_todo_statistics
                || prop.as_deref().is_some_and(|p| has_word(p, "recursive"));
            let lim = lim.unwrap_or(0);
            let ltoggle = lines[i].stars;
            let mut first = true;
            let mut cur = i;
            'up: while let Some(p) = parent(cur) {
                if !(recursive || first) || lines[p].start < lim {
                    break;
                }
                first = false;
                let entry = self.outline().entry_at(lines[p].start);
                let data = self
                    .entry_get(entry, "COOKIE_DATA", Inherit::No, false)
                    .unwrap_or_default();
                if has_word(&data.to_lowercase(), "checkbox") {
                    break 'up;
                }
                let (mut all, mut done) = (0, 0);
                for l in lines[p + 1..]
                    .iter()
                    .take_while(|l| l.stars > lines[p].stars)
                {
                    let kwd = if recursive || l.stars == ltoggle {
                        l.todo.as_ref()
                    } else {
                        None
                    };
                    if let Some(k) = kwd {
                        all += 1;
                        if done_keywords.contains(k) {
                            done += 1;
                        }
                    }
                }
                for c in cookies.iter().filter(|c| {
                    let s = usize::from(c.text_range().start());
                    s >= lines[p].start && s < lines[p].end
                }) {
                    let percent = text_of(c).contains('%');
                    values.insert(
                        usize::from(c.text_range().start()),
                        format_cookie(percent, done, all),
                    );
                }
                cur = p;
            }
        }
        cookies
            .iter()
            .map(|c| {
                let b = usize::from(c.text_range().start());
                (b, values.remove(&b).unwrap_or_default())
            })
            .collect()
    }

    /// `org-update-checkbox-count` on `from..to` (a section, or an
    /// inlinetask): each statistics cookie there as (start, end, new
    /// text), counted from checkboxes. `cookie_data` is the entry's
    /// `COOKIE_DATA`; with `todo` in it nothing is counted.
    pub fn checkbox_cookies(
        &self,
        from: usize,
        to: usize,
        cookie_data: &str,
    ) -> Vec<(usize, usize, String)> {
        let data = cookie_data.to_lowercase();
        if has_word(&data, "todo") {
            return Vec::new();
        }
        let recursive =
            !self.settings.checkbox_hierarchical_statistics || has_word(&data, "recursive");
        let mut cache = HashMap::new();
        self.parse
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::STATISTICS_COOKIE)
            .filter(|n| {
                let s = usize::from(n.text_range().start());
                s >= from && s < to
            })
            .map(|c| {
                let start = usize::from(c.text_range().start());
                let t = c.text().to_string();
                let text = t.trim_end_matches([' ', '\t']);
                let (done, all) = self.checkbox_count(&c, recursive, &mut cache);
                (
                    start,
                    start + text.len(),
                    format_cookie(text.contains('%'), done, all),
                )
            })
            .collect()
    }

    /// `org-entry-get-with-inheritance` that also returns where the base
    /// value was found (`org-entry-property-inherited-from`): the start
    /// of the entry, or 0 for a value from the top of the document.
    pub fn entry_get_with_source(
        &self,
        start: Option<EntryId>,
        prop: &str,
    ) -> (Option<String>, Option<usize>) {
        let value = self.entry_get(start, prop, Inherit::Yes, false);
        let mut node = start;
        loop {
            match node {
                Some(id) => {
                    let e = self.entry(id);
                    if e.node_properties
                        .iter()
                        .any(|p| p.key == prop.to_uppercase())
                    {
                        return (value, Some(usize::from(e.range.start())));
                    }
                    node = e.parent;
                }
                None => {
                    let top = self.top_props().has(prop);
                    return (value, top.then_some(0));
                }
            }
        }
    }

    /// Checkboxes counted for one cookie: (checked, total).
    fn checkbox_count(
        &self,
        cookie: &SyntaxNode,
        recursive: bool,
        cache: &mut HashMap<usize, (usize, usize)>,
    ) -> (usize, usize) {
        use SyntaxKind::*;
        let container = cookie.ancestors().find(|a| {
            matches!(
                a.kind(),
                DRAWER
                    | CENTER_BLOCK
                    | DYNAMIC_BLOCK
                    | INLINETASK
                    | ITEM
                    | QUOTE_BLOCK
                    | SPECIAL_BLOCK
                    | VERSE_BLOCK
            )
        });
        let (beg, end) = match &container {
            Some(c) => match org_syntax::ast::contents_range(c) {
                Some(r) => (usize::from(r.start()), usize::from(r.end())),
                None => return (0, 0),
            },
            None => {
                // From the headline the cookie is in (inlinetasks are not
                // headings here) to the next headline.
                let o = usize::from(cookie.text_range().start());
                let heads: Vec<usize> = self
                    .outline()
                    .entries
                    .iter()
                    .filter(|e| !e.inlinetask)
                    .map(|e| usize::from(e.range.start()))
                    .collect();
                let beg = heads.iter().copied().filter(|&h| h <= o).max().unwrap_or(0);
                let end = heads
                    .iter()
                    .copied()
                    .find(|&h| h > beg)
                    .unwrap_or(usize::from(self.parse.syntax().text_range().end()));
                (beg, end)
            }
        };
        if let Some(&v) = cache.get(&beg) {
            return v;
        }
        let root = self.parse.syntax();
        let text = root.to_string();
        let ctx = self.parse.context();
        // Lists with at least one checkbox item, found in order; the search
        // resumes after the outermost list around each find. Each gives the
        // structure org-element computes for its outermost list.
        let mut structs: Vec<Vec<org_syntax::ListItem>> = Vec::new();
        let mut boxes: HashMap<usize, String> = HashMap::new();
        let mut pos = beg;
        for item in root.descendants().filter(|n| n.kind() == ITEM) {
            if let Some(cb) = item.children_with_tokens().find(|t| t.kind() == CHECKBOX) {
                boxes.insert(usize::from(item.text_range().start()), cb.to_string());
            }
        }
        for item in root.descendants().filter(|n| n.kind() == ITEM) {
            let s = usize::from(item.text_range().start());
            if s < pos || s >= end {
                continue;
            }
            if !item.children_with_tokens().any(|t| t.kind() == CHECKBOX) {
                continue;
            }
            let top = structure_root(&item);
            let limit = top
                .parent()
                .and_then(|p| org_syntax::ast::contents_range(&p))
                .map_or(text.len(), |r| usize::from(r.end()));
            structs.push(org_syntax::list_structure(
                &text,
                ctx,
                usize::from(top.text_range().start()),
                limit,
            ));
            let outer = item.ancestors().filter(|a| a.kind() == PLAIN_LIST).last();
            pos = outer.map_or(end, |o| usize::from(o.text_range().end()).min(end));
        }
        let container_item = container
            .filter(|c| c.kind() == ITEM)
            .map(|c| usize::from(c.text_range().start()));
        let (mut on, mut all) = (0, 0);
        for s in &structs {
            let items: Vec<usize> = match (container_item, recursive) {
                (Some(it), true) => match s.iter().find(|i| i.pos == it) {
                    Some(c) => s
                        .iter()
                        .filter(|i| i.pos > it && i.pos < c.end)
                        .map(|i| i.pos)
                        .collect(),
                    None => Vec::new(),
                },
                (None, true) => s.iter().map(|i| i.pos).collect(),
                (Some(it), false) => {
                    let parents = list_parents(s);
                    parents
                        .iter()
                        .filter(|(_, p)| *p == Some(it))
                        .map(|(c, _)| *c)
                        .collect()
                }
                (None, false) => {
                    // `org-list-get-all-items` of the top item: the previous
                    // item of E is the first item ending at E; the next item
                    // of P is the first item whose previous is P.
                    let prev_of = |e: usize| s.iter().find(|x| x.end == e).map(|x| x.pos);
                    let next_of =
                        |p: usize| s.iter().find(|e| prev_of(e.pos) == Some(p)).map(|e| e.pos);
                    let mut out = Vec::new();
                    let mut cur = s.first().map(|i| i.pos);
                    while let Some(c) = cur {
                        out.push(c);
                        cur = next_of(c);
                    }
                    out
                }
            };
            for it in items {
                if let Some(cb) = boxes.get(&it) {
                    all += 1;
                    if cb == "[X]" {
                        on += 1;
                    }
                }
            }
        }
        cache.insert(beg, (on, all));
        (on, all)
    }
}

/// The top list of an item's structure: lists nested directly in items
/// belong to the same structure; a list inside a block or drawer starts
/// its own.
fn structure_root(item: &SyntaxNode) -> SyntaxNode {
    let mut list = item.parent().expect("item in a list");
    while let Some(parent_item) = list.parent().filter(|p| p.kind() == SyntaxKind::ITEM) {
        list = parent_item.parent().expect("item in a list");
    }
    list
}

/// `org-list-parents-alist` of a list structure.
fn list_parents(st: &[org_syntax::ListItem]) -> Vec<(usize, Option<usize>)> {
    let Some(first) = st.first() else {
        return Vec::new();
    };
    let mut ind_to_ori: Vec<(usize, Option<usize>)> = vec![(first.ind, None)];
    let mut prev_pos = vec![first.pos];
    let mut out = vec![(first.pos, None)];
    for item in &st[1..] {
        let prev_ind = ind_to_ori[0].0;
        prev_pos.insert(0, item.pos);
        if prev_ind > item.ind {
            if let Some(k) = ind_to_ori.iter().position(|e| e.0 == item.ind) {
                ind_to_ori.drain(..k);
            } else if let Some(k) = ind_to_ori.iter().position(|e| e.0 < item.ind) {
                ind_to_ori.drain(..k);
            } else {
                ind_to_ori = vec![(item.ind, None)];
            }
            out.push((item.pos, ind_to_ori[0].1));
        } else if prev_ind < item.ind {
            let origin = prev_pos.get(1).copied();
            ind_to_ori.insert(0, (item.ind, origin));
            out.push((item.pos, origin));
        } else {
            out.push((item.pos, ind_to_ori[0].1));
        }
    }
    out
}
