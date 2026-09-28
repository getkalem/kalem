//! Undo restores the text, redo reapplies it, for random edits.

use std::time::{Duration, Instant};

use org_edit::{ChangeKind, History, Selection, Transaction};
use proptest::prelude::*;

fn floor(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn transaction(text: &str, spec: &[(usize, usize, String)]) -> Transaction {
    let mut t = Transaction::new("edit");
    for (a, len, ins) in spec {
        let a = floor(text, a % (text.len() + 1));
        let b = floor(text, (a + len).min(text.len())).max(a);
        let _ = t.replace(a..b, ins.clone());
    }
    t
}

proptest! {
    #[test]
    fn undo_redo_round_trip(
        start in "[a-zç \n*]{0,40}",
        steps in prop::collection::vec(prop::collection::vec((0usize..60, 0usize..6, "[xyş\n]{0,3}"), 1..4), 1..12),
        typing in prop::collection::vec(any::<bool>(), 12),
    ) {
        let mut h = History::new();
        let mut texts = vec![start.clone()];
        let mut text = start;
        let t0 = Instant::now();
        for (i, spec) in steps.iter().enumerate() {
            let tx = transaction(&text, spec);
            let after = tx.apply(&text);
            let kind = if typing[i] { ChangeKind::Typing } else { ChangeKind::Command };
            h.record(&tx, &text, Selection::caret(0), Selection::caret(0), kind, t0 + Duration::from_millis(100 * i as u64));
            if !tx.is_empty() {
                texts.push(after.clone());
            }
            text = after;
        }
        // Undo everything: back to the start.
        let end = text.clone();
        while let Some(r) = h.undo() {
            for t in r.transactions {
                text = t.apply(&text);
            }
        }
        prop_assert_eq!(&text, &texts[0]);
        // Redo everything: back to the end.
        while let Some(r) = h.redo() {
            for t in r.transactions {
                text = t.apply(&text);
            }
        }
        prop_assert_eq!(&text, &end);
    }
}
