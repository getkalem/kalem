//! Document-wide settings read from in-buffer keywords, as
//! `org-set-regexps-and-options` computes them.

use std::sync::Arc;

use org_syntax::ast::{AstNode, Keyword};
use org_syntax::{Parse, SyntaxKind, SyntaxNode};

use crate::settings::Settings;

/// `org-split-string`: split at `sep`, ignoring a separator at the start or
/// the end, but keeping empty strings between consecutive separators.
pub(crate) fn split_string(s: &str, sep: char) -> Vec<String> {
    if !s.contains(sep) {
        return vec![s.to_string()];
    }
    let mut parts: Vec<&str> = s.split(sep).collect();
    if s.starts_with(sep) {
        parts.remove(0);
    }
    if s.ends_with(sep) {
        parts.pop();
    }
    parts.into_iter().map(str::to_string).collect()
}

/// `org--update-property-plist`: `KEY+` appends to an existing value (with
/// a space), a plain key replaces it, and new keys go to the front.
pub(crate) fn update_property_alist(key: &str, value: &str, props: &mut Vec<(String, String)>) {
    let (appending, key) = match key.strip_suffix('+') {
        Some(k) => (true, k),
        None => (false, key),
    };
    match props.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(key)) {
        None => props.insert(0, (key.to_string(), value.to_string())),
        Some(old) => {
            if appending {
                old.1 = format!("{} {}", old.1, value);
            } else {
                old.1 = value.to_string();
            }
        }
    }
}

/// Keywords (key upcased, value) in an element container, not entering
/// paragraphs, tables, blocks or objects.
fn keywords_in(node: &SyntaxNode, out: &mut Vec<(String, String)>) {
    let mut stack = vec![node.clone()];
    while let Some(n) = stack.pop() {
        if let Some(k) = Keyword::cast(n.clone()) {
            out.push((k.key(), k.value()));
            continue;
        }
        let children: Vec<SyntaxNode> = n
            .children()
            .filter(|c| c.kind() == SyntaxKind::KEYWORD || c.kind().is_greater_element())
            .collect();
        stack.extend(children.into_iter().rev());
    }
}

/// The keywords of a headline's subtree, as cached across versions: its
/// section's, and its child headlines', which are shared.
#[derive(Debug)]
pub(crate) struct Keywords {
    parts: Vec<Part>,
    /// Keywords in all.
    len: usize,
}

#[derive(Debug)]
enum Part {
    Section(Vec<(String, String)>),
    Headline(Arc<Keywords>),
}

impl Keywords {
    /// Appends the keywords in order.
    fn copy_to(&self, out: &mut Vec<(String, String)>) {
        if self.len == 0 {
            return;
        }
        for p in &self.parts {
            match p {
                Part::Section(v) => out.extend(v.iter().cloned()),
                Part::Headline(k) => k.copy_to(out),
            }
        }
    }
}

type Pass<'a> = crate::cache::Pass<'a, Keywords>;

/// The keywords of a headline's subtree, reusing cached subtrees.
fn headline_keywords(node: &SyntaxNode, pass: Option<&Pass<'_>>) -> Arc<Keywords> {
    if let Some(v) = pass.and_then(|p| p.get(node)) {
        return v;
    }
    let mut parts = Vec::new();
    let mut len = 0;
    for child in node.children() {
        match child.kind() {
            SyntaxKind::SECTION => {
                let mut v = Vec::new();
                keywords_in(&child, &mut v);
                if !v.is_empty() {
                    len += v.len();
                    parts.push(Part::Section(v));
                }
            }
            SyntaxKind::HEADLINE => {
                let k = headline_keywords(&child, pass);
                len += k.len;
                parts.push(Part::Headline(k));
            }
            _ => {}
        }
    }
    let v = Arc::new(Keywords { parts, len });
    if let Some(p) = pass {
        p.put(node, v.clone());
    }
    v
}

/// The document's keywords in order, without setup files.
fn document_keywords(
    root: &SyntaxNode,
    cache: Option<&crate::cache::ModelCache>,
) -> Vec<(String, String)> {
    let pass = cache.map(|c| c.keywords.pass());
    let mut out = Vec::new();
    for child in root.children() {
        match child.kind() {
            SyntaxKind::SECTION => keywords_in(&child, &mut out),
            SyntaxKind::HEADLINE => headline_keywords(&child, pass.as_ref()).copy_to(&mut out),
            _ => {}
        }
    }
    out
}

