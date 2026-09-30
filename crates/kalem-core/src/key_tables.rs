//! Coverage tables of other editors' keys: for each key, the command it
//! runs in Kalem or the reason it does not (`tests/keys/`). Tests check
//! them against the keymaps; the Book shows them.

/// A key of another editor and what it does in Kalem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRow {
    /// The key as that editor writes it.
    pub theirs: String,
    /// The key in Kalem's notation.
    pub keys: String,
    /// The command it runs, when it runs one.
    pub command: Option<String>,
    /// Vim's own motion does it.
    pub vim: bool,
    /// Why it is not there.
    pub reason: Option<String>,
    /// What it does.
    pub what: String,
    /// The key of the same command without Vim keys.
    pub emacs: Option<String>,
}

/// Doom Emacs's file manager keys (T2.7e.18).
pub fn doom_dired() -> Vec<KeyRow> {
    parse(include_str!("../../../tests/keys/doom-dired.toml"), "doom")
}

/// Doom Emacs's leader map (T2.7i.1).
pub fn doom_leader() -> Vec<KeyRow> {
    parse(include_str!("../../../tests/keys/doom-leader.toml"), "doom")
}

/// The table called `name` (`doom-dired`, `doom-leader`).
pub fn table(name: &str) -> Option<Vec<KeyRow>> {
    match name {
        "doom-dired" => Some(doom_dired()),
        "doom-leader" => Some(doom_leader()),
        _ => None,
    }
}

fn parse(text: &str, theirs: &str) -> Vec<KeyRow> {
    let doc: toml_edit::DocumentMut = text.parse().expect("a key table");
    let Some(rows) = doc.get("key").and_then(|k| k.as_array_of_tables()) else {
        return Vec::new();
    };
    rows.iter()
        .map(|t| {
            let s = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
            KeyRow {
                theirs: s(theirs).unwrap_or_default(),
                keys: s("keys").unwrap_or_default(),
                command: s("command"),
                vim: t.get("vim").and_then(|v| v.as_bool()).unwrap_or(false),
                reason: s("reason"),
                what: s("what").unwrap_or_default(),
                emacs: s("emacs"),
            }
        })
        .collect()
}

/// A table as Org: the key, what it does, and the command or the reason.
pub fn org_table(rows: &[KeyRow], theirs: &str) -> String {
    let cell = |s: &str| s.replace('|', "\\vert{}");
    let code = |s: &str| format!("~{}~", cell(s));
    let mut out = format!("| {theirs} | What it does | In Kalem |\n|-\n");
    for r in rows {
        let kalem = match (&r.command, &r.reason) {
            (Some(c), _) => code(c),
            (None, Some(why)) => format!("/{}/", cell(why)),
            (None, None) => "Vim".to_string(),
        };
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            code(&r.theirs),
            cell(&r.what),
            kalem
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandRegistry;
    use crate::keymap::{Keymap, Lookup, Origin, Profile, parse_keymap};
    use crate::keys::KeySequence;
    use crate::when::{Context, Value as V};

    fn listing(vim: bool) -> Context {
        let mut c = Context::default();
        c.set("editorMode", V::Str("directory".into()));
        c.set("textType", V::Str("directory".into()));
        c.flag("vimCommand", vim);
        c.flag("wdired", false);
        c.flag("inProject", true);
        c
    }

    fn lookup<'a>(m: &'a Keymap, keys: &str, ctx: &Context) -> Option<&'a str> {
        let seq = KeySequence::parse(keys).unwrap_or_else(|| panic!("{keys}"));
        match m.lookup(&seq, ctx) {
            Lookup::Command { command, .. } => Some(command),
            _ => None,
        }
    }

    #[test]
    fn doom_leader_keys() {
        let rows = doom_leader();
        assert!(rows.len() > 250, "{}", rows.len());
        let reg = CommandRegistry::with_builtins();
        let (vim, _) = Keymap::build(&reg, Profile::Vim, &[]);
        // An Org document in a project.
        let mut ctx = Context::default();
        ctx.set("editorMode", V::Str("org".into()));
        ctx.set("textType", V::Str("org".into()));
        ctx.flag("vimCommand", true);
        ctx.flag("inProject", true);
        let mut seen = std::collections::HashSet::new();
        for r in &rows {
            let row = &r.theirs;
            assert!(seen.insert(&r.keys), "{row} twice");
            assert!(!r.what.is_empty(), "{row}");
            match (&r.command, &r.reason) {
                (Some(c), None) => {
                    assert_eq!(lookup(&vim, &r.keys, &ctx), Some(c.as_str()), "{row}")
                }
                (None, Some(why)) => assert!(!why.is_empty(), "{row}"),
                _ => panic!("{row}: a command or a reason"),
            }
        }
        // Every leader binding of the Vim keymap is in the table.
        let listed: std::collections::HashSet<String> =
            rows.iter().map(|r| r.keys.clone()).collect();
        for b in vim.bindings() {
            let keys = b.keys.to_string();
            let directory = b.when.as_ref().is_some_and(|w| w.mentions("editorMode"));
            if keys.starts_with("space ") && !directory {
                assert!(
                    listed.contains(&keys),
                    "{keys} is bound but not in the table"
                );
            }
        }
    }

    #[test]
    fn doom_dired_keys() {
        let rows = doom_dired();
        assert!(rows.len() > 50, "{}", rows.len());
        let reg = CommandRegistry::with_builtins();
        let (vim, issues) = Keymap::build(&reg, Profile::Vim, &[]);
        assert!(issues.is_empty(), "{issues:#?}");
        let (entries, _) = parse_keymap(
            include_str!("../../../docs/keymaps/emacs.json"),
            Origin::User,
        );
        let (emacs, _) = Keymap::build(&reg, Profile::Word, &entries);
        let (word, _) = Keymap::build(&reg, Profile::Word, &[]);
        for r in &rows {
            let row = &r.theirs;
            match (&r.command, r.vim, &r.reason) {
                (Some(c), false, None) => {
                    assert!(reg.get(c).is_some(), "{row}: no command {c}");
                    assert_eq!(
                        lookup(&vim, &r.keys, &listing(true)),
                        Some(c.as_str()),
                        "{row}"
                    );
                }
                (None, true, None) => {
                    // The keymap leaves it to Vim.
                    let first = r.keys.split(' ').next().unwrap();
                    let seq = KeySequence::parse(first).unwrap();
                    assert_eq!(vim.lookup(&seq, &listing(true)), Lookup::None, "{row}");
                }
                (None, false, Some(why)) => assert!(!why.is_empty(), "{row}"),
                _ => panic!("{row}: one of command, vim or reason"),
            }
            if let (Some(k), Some(c)) = (&r.emacs, &r.command) {
                for (name, m) in [("Word", &word), ("Emacs", &emacs)] {
                    // C-x C-q is the Emacs keymap's.
                    if name == "Word" && k.starts_with("ctrl+x") {
                        continue;
                    }
                    assert_eq!(
                        lookup(m, k, &listing(false)),
                        Some(c.as_str()),
                        "{row} ({name}: {k})"
                    );
                }
            }
        }
        let org = org_table(&rows, "Doom");
        assert!(org.contains("| ~% R~ | Rename by regular expression | ~dired.renameRegexp~ |"));
    }
}
