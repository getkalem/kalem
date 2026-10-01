//! Workspaces (Doom's `SPC TAB`, T2.7i.15): named sets of a window's open
//! documents. Each document belongs to one workspace; the list of open
//! files, the document cycle and the pickers show the current
//! workspace's documents, and each workspace keeps the document it showed
//! last and (in the frontend) its panes. Deleting a workspace closes no
//! document: its documents join the workspace shown next.

use std::collections::HashMap;

/// A workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// Stays the same when workspaces are renamed or deleted.
    pub id: u64,
    /// Its name.
    pub name: String,
    /// The document it showed last, by the frontend's number.
    pub active: Option<u64>,
}

/// A window's workspaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspaces {
    list: Vec<Workspace>,
    current: usize,
    /// The workspace shown before (`SPC TAB \``).
    last: Option<u64>,
    next_id: u64,
    /// The workspace of each document, by id.
    members: HashMap<u64, u64>,
}

impl Default for Workspaces {
    fn default() -> Self {
        Workspaces::new()
    }
}

/// What a frontend does about workspaces, as the `workspace.*` commands
/// ask it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceOp {
    /// Choose one from the list.
    List,
    /// A new workspace, named or numbered.
    New(Option<String>),
    /// Delete the current workspace (its documents stay open).
    Delete,
    /// Rename the current workspace.
    Rename(String),
    /// The next workspace, or the one before.
    Cycle(bool),
    /// The workspace at this place, from 0.
    Switch(usize),
    /// The workspace shown before.
    Last,
    /// Save the current workspace's documents as a session.
    Save,
    /// Open a saved workspace in a new one; ask which without a name.
    Load(Option<String>),
    /// Delete a saved workspace; ask which without a name.
    DeleteSaved(Option<String>),
}

/// The prefix of the sessions saved for workspaces.
pub const SESSION_PREFIX: &str = "workspace-";

impl Workspaces {
    /// One workspace, `main`.
    pub fn new() -> Workspaces {
        Workspaces {
            list: vec![Workspace {
                id: 0,
                name: "main".into(),
                active: None,
            }],
            current: 0,
            last: None,
            next_id: 1,
            members: HashMap::new(),
        }
    }

    /// The workspaces in order.
    pub fn list(&self) -> &[Workspace] {
        &self.list
    }

    /// The current workspace.
    pub fn current(&self) -> &Workspace {
        &self.list[self.current]
    }

    /// Its place in the list.
    pub fn current_index(&self) -> usize {
        self.current
    }

    /// Whether there is more than one.
    pub fn several(&self) -> bool {
        self.list.len() > 1
    }

    /// Document `doc` joins the current workspace (if it belongs to none).
    pub fn join(&mut self, doc: u64) {
        let id = self.list[self.current].id;
        self.members.entry(doc).or_insert(id);
    }

    /// Document `doc` closed.
    pub fn leave(&mut self, doc: u64) {
        self.members.remove(&doc);
        for w in &mut self.list {
            if w.active == Some(doc) {
                w.active = None;
            }
        }
    }

    /// Whether the current workspace shows document `doc` (documents of no
    /// workspace show everywhere).
    pub fn shows(&self, doc: u64) -> bool {
        self.members
            .get(&doc)
            .is_none_or(|w| *w == self.list[self.current].id)
    }

    /// The documents of the current workspace among `docs`.
    pub fn members<'a>(&'a self, docs: &'a [u64]) -> impl Iterator<Item = u64> + 'a {
        docs.iter().copied().filter(|d| self.shows(*d))
    }

    /// The current workspace now shows document `doc`.
    pub fn showing(&mut self, doc: u64) {
        let id = self.list[self.current].id;
        self.members.insert(doc, id);
        self.list[self.current].active = Some(doc);
    }

    /// Makes workspace `i` current; `false` when it is already, or there
    /// is none.
    pub fn switch(&mut self, i: usize) -> bool {
        if i >= self.list.len() || i == self.current {
            return false;
        }
        self.last = Some(self.list[self.current].id);
        self.current = i;
        true
    }

    /// The next workspace (`back`: the one before), wrapping.
    pub fn cycle(&mut self, back: bool) -> bool {
        let n = self.list.len();
        let i = if back {
            (self.current + n - 1) % n
        } else {
            (self.current + 1) % n
        };
        self.switch(i)
    }