/// Priorities as character codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Priorities {
    /// `org-priority-highest`.
    pub highest: u32,
    /// `org-priority-lowest`.
    pub lowest: u32,
    /// `org-priority-default`.
    pub default: u32,
}

/// Document-wide settings from keywords.
#[derive(Debug, Clone)]
pub struct Info {
    /// Every keyword, with setup files expanded (see [`Parse::keywords`]).
    pub keywords: Vec<(String, String)>,
    /// `org-file-tags` from `#+FILETAGS`.
    pub file_tags: Vec<String>,
    /// `org-keyword-properties` from `#+PROPERTY` and `#+CATEGORY`, in
    /// Emacs's order.
    pub keyword_properties: Vec<(String, String)>,
    /// `org-category` after `#+CATEGORY` (the first one).
    pub category: Option<String>,
    /// `#+ARCHIVE`, the first one.
    pub archive: Option<String>,
    /// `#+COLUMNS`, the first one.
    pub columns: Option<String>,
    /// `#+PRIORITIES`, or the settings' defaults.
    pub priorities: Priorities,
}

/// `org-priority-to-value`: the first number in `s`, or its first
/// character.
fn priority_value(s: &str) -> u32 {
    let digits: String = s
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits
        .parse()
        .unwrap_or_else(|_| s.chars().next().map_or(0, |c| c as u32))
}

impl Info {
    /// Reads the keywords of `parse`.
    pub fn new(
        parse: &Parse,
        settings: &Settings,
        cache: Option<&crate::cache::ModelCache>,
    ) -> Info {
        let keywords = parse.with_setup_files(document_keywords(&parse.syntax(), cache));
        // `org-collect-keywords` with ARCHIVE, CATEGORY, COLUMNS and
        // PRIORITIES as unique keywords: their first value only.
        let first = |key: &str| {
            keywords
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone())
        };
        fn all<'a>(
            keywords: &'a [(String, String)],
            key: &'a str,
        ) -> impl Iterator<Item = &'a str> {
            keywords
                .iter()
                .filter(move |(k, _)| k == key)
                .map(|(_, v)| v.as_str())
        }
        let file_tags = all(&keywords, "FILETAGS")
            .flat_map(|v| {
                v.split_whitespace()
                    .flat_map(|w| split_string(w, ':'))
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut keyword_properties = Vec::new();
        for v in all(&keywords, "PROPERTY") {
            // `\(\S-+\)[ \t]+\(.*\)`
            let v = v.trim_start_matches([' ', '\t']);
            if let Some(i) = v.find([' ', '\t'])
                && i > 0
            {
                let (k, rest) = v.split_at(i);
                let rest = rest.trim_start_matches([' ', '\t']);
                update_property_alist(k, rest, &mut keyword_properties);
            }
        }
        let category = first("CATEGORY");
        if let Some(c) = &category {
            update_property_alist("CATEGORY", c, &mut keyword_properties);
        }
        let (h, l, d) = settings.priorities;
        let mut priorities = Priorities {
            highest: h,
            lowest: l,
            default: d,
        };
        if let Some(p) = first("PRIORITIES") {
            let w: Vec<&str> = p.split_whitespace().collect();
            if w.len() >= 3 {
                priorities = Priorities {
                    highest: priority_value(w[0]),
                    lowest: priority_value(w[1]),
                    default: priority_value(w[2]),
                };
            }
        }
        Info {
            file_tags,
            keyword_properties,
            category: category.or_else(|| settings.category.clone()),
            archive: first("ARCHIVE"),
            columns: first("COLUMNS"),
            priorities,
            keywords,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn org_split_string() {
        assert_eq!(split_string(":a:b:", ':'), vec!["a", "b"]);
        assert_eq!(split_string("a::b", ':'), vec!["a", "", "b"]);
        assert_eq!(split_string("a", ':'), vec!["a"]);
        assert_eq!(split_string(":", ':'), Vec::<String>::new());
        assert_eq!(split_string("::", ':'), vec![""]);
        assert_eq!(split_string("", ':'), vec![""]);
    }

    #[test]
    fn property_plist() {
        let mut p = Vec::new();
        update_property_alist("A", "1", &mut p);
        update_property_alist("B", "2", &mut p);
        update_property_alist("a+", "3", &mut p);
        assert_eq!(
            p,
            vec![("B".into(), "2".into()), ("A".into(), "1 3".into())]
        );
    }
}
