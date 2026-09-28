//! Internal links and names: where `[[target]]`, `[[#custom-id]]`,
//! `[[(coderef)]]`, radio links and `id:` links lead, following
//! `org-link-search`, `org-link--search-radio-target` and
//! `org-find-entry-with-id`.

use org_syntax::ast::{self, AstNode};
use org_syntax::{SyntaxKind, SyntaxNode};

use crate::properties::complex_heading_title;
use crate::{Document, EntryId};

/// A link inside the document and where it leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InternalLink {
    /// The link's start.
    pub begin: usize,
    /// `:type`.
    pub link_type: String,
    /// `:path`.
    pub path: String,
    /// The target position, if the search succeeds.
    pub target: Option<usize>,
}

/// `split-string` with its default separators `[ \f\t\n\r\v]+`.
fn words(s: &str) -> Vec<String> {
    s.split([' ', '\x0c', '\t', '\n', '\r', '\x0b'])
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn fold(s: &str) -> String {
    s.to_lowercase()
}

/// Whether `text` is `w1 SEP w2 SEP ...` exactly, with SEP one or more of
/// `seps`, compared case-insensitively.
fn is_words(text: &str, words: &[String], seps: &[char]) -> bool {
    let mut rest = text;
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            let t = rest.trim_start_matches(seps);
            if t.len() == rest.len() {
                return false;
            }
            rest = t;
        }
        let n = w.chars().count();
        let (head, tail) = match rest.char_indices().nth(n) {
            Some((i, _)) => rest.split_at(i),
            None => (rest, ""),
        };
        if fold(head) != fold(w) || head.chars().count() != n {
            return false;
        }
        rest = tail;
    }
    rest.is_empty()
}

