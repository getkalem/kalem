//! Parser configuration: the settings that change how Org parses a
//! buffer. They correspond to Emacs variables and in-buffer keywords.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// Which characters may terminate an ordered list bullet
/// (`org-plain-list-ordered-item-terminator`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemTerminator {
    /// Both `1.` and `1)` (the default, `t`).
    Both,
    /// Only `1.`.
    Dot,
    /// Only `1)`.
    Paren,
}

/// Whether a TODO keyword set is a workflow or a list of types
/// (`#+SEQ_TODO`/`#+TODO` versus `#+TYP_TODO`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoSequenceKind {
    /// Keywords are states of one workflow (`sequence`).
    Sequence,
    /// Keywords are alternatives, such as people (`type`).
    Type,
}

/// A TODO keyword of a sequence, such as `WAIT(w@/!)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoKeyword {
    /// The keyword, such as `WAIT`.
    pub name: String,
    /// The fast selection key, such as `w`.
    pub key: Option<char>,
    /// The text inside the parentheses, such as `w@/!` (logging settings).
    pub spec: Option<String>,
    /// Whether the keyword is a done state.
    pub done: bool,
}

/// One line of TODO keywords (an element of `org-todo-sets`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoSequence {
    /// Workflow or types.
    pub kind: TodoSequenceKind,
    /// The keywords in order, not-done ones first.
    pub keywords: Vec<TodoKeyword>,
}

impl TodoSequence {
    /// Parses a `#+TODO:` value as `org-set-regexps-and-options` does.
    pub fn parse(kind: TodoSequenceKind, value: &str) -> TodoSequence {
        let words: Vec<&str> = value.split_whitespace().collect();
        let sep = words.iter().position(|w| *w == "|");
        let names: Vec<(String, Option<char>, Option<String>)> = words
            .iter()
            .filter(|w| **w != "|")
            .map(|w| split_keyword(w))
            .collect();
        // Done keywords are those after the first `|`, or the last one.
        let done: Vec<String> = match sep {
            Some(i) => words[i + 1..].iter().map(|w| split_keyword(w).0).collect(),
            None => names.last().map(|n| vec![n.0.clone()]).unwrap_or_default(),
        };
        let not_done = sep.unwrap_or(names.len().saturating_sub(1));
        let keywords = names
            .into_iter()
            .enumerate()
            .map(|(i, (name, key, spec))| TodoKeyword {
                done: i >= not_done && done.contains(&name),
                name,
                key,
                spec,
            })
            .collect();
        TodoSequence { kind, keywords }
    }
}

/// `^\(.*?\)\(?:(\([^!@/]\)?.*?)\)?$`: the name, the fast key and the
/// parenthesized settings of a TODO keyword.
fn split_keyword(w: &str) -> (String, Option<char>, Option<String>) {
    match w.find('(') {
        Some(i) if w.ends_with(')') && w.len() > i + 1 => {
            let inner = &w[i + 1..w.len() - 1];
            let key = inner
                .chars()
                .next()
                .filter(|c| !matches!(c, '!' | '@' | '/'));
            (w[..i].to_string(), key, Some(inner.to_string()))
        }
        _ => (w.to_string(), None, None),
    }
}

/// Parser configuration.
///
/// The defaults match `emacs -Q` with Org 9.7 and `org-inlinetask` loaded.
/// Use [`ParseContext::for_document`] to apply a document's in-buffer
/// settings such as `#+TODO`.
#[derive(Debug, Clone)]
pub struct ParseContext {
    /// Not-done TODO keywords, in order (`org-not-done-keywords`). The
    /// parser reads this list and [`ParseContext::done_keywords`];
    /// [`ParseContext::set_todo_sequences`] keeps all three fields in step.
    pub todo_keywords: Vec<String>,
    /// Done keywords (`org-done-keywords`).
    pub done_keywords: Vec<String>,
    /// The TODO keyword sets (`org-todo-sets`), for editing commands such
    /// as cycling.
    pub todo_sequences: Vec<TodoSequence>,
    /// Registered link types (`org-link-types`).
    pub link_types: Vec<String>,
    /// Link abbreviations from `#+LINK` and `org-link-abbrev-alist`.
    pub link_abbrevs: Vec<(String, String)>,
    /// Radio targets found in the document.
    pub radio_targets: Vec<String>,
    /// Minimum level of an inlinetask (`org-inlinetask-min-level`), or
    /// `None` when inlinetasks are disabled.
    pub inlinetask_min_level: Option<usize>,
    /// `org-odd-levels-only` (`#+STARTUP: odd`).
    pub odd_levels_only: bool,
    /// `org-footnote-section`.
    pub footnote_section: Option<String>,
    /// `org-list-allow-alphabetical`.
    pub list_allow_alphabetical: bool,
    /// `org-plain-list-ordered-item-terminator`.
    pub item_terminator: ItemTerminator,
    pub(crate) compiled: OnceLock<Arc<crate::objects::ContextRegexes>>,
}

