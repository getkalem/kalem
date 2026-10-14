//! Undo and redo (design 6.2): a stack of transactions with their
//! inverses. Consecutive typing within 300 ms forms one undo step, and the
//! selection is restored.

use std::collections::VecDeque;
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
    /// The other cursors when the step began (set by
    /// [`History::set_cursors`]) and after its last change.
    extra_before: Option<Vec<Selection>>,
    extra_after: Vec<Selection>,
    kind: ChangeKind,
    time: Instant,
    label: String,
}

/// The undo history of one document.
#[derive(Debug, Clone)]
pub struct History {
    undo: VecDeque<Step>,
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
    /// The last step was recorded since the last undo or redo: the change
    /// just made, which [`History::join_next`] may join.
    fresh: bool,
}

impl Default for History {
    fn default() -> Self {
        History {
            undo: VecDeque::new(),
            fresh: false,
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
    /// The other cursors to restore.
    pub extra: Vec<Selection>,
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
            && self.fresh
            && let Some(last) = self.undo.back_mut()
        {
            last.txs.push(pair);
            last.selection_after = selection_after;
            last.time = now;
            return;
        }
        if self.join == Some(true)
            && let Some(last) = self.undo.back_mut()
        {
            last.txs.push(pair);
            last.selection_after = selection_after;
            last.time = now;
            return;
        }
        if kind == ChangeKind::Typing
            && let Some(last) = self.undo.back_mut()
            && last.kind == ChangeKind::Typing
            && now.saturating_duration_since(last.time) <= self.group_window
        {
            last.txs.push(pair);
            last.selection_after = selection_after;
            last.time = now;
            return;
        }
        self.fresh = true;
        self.undo.push_back(Step {
            txs: vec![pair],
            selection_before,
            selection_after,
            extra_before: None,
            extra_after: Vec::new(),
            kind,
            time: now,
            label: tx.label.clone(),
        });
        if self.join == Some(false) {
            self.join = Some(true);
        }
        // `while`: a limit lowered since drops all the steps over it.
        while self.undo.len() > self.limit {
            self.undo.pop_front();
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
        self.undo.back().map(|s| s.label.as_str())
    }

    /// Undoes the last step: the inverse transactions to apply. Typing
    /// after it is a step of its own, not joined to the step now last.
    pub fn undo(&mut self) -> Option<Replay> {
        self.join = None;
        self.fresh = false;
        let step = self.undo.pop_back()?;
        if let Some(last) = self.undo.back_mut() {
            last.kind = ChangeKind::Command;
        }
        let replay = Replay {
            transactions: step.txs.iter().rev().map(|(_, inv)| inv.clone()).collect(),
            selection: step.selection_before,
            start: step.selection_before,
            label: step.label.clone(),
            extra: step.extra_before.clone().unwrap_or_default(),
        };
        self.redo.push(step);
        Some(replay)
    }

    /// Redoes the last undone step.
    pub fn redo(&mut self) -> Option<Replay> {
        self.join = None;
        self.fresh = false;
        let mut step = self.redo.pop()?;
        // Typing after it is a step of its own.
        step.kind = ChangeKind::Command;
        let replay = Replay {
            transactions: step.txs.iter().map(|(f, _)| f.clone()).collect(),
            selection: step.selection_after,
            start: step.selection_before,
            label: step.label.clone(),
            extra: step.extra_after.clone(),
        };
        self.undo.push_back(step);
        Some(replay)
    }

    /// The other cursors (than the selection [`History::record`] takes)
    /// before and after the change just recorded: kept with its step, the
    /// first ones when the step began, so that undo and redo bring them
    /// back.
    pub fn set_cursors(&mut self, before: &[Selection], after: &[Selection]) {
        if let Some(last) = self.undo.back_mut() {
            last.extra_before.get_or_insert_with(|| before.to_vec());
            last.extra_after = after.to_vec();
        }
    }

    /// Whether a change may join the last step ([`History::join_next`]):
    /// it is the change just made, not one an undo or redo brought back.
    pub fn can_join_last(&self) -> bool {
        self.fresh && !self.undo.is_empty()
    }

    /// Ends the current typing group, so the next typing starts a new
    /// step (for example after the cursor moves).
    pub fn break_group(&mut self) {
        self.join = None;
        if let Some(last) = self.undo.back_mut() {
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

    /// Records the insertion of `word` at `at` as `kind`, and makes it.
    fn insert(
        h: &mut History,
        text: &mut String,
        at: usize,
        word: &str,
        kind: ChangeKind,
        now: Instant,
    ) {
        let mut tx = Transaction::new("t");
        tx.insert(at, word).unwrap();
        let (before, after) = (Selection::caret(at), Selection::caret(at + word.len()));
        h.record(&tx, text, before, after, kind, now);
        *text = tx.apply(text);
    }

    fn replay(text: &mut String, r: &Replay) {
        for t in &r.transactions {
            *text = t.apply(text);
        }
    }

    #[test]
    fn undo_and_redo_bring_the_cursors_back() {
        let (mut h, mut text, now) = (History::new(), "ab\nab\n".to_string(), Instant::now());
        // Typed with a second cursor, which a later change in the same
        // step moves on.
        let others = [Selection::caret(3)];
        insert(&mut h, &mut text, 0, "x", ChangeKind::Typing, now);
        h.set_cursors(&others, &[Selection::caret(5)]);
        insert(&mut h, &mut text, 1, "y", ChangeKind::Typing, now);
        h.set_cursors(&[Selection::caret(5)], &[Selection::caret(7)]);
        let r = h.undo().unwrap();
        assert_eq!(
            (r.selection, r.extra.as_slice()),
            (Selection::caret(0), &others[..])
        );
        replay(&mut text, &r);
        let r = h.redo().unwrap();
        assert_eq!(
            (r.selection, r.extra),
            (Selection::caret(2), vec![Selection::caret(7)])
        );
    }

    #[test]
    fn typing_after_an_undo_is_a_step_of_its_own() {
        // "x" typed, a command, the command undone, "y" typed at once:
        // undo takes back "y" alone, where it joined the step of "x".
        let (mut h, mut text, now) = (History::new(), String::new(), Instant::now());
        insert(&mut h, &mut text, 0, "x", ChangeKind::Typing, now);
        insert(&mut h, &mut text, 1, "!", ChangeKind::Command, now);
        replay(&mut text, &h.undo().unwrap());
        insert(&mut h, &mut text, 1, "y", ChangeKind::Typing, now);
        replay(&mut text, &h.undo().unwrap());
        assert_eq!(text, "x");
        // And after a redo.
        replay(&mut text, &h.redo().unwrap());
        insert(&mut h, &mut text, 2, "z", ChangeKind::Typing, now);
        replay(&mut text, &h.undo().unwrap());
        assert_eq!(text, "xy");
    }

    #[test]
    fn a_lowered_limit_drops_every_step_over_it() {
        let (mut h, mut text, now) = (History::new(), String::new(), Instant::now());
        for i in 0..10 {
            insert(&mut h, &mut text, i, "a", ChangeKind::Command, now);
        }
        h.limit = 3;
        insert(&mut h, &mut text, 10, "b", ChangeKind::Command, now);
        assert_eq!(h.depth(), 3);
    }

    #[test]
    fn a_change_joins_only_the_change_just_made() {
        let (mut h, mut text, now) = (History::new(), String::new(), Instant::now());
        insert(&mut h, &mut text, 0, "a", ChangeKind::Command, now);
        insert(&mut h, &mut text, 1, "b", ChangeKind::Command, now);
        assert!(h.can_join_last());
        replay(&mut text, &h.undo().unwrap());
        // The step last now was not just made: nothing may join it, and
        // the step undone can still be redone.
        assert!(!h.can_join_last());
        assert!(h.can_redo());
    }
}
