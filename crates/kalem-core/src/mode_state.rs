//! What a mode keeps about an open document (roadmap R3.2, R3.7): LaTeX's
//! parse and model, and the other modes' state as they move here, in one
//! slot of [`DocumentState`]. The document names no mode's types: each
//! mode's module owns its state, the accessors to it (an `impl
//! DocumentState` block of its own) and what an edit or a poll does to it,
//! through [`ModeState`].

use std::any::Any;
use std::path::Path;

use org_edit::Transaction;

use crate::DocumentMode;
use crate::document::DocumentState;

/// A document as its mode state sees it after an edit or at a poll.
#[derive(Debug, Clone, Copy)]
pub struct ModeContext<'a> {
    /// The text as it is now.
    pub text: &'a str,
    /// Its version.
    pub version: u64,
    /// Its file, if it has one.
    pub path: Option<&'a Path>,
}

/// The state one mode keeps about a document, in its slot.
pub trait ModeState: Any + Send + std::fmt::Debug {
    /// `tx` was applied: the text is now `cx.text`, at `cx.version`, and
    /// `edit` is the one edit covering `tx`'s, as the parsers take it.
    fn edited(
        &mut self,
        _cx: ModeContext<'_>,
        _tx: &Transaction,
        _edit: Option<&org_syntax::TextEdit>,
    ) {
    }

    /// Takes in what was worked out in the background. Returns whether
    /// what the view shows changed.
    fn poll(&mut self, _cx: ModeContext<'_>) -> bool {
        false
    }
}

/// The state a document in `mode` starts with, `None` for a mode that
/// keeps none here.
pub(crate) fn new(mode: &DocumentMode, cx: ModeContext<'_>) -> Option<Box<dyn ModeState>> {
    match mode {
        DocumentMode::Latex => Some(Box::new(crate::latex_view::LatexState::for_document(
            cx.text, cx.path,
        ))),
        DocumentMode::Csv => Some(Box::<crate::csv::CsvState>::default()),
        _ => None,
    }
}

/// What happened to a document that its mode may answer with the whole
/// document at hand ([`settle`]), where [`ModeState`]'s hooks see only
/// the state and the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// An edit was applied; `command` when a command made it (not typing).
    Applied {
        /// A command's edit.
        command: bool,
    },
    /// The cursor moved; `extend` when the move extends the selection.
    Moved {
        /// The selection extended.
        extend: bool,
    },
    /// A step of the history is about to be undone or redone.
    Replayed,
}

/// `doc`'s mode answers `event`.
pub(crate) fn settle(doc: &mut DocumentState, event: Event) {
    if doc.meta.mode == DocumentMode::Csv {
        crate::csv::settle(doc, event);
    }
}

impl DocumentState {
    /// The mode state of type `T`, if the document's slot holds one.
    pub fn mode_state<T: ModeState>(&self) -> Option<&T> {
        let s: &dyn Any = self.mode_slot()?;
        s.downcast_ref()
    }

    /// The mode state of type `T`, if the document's slot holds one.
    pub fn mode_state_mut<T: ModeState>(&mut self) -> Option<&mut T> {
        let s: &mut dyn Any = self.mode_slot_mut()?;
        s.downcast_mut()
    }
}
