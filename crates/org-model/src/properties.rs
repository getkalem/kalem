//! Tags and properties, following `org-get-tags`, `org-entry-get`,
//! `org-entry-get-with-inheritance` and `org-entry-properties`.

use org_syntax::ast::{AstNode, Keyword};
use org_syntax::{SyntaxKind, SyntaxNode, TextRange};

use crate::info::update_property_alist;
use crate::outline::{NodeProperty, node_properties};
use crate::{Document, EntryId, Inheritance};

/// `org-special-properties`.
pub const SPECIAL_PROPERTIES: &[&str] = &[
    "ALLTAGS",
    "BLOCKED",
    "CLOCKSUM",
    "CLOCKSUM_T",
    "CLOSED",
    "DEADLINE",
    "FILE",
    "ITEM",
    "PRIORITY",
    "SCHEDULED",
    "TAGS",
    "TIMESTAMP",
    "TIMESTAMP_IA",
    "TODO",
];

/// How [`Document::entry_get`] uses inheritance (its INHERIT argument).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Inherit {
    /// Only the entry itself (`nil`).
    No,
    /// Ancestors, the document and global properties too (`t`).
    Yes,
    /// Like `Yes` for properties selected by
    /// `org-use-property-inheritance`, otherwise like `No` (`selective`).
    Selective,
}

/// The properties of the document node (`org-data`): its top-level
/// property drawer and its category.
#[derive(Debug, Clone, Default)]
pub(crate) struct Top {
    drawer: Vec<(String, String)>,
    props: Vec<NodeProperty>,
}

impl Top {
    /// Whether the top-level drawer (or the document's category) sets
    /// `prop`.
    pub(crate) fn has(&self, prop: &str) -> bool {
        let up = prop.to_uppercase();
        self.props.iter().any(|p| p.key == up)
    }
}

/// A node of the lineage: an entry or the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Node {
    Entry(EntryId),
    Top,
}

/// `org-not-nil`.
fn not_nil(v: Option<String>) -> Option<String> {
    v.filter(|s| s != "nil")
}

fn matches_inheritance(inh: &Inheritance, name: &str, case_fold: bool) -> bool {
    match inh {
        Inheritance::All => true,
        Inheritance::None => false,
        Inheritance::Regex(r) => {
            regex_automata::meta::Regex::new(r).is_ok_and(|re| re.is_match(name))
        }
        Inheritance::List(l) => l.iter().any(|x| {
            if case_fold {
                x.eq_ignore_ascii_case(name)
            } else {
                x == name
            }
        }),
    }
}

/// `org-remove-tabs` with a tab width of 8.
fn remove_tabs(s: &str) -> String {
    let mut out = String::new();
    let mut col = 0;
    for c in s.chars() {
        if c == '\t' {
            let n = 8 - col % 8;
            out.extend(std::iter::repeat_n(' ', n));
            col += n;
        } else {
            out.push(c);
            col += 1;
        }
    }
    out
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%')
}

/// Whether `s` is `[ \t]+:[[:alnum:]_@#%:]+:[ \t]*` (optionally) followed
/// by nothing but spaces and tabs: the end of `org-complex-heading-regexp`.
fn heading_end(s: &str) -> bool {
    let t = s.trim_end_matches([' ', '\t']);
    if t.is_empty() {
        return true;
    }
    let body = t.trim_start_matches([' ', '\t']);
    body.len() < t.len()
        && body.len() >= 2
        && body.starts_with(':')
        && body.ends_with(':')
        && body.chars().all(|c| c == ':' || is_tag_char(c))
}

/// The TODO keyword of a heading line as `org-complex-heading-regexp`
/// captures it (group 2, case-sensitively): a keyword of the document
/// after the stars and spaces, followed by a space, the tags or the end of
/// the line. Unlike the parser's `:todo-keyword`, a tab or trailing
/// spaces after the keyword are enough.
pub fn complex_heading_todo(line: &str, ctx: &org_syntax::ParseContext) -> Option<String> {
    let stars = line.bytes().take_while(|b| *b == b'*').count();
    if stars == 0 || line.as_bytes().get(stars) != Some(&b' ') {
        return None;
    }
    let rest = line[stars..].trim_start_matches(' ');
    ctx.todo_sequences
        .iter()
        .flat_map(|s| s.keywords.iter().map(|k| &k.name))
        .filter(|k| {
            rest.strip_prefix(k.as_str())
                .is_some_and(|r| r.is_empty() || r.starts_with(' ') || heading_end(r))
        })
        .max_by_key(|k| k.len())
        .cloned()
}