impl Default for ParseContext {
    fn default() -> Self {
        ParseContext {
            todo_keywords: vec!["TODO".into()],
            done_keywords: vec!["DONE".into()],
            todo_sequences: vec![TodoSequence::parse(TodoSequenceKind::Sequence, "TODO DONE")],
            link_types: DEFAULT_LINK_TYPES.iter().map(|s| s.to_string()).collect(),
            link_abbrevs: Vec::new(),
            radio_targets: Vec::new(),
            inlinetask_min_level: Some(15),
            odd_levels_only: false,
            footnote_section: Some("Footnotes".into()),
            list_allow_alphabetical: false,
            item_terminator: ItemTerminator::Both,
            compiled: OnceLock::new(),
        }
    }
}

/// Link types registered in `emacs -Q` with Org 9.7, plus `attachment`
/// from org-attach.
pub const DEFAULT_LINK_TYPES: &[&str] = &[
    "eww",
    "rmail",
    "mhe",
    "irc",
    "info",
    "gnus",
    "docview",
    "bibtex",
    "bbdb",
    "w3m",
    "doi",
    "id",
    "file+sys",
    "file+emacs",
    "shell",
    "news",
    "mailto",
    "https",
    "http",
    "ftp",
    "help",
    "file",
    "elisp",
    "attachment",
];

impl ParseContext {
    /// Returns `true` if `word` is a TODO keyword of either kind.
    pub fn is_todo_keyword(&self, word: &str) -> bool {
        self.todo_keywords
            .iter()
            .chain(self.done_keywords.iter())
            .any(|k| k == word)
    }

    /// Returns `true` if `word` is a done keyword.
    pub fn is_done_keyword(&self, word: &str) -> bool {
        self.done_keywords.iter().any(|k| k == word)
    }

    pub(crate) fn regexes(&self) -> &crate::objects::ContextRegexes {
        self.compiled.get_or_init(|| {
            // Compiling these regexps is expensive (large Unicode classes),
            // and contexts are created per document, so share them between
            // contexts with the same relevant settings.
            static CACHE: OnceLock<Mutex<HashMap<String, Arc<crate::objects::ContextRegexes>>>> =
                OnceLock::new();
            let key = format!(
                "{:?}\u{0}{:?}\u{0}{:?}\u{0}{}",
                self.link_types,
                self.radio_targets,
                self.item_terminator,
                self.list_allow_alphabetical
            );
            let cache = CACHE.get_or_init(Default::default);
            let mut map = cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(r) = map.get(&key) {
                return Arc::clone(r);
            }
            let r = Arc::new(crate::objects::ContextRegexes::new(self));
            if map.len() > 64 {
                map.clear();
            }
            map.insert(key, Arc::clone(&r));
            r
        })
    }

    /// Sets the TODO keyword sets and the keyword lists the parser uses,
    /// as `org-set-regexps-and-options` computes them.
    pub fn set_todo_sequences(&mut self, sequences: Vec<TodoSequence>) {
        let all: Vec<&TodoKeyword> = sequences.iter().flat_map(|s| &s.keywords).collect();
        let mut done: Vec<String> = all
            .iter()
            .filter(|k| k.done)
            .map(|k| k.name.clone())
            .collect();
        // `(unless org-done-keywords (setq org-done-keywords (last ...)))`
        if done.is_empty()
            && let Some(last) = all.last()
        {
            done.push(last.name.clone());
        }
        self.todo_keywords = all
            .iter()
            .filter(|k| !done.contains(&k.name))
            .map(|k| k.name.clone())
            .collect();
        self.done_keywords = done;
        self.todo_sequences = sequences;
        self.compiled = Default::default();
    }

    /// Returns the configuration for `text`: `base` updated with the
    /// document's in-buffer settings (`#+TODO`, `#+SEQ_TODO`, `#+TYP_TODO`,
    /// `#+LINK`, `#+STARTUP`) and its radio targets.
    pub fn for_document(text: &str, base: &ParseContext) -> ParseContext {
        crate::prepass::context_for(text, base, &crate::prepass::NoSetupFiles)
    }

    /// Like [`ParseContext::for_document`], and also reads `#+SETUPFILE`
    /// files through `loader`.
    pub fn for_document_with(
        text: &str,
        base: &ParseContext,
        loader: &dyn crate::SetupFileLoader,
    ) -> ParseContext {
        crate::prepass::context_for(text, base, loader)
    }

    /// The context for a file on disk: in-buffer settings, including
    /// `#+SETUPFILE` files relative to the file's directory.
    pub fn for_file(text: &str, path: &std::path::Path, base: &ParseContext) -> ParseContext {
        let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or_default();
        crate::prepass::context_for(text, base, &crate::FsSetupFiles { base: dir })
    }
}
