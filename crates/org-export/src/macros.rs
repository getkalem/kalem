//! Macro expansion before export (`org-macro.el`): `{{{title}}}`,
//! `{{{n}}}`, `{{{keyword(NAME)}}}`, `{{{property(NAME)}}}`,
//! `{{{time(FORMAT)}}}`, `{{{input-file}}}`, `{{{modification-time(…)}}}`
//! and `#+MACRO:` definitions with `$1` arguments.
//!
//! Definitions written as `(eval …)` run Emacs Lisp in Emacs; Kalem does
//! not run Lisp and leaves such macros empty (reported as a known
//! difference).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use org_syntax::SyntaxKind::*;
use org_syntax::{SyntaxNode, TextSize, ast};

/// A template: text with `$1` placeholders, or a built-in.
#[derive(Debug, Clone)]
enum Template {
    Text(String),
    Keyword,
    Counter,
    Property,
    Time,
    ModificationTime(Option<jiff::Timestamp>),
    Eval,
}

/// The value of keyword `name` (`org-macro--find-keyword-value`): the
/// first one, or all joined with spaces.
fn keyword_value(keywords: &[(String, String)], name: &str, collect: bool) -> Option<String> {
    let mut vals = keywords
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone());
    if !collect {
        return vals.next();
    }
    let all: Vec<String> = vals.collect();
    if all.is_empty() {
        None
    } else {
        Some(all.join(" ").trim().to_string())
    }
}

fn templates(keywords: &[(String, String)], file: Option<&Path>) -> HashMap<String, Template> {
    let mut t: HashMap<String, Template> = HashMap::new();
    let set = |name: &str, v: Template, t: &mut HashMap<String, Template>| {
        t.entry(name.to_lowercase()).or_insert(v);
    };
    // `#+MACRO:` definitions come first: a later definition does not
    // replace an earlier one.
    for (k, v) in keywords {
        if !k.eq_ignore_ascii_case("MACRO") {
            continue;
        }
        let v = v.trim_start();
        let name_end = v.find([' ', '\t']).unwrap_or(v.len());
        let name = &v[..name_end];
        if name.is_empty() {
            continue;
        }
        let def = v[name_end..].trim_start_matches([' ', '\t']);
        let tpl = if def.starts_with("(eval") {
            Template::Eval
        } else {
            Template::Text(def.to_string())
        };
        set(name, tpl, &mut t);
    }
    for (name, key, collect) in [
        ("author", "AUTHOR", true),
        ("email", "EMAIL", false),
        ("title", "TITLE", true),
        ("date", "DATE", false),
    ] {
        set(
            name,
            Template::Text(keyword_value(keywords, key, collect).unwrap_or_default()),
            &mut t,
        );
    }
    if let Some(f) = file.filter(|f| f.exists()) {
        let name = f
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        set("input-file", Template::Text(name), &mut t);
        let mtime = std::fs::metadata(f)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| jiff::Timestamp::try_from(m).ok());
        set(
            "modification-time",
            Template::ModificationTime(mtime),
            &mut t,
        );
    }
    set("keyword", Template::Keyword, &mut t);
    set("n", Template::Counter, &mut t);
    set("property", Template::Property, &mut t);
    set("time", Template::Time, &mut t);
    t
}