/// A heading line split into the groups of `org-complex-heading-regexp`,
/// as byte ranges in the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComplexHeading {
    /// The stars (group 1).
    pub stars: std::ops::Range<usize>,
    /// The TODO keyword (group 2).
    pub todo: Option<std::ops::Range<usize>>,
    /// The priority cookie, such as `[#A]` (group 3).
    pub priority: Option<std::ops::Range<usize>>,
    /// The title (group 4), `COMMENT` included.
    pub title: Option<std::ops::Range<usize>>,
    /// The tags with their colons (group 5).
    pub tags: Option<std::ops::Range<usize>>,
}

/// Splits a heading line (without its line ending) as
/// `org-complex-heading-regexp` does, matching TODO keywords
/// case-sensitively; `None` if it is not a heading line.
pub fn complex_heading(line: &str, ctx: &org_syntax::ParseContext) -> Option<ComplexHeading> {
    let stars = line.bytes().take_while(|b| *b == b'*').count();
    if stars == 0 {
        return None;
    }
    let todo_name = complex_heading_todo(line, ctx);
    let todo = todo_name.as_ref().map(|k| {
        let s = stars + line[stars..].len() - line[stars..].trim_start_matches(' ').len();
        s..s + k.len()
    });
    // Priority choices after the keyword (or stars): with a cookie first.
    let after_kw = todo.as_ref().map_or(stars, |r| r.end);
    let priority = {
        let r = &line[after_kw..];
        let t = r.trim_start_matches(' ');
        let b = t.as_bytes();
        (t.len() < r.len() && b.len() >= 4 && b[0] == b'[' && b[1] == b'#')
            .then(|| {
                let c = t[2..].chars().next()?;
                let after = &t[2 + c.len_utf8()..];
                let rest = after.strip_prefix(']')?;
                (rest.is_empty() || rest.starts_with(' ') || heading_end(rest)).then(|| {
                    let s = after_kw + (r.len() - t.len());
                    s..s + 3 + c.len_utf8()
                })
            })
            .flatten()
    };
    let after = priority.as_ref().map_or(after_kw, |r| r.end);
    let r = &line[after..];
    let (title, tail) = if heading_end(r) {
        (None, after)
    } else {
        let t = r.trim_start_matches(' ');
        let s = after + (r.len() - t.len());
        let i = t
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(t.len()))
            .find(|&i| heading_end(&t[i..]))
            .unwrap_or(t.len());
        (Some(s..s + i), s + i)
    };
    let rest = &line[tail..];
    let body = rest.trim_matches([' ', '\t']);
    let tags = (!body.is_empty()).then(|| {
        let s = tail + (rest.len() - rest.trim_start_matches([' ', '\t']).len());
        s..s + body.len()
    });
    Some(ComplexHeading {
        stars: 0..stars,
        todo,
        priority,
        title,
        tags,
    })
}

/// The title of a heading line as `org-complex-heading-regexp` captures
/// it (group 4), given the line's TODO keyword (see
/// [`complex_heading_todo`]); `None` if `line` is not a heading line.
pub fn complex_heading_title(line: &str, todo: Option<&str>) -> Option<String> {
    let rest = line.trim_start_matches('*');
    if rest.len() == line.len() {
        return None;
    }
    // Keyword and priority choices, preferred first.
    let with_kw: Option<&str> = todo.and_then(|kw| {
        let t = rest.trim_start_matches(' ');
        (t.len() < rest.len()).then_some(())?;
        t.strip_prefix(kw)
    });
    let kw_choices: Vec<&str> = with_kw.into_iter().chain(std::iter::once(rest)).collect();
    for r in kw_choices {
        let with_pri = {
            let t = r.trim_start_matches(' ');
            let b = t.as_bytes();
            (t.len() < r.len() && b.len() >= 4 && b[0] == b'[' && b[1] == b'#')
                .then(|| {
                    let c = t[2..].chars().next()?;
                    let after = &t[2 + c.len_utf8()..];
                    after.strip_prefix(']')
                })
                .flatten()
        };
        for r in with_pri.into_iter().chain(std::iter::once(r)) {
            // Title skipped (lazy optional group).
            if heading_end(r) {
                return Some(String::new());
            }
            // Title present: ` +` then the shortest text.
            let t = r.trim_start_matches(' ');
            if t.len() == r.len() {
                continue;
            }
            for (i, _) in t.char_indices().chain(std::iter::once((t.len(), ' '))) {
                if heading_end(&t[i..]) {
                    return Some(t[..i].to_string());
                }
            }
        }
    }
    None
}