    /// The workspace shown before.
    pub fn switch_last(&mut self) -> bool {
        let Some(id) = self.last else {
            return false;
        };
        match self.list.iter().position(|w| w.id == id) {
            Some(i) => self.switch(i),
            None => false,
        }
    }

    /// A new workspace after the others, current: `name`, or `#N`.
    pub fn add(&mut self, name: Option<&str>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let name = match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(n) => n.to_string(),
            None => {
                let mut k = self.list.len() + 1;
                while self.list.iter().any(|w| w.name == format!("#{k}")) {
                    k += 1;
                }
                format!("#{k}")
            }
        };
        self.list.push(Workspace {
            id,
            name,
            active: None,
        });
        let i = self.list.len() - 1;
        self.switch(i);
        i
    }

    /// Renames the current workspace.
    pub fn rename(&mut self, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        self.list[self.current].name = name.to_string();
        true
    }

    /// Deletes the current workspace, the last one excepted: its
    /// documents join the workspace shown next (the last shown, else the
    /// one before it), which becomes current. Its id, for the frontend to
    /// forget its panes.
    pub fn delete(&mut self) -> Option<u64> {
        if self.list.len() < 2 {
            return None;
        }
        let gone = self.list.remove(self.current);
        let next = self
            .last
            .and_then(|id| self.list.iter().position(|w| w.id == id))
            .unwrap_or(self.current.saturating_sub(1));
        self.current = next.min(self.list.len() - 1);
        let to = self.list[self.current].id;
        for w in self.members.values_mut() {
            if *w == gone.id {
                *w = to;
            }
        }
        self.last = None;
        Some(gone.id)
    }

    /// The names of the saved workspaces.
    pub fn saved() -> Vec<String> {
        crate::sessions::names()
            .into_iter()
            .filter_map(|n| n.strip_prefix(SESSION_PREFIX).map(str::to_string))
            .collect()
    }

    /// The session name a workspace called `name` is saved under.
    pub fn session_name(name: &str) -> String {
        let safe: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{SESSION_PREFIX}{safe}")
    }

    /// The list to choose a workspace from: `workspace.switch` with its
    /// place.
    pub fn items(&self) -> Vec<crate::palette::PaletteItem> {
        let category = crate::tr!("category-workspaces");
        self.list
            .iter()
            .enumerate()
            .map(|(i, w)| crate::palette::PaletteItem {
                id: crate::palette::invocation(
                    "workspace.switch",
                    &serde_json::json!({ "index": i }),
                ),
                title: if i == self.current {
                    format!("{} ●", w.name)
                } else {
                    w.name.clone()
                },
                category: category.clone(),
                keys: if i < 9 {
                    format!("SPC TAB {}", i + 1)
                } else {
                    String::new()
                },
                also: String::new(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documents_by_workspace() {
        let mut w = Workspaces::new();
        w.join(1);
        w.join(2);
        assert!(w.shows(1) && w.shows(2));
        let second = w.add(None);
        assert_eq!(w.current().name, "#2");
        assert_eq!(second, 1);
        assert!(!w.shows(1));
        w.join(3);
        assert_eq!(w.members(&[1, 2, 3]).collect::<Vec<_>>(), [3]);
        // A document opened from another workspace moves here.
        w.showing(2);
        assert!(w.shows(2));
        assert_eq!(w.current().active, Some(2));
        // Back and forth.
        assert!(w.switch(0));
        assert_eq!(w.members(&[1, 2, 3]).collect::<Vec<_>>(), [1]);
        assert!(w.switch_last());
        assert_eq!(w.current_index(), 1);
        assert!(w.cycle(false));
        assert_eq!(w.current_index(), 0);
        // Deleting keeps the documents: they join the next one shown.
        w.switch(1);
        assert!(w.rename("notes"));
        assert_eq!(w.current().name, "notes");
        let gone = w.delete();
        assert_eq!(gone, Some(1));
        assert_eq!(w.current().name, "main");
        assert_eq!(w.members(&[1, 2, 3]).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(w.delete(), None, "the last stays");
        w.leave(1);
        assert_eq!(w.items().len(), 1);
        assert_eq!(Workspaces::session_name("my notes"), "workspace-my_notes");
    }
}
