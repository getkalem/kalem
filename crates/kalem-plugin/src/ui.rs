//! `kalem.ui` of design §11.4 over the `extension` world: notifications,
//! questions answered later through a function, status bar items, and
//! panels built as a [`Tree`] of widgets the editor renders (D11).
//!
//! ```ignore
//! use kalem_plugin::ui::{self, PanelSpec, Placement, Tree, WidgetKind};
//!
//! ui::confirm("Delete the draft?", |yes| if yes { /* … */ });
//! ui::panel(
//!     PanelSpec { id: "todo.list".into(), title: "To do".into(), placement: Placement::Side },
//!     |key, _event| ui::notify(&format!("{key} clicked"), ui::Level::Info),
//! )?;
//! let mut tree = Tree::new(WidgetKind::Column);
//! tree.add(Tree::ROOT, "refresh", ui::button("Refresh"));
//! ui::set_panel("todo.list", &tree)?;
//! ```

use crate::extension::kalem::plugin::ui as api;
use crate::kalem::{Disposable, HANDLERS, Local};

pub use api::{
    Alignment, Answer, Button, Checkbox, Input, Item, Label, Level, PanelEvent, PanelSpec,
    PickItem, PickOptions, Placement, Progress, PromptOptions, StatusOptions, TextStyle, Widget,
    WidgetKind, WidgetTree,
};

/// Shows `message` for a few seconds, and in the message log.
pub fn notify(message: &str, level: Level) {
    api::notify(message, level);
}

fn ask(request: u64, answered: impl FnOnce(Answer) + 'static) {
    HANDLERS.with(|h| h.borrow_mut().answers.insert(request, Box::new(answered)));
}

/// Asks for a line of text; `answered` gets it, or `None` when the user
/// cancels.
pub fn prompt(
    title: &str,
    options: &PromptOptions,
    answered: impl FnOnce(Option<String>) + 'static,
) {
    ask(api::prompt(title, options), move |a| {
        answered(match a {
            Answer::Text(t) => t,
            _ => None,
        })
    });
}

/// Asks yes or no; `answered` gets `false` when the user cancels.
pub fn confirm(message: &str, answered: impl FnOnce(bool) + 'static) {
    ask(api::confirm(message), move |a| {
        answered(matches!(a, Answer::Confirmed(true)))
    });
}

/// Offers `items`; `answered` gets the indices chosen, none when the user
/// cancels.
pub fn quick_pick(
    items: &[PickItem],
    options: &PickOptions,
    answered: impl FnOnce(Vec<u32>) + 'static,
) {
    ask(api::quick_pick(items, options), move |a| {
        answered(match a {
            Answer::Picked(p) => p,
            _ => Vec::new(),
        })
    });
}

/// Shows the plugin's status bar item `id`, or changes it.
pub fn status(id: &str, text: &str, options: &StatusOptions) -> Result<Disposable, String> {
    Ok(Disposable {
        host: api::status(id, text, options)?,
        local: Local::Other,
    })
}

/// A status bar item at the left, without a tooltip or a command.
pub fn status_options() -> StatusOptions {
    StatusOptions {
        tooltip: None,
        command: None,
        alignment: Alignment::Left,
        priority: 0,
    }
}

/// Adds a panel, empty until [`set_panel`]; `on` hears what the user does
/// to its widgets, by key.
pub fn panel(
    spec: PanelSpec,
    on: impl FnMut(&str, &PanelEvent) + 'static,
) -> Result<Disposable, String> {
    let id = spec.id.clone();
    let host = api::register_panel(&spec)?;
    HANDLERS.with(|h| h.borrow_mut().panels.insert(id.clone(), Some(Box::new(on))));
    Ok(Disposable {
        host,
        local: Local::Panel(id),
    })
}

/// Replaces panel `id`'s content.
pub fn set_panel(id: &str, tree: &Tree) -> Result<(), String> {
    api::set_panel(id, &tree.0)
}

/// A tree of widgets, built from its root down: each widget added under
/// one already there, so the tree is always one.
#[derive(Debug, Clone)]
pub struct Tree(WidgetTree);

impl Tree {
    /// The root's index.
    pub const ROOT: u32 = 0;

    /// A tree of its root alone.
    pub fn new(root: WidgetKind) -> Tree {
        Tree(WidgetTree {
            widgets: vec![Widget {
                key: String::new(),
                kind: root,
                children: Vec::new(),
            }],
        })
    }

    /// Adds a widget under `parent`, named `key` in its events; its index.
    pub fn add(&mut self, parent: u32, key: &str, kind: WidgetKind) -> u32 {
        let at = self.0.widgets.len() as u32;
        self.0.widgets.push(Widget {
            key: key.into(),
            kind,
            children: Vec::new(),
        });
        if let Some(p) = self.0.widgets.get_mut(parent as usize) {
            p.children.push(at);
        }
        at
    }

    /// The widgets, the root first.
    pub fn widgets(&self) -> &[Widget] {
        &self.0.widgets
    }
}

/// A plain text.
pub fn label(text: &str) -> WidgetKind {
    WidgetKind::Label(Label {
        text: text.into(),
        style: TextStyle::Normal,
    })
}

/// A button whose clicks the plugin hears.
pub fn button(label: &str) -> WidgetKind {
    WidgetKind::Button(Button {
        label: label.into(),
        command: None,
    })
}

/// An entry of a list.
pub fn item(label: &str) -> WidgetKind {
    WidgetKind::Item(Item {
        label: label.into(),
        detail: None,
        expanded: None,
        selected: false,
    })
}