/// `org-ts-regexp-both` at the start of `s`: `[[<]DATE(?: .*?)?[]>]`.
fn ts_at(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    if b.len() < 12 || !matches!(b[0], b'<' | b'[') {
        return None;
    }
    let date = &b[1..11];
    let ok = date.iter().enumerate().all(|(i, c)| match i {
        4 | 7 => *c == b'-',
        _ => c.is_ascii_digit(),
    });
    if !ok {
        return None;
    }
    match b[11] {
        b']' | b'>' => Some(&s[..12]),
        b' ' => {
            let rest = &s[12..];
            let end = rest.find([']', '>', '\n'])?;
            (rest.as_bytes()[end] != b'\n').then(|| &s[..12 + end + 1])
        }
        _ => None,
    }
}

impl Document {
    pub(crate) fn top_props(&self) -> &Top {
        self.top.get_or_init(|| {
            // A property drawer at the start of the document, after blank
            // lines and comments only.
            let root = self.parse.syntax();
            let drawer = root
                .children()
                .find(|n| n.kind() == SyntaxKind::SECTION)
                .filter(|s| {
                    s.parent().is_some_and(|p| p.kind() == SyntaxKind::DOCUMENT)
                        && root
                            .children()
                            .next()
                            .is_some_and(|f| f.kind() == SyntaxKind::SECTION)
                })
                .and_then(|s| s.children().find(|c| c.kind() != SyntaxKind::COMMENT))
                .filter(|n| n.kind() == SyntaxKind::PROPERTY_DRAWER)
                .and_then(org_syntax::ast::PropertyDrawer::cast)
                .map(|d| {
                    d.properties()
                        .map(|p| (p.key(), p.value()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let mut props = node_properties(&drawer);
            if !props.iter().any(|p| p.key == "CATEGORY") {
                // `org-element--get-category`: the last `#+CATEGORY` in the
                // document, else `org-category`, else the file name.
                let last = root
                    .descendants()
                    .filter_map(Keyword::cast)
                    .filter(|k| k.key() == "CATEGORY")
                    .last()
                    .map(|k| k.value());
                let cat = last.or_else(|| self.info().category.clone()).or_else(|| {
                    self.file_name.as_ref().map(|f| {
                        let base = f.rsplit(['/', '\\']).next().unwrap_or(f);
                        base.rsplit_once('.').map_or(base, |(a, _)| a).to_string()
                    })
                });
                if let Some(c) = cat {
                    props.push(NodeProperty {
                        key: "CATEGORY".into(),
                        values: vec![c],
                    });
                }
            }
            Top { drawer, props }
        })
    }

    fn node_props(&self, n: Node) -> &[NodeProperty] {
        match n {
            Node::Entry(id) => &self.entry(id).node_properties,
            Node::Top => &self.top_props().props,
        }
    }

    fn lineage(&self, start: Option<EntryId>) -> impl Iterator<Item = Node> + '_ {
        let entries = start.into_iter().flat_map(move |id| {
            std::iter::once(id)
                .chain(self.outline().ancestors(id))
                .map(Node::Entry)
        });
        entries.chain(std::iter::once(Node::Top))
    }

    /// `org-remove-uninherited-tags` for one tag.
    fn tag_inherited(&self, tag: &str) -> bool {
        let s = &self.settings;
        match &s.tag_inheritance {
            Inheritance::All => !s.tags_exclude_from_inheritance.iter().any(|t| t == tag),
            Inheritance::None => false,
            Inheritance::Regex(_) => {
                matches_inheritance(&s.tag_inheritance, tag, false)
                    && !s.tags_exclude_from_inheritance.iter().any(|t| t == tag)
            }
            Inheritance::List(l) => l.iter().any(|t| t == tag),
        }
    }

    /// `org-get-tags` with inheritance: file tags, inherited tags and local
    /// tags, each tag once, at its most local place.
    pub fn tags(&self, id: EntryId) -> Vec<String> {
        let local = &self.entry(id).local_tags;
        if self.settings.tag_inheritance == Inheritance::None {
            return local.clone();
        }
        let mut ancestors: Vec<EntryId> = self.outline().ancestors(id).collect();
        ancestors.reverse();
        let inherited = self
            .info()
            .file_tags
            .iter()
            .chain(
                ancestors
                    .iter()
                    .flat_map(|a| self.entry(*a).local_tags.iter()),
            )
            .filter(|t| self.tag_inherited(t));
        let all: Vec<&String> = inherited.chain(local.iter()).collect();
        let mut out: Vec<String> = Vec::new();
        for (i, t) in all.iter().enumerate() {
            if !all[i + 1..].contains(t) {
                out.push((*t).clone());
            }
        }
        out
    }

    /// `org-property-inherit-p`.
    pub fn property_inherited(&self, name: &str) -> bool {
        matches_inheritance(&self.settings.property_inheritance, name, true)
    }

    /// `org--property-local-values`: the base value and the `+` values.
    fn local_values(
        &self,
        n: Node,
        prop: &str,
        literal_nil: bool,
    ) -> Option<(Option<String>, Vec<String>)> {
        let up = prop.to_uppercase();
        let props = self.node_props(n);
        let base = props
            .iter()
            .find(|p| p.key == up)
            .and_then(|p| p.values.last().cloned());
        let plus = format!("{up}+");
        let extra: Vec<String> = props
            .iter()
            .find(|p| p.key == plus)
            .map(|p| p.values.clone())
            .unwrap_or_default();
        let base = if literal_nil { base } else { not_nil(base) };
        (base.is_some() || !extra.is_empty()).then_some((base, extra))
    }

    /// `org--property-global-or-keyword-value`.
    fn global_value(&self, prop: &str, literal_nil: bool) -> Option<String> {
        let find = |list: &[(String, String)]| {
            list.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(prop))
                .map(|(_, v)| v.clone())
        };
        let v = find(&self.info().keyword_properties)
            .or_else(|| find(&self.settings.global_properties))
            .or_else(|| find(&self.settings.global_properties_fixed));
        if literal_nil { v } else { not_nil(v) }
    }

    /// `org-entry-get-with-inheritance` from `start` (`None` for the
    /// document before its first headline).
    pub fn entry_get_with_inheritance(
        &self,
        start: Option<EntryId>,
        prop: &str,
        literal_nil: bool,
    ) -> Option<String> {
        let mut values: Vec<String> = Vec::new();
        let mut found = false;
        for n in self.lineage(start) {
            if let Some((val, extra)) = self.local_values(n, prop, true) {
                let mut v: Vec<String> = Vec::new();
                let has_base = val.is_some();
                v.extend(val);
                v.extend(extra);
                v.append(&mut values);
                values = v;
                if has_base {
                    found = true;
                    break;
                }
            }
        }
        if !found && let Some(g) = self.global_value(prop, true) {
            values.insert(0, g);
        }
        if values.is_empty() {
            return None;
        }
        let joined = values.join(" ");
        if literal_nil {
            Some(joined)
        } else {
            not_nil(Some(joined))
        }
    }

    /// `org-get-category`.
    pub fn category(&self, id: Option<EntryId>) -> String {
        self.entry_get_with_inheritance(id, "CATEGORY", false)
            .unwrap_or_else(|| "???".into())
    }

    /// `org-entry-get`: the value of `prop` at `id` (`None` for the
    /// document before its first headline).
    pub fn entry_get(
        &self,
        id: Option<EntryId>,
        prop: &str,
        inherit: Inherit,
        literal_nil: bool,
    ) -> Option<String> {
        if prop.eq_ignore_ascii_case("CATEGORY")
            || SPECIAL_PROPERTIES
                .iter()
                .any(|s| s.eq_ignore_ascii_case(prop))
        {
            let up = prop.to_uppercase();
            let props = if up == "CATEGORY" {
                self.standard_properties(id)
            } else {
                self.special_properties(id)
            };
            return props
                .into_iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(prop))
                .map(|(_, v)| v);
        }
        let inherit = match inherit {
            Inherit::No => false,
            Inherit::Yes => true,
            Inherit::Selective => self.property_inherited(prop),
        };
        if inherit {
            return self.entry_get_with_inheritance(id, prop, literal_nil);
        }
        let node = id.map_or(Node::Top, Node::Entry);
        let (base, extra) = self.local_values(node, prop, literal_nil)?;
        let joined = base.into_iter().chain(extra).collect::<Vec<_>>().join(" ");
        if literal_nil {
            Some(joined)
        } else {
            not_nil(Some(joined))
        }
    }

    /// `(org-entry-properties POM 'standard)`: drawer properties (the first
    /// of duplicate keys, with `KEY+` values appended) and the category.
    pub fn standard_properties(&self, id: Option<EntryId>) -> Vec<(String, String)> {
        let drawer = match id {
            Some(id) => &self.entry(id).drawer,
            None => &self.top_props().drawer,
        };
        let mut props: Vec<(String, String)> = Vec::new();
        let mut seen_base: Vec<String> = Vec::new();
        for (k, v) in drawer {
            let key = k.to_uppercase();
            let extend = key.ends_with('+');
            let base = key.trim_end_matches('+');
            if SPECIAL_PROPERTIES
                .iter()
                .any(|s| s.eq_ignore_ascii_case(base))
            {
                continue;
            }
            if extend {
                update_property_alist(&key, v, &mut props);
            } else if seen_base.contains(&key) {
                continue;
            } else {
                seen_base.push(key.clone());
                match props.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case(&key)) {
                    Some(p) => p.1 = format!("{} {}", v, p.1),
                    None => props.insert(0, (key, v.clone())),
                }
            }
        }
        if !props.iter().any(|(k, _)| k == "CATEGORY") {
            props.insert(0, ("CATEGORY".into(), self.category(id)));
        }
        props
    }

