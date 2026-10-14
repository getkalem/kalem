//! Undo and redo (design 6.2): a stack of transactions with their
//! inverses. Consecutive typing within 300 ms forms one undo step, and the
//! selection is restored.

use std::time::{Duration, Instant};

use crate::transaction::{Selection, Transaction};

/// What kind of change a transaction is, for grouping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// Typing or deleting characters: merges with the previous step when
    /// it was typing too and came soon enough.
    Typing,
    /// Any other command: always its own step.
    Command,
}

#[derive(Debug, Clone)]
struct Step {
    /// Forward and inverse transactions, in the order they were applied.
    txs: Vec<(Transaction, Transaction)>,
    selection_before: Selection,
    selection_after: Selection,
    kind: ChangeKind,
    time: Instant,
    label: String,
}

/// The undo history of one document.
#[derive(Debug, Clone)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Typing within this time joins the previous step (300 ms).
    pub group_window: Duration,
    /// The most steps kept.
    pub limit: usize,
    /// While set, every change joins one step (Vim's insert session is
    /// one step): `false` until its first change makes the step, `true`
    /// after. Not the number of steps when it began, which stops growing
    /// once the oldest steps are dropped at the limit.
    join: Option<bool>,
    /// While set, a change joins the last step, whatever its kind
    /// ([`History::join_next`]).
    join_last: bool,
}

impl Default for History {
    fn default() -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            group_window: Duration::from_millis(300),
            limit: 10_000,
            join: None,
            join_last: false,
        }
    }
}

/// Transactions to apply for an undo or a redo, in order, and the
/// selection afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replay {
    /// Apply these in order.
    pub transactions: Vec<Transaction>,
    /// The selection to restore.
    pub selection: Selection,
    /// The selection the step began with (Vim puts the cursor there on
    /// redo too).
    pub start: Selection,
    /// The step's label.
    pub label: String,
}

impl History {
    /// An empty history.
    pub fn new() -> History {
        History::default()
    }

    /// Records `tx`, which was just applied to `before` (the text before
    /// it).
    pub fn record(
        &mut self,
        tx: &Transaction,
        before: &str,
        selection_before: Selection,
        selection_after: Selection,
        kind: ChangeKind,
        now: Instant,
    ) {
        if tx.is_empty() {
            return;
        }
        self.redo.clear();
        let pair = (tx.clone(), tx.invert(before));
        if self.join_last
            && let Some(last) = self.undo.last_mut()
        {
            last.txs.push(pair);
            last.selection_after = selection_after;
            last.time = now;
            return;
        }
        if self.join == Some(true)
            && let Some(last) = self.undo.last_mut()
        {
            last.txs.push(pair);
            last.selection_after = selection_after;
            last.time = now;
            return;
        }
        if kind == ChangeKind::Typing
            && let Some(last) = self.undo.last_mut()
            && last.kind == ChangeKind::Typing
            && now.saturating_duration_since(last.time) <= self.group_window
        {
            last.txs.push(pair);
            last.selection_after = selection_after;
            last.time = now;
            return;
        }
        self.undo.push(Step {
            txs: vec![pair],
            selection_before,
            selection_after,
            kind,
            time: now,
            label: tx.label.clone(),
        });
        if self.join == Some(false) {
            self.join = Some(true);
        }
        if self.undo.len() > self.limit {
            self.undo.remove(0);
        }
    }

    /// Whether there is something to undo.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether there is something to redo.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// How many steps there are to undo.
    pub fn depth(&self) -> usize {
        self.undo.len()
    }

    /// Forgets the steps there are to redo (a cancelled entry's, undone).
    pub fn forget_redo(&mut self) {
        self.redo.clear();
    }

    /// The label of the next undo step.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    /// Undoes the last step: the inverse transactions to apply.
    pub fn undo(&mut self) -> Option<Replay> {
        self.join = None;
        let step = self.undo.pop()?;
        let replay = Replay {
            transactions: step.txs.iter().rev().map(|(_, inv)| inv.clone()).collect(),
            selection: step.selection_before,
            start: step.selection_before,
            label: step.label.clone(),
        };
        self.redo.push(step);
        Some(replay)
    }

    /// Redoes the last undone step.
    pub fn redo(&mut self) -> Option<Replay> {
        self.join = None;
        let step = self.redo.pop()?;
        let replay = Replay {
            transactions: step.txs.iter().map(|(f, _)| f.clone()).collect(),
            selection: step.selection_after,
            start: step.selection_before,
            label: step.label.clone(),
        };
        self.undo.push(step);
        Some(replay)
    }

    /// Ends the current typing group, so the next typing starts a new
    /// step (for example after the cursor moves).
    pub fn break_group(&mut self) {
        self.join = None;
        if let Some(last) = self.undo.last_mut() {
            last.kind = ChangeKind::Command;
        }
    }

    /// While `on`, a change joins the last step, whatever its kind: a
    /// change made again another way (a language server's form of the new
    /// line just made) is undone with it.
    pub fn join_next(&mut self, on: bool) {
        self.join_last = on;
    }

    /// From now until [`History::break_group`], every change joins one
    /// step.
    pub fn begin_join(&mut self) {
        self.break_group();
        self.join = Some(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Records the insertion of `word` at the end of `text`, and makes it.
    fn type_word(h: &mut History, text: &mut String, word: &str, now: Instant) {
        let at = text.len();
        let mut tx = Transaction::new("t");
        tx.insert(at, word).unwrap();
        let (before, after) = (Selection::caret(at), Selection::caret(at + word.len()));
        h.record(&tx, text, before, after, ChangeKind::Command, now);
        *text = tx.apply(text);
    }

    #[test]
    fn a_joined_session_stays_one_step_at_the_limit() {
        // Vim's insert session is one undo step while the oldest steps
        // are dropped at the limit: once the history was full, each change
        // of the session became a step of its own.
        let mut h = History {
            limit: 2,
            ..History::default()
        };
        let (mut text, now) = (String::new(), Instant::now());
        for word in ["a", "b", "c"] {
            type_word(&mut h, &mut text, word, now);
        }
        assert_eq!(h.depth(), 2);
        h.begin_join();
        for word in ["x", "y", "z"] {
            type_word(&mut h, &mut text, word, now);
        }
        assert_eq!((text.as_str(), h.depth()), ("abcxyz", 2));
        for t in h.undo().unwrap().transactions {
            text = t.apply(&text);
        }
        assert_eq!(text, "abc");
    }
}
