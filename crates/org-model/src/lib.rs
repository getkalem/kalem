//! The document model of Org mode files.
//!
//! `org-model` computes, from an [`org_syntax::Parse`], what Org computes
//! from a buffer: the outline, TODO states, tags with inheritance,
//! properties (drawers, `#+PROPERTY`, inheritance, special properties),
//! categories, and more. Each function follows the Emacs function it is
//! named after in its documentation, and the results are compared with
//! Emacs 30.1 / Org 9.7 by `kalem diff-emacs --model`.
//!
//! ```
//! let doc = org_model::Document::new(org_syntax::parse(
//!     "#+FILETAGS: :project:\n* TODO Task :work:\n** Sub :home:\n",
//! ));
//! let sub = doc.outline().entries.iter().position(|e| e.raw_title == "Sub").unwrap();
//! let tags = doc.tags(org_model::EntryId(sub));
//! assert_eq!(tags, ["project", "work", "home"]);
//! ```

mod cache;
mod clock;
mod footnotes;
mod info;
mod links;
mod matcher;
mod outline;
mod properties;
mod queries;
mod settings;
mod stats;
mod tags;
pub mod time;

use std::sync::{Arc, OnceLock};

pub use cache::ModelCache;
pub use footnotes::FootnoteRef;
pub use info::{Info, Priorities};
pub use links::InternalLink;
pub use matcher::{Matcher, emacs_regex, string_to_number};
pub use outline::{Entry, EntryId, NodeProperty, Outline};
pub use properties::{
    ComplexHeading, Inherit, SPECIAL_PROPERTIES, complex_heading, complex_heading_title,
    complex_heading_todo,
};
pub use queries::AllowedValues;
pub use settings::{Inheritance, Settings};
pub use tags::{TagTable, TagToken, expand_group_tags};

use org_syntax::Parse;

/// A parsed document with lazily computed model views.
#[derive(Debug)]
pub struct Document {
    parse: Parse,
    settings: Arc<Settings>,
    file_name: Option<String>,
    info: OnceLock<Info>,
    outline: OnceLock<Outline>,
    top: OnceLock<properties::Top>,
    cache: Option<Arc<ModelCache>>,
}

impl Document {
    /// A document with the default settings and no file name.
    pub fn new(parse: Parse) -> Document {
        Document::with_settings(parse, Arc::new(Settings::default()), None)
    }

    /// A document with explicit settings, and the file name that gives the
    /// default category (`buffer-file-name`).
    pub fn with_settings(
        parse: Parse,
        settings: Arc<Settings>,
        file_name: Option<String>,
    ) -> Document {
        Document {
            parse,
            settings,
            file_name,
            info: OnceLock::new(),
            outline: OnceLock::new(),
            top: OnceLock::new(),
            cache: None,
        }
    }

    /// A new version of a document: subtrees unchanged since the version
    /// that used the same `cache` are not computed again.
    pub fn with_cache(
        parse: Parse,
        settings: Arc<Settings>,
        file_name: Option<String>,
        cache: Arc<ModelCache>,
    ) -> Document {
        let mut d = Document::with_settings(parse, settings, file_name);
        d.cache = Some(cache);
        d
    }

    /// The parse.
    pub fn parse(&self) -> &Parse {
        &self.parse
    }

    /// The settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Document-wide settings from keywords.
    pub fn info(&self) -> &Info {
        self.info
            .get_or_init(|| Info::new(&self.parse, &self.settings, self.cache.as_deref()))
    }

    /// The outline.
    pub fn outline(&self) -> &Outline {
        self.outline.get_or_init(|| {
            Outline::build(
                &self.parse.syntax(),
                self.parse.context(),
                self.cache.as_deref(),
            )
        })
    }

    /// The entry with the given id.
    pub fn entry(&self, id: EntryId) -> &Entry {
        self.outline().get(id)
    }
}

/// The planning line of an entry.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Planning {
    /// `SCHEDULED:`.
    pub scheduled: Option<time::Timestamp>,
    /// `DEADLINE:`.
    pub deadline: Option<time::Timestamp>,
    /// `CLOSED:`.
    pub closed: Option<time::Timestamp>,
}

/// The TODO state of an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoState {
    /// The keyword.
    pub keyword: String,
    /// Whether it is a done state (`org-done-keywords`).
    pub done: bool,
    /// The first keyword set that contains it (an index into
    /// [`org_syntax::ParseContext::todo_sequences`]).
    pub sequence: Option<usize>,
}

impl Document {
    /// The syntax node of an entry.
    pub fn node(&self, id: EntryId) -> org_syntax::SyntaxNode {
        let range = self.entry(id).range;
        let root = self.parse.syntax();
        match root.covering_element(range) {
            rowan::NodeOrToken::Node(n) => n
                .ancestors()
                .find(|a| {
                    a.text_range() == range
                        && matches!(
                            a.kind(),
                            org_syntax::SyntaxKind::HEADLINE | org_syntax::SyntaxKind::INLINETASK
                        )
                })
                .unwrap_or(n),
            rowan::NodeOrToken::Token(t) => t.parent().unwrap_or(root),
        }
    }

    /// The entry's planning line.
    pub fn planning(&self, id: EntryId) -> Planning {
        use org_syntax::ast::{AstNode, Headline, Inlinetask};
        let node = self.node(id);
        let planning = Headline::cast(node.clone())
            .and_then(|h| h.planning())
            .or_else(|| Inlinetask::cast(node).and_then(|h| h.planning()));
        let Some(p) = planning else {
            return Planning::default();
        };
        let conv = |t: Option<org_syntax::ast::Timestamp>| {
            t.and_then(|t| time::Timestamp::from_node(t.syntax()))
        };
        Planning {
            scheduled: conv(p.scheduled()),
            deadline: conv(p.deadline()),
            closed: conv(p.closed()),
        }
    }

    /// The entry's TODO state (`org-get-todo-state`, `org-entry-is-done-p`).
    pub fn todo_state(&self, id: EntryId) -> Option<TodoState> {
        let keyword = self.entry(id).todo.clone()?;
        let ctx = self.parse.context();
        Some(TodoState {
            done: ctx.done_keywords.contains(&keyword),
            sequence: ctx
                .todo_sequences
                .iter()
                .position(|s| s.keywords.iter().any(|k| k.name == keyword)),
            keyword,
        })
    }
}