    /// `(org-entry-properties POM 'special)` without `FILE`, `CLOCKSUM`
    /// and `CLOCKSUM_T`.
    pub fn special_properties(&self, id: Option<EntryId>) -> Vec<(String, String)> {
        let mut props: Vec<(String, String)> = Vec::new();
        let Some(id) = id else {
            props.push(("CATEGORY".into(), self.category(None)));
            return props;
        };
        let e = self.entry(id);
        if let Some(t) = complex_heading_title(&e.line, e.todo.as_deref()) {
            let t = remove_tabs(&t);
            let t = if t.trim().is_empty() {
                String::new()
            } else {
                t
            };
            props.push(("ITEM".into(), t));
        }
        if let Some(k) = &e.todo {
            props.push(("TODO".into(), k.clone()));
        }
        // `.*?\(\[#\([A-Z0-9]+\)\] ?\)` from the start of the line,
        // case-folded: the first cookie in the line, even inside the title.
        let pri = e.line.match_indices("[#").find_map(|(i, _)| {
            let rest = &e.line[i + 2..];
            let n = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
            (n > 0 && rest[n..].starts_with(']')).then(|| rest[..n].to_string())
        });
        let pri = pri.unwrap_or_else(|| {
            char::from_u32(self.info().priorities.default)
                .unwrap_or('B')
                .to_string()
        });
        props.push(("PRIORITY".into(), pri));
        let tag_string = |t: &[String]| format!(":{}:", t.join(":"));
        if !e.local_tags.is_empty() {
            props.push(("TAGS".into(), tag_string(&e.local_tags)));
        }
        let all = self.tags(id);
        if !all.is_empty() {
            props.push(("ALLTAGS".into(), tag_string(&all)));
        }
        props.push(("BLOCKED".into(), String::new()));
        // Planning keywords on the line after the headline.
        if let Some(next) = &e.next_line {
            let t = next.trim_start_matches([' ', '\t']).to_ascii_uppercase();
            if ["CLOSED:", "DEADLINE:", "SCHEDULED:"]
                .iter()
                .any(|k| t.starts_with(k))
            {
                let upper = next.to_ascii_uppercase();
                for key in ["CLOSED", "DEADLINE", "SCHEDULED"] {
                    let pat = format!("{key}:");
                    if let Some(i) = upper.rfind(&pat) {
                        let after = next[i + pat.len()..].trim_start_matches([' ', '\t']);
                        if let Some(ts) = ts_at(after) {
                            props.push((key.into(), ts.to_string()));
                        }
                    }
                }
            }
        }
        // The first active and inactive timestamps of the headline line,
        // then of the section (without its planning line).
        let (mut active, mut inactive) = (None, None);
        for ts in self.entry_timestamps(e.range) {
            let (kind, raw) = ts;
            match kind {
                true if active.is_none() => active = Some(raw),
                false if inactive.is_none() => inactive = Some(raw),
                _ => {}
            }
        }
        if let Some(a) = active {
            props.push(("TIMESTAMP".into(), a));
        }
        if let Some(i) = inactive {
            props.push(("TIMESTAMP_IA".into(), i));
        }
        props.push(("CATEGORY".into(), self.category(Some(id))));
        props
    }

