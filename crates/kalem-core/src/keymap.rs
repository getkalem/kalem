//! Keymaps (design §7.3, §11.7): key bindings from the commands' default
//! keys, a profile (Word-like or Vim) and the user's `keymap.json`, looked
//! up against when-clauses, with a report of bindings that can never
//! apply, and variants for terminals that cannot send every chord.
//!
//! A keymap file is a JSON array (comments allowed) of entries:
//!
//! ```json
//! [
//!   { "keys": "ctrl+shift+w", "command": "org.tags.set", "when": "onHeadline",
//!     "args": { "tags": ["work"] }, "terminalKeys": "alt+w" },
//!   { "keys": "ctrl+b", "command": "-org.emphasis.bold" }
//! ]
//! ```
//!
//! A command with a leading `-` removes bindings of that command (with
//! these keys, or all of them without `keys`). Where several bindings of
//! the same keys apply, the last one wins: defaults, then the profile, then
//! the user's entries.

use std::fmt;

use serde_json::Value;

use crate::command::CommandRegistry;
use crate::keys::KeySequence;
use crate::when::{Context, WhenClause};

/// A built-in keymap profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Word-like keys, the default.
    Word,
    /// Vim's modal keys (`kalem_core::vim`) and Doom Emacs's leader keys;
    /// the Word-like keys only where the Vim layer is off (the file
    /// manager) and those Doom keeps (Meta with the arrows or Enter).
    Vim,
}

impl Profile {
    /// The profile named in settings (`keymap_profile = "word"`).
    pub fn from_name(name: &str) -> Option<Profile> {
        match name {
            "word" => Some(Profile::Word),
            "vim" => Some(Profile::Vim),
            _ => None,
        }
    }

    /// The profile's keymap files, in order.
    fn files(self) -> &'static [&'static str] {
        const WORD: &str = include_str!("../keymaps/word.json");
        const VIM: &str = include_str!("../keymaps/vim.json");
        match self {
            Profile::Word => &[WORD],
            Profile::Vim => &[WORD, VIM],
        }
    }
}

/// The leader key of Vim's leader bindings (`editor.vim.leader`), unless
/// changed.
pub const DEFAULT_LEADER: &str = "space";

/// `keys` with every `leader` (or `<leader>`) chord replaced by `leader`.
fn with_leader(keys: &str, leader: &str) -> String {
    keys.split_whitespace()
        .map(|k| {
            if k.eq_ignore_ascii_case("leader") || k.eq_ignore_ascii_case("<leader>") {
                leader
            } else {
                k
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where a binding comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// A command's default keys.
    Default,
    /// The keymap profile.
    Profile,
    /// The user's `keymap.json`.
    User,
}

/// A key binding.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    /// The keys.
    pub keys: KeySequence,
    /// The command ID.
    pub command: String,
    /// The command's arguments (`null` for none).
    pub args: Value,
    /// When the binding applies (the command's own when-clause applies
    /// too).
    pub when: Option<WhenClause>,
    /// Keys to use in terminals that cannot send `keys`.
    pub terminal_keys: Option<KeySequence>,
    /// Where it comes from.
    pub origin: Origin,
}

/// An entry of a keymap file.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// A binding.
    Add(Binding),
    /// `-command`: removes the command's bindings, those with `keys` only
    /// if given.
    Remove {
        /// The command ID.
        command: String,
        /// The keys.
        keys: Option<KeySequence>,
    },
}

/// The kinds of keymap problems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    /// An entry that could not be read; it is skipped.
    Invalid,
    /// A binding of a command that does not exist; it is skipped.
    UnknownCommand,
    /// A binding that never applies, because a later one with the same keys
    /// applies in the same context.
    Shadowed,
    /// A binding that never applies, because longer bindings start with its
    /// keys and wait for the next key.
    PrefixShadowed,
    /// A binding the terminal cannot send and that has no variant.
    NoTerminalKey,
    /// A binding whose when-clause names a text type Kalem does not know
    /// (`textType == pyhton`); it never applies there.
    UnknownTextType,
}

/// A keymap problem.
#[derive(Debug, Clone, PartialEq)]
pub struct KeymapIssue {
    /// What kind.
    pub kind: IssueKind,
    /// The keys involved, if known.
    pub keys: Option<KeySequence>,
    /// A description for the user.
    pub message: String,
}

