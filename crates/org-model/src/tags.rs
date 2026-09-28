//! The tag table of `#+TAGS` (`org-tag-string-to-alist`): tags with their
//! fast selection keys, mutually exclusive groups `{ ... }`, and group tags
//! `{ Group : a b }` or `[ Group : a b ]`, which match strings expand to
//! their members (`org-tags-expand`).

use crate::Document;

/// An element of a tag table, as in `org-tag-alist`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagToken {
    /// A tag, or a regular expression `{...}` in a group, with its fast
    /// selection key.
    Tag {
        /// The tag.
        name: String,
        /// The key after it in parentheses.
        key: Option<char>,
    },
    /// `{`: the start of a mutually exclusive group.
    StartGroup,
    /// `}`
    EndGroup,
    /// `[`: the start of a group tag definition that is not exclusive.
    StartGroupTag,
    /// `]`
    EndGroupTag,
    /// `:` between a group tag and its members.
    GroupTags,
    /// A line break in the table.
    Newline,
}

/// A tag table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagTable {
    /// The elements in order.
    pub tokens: Vec<TagToken>,
}

/// `org-tag-re`.
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '@' | '#' | '%')
}

impl TagTable {
    /// Parses the value of `#+TAGS` keywords joined by line feeds.
    pub fn parse(s: &str) -> TagTable {
        let mut tokens: Vec<TagToken> = Vec::new();
        let mut group_flag = false;
        for line in s.split('\n').filter(|l| !l.is_empty()) {
            tokens.push(TagToken::Newline);
            let words: Vec<&str> = line.split_whitespace().collect();
            for (i, w) in words.iter().enumerate() {
                let next_is_colon = words.get(i + 2) == Some(&":");
                match *w {
                    "{" => {
                        tokens.push(TagToken::StartGroup);
                        group_flag |= next_is_colon;
                    }
                    "}" => {
                        tokens.push(TagToken::EndGroup);
                        group_flag = false;
                    }
                    "[" => {
                        tokens.push(TagToken::StartGroupTag);
                        group_flag |= next_is_colon;
                    }
                    "]" => {
                        tokens.push(TagToken::EndGroupTag);
                        group_flag = false;
                    }
                    ":" => tokens.push(TagToken::GroupTags),
                    w => {
                        let Some((name, key)) = split_tag(w) else {
                            continue;
                        };
                        let known = tokens
                            .iter()
                            .any(|t| matches!(t, TagToken::Tag { name: n, .. } if *n == name));
                        if group_flag || !known {
                            tokens.push(TagToken::Tag { name, key });
                        }
                    }
                }
            }
        }
        if !tokens.is_empty() {
            tokens.remove(0);
        }
        TagTable { tokens }
    }

    /// The tags, in order.
    pub fn tags(&self) -> impl Iterator<Item = (&str, Option<char>)> {
        self.tokens.iter().filter_map(|t| match t {
            TagToken::Tag { name, key } => Some((name.as_str(), *key)),
            _ => None,
        })
    }

    /// Group tags and their members (`org-tag-alist-to-groups`).
    pub fn groups(&self) -> Vec<(String, Vec<String>)> {
        let mut groups = Vec::new();
        let mut status = 0; // 0 outside, 1 in a group, 2 after `:`
        let mut current: Vec<String> = Vec::new();
        for t in &self.tokens {
            match t {
                TagToken::StartGroup | TagToken::StartGroupTag => status = 1,
                TagToken::EndGroup | TagToken::EndGroupTag => {
                    if status == 2 && !current.is_empty() {
                        let head = current.remove(0);
                        groups.push((head, std::mem::take(&mut current)));
                    }
                    status = 0;
                    current.clear();
                }
                TagToken::GroupTags => status = 2,
                TagToken::Tag { name, .. } if status == 2 => current.push(name.clone()),
                TagToken::Tag { name, .. } if status == 1 => current = vec![name.clone()],
                _ => {}
            }
        }
        groups
    }

    /// The mutually exclusive groups: the tags between `{` and `}`.
    pub fn exclusive_groups(&self) -> Vec<Vec<String>> {
        let mut out = Vec::new();
        let mut current: Option<Vec<String>> = None;
        for t in &self.tokens {
            match t {
                TagToken::StartGroup => current = Some(Vec::new()),
                TagToken::EndGroup => {
                    if let Some(g) = current.take() {
                        out.push(g);
                    }
                }
                TagToken::Tag { name, .. } => {
                    if let Some(g) = &mut current {
                        g.push(name.clone());
                    }
                }
                _ => {}
            }
        }
        out
    }
}

/// `\`\(TAG\|{.+?}\)\(?:(\(.\))\)?\'`
fn split_tag(w: &str) -> Option<(String, Option<char>)> {
    let (body, key) = match w.strip_suffix(')') {
        Some(r) => match r.rfind('(') {
            Some(i) if r[i + 1..].chars().count() == 1 => (&r[..i], r[i + 1..].chars().next()),
            _ => (w, None),
        },
        None => (w, None),
    };
    let tag = !body.is_empty() && body.chars().all(is_tag_char);
    let regexp = body.len() >= 3 && body.starts_with('{') && body.ends_with('}');
    (tag || regexp).then(|| (body.to_string(), key))
}

/// A word character of `org-mode-tags-syntax-table`.
fn is_word(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '@' | '_')
}

