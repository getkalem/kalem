//! The Vim profile in the graphical editor: keys go to `kalem_core::vim`
//! first, and what it asks for (commands, messages, marks) happens here.

use gpui::{ClipboardItem, Context, KeyDownEvent, Keystroke, Window};
use kalem_core::vim::{Host, Key, Vim};

use crate::editor::Editor;

/// A key for the Vim layer.
pub fn key(k: &Keystroke) -> Key {
    let m = k.modifiers;
    if m.platform || m.alt || m.function {
        return Key::Other;
    }
    let one = |s: &str| {
        let mut c = s.chars();
        match (c.next(), c.next()) {
            (Some(ch), None) => Some(ch),
            _ => None,
        }
    };
    if m.control {
        return match k.key.as_str() {
            "escape" => Key::Esc,
            other => one(other).map_or(Key::Other, |c| Key::Ctrl(c.to_ascii_lowercase())),
        };
    }
    match k.key.as_str() {
        "escape" => Key::Esc,
        "enter" => Key::Enter,
        "backspace" => Key::Backspace,
        "tab" => Key::Tab,
        "left" => Key::Left,
        "right" => Key::Right,
        "up" => Key::Up,
        "down" => Key::Down,
        "space" => Key::Char(' '),
        other => {
            let typed = k.key_char.as_deref().and_then(one);
            match (typed, one(other)) {
                (Some(c), _) => Key::Char(c),
                (None, Some(c)) if m.shift => Key::Char(c.to_uppercase().next().unwrap_or(c)),
                (None, Some(c)) => Key::Char(c),
                _ => Key::Other,
            }
        }
    }
}

struct GuiHost<'a, 'b> {
    cx: &'a mut Context<'b, Editor>,
    lines: usize,
    rich: bool,
}

impl Host for GuiHost<'_, '_> {
    fn clipboard(&mut self) -> Option<String> {
        self.cx.read_from_clipboard().and_then(|i| i.text())
    }

    fn set_clipboard(&mut self, text: &str) {
        self.cx
            .write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    }

    fn page_lines(&self) -> usize {
        self.lines
    }

    fn rich_view(&self) -> bool {
        self.rich
    }
}

impl Editor {
    /// Starts or stops the Vim layer as the settings say.
    pub fn refresh_vim(&mut self) {
        let config = &self.shared.config;
        let on = config.keymap_profile() == kalem_core::Profile::Vim
            && Vim::applies(&config.strings("editor.vim.modes"), &self.doc.meta.mode);
        match (on, self.vim.is_some()) {
            (true, false) => {
                let mut v = Vim::new();
                v.leader = Vim::leader_key(&config.vim_leader());
                self.vim = Some(v);
            }
            (false, true) => self.vim = None,
            _ => {}
        }
    }

    /// A key for the Vim layer; `true` if it used it.
    pub fn vim_key_down(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(mut v) = self.vim.take() else {
            return false;
        };
        let k = key(&ev.keystroke);
        if v.takes_text() && !matches!(k, Key::Esc | Key::Ctrl('[')) {
            self.vim = Some(v);
            return false;
        }
        let lines = (self.list.viewport_bounds().size.height / gpui::px(self.theme.size * 1.45))
            .floor()
            .max(4.) as usize;
        let out = {
            let rich = !self.source;
            let mut host = GuiHost { cx, lines, rich };
            v.key(&mut self.doc, k, &mut host)
        };
        self.vim = Some(v);
        if !out.handled {
            return false;
        }
        if let Some(h) = out.highlights {
            self.highlights = h;
        }
        if let Some((m, error)) = out.message {
            self.status = Some((m, error));
        } else if !out.commands.is_empty() {
            self.status = None;
        }
        self.goal_x = None;
        self.after_change(cx);
        for (id, args) in out.commands {
            self.run_command(&id, args, window, cx);
        }
        if out.force_quit {
            cx.quit();
        }
        true
    }
}