impl fmt::Display for KeymapIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// Removes `//` and `/* */` comments outside strings (keymap files are
/// JSON with comments).
pub fn strip_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut start) = (0, 0);
    let mut in_string = false;
    while i < b.len() {
        if in_string {
            match b[i] {
                b'\\' => i += 1,
                b'"' => in_string = false,
                _ => {}
            }
            i += 1;
            continue;
        }
        match (b[i], b.get(i + 1)) {
            (b'"', _) => {
                in_string = true;
                i += 1;
            }
            (b'/', Some(b'/')) => {
                out.push_str(&text[start..i]);
                i = text[i..].find('\n').map_or(b.len(), |n| i + n);
                start = i;
            }
            (b'/', Some(b'*')) => {
                out.push_str(&text[start..i]);
                i = text[i + 2..].find("*/").map_or(b.len(), |n| i + 2 + n + 2);
                // Keep line numbers of serde_json's errors.
                out.push_str(&"\n".repeat(text[start..i].matches('\n').count()));
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&text[start..]);
    out
}

/// Reads a keymap file. Entries with errors are skipped and reported.
pub fn parse_keymap(text: &str, origin: Origin) -> (Vec<Entry>, Vec<KeymapIssue>) {
    parse_keymap_with(text, origin, DEFAULT_LEADER)
}

/// Reads a keymap file whose `leader` chords stand for `leader`.
pub fn parse_keymap_with(
    text: &str,
    origin: Origin,
    leader: &str,
) -> (Vec<Entry>, Vec<KeymapIssue>) {
    let mut issues = Vec::new();
    let invalid = |message: String| KeymapIssue {
        kind: IssueKind::Invalid,
        keys: None,
        message,
    };
    let items = match serde_json::from_str::<Value>(&strip_comments(text)) {
        Ok(Value::Array(items)) => items,
        Ok(_) => {
            return (
                Vec::new(),
                vec![invalid("A keymap is a JSON array of bindings".into())],
            );
        }
        Err(e) => {
            return (
                Vec::new(),
                vec![invalid(format!("The keymap is not valid JSON: {e}"))],
            );
        }
    };
    let mut entries = Vec::new();
    for (n, item) in items.iter().enumerate() {
        let n = n + 1;
        let field = |k: &str| item.get(k).and_then(Value::as_str);
        let Some(command) = field("command") else {
            issues.push(invalid(format!("Binding {n} has no command")));
            continue;
        };
        let keys_text = field("keys")
            .or_else(|| field("key"))
            .map(|k| with_leader(k, leader));
        let keys = match keys_text.as_deref().map(|k| (k, KeySequence::parse(k))) {
            Some((k, None)) => {
                issues.push(invalid(format!("Binding {n}: `{k}` is not a key")));
                continue;
            }
            Some((_, Some(k))) => Some(k),
            None => None,
        };
        if let Some(c) = command.strip_prefix('-') {
            entries.push(Entry::Remove {
                command: c.to_string(),
                keys,
            });
            continue;
        }
        let Some(keys) = keys else {
            issues.push(invalid(format!("Binding {n} ({command}) has no keys")));
            continue;
        };
        let when = match field("when").map(WhenClause::parse) {
            Some(Err(e)) => {
                issues.push(invalid(format!(
                    "Binding {n}: when-clause: {} at {}",
                    e.message, e.at
                )));
                continue;
            }
            Some(Ok(w)) => Some(w),
            None => None,
        };
        let terminal_text = field("terminalKeys").map(|k| with_leader(k, leader));
        let terminal_keys = match terminal_text.as_deref().map(|k| (k, KeySequence::parse(k))) {
            Some((k, None)) => {
                issues.push(invalid(format!("Binding {n}: `{k}` is not a key")));
                continue;
            }
            Some((_, k)) => k,
            None => None,
        };
        entries.push(Entry::Add(Binding {
            keys,
            command: command.to_string(),
            args: item.get("args").cloned().unwrap_or(Value::Null),
            when,
            terminal_keys,
            origin,
        }));
    }
    (entries, issues)
}

fn and(a: Option<WhenClause>, b: Option<WhenClause>) -> Option<WhenClause> {
    match (a, b) {
        (Some(a), Some(b)) => Some(WhenClause::And(Box::new(a), Box::new(b))),
        (a, b) => a.or(b),
    }
}

/// What pressed keys do.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup<'a> {
    /// Run this command with these arguments.
    Command {
        /// The command ID.
        command: &'a str,
        /// The arguments.
        args: &'a Value,
    },
    /// The keys start longer bindings: wait for the next key.
    Prefix,
    /// Nothing is bound.
    None,
}

#[derive(Debug, Clone)]
struct Active {
    binding: Binding,
    /// The binding's and the command's when-clauses.
    when: Option<WhenClause>,
}

/// The active key bindings.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    bindings: Vec<Active>,
}

impl Keymap {
    /// The keymap of a profile with the user's entries, and its problems.
    pub fn build(
        registry: &CommandRegistry,
        profile: Profile,
        user: &[Entry],
    ) -> (Keymap, Vec<KeymapIssue>) {
        Keymap::build_with(registry, profile, user, DEFAULT_LEADER)
    }