/// `$N` placeholders replaced by arguments (missing ones by nothing).
fn fill(template: &str, args: &[String]) -> String {
    let mut out = String::new();
    let b = template.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'$' && i + 1 < b.len() && b[i + 1].is_ascii_digit() {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let n: usize = template[i + 1..j].parse().unwrap_or(0);
            if n >= 1
                && let Some(a) = args.get(n - 1)
            {
                out.push_str(a);
            }
            i = j;
            continue;
        }
        let c = template[i..].chars().next().expect("a char");
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// `format-time-string` for the common specifiers.
pub fn format_time(fmt: &str, t: &jiff::Zoned) -> String {
    // jiff's strftime knows the usual specifiers; `%e` and friends too.
    jiff::fmt::strtime::format(fmt, t).unwrap_or_else(|_| fmt.to_string())
}

/// A macro found in the text: its range (without trailing blanks), key
/// and arguments.
struct Found {
    start: usize,
    end: usize,
    key: String,
    args: Vec<String>,
}

fn in_commented_heading(n: &SyntaxNode) -> bool {
    n.ancestors().any(|a| {
        a.kind() == HEADLINE
            && ast::AstNode::cast(a.clone()).is_some_and(|h: ast::Headline| h.is_commented())
    })
}

/// The macro starting at `at` in `text`, parsed on its own.
fn parse_macro_at(text: &str, at: usize) -> Option<Found> {
    let line_end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    let p = org_syntax::parse(&text[at..line_end]);
    let m = p.syntax().descendants().find(|n| n.kind() == MACRO)?;
    if usize::from(m.text_range().start()) != 0 {
        return None;
    }
    let mac: ast::Macro = ast::AstNode::cast(m.clone())?;
    let blank = ast::post_blank(&m);
    Some(Found {
        start: at,
        end: at + usize::from(m.text_range().end()) - blank,
        key: mac.key(),
        args: mac.args(),
    })
}

fn next_macro(text: &str, from: usize, parsed: &[&str]) -> Option<Found> {
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let mut pos = from;
    while let Some(i) = text[pos..].find("{{{") {
        let at = pos + i;
        pos = at + 3;
        let ok = text[at + 3..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !ok {
            continue;
        }
        let Some(tok) = root
            .token_at_offset(TextSize::from(at as u32))
            .right_biased()
        else {
            continue;
        };
        let parent = tok.parent().expect("a node");
        if in_commented_heading(&parent) {
            continue;
        }
        if let Some(m) = tok.parent_ancestors().find(|a| a.kind() == MACRO)
            && usize::from(m.text_range().start()) == at
        {
            let mac: ast::Macro = ast::AstNode::cast(m.clone())?;
            let blank = ast::post_blank(&m);
            return Some(Found {
                start: at,
                end: usize::from(m.text_range().end()) - blank,
                key: mac.key(),
                args: mac.args(),
            });
        }
        // In a parsed keyword (`#+TITLE:`) or an `EXPORT_…` property.
        let keyword = tok
            .parent_ancestors()
            .find(|a| matches!(a.kind(), KEYWORD | NODE_PROPERTY));
        if let Some(k) = keyword {
            let key = match k.kind() {
                KEYWORD => ast::AstNode::cast(k.clone()).map(|x: ast::Keyword| x.key()),
                _ => ast::AstNode::cast(k.clone()).map(|x: ast::NodeProperty| x.key()),
            }
            .unwrap_or_default();
            let wanted = if k.kind() == KEYWORD {
                parsed.iter().any(|p| p.eq_ignore_ascii_case(&key))
            } else {
                key.to_uppercase()
                    .strip_prefix("EXPORT_")
                    .map(|r| r.trim_end_matches('+'))
                    .is_some_and(|r| parsed.iter().any(|p| p.eq_ignore_ascii_case(r)))
            };
            if wanted && let Some(f) = parse_macro_at(text, at) {
                return Some(f);
            }
        }
    }
    None
}

/// The property `name` of the entry around `at` (no inheritance), or of
/// the file before the first headline.
fn property_at(text: &str, at: usize, name: &str) -> Option<String> {
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let tok = root
        .token_at_offset(TextSize::from(at.min(text.len()) as u32))
        .right_biased()?;
    let headline = tok.parent_ancestors().find(|a| a.kind() == HEADLINE);
    match headline {
        Some(h) => {
            let h: ast::Headline = ast::AstNode::cast(h)?;
            h.properties()
                .into_iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v)
        }
        None => parse
            .keywords()
            .into_iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("PROPERTY"))
            .filter_map(|(_, v)| {
                let (k, val) = v.split_once(char::is_whitespace)?;
                k.eq_ignore_ascii_case(name).then(|| val.trim().to_string())
            })
            .next_back(),
    }
}

