//! Radio link matching: the search `org-target-link-regexp` performs, done
//! with a trie of case-folded targets instead of one large regexp, so that
//! documents with thousands of radio targets stay fast (§3.6).
//!
//! Emacs builds `\(?:^\|[^[:alnum:]]\|\c|\)\(T1\|T2\|...\)\(?:$\|[^[:alnum:]]\|\c|\)`
//! with targets sorted longest first, every run of spaces in a target
//! turned into `\s-+`, and matches case-insensitively. At a given start the
//! first alternative whose trailing boundary also matches wins; the matcher
//! keeps that rule by remembering each target's position in the list.

use crate::tables;

#[derive(Debug, Clone, Default)]
struct Node {
    /// Edges on case-folded characters, sorted by character.
    edges: Vec<(char, u32)>,
    /// Edge for a run of whitespace (a run of spaces in the target).
    space: Option<u32>,
    /// Position in the alternation of the first target ending here.
    order: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct RadioMatcher {
    nodes: Vec<Node>,
}

/// Emacs's case canonicalization for `case-fold-search`.
fn fold(c: char) -> char {
    tables::canon(c)
}

impl Node {
    fn edge(&self, c: char) -> Option<u32> {
        self.edges
            .binary_search_by_key(&c, |e| e.0)
            .ok()
            .map(|i| self.edges[i].1)
    }
}

impl RadioMatcher {
    /// Builds the matcher from the targets in document order.
    pub(crate) fn new(targets: &[String]) -> Self {
        let mut sorted: Vec<&String> = targets.iter().collect();
        // `org-update-radio-target-regexp` sorts longest first (stable).
        sorted.sort_by_key(|t| std::cmp::Reverse(t.len()));
        let mut m = RadioMatcher {
            nodes: vec![Node::default()],
        };
        for (order, t) in sorted.iter().enumerate() {
            let mut node = 0usize;
            let mut chars = t.chars().peekable();
            while let Some(c) = chars.next() {
                if c == ' ' {
                    while chars.peek() == Some(&' ') {
                        chars.next();
                    }
                    node = match m.nodes[node].space {
                        Some(n) => n as usize,
                        None => {
                            let n = m.nodes.len();
                            m.nodes.push(Node::default());
                            m.nodes[node].space = Some(n as u32);
                            n
                        }
                    };
                    continue;
                }
                let f = fold(c);
                node = match m.nodes[node].edges.binary_search_by_key(&f, |e| e.0) {
                    Ok(i) => m.nodes[node].edges[i].1 as usize,
                    Err(i) => {
                        let n = m.nodes.len();
                        m.nodes.push(Node::default());
                        m.nodes[node].edges.insert(i, (f, n as u32));
                        n
                    }
                };
            }
            if node != 0 {
                let o = &mut m.nodes[node].order;
                if o.is_none() {
                    *o = Some(order as u32);
                }
            }
        }
        m
    }

    /// The trailing group `\(?:$\|[^[:alnum:]]\|\c|\)` at `e`: the end of
    /// the whole match, if it matches.
    fn trailing(s: &str, e: usize, zv: usize) -> Option<usize> {
        let Some(c) = s[e..zv].chars().next() else {
            return Some(e);
        };
        if c == '\n' {
            return Some(e);
        }
        (!tables::is_alnum(c) || tables::bits(c) & tables::LINE_BREAKABLE != 0)
            .then(|| e + c.len_utf8())
    }

    fn walk(
        &self,
        s: &str,
        zv: usize,
        node: usize,
        at: usize,
        best: &mut Option<(u32, usize, usize)>,
    ) {
        let n = &self.nodes[node];
        if let Some(o) = n.order
            && best.is_none_or(|b| o < b.0)
            && let Some(w) = Self::trailing(s, at, zv)
        {
            *best = Some((o, at, w));
        }
        let Some(c) = s[at..zv].chars().next() else {
            return;
        };
        if let Some(next) = n.edge(fold(c)) {
            self.walk(s, zv, next as usize, at + c.len_utf8(), best);
        }
        if let Some(next) = n.space
            && tables::is_space(c)
        {
            // `\s-+` is greedy: the whole run first, then shorter runs that
            // end before a whitespace character the target spells out.
            let mut ends = vec![];
            let mut e = at;
            for (i, d) in s[at..zv].char_indices() {
                if !tables::is_space(d) {
                    break;
                }
                if i > 0 && d != ' ' {
                    ends.push(at + i);
                }
                e = at + i + d.len_utf8();
            }
            self.walk(s, zv, next as usize, e, best);
            for e in ends.into_iter().rev() {
                self.walk(s, zv, next as usize, e, best);
            }
        }
    }

    /// The target matching at `p` (in `s`, which ends at `zv` for the
    /// match): the target's end and the end of the whole match.
    pub(crate) fn match_at(&self, s: &str, p: usize, zv: usize) -> Option<(usize, usize)> {
        let root = &self.nodes[0];
        let c = s[p..zv].chars().next()?;
        root.edge(fold(c))?;
        let mut best = None;
        crate::deep(|| self.walk(s, zv, 0, p, &mut best));
        best.map(|(_, e, w)| (e, w))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(targets: &[&str]) -> RadioMatcher {
        RadioMatcher::new(&targets.iter().map(|t| t.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn matches_like_the_alternation() {
        let r = m(&["foo", "foo bar"]);
        // The longer target is tried first.
        assert_eq!(r.match_at("foo bar.", 0, 8), Some((7, 8)));
        // Its trailing boundary fails, so the shorter one wins.
        assert_eq!(r.match_at("foo barx", 0, 8), Some((3, 4)));
        // Case-insensitive, spaces match any whitespace run.
        assert_eq!(r.match_at("FOO \n\t Bar", 0, 10), Some((10, 10)));
        // End of line and end of text are boundaries.
        assert_eq!(r.match_at("foo\nx", 0, 5), Some((3, 3)));
        assert_eq!(r.match_at("foo", 0, 3), Some((3, 3)));
        assert_eq!(r.match_at("food", 0, 4), None);
        // Turkish letters fold through Emacs's case table.
        let r = m(&["Çalışma"]);
        assert_eq!(
            r.match_at("çALIşMA ", 0, "çALIşMA ".len()),
            None,
            "ı and I differ in Emacs"
        );
        assert!(r.match_at("çalışma ", 0, "çalışma ".len()).is_some());
    }
}