    /// [`Keymap::build`] with Vim's leader key.
    pub fn build_with(
        registry: &CommandRegistry,
        profile: Profile,
        user: &[Entry],
        leader: &str,
    ) -> (Keymap, Vec<KeymapIssue>) {
        let mut all: Vec<Binding> = Vec::new();
        for c in registry.commands() {
            for k in &c.default_keys {
                all.push(Binding {
                    keys: k.clone(),
                    command: c.id.clone(),
                    args: Value::Null,
                    when: None,
                    terminal_keys: None,
                    origin: Origin::Default,
                });
            }
        }
        // The plugins' bindings, as default keys (`crate::extensions`), to
        // the commands there are.
        all.extend(
            crate::extensions::bindings()
                .into_iter()
                .filter(|b| registry.get(&b.command).is_some()),
        );
        // In the Vim profile the Word-like keys with Control or Alt give
        // way to Vim's while the Vim layer is on (`vimActive`; Control
        // only where it is Vim's, `vimOwnsCtrl`, not Command on macOS):
        // the keys from the default keys and `word.json`, unless the Vim
        // profile's own file binds them again, as Doom keeps them.
        let mut word_like: Vec<bool> = vec![profile == Profile::Vim; all.len()];
        let mut profile_entries = Vec::new();
        for (i, file) in profile.files().iter().enumerate() {
            let (entries, profile_issues) = parse_keymap_with(file, Origin::Profile, leader);
            debug_assert!(
                profile_issues.is_empty() || leader != DEFAULT_LEADER,
                "{profile_issues:?}"
            );
            let word = profile == Profile::Vim && i + 1 < profile.files().len();
            profile_entries.extend(entries.into_iter().map(|e| (e, word)));
        }
        let user = user.iter().map(|e| (e.clone(), false));
        for (e, word) in profile_entries.into_iter().chain(user) {
            match e {
                Entry::Add(b) => {
                    let same = all.iter_mut().zip(word_like.iter_mut()).find(|(a, _)| {
                        a.keys == b.keys
                            && a.command == b.command
                            && a.args == b.args
                            && a.when == b.when
                    });
                    match same {
                        Some((a, w)) => {
                            if b.terminal_keys.is_some() {
                                a.terminal_keys = b.terminal_keys.clone();
                            }
                            *w &= word;
                        }
                        None => {
                            all.push(b);
                            word_like.push(word);
                        }
                    }
                }
                Entry::Remove { command, keys } => {
                    let keep: Vec<bool> = all
                        .iter()
                        .map(|a| {
                            !(a.command == command && keys.as_ref().is_none_or(|k| &a.keys == k))
                        })
                        .collect();
                    let mut k = keep.iter();
                    all.retain(|_| *k.next().unwrap_or(&true));
                    let mut k = keep.iter();
                    word_like.retain(|_| *k.next().unwrap_or(&true));
                }
            }
        }
        for (b, word) in all.iter_mut().zip(&word_like) {
            let Some(first) = b.keys.0.first() else {
                continue;
            };
            // What Doom keeps under Evil: the Meta arrows (Org's, Markdown's,
            // the tables' structure moves), Control or Meta with Enter, and
            // completion on Control+Space.
            let m = first.mods;
            let arrow = matches!(first.key.as_str(), "up" | "down" | "left" | "right");
            let doom = (m.alt && !m.ctrl && arrow)
                || (first.key == "enter" && (m.alt || m.ctrl))
                || (m.ctrl && !m.alt && !m.shift && first.key == "space");
            if doom {
                continue;
            }
            let flag = if first.mods.ctrl {
                "vimOwnsCtrl"
            } else if first.mods.alt {
                "vimActive"
            } else {
                continue;
            };
            if *word {
                let off = WhenClause::Not(Box::new(WhenClause::Key(flag.into())));
                b.when = Some(match b.when.take() {
                    Some(w) => WhenClause::And(Box::new(off), Box::new(w)),
                    None => off,
                });
            }
        }
        let mut issues = Vec::new();
        let mut bindings = Vec::new();
        for b in all {
            for t in b.when.iter().flat_map(|w| w.values("textType")) {
                if !crate::command::known_text_type(&t) {
                    issues.push(KeymapIssue {
                        kind: IssueKind::UnknownTextType,
                        message: format!(
                            "`{}` is bound for the text type `{t}`, which Kalem does not know",
                            b.keys
                        ),
                        keys: Some(b.keys.clone()),
                    });
                }
            }
            match registry.get(&b.command) {
                Some(c) => bindings.push(Active {
                    when: and(b.when.clone(), c.when.clone()),
                    binding: b,
                }),
                None => issues.push(KeymapIssue {
                    kind: IssueKind::UnknownCommand,
                    message: format!(
                        "`{}` is bound to `{}`, which does not exist",
                        b.keys, b.command
                    ),
                    keys: Some(b.keys),
                }),
            }
        }
        let keymap = Keymap { bindings };
        issues.extend(keymap.conflicts());
        for i in &issues {
            tracing::info!(kind = ?i.kind, "keymap: {}", i.message);
        }
        (keymap, issues)
    }

