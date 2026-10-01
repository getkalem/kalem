//! Positions that move with the text as it changes: Vim's marks (`ma`,
//! `'a`), its jump list (CTRL-O, CTRL-I) and change list (`g;`, `g,`).

use std::collections::HashMap;

use org_edit::{Assoc, Transaction};

/// The marks of a document.
#[derive(Debug, Clone, Default)]
pub struct Marks {
    /// Marks by name: `a` to `z`, and Vim's own (`[`, `]`, `<`, `>`, `.`,
    /// `^`, `'`).
    pub named: HashMap<char, usize>,
    /// The jump list, oldest first.
    pub jumps: Vec<usize>,
    /// Where CTRL-O and CTRL-I are in the jump list (`jumps.len()`: after
    /// the newest).
    pub jump_index: usize,
    /// The change list, oldest first.
    pub changes: Vec<usize>,
    /// Where `g;` and `g,` are in it.
    pub change_index: usize,
    /// Positions a command holds while it works (`:g`, `:normal`).
    pub held: Vec<usize>,
}

/// At most this many jumps and changes are kept, as Vim does.
const KEEP: usize = 100;

impl Marks {
    /// Moves every position through `tx`.
    pub fn map(&mut self, tx: &Transaction) {
        for p in self.named.values_mut() {
            *p = tx.map(*p, Assoc::Before);
        }
        for p in self
            .jumps
            .iter_mut()
            .chain(self.changes.iter_mut())
            .chain(self.held.iter_mut())
        {
            *p = tx.map(*p, Assoc::Before);
        }
    }

    /// Records a jump from `pos`: an older entry on the same line goes.
    pub fn jump(&mut self, pos: usize, line_of: impl Fn(usize) -> usize) {
        let line = line_of(pos);
        self.jumps.retain(|p| line_of(*p) != line);
        self.jumps.push(pos);
        if self.jumps.len() > KEEP {
            self.jumps.remove(0);
        }
        self.jump_index = self.jumps.len();
    }

    /// Records a change at `pos`.
    pub fn change(&mut self, pos: usize, line_of: impl Fn(usize) -> usize) {
        if self
            .changes
            .last()
            .is_some_and(|p| line_of(*p) == line_of(pos))
        {
            self.changes.pop();
        }
        self.changes.push(pos);
        if self.changes.len() > KEEP {
            self.changes.remove(0);
        }
        self.change_index = self.changes.len();
    }
}
