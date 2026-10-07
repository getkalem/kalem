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
    /// The first document line shown.
    top: usize,
    /// A new first line asked for (CTRL-E, `zt`).
    scroll: Option<usize>,
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

    fn visible_lines(&self) -> Option<(usize, usize)> {
        let top = self.scroll.unwrap_or(self.top);
        Some((top, top + self.lines.saturating_sub(1)))
    }

    fn scroll(&mut self, by: isize, last: usize) -> Option<(usize, usize)> {
        let top = self.scroll.unwrap_or(self.top) as isize + by;
        self.scroll = Some(top.clamp(0, last as isize) as usize);
        self.visible_lines()
    }

    fn scroll_to(&mut self, line: usize, at: u8) {
        let above = match at {
            0 => 0,
            1 => self.lines / 2,
            _ => self.lines.saturating_sub(1),
        };
        self.scroll = Some(line.saturating_sub(above));
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
        self.doc.csv_vim = self.vim.is_some();
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
        // Insert mode's own keys go to the layer; typed characters and
        // keys it does not know go to the editor.
        if v.takes_text() && matches!(k, Key::Char(_) | Key::Other) {
            self.vim = Some(v);
            return false;
        }
        let lines = (self.list.viewport_bounds().size.height / gpui::px(self.theme.size * 1.45))
            .floor()
            .max(4.) as usize;
        let top = self
            .visible
            .get(self.list.logical_scroll_top().item_ix)
            .copied()
            .unwrap_or(0);
        let (out, scroll) = {
            let rich = !self.source;
            let mut host = GuiHost {
                cx,
                lines,
                rich,
                top,
                scroll: None,
            };
            let out = v.key(&mut self.doc, k, &mut host);
            (out, host.scroll)
        };
        // CTRL-E, CTRL-Y, `zt`, `zz`, `zb`: the view moves.
        if let Some(line) = scroll {
            let item_ix = self.visible.partition_point(|l| *l < line);
            self.list.scroll_to(gpui::ListOffset {
                item_ix,
                offset_in_item: gpui::px(0.),
            });
        }
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
        true
    }
}