    /// Bindings that never apply.
    fn conflicts(&self) -> Vec<KeymapIssue> {
        let mut issues = Vec::new();
        for (i, a) in self.bindings.iter().enumerate() {
            let (ak, ac) = (&a.binding.keys, &a.binding.command);
            if let Some(b) = self.bindings[i + 1..]
                .iter()
                .find(|b| b.binding.keys == *ak && b.when == a.when)
            {
                issues.push(KeymapIssue {
                    kind: IssueKind::Shadowed,
                    keys: Some(ak.clone()),
                    message: format!(
                        "`{ak}` runs `{}`, so its binding to `{ac}` never applies",
                        b.binding.command
                    ),
                });
            } else if let Some(b) = self
                .bindings
                .iter()
                .find(|b| b.binding.keys.starts_with(ak) && (b.when.is_none() || b.when == a.when))
            {
                issues.push(KeymapIssue {
                    kind: IssueKind::PrefixShadowed,
                    keys: Some(ak.clone()),
                    message: format!(
                        "`{}` starts with `{ak}`, so its binding to `{ac}` never applies",
                        b.binding.keys
                    ),
                });
            }
        }
        issues
    }

    /// What `pressed` does in `ctx`.
    pub fn lookup(&self, pressed: &KeySequence, ctx: &Context) -> Lookup<'_> {
        let applies = |a: &&Active| a.when.as_ref().is_none_or(|w| w.eval(ctx));
        if self
            .bindings
            .iter()
            .filter(applies)
            .any(|a| a.binding.keys.starts_with(pressed))
        {
            return Lookup::Prefix;
        }
        match self
            .bindings
            .iter()
            .rev()
            .filter(applies)
            .find(|a| a.binding.keys == *pressed)
        {
            Some(a) => Lookup::Command {
                command: &a.binding.command,
                args: &a.binding.args,
            },
            None => Lookup::None,
        }
    }

    /// Whether a binding written for the Vim layer (its when-clause looks
    /// at `vimCommand`) applies to `pressed`, or starts with it, in `ctx`:
    /// such keys go to the keymap before Vim, as the leader does.
    pub fn vim_bound(&self, pressed: &KeySequence, ctx: &Context) -> bool {
        self.bindings.iter().any(|a| {
            a.binding
                .when
                .as_ref()
                .is_some_and(|w| w.mentions("vimCommand"))
                && (a.binding.keys == *pressed || a.binding.keys.starts_with(pressed))
                && a.when.as_ref().is_none_or(|w| w.eval(ctx))
        })
    }

    /// What may follow `pressed` in `ctx`: each next key with the command
    /// it runs, or `None` for a group of longer bindings (with the
    /// commands in it), in the order of the bindings.
    pub fn continuations(
        &self,
        pressed: &KeySequence,
        ctx: &Context,
    ) -> Vec<(crate::keys::KeyChord, Option<String>, Vec<String>)> {
        let mut out: Vec<(crate::keys::KeyChord, Option<String>, Vec<String>)> = Vec::new();
        for a in self.bindings.iter().rev() {
            if !a.when.as_ref().is_none_or(|w| w.eval(ctx)) || !a.binding.keys.starts_with(pressed)
            {
                continue;
            }
            let next = a.binding.keys.0[pressed.0.len()].clone();
            let last = a.binding.keys.0.len() == pressed.0.len() + 1;
            match out.iter_mut().find(|o| o.0 == next) {
                Some(o) => {
                    if o.1.is_none() && !last {
                        o.2.push(a.binding.command.clone());
                    }
                }
                None => out.push((
                    next,
                    last.then(|| a.binding.command.clone()),
                    if last {
                        Vec::new()
                    } else {
                        vec![a.binding.command.clone()]
                    },
                )),
            }
        }
        out.reverse();
        out
    }

    /// A which-key line for `pressed`: `p Switch Project · f Find File…`,
    /// groups as `+Project`.
    pub fn hint(&self, registry: &CommandRegistry, pressed: &KeySequence, ctx: &Context) -> String {
        let parts: Vec<String> = self
            .which_key(registry, pressed, ctx)
            .into_iter()
            .map(|(k, l)| format!("{k} {l}"))
            .collect();
        format!("{pressed} -   {}", parts.join("  ·  "))
    }

    /// What may follow `pressed`, for a which-key panel: each next key and
    /// its command's title, or `+Category` for a group of longer bindings.
    pub fn which_key(
        &self,
        registry: &CommandRegistry,
        pressed: &KeySequence,
        ctx: &Context,
    ) -> Vec<(String, String)> {
        let mut parts = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (key, command, group) in self.continuations(pressed, ctx) {
            if !seen.insert(key.clone()) {
                continue;
            }
            let label = match command {
                Some(c) => registry.get(&c).map_or(c.clone(), |c| c.display_title()),
                None => {
                    // The category most of the group's commands share.
                    let mut counts: Vec<(String, usize)> = Vec::new();
                    for c in group.iter().filter_map(|c| registry.get(c)) {
                        let cat = c.display_category();
                        match counts.iter_mut().find(|x| x.0 == cat) {
                            Some(x) => x.1 += 1,
                            None => counts.push((cat, 1)),
                        }
                    }
                    let best = counts.iter().max_by_key(|x| x.1).map(|x| x.0.clone());
                    format!("+{}", best.unwrap_or_default())
                }
            };
            parts.push((key.to_string(), label));
        }
        // After the leader: Doom's group names, and its keys Kalem does
        // not bind yet, marked (T2.7i.19).
        let leader = pressed
            .0
            .first()
            .is_some_and(|c| c.key == "space" && c.mods == crate::keys::Modifiers::default());
        if leader {
            if pressed.0.len() == 1 {
                for (k, label) in parts.iter_mut() {
                    if label.starts_with('+')
                        && let Some(name) = doom_group(k)
                    {
                        *label = format!("+{name}");
                    }
                }
            }
            for row in crate::key_tables::doom_leader() {
                let (Some(reason), Some(keys)) = (&row.reason, KeySequence::parse(&row.keys))
                else {
                    continue;
                };
                let later = ["Planned", "Not yet", "Waits", "Needs"]
                    .iter()
                    .any(|p| reason.starts_with(p));
                if !later || !keys.starts_with(pressed) || keys.0.len() <= pressed.0.len() {
                    continue;
                }
                let next = keys.0[pressed.0.len()].to_string();
                if !seen.insert(keys.0[pressed.0.len()].clone()) {
                    continue;
                }
                let label = if keys.0.len() > pressed.0.len() + 1 {
                    format!("+{}", doom_group(&next).unwrap_or("…"))
                } else if reason.starts_with("Needs") {
                    format!("{} ({})", row.what, crate::tr!("which-key-plugin"))
                } else {
                    format!("{} ({})", row.what, crate::tr!("which-key-later"))
                };
                parts.push((next, label));
            }
        }
        parts
    }

    /// The bindings, in order.
    pub fn bindings(&self) -> impl Iterator<Item = &Binding> {
        self.bindings.iter().map(|a| &a.binding)
    }

    /// The keys that run `command`, for menus and the palette.
    pub fn keys_for(&self, command: &str) -> Vec<&KeySequence> {
        self.bindings()
            .filter(|b| b.command == command)
            .map(|b| &b.keys)
            .collect()
    }

    /// The keymap for a terminal. Without an enhanced keyboard protocol,
    /// bindings the terminal cannot send use their terminal keys, or
    /// [`KeySequence::terminal_variant`] when no binding uses those keys;
    /// the others are dropped, and reported unless they are defaults whose
    /// command has a key the terminal sends.
    pub fn for_terminal(&self, enhanced: bool) -> (Keymap, Vec<KeymapIssue>) {
        if enhanced {
            return (self.clone(), Vec::new());
        }
        let taken = |k: &KeySequence| {
            self.bindings.iter().any(|a| {
                let o = &a.binding.keys;
                o == k || o.starts_with(k) || k.starts_with(o)
            })
        };
        let mut issues = Vec::new();
        let mut bindings = Vec::new();
        for a in &self.bindings {
            let keys = &a.binding.keys;
            let explicit = a
                .binding
                .terminal_keys
                .clone()
                .filter(KeySequence::terminal_safe);
            let variant = if keys.terminal_safe() {
                Some(keys.clone())
            } else {
                explicit.or_else(|| keys.terminal_variant().filter(|v| !taken(v)))
            };
            // A default key the terminal cannot send is not missed when the
            // command has another it can (Shift+Space and `g r`); a user's
            // key is reported.
            let covered = || {
                a.binding.origin == Origin::Default
                    && self.bindings.iter().any(|b| {
                        b.binding.command == a.binding.command
                            && b.binding.args == a.binding.args
                            && b.binding.keys.terminal_safe()
                    })
            };
            match variant {
                Some(k) => {
                    let mut a = a.clone();
                    a.binding.keys = k;
                    bindings.push(a);
                }
                None if covered() => {}
                None => issues.push(KeymapIssue {
                    kind: IssueKind::NoTerminalKey,
                    keys: Some(keys.clone()),
                    message: format!(
                        "`{keys}` ({}) cannot be typed in this terminal",
                        a.binding.command
                    ),
                }),
            }
        }
        (Keymap { bindings }, issues)
    }
}