    /// Timestamps (active or not, raw value) in the headline's title and
    /// its section, outside the planning line, in document order.
    fn entry_timestamps(&self, range: TextRange) -> Vec<(bool, String)> {
        use org_syntax::ast::{Timestamp, TimestampType};
        let root = self.parse.syntax();
        let Some(node) = root
            .covering_element(range)
            .into_node()
            .and_then(|n| n.ancestors().find(|a| a.text_range() == range))
        else {
            return Vec::new();
        };
        let mut scopes: Vec<SyntaxNode> = Vec::new();
        scopes.extend(
            node.children()
                .filter(|c| c.kind() == SyntaxKind::HEADLINE_TITLE),
        );
        scopes.extend(node.children().filter(|c| c.kind() == SyntaxKind::SECTION));
        let mut out = Vec::new();
        for s in scopes {
            for d in s.descendants() {
                // `org-element-context` does not enter planning lines and
                // clocks: their timestamps are not objects there.
                if d.ancestors()
                    .any(|a| matches!(a.kind(), SyntaxKind::PLANNING | SyntaxKind::CLOCK))
                {
                    continue;
                }
                if let Some(t) = Timestamp::cast(d) {
                    match t.timestamp_type() {
                        TimestampType::Active | TimestampType::ActiveRange => {
                            out.push((true, t.raw_value()))
                        }
                        TimestampType::Inactive | TimestampType::InactiveRange => {
                            out.push((false, t.raw_value()))
                        }
                        _ => {}
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_titles() {
        assert_eq!(
            complex_heading_title("* TODO [#A] Top task :work:", Some("TODO")).as_deref(),
            Some("Top task")
        );
        assert_eq!(
            complex_heading_title("** COMMENT Commented child :work:", None).as_deref(),
            Some("COMMENT Commented child")
        );
        assert_eq!(
            complex_heading_title("* TODO", Some("TODO")).as_deref(),
            Some("")
        );
        assert_eq!(
            complex_heading_title("* [#A]title", None).as_deref(),
            Some("[#A]title")
        );
        assert_eq!(
            complex_heading_title("* a :b: c :d:", None).as_deref(),
            Some("a :b: c")
        );
    }

    #[test]
    fn timestamps_in_planning_lines() {
        assert_eq!(
            ts_at("<2026-10-05 Mon -2d> x"),
            Some("<2026-10-05 Mon -2d>")
        );
        assert_eq!(
            ts_at("[2026-09-20 Sun 10:00]"),
            Some("[2026-09-20 Sun 10:00]")
        );
        assert_eq!(ts_at("<2026-10-05>"), Some("<2026-10-05>"));
        assert_eq!(ts_at("<2026-10-5>"), None);
    }
}

#[cfg(test)]
mod complex_heading_tests {
    use super::*;

    fn split(line: &str) -> Vec<Option<&str>> {
        let ctx = org_syntax::parse("").context().clone();
        let h = complex_heading(line, &ctx).unwrap();
        [Some(h.stars), h.todo, h.priority, h.title, h.tags]
            .into_iter()
            .map(|r| r.map(|r| &line[r]))
            .collect()
    }

    #[test]
    fn groups_follow_the_regexp() {
        assert_eq!(
            split("** TODO [#A] Title  :a:b:"),
            [
                Some("**"),
                Some("TODO"),
                Some("[#A]"),
                Some("Title"),
                Some(":a:b:")
            ]
        );
        assert_eq!(
            split("* TODOS x"),
            [Some("*"), None, None, Some("TODOS x"), None]
        );
        assert_eq!(split("* TODO"), [Some("*"), Some("TODO"), None, None, None]);
        assert_eq!(
            split("* [#A]x"),
            [Some("*"), None, None, Some("[#A]x"), None]
        );
        assert_eq!(
            split("* a :b: c :d:"),
            [Some("*"), None, None, Some("a :b: c"), Some(":d:")]
        );
        assert_eq!(
            split("* DONE\t:t:"),
            [Some("*"), Some("DONE"), None, None, Some(":t:")]
        );
        // Each group agrees with the title-only function.
        for line in [
            "* TODO [#A] Title  :a:b:",
            "* COMMENT x",
            "*  spaced   ",
            "* [#B] :t:",
        ] {
            let ctx = org_syntax::parse("").context().clone();
            let h = complex_heading(line, &ctx).unwrap();
            let todo = h.todo.clone().map(|r| &line[r]);
            assert_eq!(
                h.title.map(|r| line[r].to_string()).unwrap_or_default(),
                complex_heading_title(line, todo).unwrap(),
                "{line}"
            );
        }
    }
}
