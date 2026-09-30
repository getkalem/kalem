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
    /// `{{{modification-time(FORMAT, VC)}}}`: the file's time, or with a
    /// second argument its last commit's.
    ModificationTime(Option<jiff::Timestamp>, std::path::PathBuf),
    /// `{{{date}}}` when `#+DATE` is one timestamp: the value, or with an
    /// argument the timestamp formatted (`org-macro--find-date`).
    Date(String, ast::DateTime),
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

fn templates(
    definitions: &[(String, String)],
    keywords: &[(String, String)],
    file: Option<&Path>,
) -> HashMap<String, Template> {
    let mut t: HashMap<String, Template> = HashMap::new();
    let set = |name: &str, v: Template, t: &mut HashMap<String, Template>| {
        t.entry(name.to_lowercase()).or_insert(v);
    };
    // `#+MACRO:` definitions come first: a later definition does not
    // replace an earlier one.
    for (k, v) in definitions {
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
    // The keywords' values come after the definitions in
    // `org-macro--set-templates`: a keyword that is present replaces a
    // `#+MACRO:` of the same name; an absent one leaves it.
    for (name, key, collect) in [
        ("author", "AUTHOR", true),
        ("email", "EMAIL", false),
        ("title", "TITLE", true),
        ("date", "DATE", false),
    ] {
        match keyword_value(keywords, key, collect) {
            Some(v) => {
                let tpl = match single_timestamp(&v).filter(|_| name == "date") {
                    Some(ts) => Template::Date(v, ts),
                    None => Template::Text(v),
                };
                t.insert(name.to_string(), tpl);
            }
            None => set(name, Template::Text(String::new()), &mut t),
        }
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
            Template::ModificationTime(mtime, f.to_path_buf()),
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
fn property_at(
    text: &str,
    at: usize,
    name: &str,
    location: Option<&str>,
    file: Option<&Path>,
) -> Option<String> {
    let parse = org_syntax::parse(text);
    property_in(&parse.syntax(), &parse.keywords(), at, name, location, file)
}

/// The headline `location` names, as `org-link-search` finds it for the
/// `property` macro with `org-link-search-must-match-exact-headline`:
/// `#ID` by its `CUSTOM_ID`, `*Title` or `Title` by its title.
fn find_location(root: &SyntaxNode, location: &str) -> Option<ast::Headline> {
    let loc = location.trim();
    let headlines = root
        .descendants()
        .filter(|n| n.kind() == HEADLINE)
        .filter_map(<ast::Headline as ast::AstNode>::cast);
    if let Some(id) = loc.strip_prefix('#') {
        return headlines.into_iter().find(|h| {
            h.properties()
                .iter()
                .any(|(k, v)| k.eq_ignore_ascii_case("CUSTOM_ID") && v.trim() == id)
        });
    }
    let title = loc.strip_prefix('*').unwrap_or(loc).trim();
    let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    headlines
        .into_iter()
        .find(|h| squash(&h.raw_value()) == squash(title))
}

/// [`property_at`] in the tree `root`, with its `keywords`, as
/// `org-macro--get-property` gives it: at `location` when there is one
/// (an error message when it names no headline), else at the entry
/// around `at`; the special properties as `org-entry-properties` gives
/// them; before the first headline, the file's property drawer.
fn property_in(
    root: &SyntaxNode,
    keywords: &[(String, String)],
    at: usize,
    name: &str,
    location: Option<&str>,
    file: Option<&Path>,
) -> Option<String> {
    let len = usize::from(root.text_range().end());
    let headline = match location.filter(|l| !l.trim().is_empty()) {
        Some(l) => Some(find_location(root, l)?),
        None => {
            let tok = root
                .token_at_offset(TextSize::from(at.min(len) as u32))
                .right_biased()?;
            tok.parent_ancestors()
                .find(|a| a.kind() == HEADLINE)
                .and_then(<ast::Headline as ast::AstNode>::cast)
        }
    };
    let upper = name.to_ascii_uppercase();
    let Some(h) = headline else {
        // Before the first headline: the property drawer at the top.
        return root
            .children()
            .find(|c| c.kind() == SECTION)
            .and_then(|sec| sec.children().find(|c| c.kind() == PROPERTY_DRAWER))
            .and_then(|d| {
                d.descendants()
                    .filter_map(<ast::NodeProperty as ast::AstNode>::cast)
                    .find(|p| p.key().eq_ignore_ascii_case(name))
                    .map(|p| p.value().trim().to_string())
            });
    };
    let category = || {
        keywords
            .iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case("CATEGORY"))
            .map(|(_, v)| v.trim().to_string())
            .or_else(|| {
                file.and_then(|f| f.file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
            })
    };
    match upper.as_str() {
        "TODO" => h.todo_keyword().map(|t| t.text().to_string()),
        "PRIORITY" => Some(h.priority().unwrap_or('B').to_string()),
        "ITEM" => Some(h.raw_value()),
        "TAGS" => {
            let t = h.tags();
            (!t.is_empty()).then(|| format!(":{}:", t.join(":")))
        }
        "FILE" => file.map(|f| f.to_string_lossy().into_owned()),
        "SCHEDULED" => h
            .planning()
            .and_then(|p| p.scheduled())
            .map(|t| ast::AstNode::syntax(&t).text().to_string()),
        "DEADLINE" => h
            .planning()
            .and_then(|p| p.deadline())
            .map(|t| ast::AstNode::syntax(&t).text().to_string()),
        "CLOSED" => h
            .planning()
            .and_then(|p| p.closed())
            .map(|t| ast::AstNode::syntax(&t).text().to_string()),
        _ => {
            let own = h
                .properties()
                .into_iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| v);
            match (own, upper.as_str()) {
                (Some(v), _) => Some(v),
                // The category is inherited from the headlines above, the
                // file's `#+CATEGORY`, or its name.
                (None, "CATEGORY") => ast::AstNode::syntax(&h)
                    .ancestors()
                    .skip(1)
                    .filter_map(<ast::Headline as ast::AstNode>::cast)
                    .find_map(|a| {
                        a.properties()
                            .into_iter()
                            .find(|(k, _)| k.eq_ignore_ascii_case("CATEGORY"))
                            .map(|(_, v)| v)
                    })
                    .or_else(category),
                (None, _) => None,
            }
        }
    }
}

/// What the macro `key` called with `args` expands to (`None` when it is
/// undefined), `property` giving the entry's properties.
fn value_of(
    templates: &HashMap<String, Template>,
    keywords: &[(String, String)],
    counters: &mut HashMap<String, i64>,
    key: &str,
    args: &[String],
    now: &jiff::Zoned,
    property: impl FnOnce(&str, Option<&str>) -> Option<String>,
) -> Option<String> {
    match templates.get(key) {
        Some(Template::Text(t)) => Some(fill(t, args)),
        Some(Template::Keyword) => Some(
            keyword_value(
                keywords,
                args.first().map(String::as_str).unwrap_or(""),
                true,
            )
            .unwrap_or_default(),
        ),
        Some(Template::Counter) => {
            let name = args
                .first()
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let action = args
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
            let name = args.first().cloned().unwrap_or_default();
            Some(property(&name, args.get(1).map(String::as_str)).unwrap_or_default())
        }
        Some(Template::Time) => Some(format_time(
            args.first().map(String::as_str).unwrap_or(""),
            now,
        )),
        Some(Template::ModificationTime(t, file)) => {
            let vc = args
                .get(1)
                .filter(|a| !a.trim().is_empty())
                .and_then(|_| last_commit_time(file));
            let z = vc
                .or(*t)
                .map(|t| t.to_zoned(now.time_zone().clone()))
                .unwrap_or_else(|| now.clone());
            Some(format_time(
                args.first().map(String::as_str).unwrap_or(""),
                &z,
            ))
        }
        Some(Template::Date(raw, ts)) => Some(match args.first() {
            Some(f) if !f.trim().is_empty() => {
                // `org-format-timestamp`: the start, at midnight without a
                // time, in the local time zone.
                let (h, m) = ts.time.unwrap_or((0, 0));
                let n = |x: u32| i8::try_from(x).unwrap_or(i8::MAX);
                i16::try_from(ts.year)
                    .ok()
                    .and_then(|y| {
                        jiff::civil::DateTime::new(y, n(ts.month), n(ts.day), n(h), n(m), 0, 0).ok()
                    })
                    .and_then(|d| d.to_zoned(now.time_zone().clone()).ok())
                    .map_or_else(|| raw.clone(), |z| format_time(f, &z))
            }
            _ => raw.clone(),
        }),
        Some(Template::Eval) => Some(String::new()),
        None => None,
    }
}

/// The timestamp a keyword's value is, when it is that alone.
fn single_timestamp(value: &str) -> Option<ast::DateTime> {
    let p = org_syntax::parse(value.trim());
    let para = p.syntax().descendants().find(|n| n.kind() == PARAGRAPH)?;
    let mut objects = para.children_with_tokens().filter(|c| {
        c.as_token()
            .is_none_or(|t| !t.text().chars().all(char::is_whitespace))
    });
    let ts = objects
        .next()?
        .into_node()
        .filter(|n| n.kind() == TIMESTAMP)?;
    if objects.next().is_some() {
        return None;
    }
    <ast::Timestamp as ast::AstNode>::cast(ts)?.start()
}

/// The time of the last commit of `file` (`org-macro--vc-modified-time`,
/// for Git, the author date `git log` prints).
fn last_commit_time(file: &Path) -> Option<jiff::Timestamp> {
    let dir = file.parent().filter(|d| !d.as_os_str().is_empty())?;
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["log", "-1", "--format=%aI", "--"])
        .arg(file.file_name()?)
        .output()
        .ok()?;
    let s = String::from_utf8(out.stdout).ok()?;
    s.trim().parse().ok()
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
    expand_tracking(text, parsed, file, now, &mut [])
}

/// [`expand`], moving the byte offsets `marks` with the text around them.
pub fn expand_tracking(
    text: &str,
    parsed: &[&str],
    file: Option<&Path>,
    now: &jiff::Zoned,
    marks: &mut [usize],
) -> Result<String, String> {
    if !text.contains("{{{") {
        return Ok(text.to_string());
    }
    let parse = crate::parse_document(text, file);
    // `#+MACRO:` definitions come from setup files too; the values of
    // `{{{title}}}` and `{{{keyword}}}` from the document only
    // (`org-macro--find-keyword-value`).
    let keywords = org_syntax::parse(text).keywords();
    let templates = templates(&parse.keywords(), &keywords, file);
    let mut counters: HashMap<String, i64> = HashMap::new();
    let mut record: HashSet<(usize, String, Vec<String>)> = HashSet::new();
    let mut text = text.to_string();
    let mut pos = 0;
    while let Some(m) = next_macro(&text, pos, parsed) {
        let value = value_of(
            &templates,
            &keywords,
            &mut counters,
            &m.key,
            &m.args,
            now,
            |name, location| property_at(&text, m.start, name, location, file),
        );
        match value {
            Some(v) => {
                let sig = (m.start, m.key.clone(), m.args.clone());
                if !record.insert(sig) {
                    return Err(format!("Circular macro expansion: {}", m.key));
                }
                for o in marks.iter_mut() {
                    if *o >= m.end {
                        *o = *o - (m.end - m.start) + v.len();
                    } else if *o > m.start {
                        *o = m.start + v.len();
                    }
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

/// The `results` macros replaced by their argument, as `org-export-as`
/// does once Babel has run (`org-macro-replace-all` with `("results" .
/// "$1")`): the inline results already in the document are exported.
pub(crate) fn expand_results(text: &str, parsed: &[&str]) -> String {
    if !text.contains("{{{results(") {
        return text.to_string();
    }
    let mut text = text.to_string();
    let mut pos = 0;
    while let Some(m) = next_macro(&text, pos, parsed) {
        if m.key != "results" {
            pos = m.end;
            continue;
        }
        let v = fill("$1", &m.args);
        text.replace_range(m.start..m.end, &v);
        pos = m.start + v.len();
    }
    text
}

/// What each macro of the document `root` expands to, for showing it:
/// its range (without trailing blanks) and its expansion with nested
/// macros expanded, or `None` when it is undefined or does not end.
/// Keyword values are left alone, and definitions come from the document
/// itself, not its setup files.
pub fn expansions(
    root: &SyntaxNode,
    file: Option<&Path>,
    now: &jiff::Zoned,
) -> Vec<(std::ops::Range<usize>, Option<String>)> {
    let mut keywords: Vec<(String, String)> = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(n) = stack.pop() {
        if let Some(k) = <ast::Keyword as ast::AstNode>::cast(n.clone()) {
            keywords.push((k.key(), k.value()));
            continue;
        }
        let children: Vec<SyntaxNode> = n
            .children()
            .filter(|c| c.kind() == KEYWORD || c.kind().is_greater_element())
            .collect();
        stack.extend(children.into_iter().rev());
    }
    let templates = templates(&keywords, &keywords, file);
    let mut counters: HashMap<String, i64> = HashMap::new();
    let mut out = Vec::new();
    for n in root.descendants().filter(|n| n.kind() == MACRO) {
        if in_commented_heading(&n) {
            continue;
        }
        let Some(mac) = <ast::Macro as ast::AstNode>::cast(n.clone()) else {
            continue;
        };
        let start = usize::from(n.text_range().start());
        let end = usize::from(n.text_range().end()) - ast::post_blank(&n);
        let property = |name: &str, location: Option<&str>| {
            property_in(root, &keywords, start, name, location, None)
        };
        let mut v = value_of(
            &templates,
            &keywords,
            &mut counters,
            &mac.key(),
            &mac.args(),
            now,
            property,
        );
        // Macros in the expansion, a few levels deep.
        let mut steps = 0;
        while let Some(t) = v.as_ref()
            && let Some(i) = t.find("{{{")
        {
            steps += 1;
            let inner = parse_macro_at(t, i);
            v = match inner {
                Some(f) if steps <= 20 => value_of(
                    &templates,
                    &keywords,
                    &mut counters,
                    &f.key,
                    &f.args,
                    now,
                    |name, location| property_in(root, &keywords, start, name, location, None),
                )
                .map(|x| format!("{}{x}{}", &t[..f.start], &t[f.end..])),
                _ => None,
            };
        }
        out.push((start..end, v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_as_emacs_gives_it() {
        let text = ":PROPERTIES:\n:FILEP: fp\n:END:\n#+PROPERTY: A top\nTop\n\
                    * TODO [#A] Head :t1:\n:PROPERTIES:\n:X: xval\n:END:\nbody\n\
                    * Other\n:PROPERTIES:\n:X: other\n:CUSTOM_ID: o\n:END:\n";
        let at = |s: &str| text.find(s).unwrap();
        let p = |pos, name, loc| property_at(text, pos, name, loc, Some(Path::new("/d/m1.org")));
        assert_eq!(p(at("Top"), "FILEP", None).as_deref(), Some("fp"));
        assert_eq!(p(at("Top"), "A", None), None);
        assert_eq!(p(at("body"), "TODO", None).as_deref(), Some("TODO"));
        assert_eq!(p(at("body"), "PRIORITY", None).as_deref(), Some("A"));
        assert_eq!(p(at("body"), "ITEM", None).as_deref(), Some("Head"));
        assert_eq!(p(at("body"), "TAGS", None).as_deref(), Some(":t1:"));
        assert_eq!(p(at("body"), "CATEGORY", None).as_deref(), Some("m1"));
        assert_eq!(p(at("body"), "X", Some("Other")).as_deref(), Some("other"));
        assert_eq!(p(at("body"), "X", Some("#o")).as_deref(), Some("other"));
        assert_eq!(p(at("body"), "X", Some("*Head")).as_deref(), Some("xval"));
        assert_eq!(p(at("body"), "X", Some("Nowhere")), None);
        assert_eq!(p(at("Other"), "PRIORITY", None).as_deref(), Some("B"));
    }

    #[test]
    fn results_expand_to_their_argument() {
        assert_eq!(
            expand_results(
                "A {{{results(=16=)}}} and {{{title}}} {{{results(x\\, y)}}}.",
                &[]
            ),
            "A =16= and {{{title}}} x, y."
        );
    }

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

    #[test]
    fn expansions_for_display() {
        let t = "#+TITLE: T\n#+MACRO: two {{{title}}}-{{{n}}}\n#+MACRO: loop {{{loop}}}\n{{{two}}} {{{nope}}} {{{n}}} {{{loop}}}\n* H\n:PROPERTIES:\n:P: v\n:END:\n{{{property(P)}}}\n";
        let root = org_syntax::parse(t).syntax();
        let shown: Vec<_> = expansions(&root, None, &now())
            .into_iter()
            .map(|(r, v)| (&t[r], v))
            .collect();
        assert_eq!(
            shown,
            [
                ("{{{two}}}", Some("T-1".to_string())),
                ("{{{nope}}}", None),
                ("{{{n}}}", Some("2".to_string())),
                ("{{{loop}}}", None),
                ("{{{property(P)}}}", Some("v".to_string())),
            ]
        );
    }
}