/// `org--tags-expand-group`.
fn expand_group(group: &[String], groups: &[(String, Vec<String>)], expanded: &mut Vec<String>) {
    for tag in group {
        if expanded.contains(tag) {
            continue;
        }
        expanded.push(tag.clone());
        if let Some((_, members)) = groups.iter().find(|(g, _)| g == tag) {
            expand_group(members, groups, expanded);
        }
    }
}

fn regexp_quote(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if matches!(c, '[' | '*' | '.' | '\\' | '?' | '+' | '^' | '$') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `org-tags-expand`: every group tag of `groups` in `input` (a whole
/// word, found case-insensitively outside `{...}` regular expressions)
/// becomes a regular expression matching the group and its members.
pub fn expand_group_tags(input: &str, groups: &[(String, Vec<String>)]) -> String {
    if groups.is_empty() {
        return input.to_string();
    }
    // Regular expressions `{.+?}` are left alone.
    let mut protected = vec![false; input.len()];
    let mut s = 0;
    while let Some(i) = input[s..].find('{') {
        let start = s + i;
        let Some(close) = input[start + 1..]
            .char_indices()
            .skip(1)
            .find(|(_, c)| *c == '}')
            .map(|(j, _)| start + 1 + j)
        else {
            break;
        };
        protected[start..=close].iter_mut().for_each(|p| *p = true);
        s = close + 1;
    }
    let mut out = String::new();
    let mut i = 0;
    let chars: Vec<(usize, char)> = input.char_indices().collect();
    let mut k = 0;
    while k < chars.len() {
        let (pos, c) = chars[k];
        let word_start = is_word(c) && (k == 0 || !is_word(chars[k - 1].1));
        if word_start && !protected[pos] {
            let mut e = k;
            while e < chars.len() && is_word(chars[e].1) {
                e += 1;
            }
            let end = chars.get(e).map_or(input.len(), |(p, _)| *p);
            let word = &input[pos..end];
            if groups
                .iter()
                .any(|(g, _)| g.to_lowercase() == word.to_lowercase())
            {
                // The operator before the word goes with it.
                let op_start = if pos > i && matches!(input.as_bytes()[pos - 1], b'+' | b'-') {
                    pos - 1
                } else {
                    pos
                };
                out.push_str(&input[i..op_start]);
                out.push_str(&input[op_start..pos]);
                let mut expanded = Vec::new();
                expand_group(&[word.to_string()], groups, &mut expanded);
                let (mut regexps, mut regular) = (Vec::new(), Vec::new());
                for t in &expanded {
                    match t.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
                        Some(r) => regexps.push(r.to_string()),
                        None => regular.push(regexp_quote(t)),
                    }
                }
                // `regexp-opt` sorts; the order does not change matches.
                regular.sort();
                let regexp = regexps.join("\\|");
                out.push_str(&if regular.is_empty() {
                    format!("{{{regexp}}}")
                } else {
                    let regular = format!("\\(?:{}\\)", regular.join("\\|"));
                    if regexps.is_empty() {
                        format!("{{\\<{regular}\\>}}")
                    } else {
                        format!("{{\\<{regular}\\>\\|{regexp}}}")
                    }
                });
                i = end;
            }
            k = e;
            continue;
        }
        k += 1;
    }
    out.push_str(&input[i..]);
    out
}

impl Document {
    /// The tag table: `#+TAGS` of the document, or the settings' table.
    pub fn tag_table(&self) -> TagTable {
        let values: Vec<&str> = self
            .info()
            .keywords
            .iter()
            .filter(|(k, _)| k == "TAGS")
            .map(|(_, v)| v.as_str())
            .collect();
        if values.is_empty() {
            TagTable::parse(&self.settings().tag_alist)
        } else {
            TagTable::parse(&values.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        let t = TagTable::parse(
            "@work(w) @home(h) laptop\n{ @office @road } [ Work : Lab Conf ] { Proj : {P@.+} }",
        );
        assert_eq!(
            t.tags().map(|(n, _)| n).collect::<Vec<_>>(),
            [
                "@work", "@home", "laptop", "@office", "@road", "Work", "Lab", "Conf", "Proj",
                "{P@.+}"
            ]
        );
        assert_eq!(t.tags().next(), Some(("@work", Some('w'))));
        assert_eq!(
            t.groups(),
            vec![
                ("Work".into(), vec!["Lab".into(), "Conf".into()]),
                ("Proj".into(), vec!["{P@.+}".into()])
            ]
        );
        assert_eq!(
            t.exclusive_groups(),
            vec![
                vec!["@office".to_string(), "@road".into()],
                vec!["Proj".into(), "{P@.+}".into()]
            ]
        );
    }

    #[test]
    fn expansion() {
        let groups =
            TagTable::parse("[ Work : Lab Conf ] { Proj : {P@.+} } [ All : Work Proj ]").groups();
        assert_eq!(
            expand_group_tags("Work+x", &groups),
            "{\\<\\(?:Conf\\|Lab\\|Work\\)\\>}+x"
        );
        assert_eq!(
            expand_group_tags("-Proj", &groups),
            "-{\\<\\(?:Proj\\)\\>\\|P@.+}"
        );
        assert_eq!(expand_group_tags("{Work}|Home", &groups), "{Work}|Home");
        assert_eq!(expand_group_tags("work", &groups), "{\\<\\(?:work\\)\\>}");
        assert!(expand_group_tags("All", &groups).contains("Lab"));
    }
}
