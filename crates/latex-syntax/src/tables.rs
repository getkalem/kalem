//! One pass before parsing: which `{` closes where and which `\begin`
//! ends where (the nearest balanced partner, as TeX pairs them), and where
//! the sectioning commands are. The parser uses them to decide whether a
//! group or an environment is closed without scanning ahead again, which
//! keeps parsing linear on unbalanced input.

use std::collections::HashMap;

use crate::lexer::{self, Tok};
use crate::signatures;

#[derive(Debug, Default)]
pub(crate) struct Tables {
    /// `{` to its `}`.
    pub(crate) braces: HashMap<usize, usize>,
    /// `\begin` to its `\end`.
    pub(crate) envs: HashMap<usize, usize>,
    /// Sectioning commands, in order.
    pub(crate) sections: Vec<usize>,
    /// `\makeatletter` and `\makeatother`, in order.
    pub(crate) toggles: Vec<(usize, bool)>,
}

impl Tables {
    /// The first sectioning command at or after `pos`.
    pub(crate) fn section_after(&self, pos: usize) -> Option<usize> {
        let i = self.sections.partition_point(|&s| s < pos);
        self.sections.get(i).copied()
    }
}

/// The tables of `src[start..end]`, with `@` a letter at the start when
/// `at_letter`.
pub(crate) fn build(src: &str, start: usize, end: usize, mut at_letter: bool) -> Tables {
    let b = src.as_bytes();
    let mut t = Tables::default();
    let mut braces: Vec<usize> = Vec::new();
    let mut envs: Vec<(usize, &str)> = Vec::new();
    let mut pos = start;
    while pos < end {
        let (tok, e) = lexer::next(src, pos, end, at_letter);
        match tok {
            Tok::LBrace => braces.push(pos),
            Tok::RBrace => {
                if let Some(open) = braces.pop() {
                    t.braces.insert(open, pos);
                }
            }
            Tok::ControlWord => {
                let name = &src[pos + 1..e];
                match name {
                    "verb" | "lstinline" => {
                        if let Some(v) = lexer::verb_end(b, e, end, name == "lstinline") {
                            pos = v;
                            continue;
                        }
                    }
                    "url" | "href" => {
                        if let Some((_, close)) = lexer::raw_braces(b, e, end) {
                            pos = close + 1;
                            continue;
                        }
                    }
                    "makeatletter" | "makeatother" => {
                        at_letter = name == "makeatletter";
                        t.toggles.push((pos, at_letter));
                    }
                    "begin" => {
                        if let Some((_, s, k)) = lexer::env_name(b, e, end) {
                            let env = &src[s..k];
                            if signatures::is_verbatim(env) {
                                let close = format!("\\end{{{env}}}");
                                if let Some(i) = src[k..end].find(&close) {
                                    t.envs.insert(pos, k + i);
                                    pos = k + i + close.len();
                                    continue;
                                }
                            }
                            envs.push((pos, env));
                            pos = k + 1;
                            continue;
                        }
                    }
                    "end" => {
                        if let Some((_, s, k)) = lexer::env_name(b, e, end) {
                            let env = &src[s..k];
                            if let Some(i) = envs.iter().rposition(|(_, n)| *n == env) {
                                // Environments opened inside it and not
                                // closed stay open.
                                t.envs.insert(envs[i].0, pos);
                                envs.truncate(i);
                            }
                            pos = k + 1;
                            continue;
                        }
                    }
                    n if signatures::is_sectioning(n) => t.sections.push(pos),
                    _ => {}
                }
            }
            _ => {}
        }
        pos = e;
    }
    t
}