/// Every macro in `text` replaced by its expansion; `parsed` are the
/// keywords whose values are expanded too (`TITLE`, `DATE`, `AUTHOR`…).
/// An undefined macro is an error, as in Emacs.
pub fn expand(
    text: &str,
    parsed: &[&str],
    file: Option<&Path>,
    now: &jiff::Zoned,
) -> Result<String, String> {
    if !text.contains("{{{") {
        return Ok(text.to_string());
    }
    let keywords = org_syntax::parse(text).keywords();
    let templates = templates(&keywords, file);
    let mut counters: HashMap<String, i64> = HashMap::new();
    let mut record: HashSet<(usize, String, Vec<String>)> = HashSet::new();
    let mut text = text.to_string();
    let mut pos = 0;
    while let Some(m) = next_macro(&text, pos, parsed) {
        let value = match templates.get(&m.key) {
            Some(Template::Text(t)) => Some(fill(t, &m.args)),
            Some(Template::Keyword) => Some(
                keyword_value(
                    &keywords,
                    m.args.first().map(String::as_str).unwrap_or(""),
                    true,
                )
                .unwrap_or_default(),
            ),
            Some(Template::Counter) => {
                let name = m
                    .args
                    .first()
                    .map(|s| s.trim().to_string())
                    .unwrap_or_default();
                let action = m
                    .args
                    .get(1)
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let cur = counters.get(&name).copied();
                let v = match action.as_deref() {
                    None => cur.unwrap_or(0) + 1,
                    Some("-") => cur.unwrap_or(1),
                    Some(a) if a.chars().all(|c| c.is_ascii_digit()) => a.parse().unwrap_or(1),
                    Some(_) => 1,
                };
                counters.insert(name, v);
                Some(v.to_string())
            }
            Some(Template::Property) => {
                let name = m.args.first().cloned().unwrap_or_default();
                Some(property_at(&text, m.start, &name).unwrap_or_default())
            }
            Some(Template::Time) => Some(format_time(
                m.args.first().map(String::as_str).unwrap_or(""),
                now,
            )),
            Some(Template::ModificationTime(t)) => {
                let z = t
                    .map(|t| t.to_zoned(now.time_zone().clone()))
                    .unwrap_or_else(|| now.clone());
                Some(format_time(
                    m.args.first().map(String::as_str).unwrap_or(""),
                    &z,
                ))
            }
            Some(Template::Eval) => Some(String::new()),
            None => None,
        };
        match value {
            Some(v) => {
                let sig = (m.start, m.key.clone(), m.args.clone());
                if !record.insert(sig) {
                    return Err(format!("Circular macro expansion: {}", m.key));
                }
                text.replace_range(m.start..m.end, &v);
                pos = m.start;
            }
            None if m.key == "results" => pos = m.end,
            None => return Err(format!("Undefined Org macro: {}; aborting", m.key)),
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> jiff::Zoned {
        "2026-09-28T10:00:00[UTC]".parse().unwrap()
    }

    #[test]
    fn expanding() {
        let t = "#+TITLE: T\n#+MACRO: greet Hello, $1!\n{{{title}}} {{{greet(you)}}} {{{n}}} {{{n}}} {{{n(x)}}} {{{time(%Y)}}}\n";
        let out = expand(t, &["TITLE"], None, &now()).unwrap();
        assert!(out.ends_with("\nT Hello, you! 1 2 1 2026\n"), "{out}");
        assert!(expand("{{{nope}}}\n", &[], None, &now()).is_err());
        let t = "#+TITLE: {{{keyword(X)}}} done\n#+X: from x\n";
        assert_eq!(
            expand(t, &["TITLE"], None, &now()).unwrap(),
            "#+TITLE: from x done\n#+X: from x\n"
        );
        let t = "* H\n:PROPERTIES:\n:P: v\n:END:\n{{{property(P)}}}\n";
        assert!(expand(t, &[], None, &now()).unwrap().ends_with("\nv\n"));
    }
}