/// `org-link--normalize-string`: statistics cookies become spaces, runs of
/// spaces and tabs one space, and the result is trimmed.
fn normalize(s: &str) -> String {
    let mut out = String::new();
    let b: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < b.len() {
        if b[i] == '[' {
            let mut j = i + 1;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            let cookie_end = if b.get(j) == Some(&'%') && b.get(j + 1) == Some(&']') {
                Some(j + 2)
            } else if b.get(j) == Some(&'/') {
                let mut k = j + 1;
                while k < b.len() && b[k].is_ascii_digit() {
                    k += 1;
                }
                (b.get(k) == Some(&']')).then_some(k + 1)
            } else {
                None
            };
            if let Some(e) = cookie_end {
                out.push(' ');
                i = e;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    let mut collapsed = String::new();
    let mut space = false;
    for c in out.chars() {
        if c == ' ' || c == '\t' {
            if !space {
                collapsed.push(' ');
            }
            space = true;
        } else {
            collapsed.push(c);
            space = false;
        }
    }
    collapsed
        .trim_matches(|c: char| c.is_whitespace())
        .to_string()
}

impl Document {
    fn start(n: &SyntaxNode) -> usize {
        usize::from(n.text_range().start())
    }

    /// `org-link-search`: where a search string leads (`None` when Emacs
    /// would signal "No match").
    pub fn link_search(&self, s: &str) -> Option<usize> {
        if s.trim().is_empty() {
            return None;
        }
        let normalized = normalize_newlines(s);
        let starred = normalized.starts_with('*');
        let words = words(if starred { &s[1..] } else { s });
        let root = self.parse.syntax();
        if let Some(id) = normalized.strip_prefix('#') {
            return self.find_property_value("CUSTOM_ID", id);
        }
        if normalized.starts_with('(') && normalized.ends_with(')') && normalized.len() >= 2 {
            let coderef = &normalized[1..normalized.len() - 1];
            return self.find_coderef(coderef);
        }
        if normalized.len() >= 2 && normalized.starts_with('/') && normalized.ends_with('/') {
            return None;
        }
        if !starred {
            // Dedicated targets `<<words>>`.
            for t in root
                .descendants()
                .filter(|n| n.kind() == SyntaxKind::TARGET)
            {
                let inner = t.text().to_string();
                let inner = inner.trim_end_matches([' ', '\t']);
                if let Some(v) = inner.strip_prefix("<<").and_then(|x| x.strip_suffix(">>"))
                    && is_words(v, &words, &[' ', '\t', '\n'])
                {
                    return Some(Self::start(&t));
                }
            }
            // Elements named after the words.
            for n in root.descendants() {
                if !n.kind().is_element() {
                    continue;
                }
                let name = ast::element_name(&n);
                for k in ast::affiliated_keywords(&n) {
                    if k.key() != "NAME" {
                        continue;
                    }
                    // `^[ \t]*#\+NAME: +WORDS[ \t]*$`
                    let line = k.syntax().text().to_string();
                    let line = line.trim_end_matches(['\n', '\r']);
                    let Some(colon) = line.find(':') else {
                        continue;
                    };
                    let after = &line[colon + 1..];
                    let value = after.trim_start_matches(' ');
                    if value.len() == after.len() || !k.raw_key().eq_ignore_ascii_case("NAME") {
                        continue;
                    }
                    if is_words(value.trim_end_matches([' ', '\t']), &words, &[' ', '\t'])
                        && name.as_ref().is_some_and(|nm| {
                            let a: Vec<String> = self::words(nm).iter().map(|w| fold(w)).collect();
                            let b: Vec<String> = words.iter().map(|w| fold(w)).collect();
                            a == b
                        })
                    {
                        return Some(Self::start(k.syntax()));
                    }
                }
            }
        }
        // Headlines whose title, without TODO keyword, priority, COMMENT,
        // tags and statistics cookies, has the same words.
        let target: Vec<String> = words.iter().map(|w| fold(w)).collect();
        for (i, e) in self.outline().entries.iter().enumerate() {
            let title = complex_heading_title(&e.line, e.todo.as_deref()).unwrap_or_default();
            let title = strip_comment(&title);
            let got: Vec<String> = self::words(&normalize(title))
                .iter()
                .map(|w| fold(w))
                .collect();
            if got == target {
                let _ = i;
                return Some(usize::from(e.range.start()));
            }
        }
        None
    }

    /// `org-find-property` with a value: the first entry whose drawer sets
    /// `prop` to `value` (both case-insensitively).
    fn find_property_value(&self, prop: &str, value: &str) -> Option<usize> {
        let root = self.parse.syntax();
        for p in root.descendants().filter_map(ast::NodeProperty::cast) {
            if p.key().eq_ignore_ascii_case(prop) && fold(&p.value()) == fold(value) {
                // The entry's start, or the document's.
                let start = p
                    .syntax()
                    .ancestors()
                    .find(|a| matches!(a.kind(), SyntaxKind::HEADLINE | SyntaxKind::INLINETASK))
                    .map_or(0, |h| Self::start(&h));
                return Some(start);
            }
        }
        None
    }

    fn find_coderef(&self, coderef: &str) -> Option<usize> {
        let root = self.parse.syntax();
        for n in root.descendants() {
            if !matches!(n.kind(), SyntaxKind::SRC_BLOCK | SyntaxKind::EXAMPLE_BLOCK) {
                continue;
            }
            // `org-src-coderef-format`: `-l "FORMAT"` or `(ref:%s)`.
            let switches = ast::SrcBlock::cast(n.clone())
                .and_then(|b| b.switches())
                .or_else(|| ast::ExampleBlock::cast(n.clone()).and_then(|b| b.switches()));
            // `-l +"\([^"\n]+\)"`
            let format = switches
                .as_deref()
                .and_then(|s| {
                    s.match_indices("-l").find_map(|(i, _)| {
                        let rest = &s[i + 2..];
                        let t = rest.trim_start_matches(' ');
                        if t.len() == rest.len() {
                            return None;
                        }
                        let t = t.strip_prefix('"')?;
                        let end = t.find(['"', '\n'])?;
                        (end > 0 && t[end..].starts_with('"')).then(|| t[..end].to_string())
                    })
                })
                .unwrap_or_else(|| "(ref:%s)".into());
            let label = format.replacen("%s", coderef, 1);
            let text = n.text().to_string();
            let base = Self::start(&n);
            let mut off = 0;
            for line in text.split_inclusive('\n') {
                let l = line.trim_end_matches(['\n', '\r']);
                // The label ends the line (only blanks after it).
                if let Some((pos, _)) = l
                    .match_indices(&label)
                    .find(|(p, _)| l[p + label.len()..].trim_matches([' ', '\t']).is_empty())
                {
                    return Some(base + off + pos);
                }
                off += line.len();
            }
        }
        None
    }

    /// `org-link--search-radio-target`.
    pub fn radio_target(&self, target: &str) -> Option<usize> {
        let w = words(target);
        self.parse
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::RADIO_TARGET)
            .find(|t| {
                let s = t.text().to_string();
                let s = s.trim_end_matches([' ', '\t']);
                s.strip_prefix("<<<")
                    .and_then(|x| x.strip_suffix(">>>"))
                    .is_some_and(|v| is_words(v, &w, &[' ', '\t', '\n']))
            })
            .map(|t| Self::start(&t))
    }

    /// `org-find-entry-with-id`: the first entry whose `ID` is `id`.
    pub fn entry_with_id(&self, id: &str) -> Option<EntryId> {
        self.find_by_id(id)
    }

    /// Links that point inside the document, with their targets.
    pub fn internal_links(&self) -> Vec<InternalLink> {
        let ctx = self.parse.context();
        let root = self.parse.syntax();
        let mut out = Vec::new();
        for l in root.descendants().filter_map(ast::Link::cast) {
            let info = l.info(ctx);
            let target = match info.link_type.as_str() {
                "fuzzy" => self.link_search(&info.path),
                "custom-id" | "coderef" => self.link_search(&info.raw_link),
                "radio" => self.radio_target(&info.path),
                "id" => self
                    .entry_with_id(&info.path)
                    .map(|e| usize::from(self.entry(e).range.start())),
                _ => continue,
            };
            out.push(InternalLink {
                begin: usize::from(l.syntax().text_range().start()),
                link_type: info.link_type,
                path: info.path,
                target,
            });
        }
        out
    }

    /// Elements named with `#+NAME`, with the start of their first `#+NAME`
    /// line.
    pub fn names(&self) -> Vec<(String, usize)> {
        let root = self.parse.syntax();
        root.descendants()
            .filter(|n| n.kind().is_element())
            .filter_map(|n| {
                let name = ast::element_name(&n)?;
                let k = ast::affiliated_keywords(&n).find(|k| k.key() == "NAME")?;
                Some((name, Self::start(k.syntax())))
            })
            .collect()
    }

    /// The element named `name` (`#+NAME`), compared case-insensitively
    /// word by word, like the name step of `org-link-search`.
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        let w: Vec<String> = words(name).iter().map(|x| fold(x)).collect();
        self.names()
            .into_iter()
            .find(|(n, _)| words(n).iter().map(|x| fold(x)).collect::<Vec<_>>() == w)
            .map(|(_, p)| p)
    }
}

/// `(replace-regexp-in-string "\n[ \t]*" " " s)`.
fn normalize_newlines(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\n' {
            while matches!(it.peek(), Some(' ') | Some('\t')) {
                it.next();
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

/// Removes a leading `COMMENT` keyword and the blanks after it.
fn strip_comment(title: &str) -> &str {
    match title.strip_prefix("COMMENT") {
        Some(rest) if rest.starts_with([' ', '\t']) => rest.trim_start_matches([' ', '\t']),
        _ => title,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches() {
        let text = "* TODO [#A] My Heading [1/2] :tag:\n** COMMENT Other one\n<<My Target>> text\n#+NAME: tbl one\n| a |\n* Custom\n:PROPERTIES:\n:CUSTOM_ID: cid\n:ID: 42\n:END:\n#+begin_src sh -n\necho (ref:x)\n#+end_src\n<<<radio thing>>>\n";
        let doc = Document::new(org_syntax::parse(text));
        assert_eq!(doc.link_search("my heading"), Some(0));
        assert_eq!(doc.link_search("*Other one"), Some(35));
        assert_eq!(doc.link_search("my  target"), text.find("<<My"));
        assert_eq!(doc.link_search("TBL one"), text.find("#+NAME"));
        assert_eq!(doc.link_search("#CID"), text.find("* Custom"));
        assert_eq!(doc.link_search("(x)"), text.find("(ref:x)"));
        assert_eq!(doc.link_search("nothing"), None);
        assert_eq!(doc.radio_target("radio   thing"), text.find("<<<radio"));
        assert_eq!(doc.find_by_name("tbl one"), text.find("#+NAME"));
        assert_eq!(normalize("a [1/2]  b [50%]"), "a b");
    }
}
