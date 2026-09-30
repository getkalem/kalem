//! The command palette's items and fuzzy matching, shared by the
//! frontends.

use crate::command::CommandRegistry;
use crate::keymap::Keymap;
use crate::keys::KeySequence;
use crate::when::Context;

/// A command in the palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    /// The command.
    pub id: String,
    /// Its title.
    pub title: String,
    /// Its category.
    pub category: String,
    /// Its key, if bound.
    pub keys: String,
    /// More words it is found by: its ID and English title (`dired`
    /// finds the file manager in any language).
    pub also: String,
}

/// The commands that apply in `ctx`, with their first key written by
/// `show`.
pub fn items(
    registry: &CommandRegistry,
    keymap: &Keymap,
    ctx: &Context,
    show: impl Fn(&KeySequence) -> String,
) -> Vec<PaletteItem> {
    registry
        .commands()
        .filter(|c| c.when.as_ref().is_none_or(|w| w.eval(ctx)))
        .map(|c| PaletteItem {
            id: c.id.clone(),
            title: c.display_title(),
            category: c.display_category(),
            keys: keymap
                .keys_for(&c.id)
                .first()
                .map(|k| show(k))
                .unwrap_or_default(),
            also: format!("{} {}", c.id.replace('.', " "), c.title),
        })
        .collect()
}

/// The items of the menus (`crate::menus`) that apply in `ctx`, each
/// titled by its menu and its label, with its first key written by
/// `show`: the menus of the terminal editor (F10), and a way to reach
/// every menu item from the keyboard in the graphical one.
pub fn menu_items(
    registry: &CommandRegistry,
    keymap: &Keymap,
    ctx: &Context,
    show: impl Fn(&KeySequence) -> String,
) -> Vec<PaletteItem> {
    use crate::menus::MenuEntry;
    let mut out = Vec::new();
    for m in crate::menus::menus() {
        for e in m.entries {
            let (label, id, args) = match e {
                MenuEntry::Separator => continue,
                MenuEntry::Command { label, id, args } => (label, id, args),
                MenuEntry::Open(label) => (label, "file.open", None),
                MenuEntry::AddProjectFolder(label) => (label, "project.add", None),
            };
            if !registry.offered(id, ctx) {
                continue;
            }
            out.push(PaletteItem {
                id: args.as_ref().map_or(id.to_string(), |a| invocation(id, a)),
                title: format!("{} › {label}", m.name),
                category: m.name.clone(),
                keys: keymap
                    .keys_for(id)
                    .first()
                    .map(|k| show(k))
                    .unwrap_or_default(),
                also: id.replace('.', " "),
            });
        }
    }
    out
}

/// A fuzzy match of `query` in `text`: every query character in order,
/// ignoring case. Lower scores are better: early, contiguous matches and
/// matches at word starts.
pub fn fuzzy(query: &str, text: &str) -> Option<i64> {
    let t: Vec<char> = text.chars().flat_map(char::to_lowercase).collect();
    let mut score = 0i64;
    let mut at = 0usize;
    let mut last: Option<usize> = None;
    for q in query.chars().flat_map(char::to_lowercase) {
        if q == ' ' {
            continue;
        }
        let i = at + t[at..].iter().position(|c| *c == q)?;
        let word_start = i == 0 || !t[i - 1].is_alphanumeric();
        score += match last {
            Some(l) if l + 1 == i => 0,
            _ if word_start => 1,
            _ => 3 + (i - at) as i64,
        };
        last = Some(i);
        at = i + 1;
    }
    Some(score)
}

/// The items matching `input`, best first: by title, then by category and
/// title.
pub fn matches<'a>(items: &'a [PaletteItem], input: &str) -> Vec<&'a PaletteItem> {
    let mut scored: Vec<(i64, &PaletteItem)> = items
        .iter()
        .filter_map(|it| {
            let hay = format!("{} {}", it.category, it.title);
            fuzzy(input, &it.title)
                .or_else(|| fuzzy(input, &hay).map(|s| s + 10))
                .or_else(|| fuzzy(input, &it.also).map(|s| s + 20))
                // An ID typed with its dots (`org.todo.cycle`).
                .or_else(|| fuzzy(&input.replace('.', " "), &it.also).map(|s| s + 20))
                .map(|s| (s, it))
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.title.cmp(&b.1.title)));
    scored.into_iter().map(|(_, it)| it).collect()
}

/// A palette item's `id` that runs `command` with `args`: the command,
/// a space and the arguments as JSON ([`split_invocation`] reads it).
pub fn invocation(command: &str, args: &serde_json::Value) -> String {
    format!("{command} {args}")
}

/// The command and arguments of a palette item's `id`: a bare command
/// has none (`null`).
pub fn split_invocation(id: &str) -> (&str, serde_json::Value) {
    match id.split_once(' ') {
        Some((command, args)) => (
            command,
            serde_json::from_str(args).unwrap_or(serde_json::Value::Null),
        ),
        None => (id, serde_json::Value::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_matching() {
        assert!(
            fuzzy("sav", "Save").unwrap() < fuzzy("sav", "Insert Table And View").unwrap_or(99)
        );
        assert!(fuzzy("ctd", "Cycle TODO State").is_some());
        assert!(fuzzy("tc", "Cycle TODO State").is_none());
        assert!(fuzzy("zz", "Save").is_none());
        let items = [PaletteItem {
            id: "org.todo.cycle".into(),
            title: "Cycle TODO State".into(),
            category: "Org".into(),
            keys: String::new(),
            also: "org todo cycle".into(),
        }];
        assert_eq!(matches(&items, "org.todo.cycle").len(), 1);
        assert_eq!(fuzzy("", "Save"), Some(0));
    }
}