/// Whether the keys after a sequence half-typed since `since` show now
/// (`keys.hints`, after `keys.hints_delay`), and if not, how long until
/// they do (`None`: never).
pub fn hints_due(
    config: &crate::settings::Config,
    since: Option<std::time::Instant>,
    now: std::time::Instant,
) -> Option<std::time::Duration> {
    let since = since?;
    if !config.bool("keys.hints") {
        return None;
    }
    let delay = std::time::Duration::from_millis(config.int("keys.hints_delay").max(0) as u64);
    Some(delay.saturating_sub(now.saturating_duration_since(since)))
}

/// Doom Emacs's name for the group of leader keys after `key`.
fn doom_group(key: &str) -> Option<&'static str> {
    Some(match key {
        "b" => "buffer",
        "f" => "file",
        "s" => "search",
        "w" => "window",
        "p" => "project",
        "t" => "toggle",
        "o" => "open",
        "h" => "help",
        "c" => "code",
        "g" => "git",
        "n" => "notes",
        "i" => "insert",
        "q" => "quit",
        "tab" => "workspace",
        "m" => "localleader",
        "r" => "remote",
        "a" => "actions",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::when::Value as V;

    fn org_ctx(flags: &[&str]) -> Context {
        let mut c = Context::default();
        c.set("editorMode", V::Str("org".into()));
        c.set("textType", V::Str("org".into()));
        for f in flags {
            c.flag(f, true);
        }
        c
    }

    fn keys(k: &str) -> KeySequence {
        KeySequence::parse(k).unwrap()
    }

    fn run<'a>(m: &'a Keymap, k: &str, ctx: &Context) -> Option<(&'a str, &'a Value)> {
        match m.lookup(&keys(k), ctx) {
            Lookup::Command { command, args } => Some((command, args)),
            _ => None,
        }
    }

    #[test]
    fn unknown_text_types() {
        let reg = CommandRegistry::with_builtins();
        let json = r#"[{ "keys": "ctrl+alt+x", "command": "edit.undo", "when": "textType == pyhton" },
                       { "keys": "ctrl+alt+y", "command": "edit.undo", "when": "textType == python" }]"#;
        let (entries, _) = parse_keymap(json, Origin::User);
        let (_, issues) = Keymap::build(&reg, Profile::Word, &entries);
        let unknown: Vec<_> = issues
            .iter()
            .filter(|i| i.kind == IssueKind::UnknownTextType)
            .collect();
        assert_eq!(unknown.len(), 1, "{issues:#?}");
        assert!(unknown[0].message.contains("pyhton"));
    }

    #[test]
    fn profiles_have_no_issues() {
        let reg = CommandRegistry::with_builtins();
        for p in [Profile::Word, Profile::Vim] {
            let (_, issues) = Keymap::build(&reg, p, &[]);
            assert!(issues.is_empty(), "{p:?}: {issues:#?}");
        }
    }

    /// Typing text never runs a command: no binding of a printable key
    /// without Control, Alt or Command applies in a document, outside
    /// Vim's normal mode (file manager keys are for listings only).
    #[test]
    fn which_key_after_the_leader() {
        let reg = CommandRegistry::with_builtins();
        let (m, _) = Keymap::build(&reg, Profile::Vim, &[]);
        let ctx = org_ctx(&["hasFile", "inProject", "vimCommand"]);
        let items = m.which_key(&reg, &keys("space"), &ctx);
        let label = |k: &str| items.iter().find(|i| i.0 == k).map(|i| i.1.clone());
        // Doom's group names.
        assert_eq!(label("f").as_deref(), Some("+file"));
        assert_eq!(label("b").as_deref(), Some("+buffer"));
        // A Doom key Kalem does not bind yet, marked.
        assert_eq!(label("shift+x").as_deref(), Some("Capture (later)"));
    }

    #[test]
    fn hints_wait_their_delay() {
        let config = crate::settings::Config::default();
        let t = std::time::Instant::now();
        let ms = std::time::Duration::from_millis;
        assert_eq!(hints_due(&config, None, t), None);
        assert_eq!(hints_due(&config, Some(t), t), Some(ms(400)));
        assert_eq!(hints_due(&config, Some(t), t + ms(500)), Some(ms(0)));
        let config = crate::settings::Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "[keys]\nhints = false\n",
        )]);
        assert_eq!(hints_due(&config, Some(t), t + ms(500)), None);
    }

    #[test]
    fn typed_characters_are_text() {
        let reg = CommandRegistry::with_builtins();
        let emacs = parse_keymap(
            include_str!("../../../docs/keymaps/emacs.json"),
            Origin::User,
        )
        .0;
        for (name, profile, user) in [
            ("word", Profile::Word, Vec::new()),
            ("vim", Profile::Vim, Vec::new()),
            ("emacs", Profile::Word, emacs),
        ] {
            let (m, _) = Keymap::build(&reg, profile, &user);
            for mode in ["org", "markdown", "latex", "text", "csv"] {
                let mut ctx = org_ctx(&["hasFile", "inProject"]);
                ctx.set("editorMode", V::Str(mode.into()));
                ctx.set("textType", V::Str(mode.into()));
                ctx.set("vimMode", V::Str("insert".into()));
                for b in m.bindings() {
                    let first = &b.keys.0[0];
                    let plain = !first.mods.ctrl && !first.mods.alt && !first.mods.cmd;
                    if !plain || first.key.chars().count() != 1 {
                        continue;
                    }
                    let seq = KeySequence(vec![first.clone()]);
                    assert!(
                        matches!(m.lookup(&seq, &ctx), Lookup::None),
                        "{name}, {mode}: typing {} runs {:?}",
                        seq,
                        m.lookup(&seq, &ctx)
                    );
                }
            }
        }
    }

    #[test]
    fn word_profile() {
        let reg = CommandRegistry::with_builtins();
        let (m, _) = Keymap::build(&reg, Profile::Word, &[]);
        let plain = org_ctx(&[]);
        assert_eq!(run(&m, "ctrl+b", &plain).unwrap().0, "org.emphasis.bold");
        assert_eq!(run(&m, "ctrl+2", &plain).unwrap().1["level"], 2);
        // Context decides between commands on the same keys.
        assert_eq!(
            run(&m, "alt+up", &org_ctx(&["onHeadline"])).unwrap().0,
            "org.headline.moveSubtreeUp"
        );
        assert_eq!(
            run(&m, "alt+up", &org_ctx(&["inList"])).unwrap().0,
            "list.moveUp"
        );
        assert_eq!(
            run(&m, "alt+up", &org_ctx(&["inList", "inTable"]))
                .unwrap()
                .0,
            "table.moveRowUp"
        );
        assert!(run(&m, "alt+up", &plain).is_none());
        assert!(run(&m, "ctrl+0", &plain).is_none());
        // Org commands need an Org document.
        let mut md = Context::default();
        md.set("editorMode", V::Str("markdown".into()));
        assert!(run(&m, "ctrl+b", &md).is_none());
        assert_eq!(run(&m, "ctrl+z", &md).unwrap().0, "edit.undo");
        assert!(
            m.keys_for("org.todo.cycle")
                .iter()
                .any(|k| k.to_string() == "ctrl+enter")
        );
    }

    #[test]
    fn csv_grid_has_the_table_keys() {
        // The keys of Org tables move and insert rows and columns in a
        // CSV grid too, in the Word-like profile and the Emacs keymap.
        let reg = CommandRegistry::with_builtins();
        let mut csv = Context::default();
        csv.set("editorMode", V::Str("csv".into()));
        csv.set("textType", V::Str("csv".into()));
        let (m, _) = Keymap::build(&reg, Profile::Word, &[]);
        for (k, c) in [
            ("alt+up", "csv.moveRowUp"),
            ("alt+down", "csv.moveRowDown"),
            ("alt+left", "csv.moveColumnLeft"),
            ("alt+right", "csv.moveColumnRight"),
            ("alt+shift+up", "csv.deleteRow"),
            ("alt+shift+down", "csv.insertRow"),
            ("alt+shift+left", "csv.deleteColumn"),
            ("alt+shift+right", "csv.insertColumn"),
        ] {
            assert_eq!(run(&m, k, &csv).map(|r| r.0), Some(c), "{k}");
        }
        let text = include_str!("../../../docs/keymaps/emacs.json");
        let (entries, _) = parse_keymap(text, Origin::User);
        let (m, _) = Keymap::build(&reg, Profile::Word, &entries);
        assert_eq!(run(&m, "M-<up>", &csv).unwrap().0, "csv.moveRowUp");
        assert_eq!(run(&m, "C-c ^", &csv).unwrap().0, "csv.sortFileBy");
        assert_eq!(run(&m, "C-c +", &csv).unwrap().0, "csv.sumColumn");
        assert_eq!(run(&m, "C-c ?", &csv).unwrap().0, "csv.cellCoordinates");
        assert_eq!(run(&m, "C-c }", &csv).unwrap().0, "csv.toggleCoordinates");
        // Org's own table keys are untouched.
        let ctx = org_ctx(&["inTable"]);
        assert_eq!(run(&m, "C-c ^", &ctx).unwrap().0, "table.sortRows");
    }

    #[test]
    fn markdown_tables_take_tab() {
        let reg = CommandRegistry::with_builtins();
        let (m, _) = Keymap::build(&reg, Profile::Word, &[]);
        let mut md = Context::default();
        md.set("editorMode", V::Str("markdown".into()));
        md.set("textType", V::Str("markdown".into()));
        let outside = run(&m, "tab", &md).map(|r| r.0);
        assert_ne!(outside, Some("markdown.table.nextField"));
        md.flag("inMarkdownTable", true);
        assert_eq!(run(&m, "tab", &md).unwrap().0, "markdown.table.nextField");
        assert_eq!(
            run(&m, "shift+tab", &md).unwrap().0,
            "markdown.table.previousField"
        );
        assert_eq!(
            run(&m, "ctrl+shift+c", &md).unwrap().0,
            "markdown.toggleCheckbox"
        );
        // Org's table keys move a table's rows and columns, the lines
        // elsewhere.
        assert_eq!(
            run(&m, "alt+up", &md).unwrap().0,
            "markdown.table.moveRowUp"
        );
        assert_eq!(
            run(&m, "alt+shift+right", &md).unwrap().0,
            "markdown.table.insertColumn"
        );
        md.flag("inMarkdownTable", false);
        assert_eq!(run(&m, "alt+up", &md).unwrap().0, "lines.moveUp");
        md.flag("inMarkdownTable", true);
        // Enter continues a list only in one.
        md.flag("inMarkdownTable", false);
        assert_ne!(run(&m, "enter", &md).map(|r| r.0), Some("markdown.newline"));
        md.flag("inMarkdownList", true);
        assert_eq!(run(&m, "enter", &md).unwrap().0, "markdown.newline");
    }

    #[test]
    fn emacs_example() {
        // `docs/keymaps/emacs.json`, the Emacs Org keys as a user keymap.
        let reg = CommandRegistry::with_builtins();
        let text = include_str!("../../../docs/keymaps/emacs.json");
        let (entries, issues) = parse_keymap(text, Origin::User);
        assert!(issues.is_empty(), "{issues:#?}");
        let (m, issues) = Keymap::build(&reg, Profile::Word, &entries);
        assert!(issues.is_empty(), "{issues:#?}");
        let ctx = org_ctx(&["onHeadline"]);
        assert_eq!(m.lookup(&keys("C-c"), &ctx), Lookup::Prefix);
        assert_eq!(m.lookup(&keys("C-c C-x"), &ctx), Lookup::Prefix);
        assert_eq!(run(&m, "C-c C-t", &ctx).unwrap().0, "org.todo.cycle");
        assert_eq!(m.lookup(&keys("C-c C-z"), &ctx), Lookup::None);
        assert_eq!(run(&m, "C-x C-s", &ctx).unwrap().0, "app.save");
        // Word-like keys Emacs does not use stay.
        assert_eq!(run(&m, "ctrl+b", &ctx).unwrap().0, "org.emphasis.bold");
        assert_eq!(
            run(&m, "C-c C-c", &org_ctx(&["inTable"])).unwrap().0,
            "table.align"
        );
        assert_eq!(
            run(&m, "C-c C-c", &org_ctx(&["inList"])).unwrap().0,
            "list.toggleCheckbox"
        );
    }

    #[test]
    fn user_entries() {
        let reg = CommandRegistry::with_builtins();
        let json = r#"[
            // Bold elsewhere.
            { "keys": "ctrl+b", "command": "-org.emphasis.bold" },
            { "keys": "ctrl+shift+b", "command": "org.emphasis.bold" }, /* and */
            { "key": "ctrl+alt+t", "command": "org.todo.set", "args": { "state": "DONE" } },
            { "keys": "ctrl+q", "command": "no.such" },
            { "keys": "hyper+x", "command": "edit.undo" },
            { "keys": "ctrl+z", "command": "edit.redo" },
            { "keys": "ctrl+k", "command": "edit.undo", "when": "a &&" },
            { "keys": "ctrl+m ctrl+n", "command": "edit.undo" },
            { "keys": "ctrl+m", "command": "edit.redo" }
        ]"#;
        let (entries, issues) = parse_keymap(json, Origin::User);
        let kinds: Vec<IssueKind> = issues.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, [IssueKind::Invalid, IssueKind::Invalid]);
        let (m, issues) = Keymap::build(&reg, Profile::Word, &entries);
        let kinds: Vec<IssueKind> = issues.iter().map(|i| i.kind).collect();
        assert_eq!(
            kinds,
            [
                IssueKind::UnknownCommand,
                IssueKind::Shadowed,
                IssueKind::PrefixShadowed
            ],
            "{issues:#?}"
        );
        let ctx = org_ctx(&[]);
        assert!(run(&m, "ctrl+b", &ctx).is_none());
        assert_eq!(
            run(&m, "ctrl+shift+b", &ctx).unwrap().0,
            "org.emphasis.bold"
        );
        assert_eq!(run(&m, "ctrl+alt+t", &ctx).unwrap().1["state"], "DONE");
        assert_eq!(run(&m, "ctrl+z", &ctx).unwrap().0, "edit.redo");
    }

    #[test]
    fn terminals() {
        let reg = CommandRegistry::with_builtins();
        let (m, _) = Keymap::build(&reg, Profile::Word, &[]);
        let (t, issues) = m.for_terminal(false);
        assert!(issues.is_empty(), "{issues:#?}");
        let ctx = org_ctx(&[]);
        assert_eq!(run(&t, "ctrl+t", &ctx).unwrap().0, "org.todo.cycle");
        assert_eq!(run(&t, "alt+t", &ctx).unwrap().0, "table.create");
        assert_eq!(run(&t, "alt+i", &ctx).unwrap().0, "org.emphasis.italic");
        assert_eq!(run(&t, "alt+1", &ctx).unwrap().1["level"], 1);
        assert!(t.bindings().all(|b| b.keys.terminal_safe()));
        // The Emacs example in a terminal.
        let text = include_str!("../../../docs/keymaps/emacs.json");
        let (entries, _) = parse_keymap(text, Origin::User);
        let (o, _) = Keymap::build(&reg, Profile::Word, &entries);
        let (t, issues) = o.for_terminal(false);
        assert_eq!(run(&t, "C-_", &ctx).unwrap().0, "edit.undo");
        let dropped: Vec<String> = issues
            .iter()
            .map(|i| i.keys.as_ref().unwrap().to_string())
            .collect();
        // `C-/` becomes `C-_`, which is bound already; `C-?` is DEL,
        // Shift+Enter is Enter, and Ctrl with a comma is a comma (Emacs in a
        // terminal cannot get `C-c C-,` either).
        assert!(
            ["ctrl+?", "alt+shift+enter", "ctrl+c ctrl+,"]
                .iter()
                .all(|k| dropped.contains(&k.to_string())),
            "{dropped:?}"
        );
        assert_eq!(
            o.for_terminal(true).0.bindings().count(),
            o.bindings().count()
        );
    }
}
