//! The settings panel (`app.settings`, Ctrl+, or `SPC h v`), lazygit's
//! way: every setting in a framed list grouped by table, the chosen
//! one's description below, a key for each change
//! ([`kalem_core::settings_list`]).

use std::cell::RefCell;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

use kalem_core::l10n::tr;
use kalem_core::settings::{Config, SPECS};
use kalem_core::settings_list::{self, Browser, Edit, Line};

use crate::caps::Caps;
use crate::panels::{accent_style, panel_style, selected_style};

/// The open settings panel.
#[derive(Debug, Default)]
pub struct SettingsPanel {
    /// The list as browsed.
    pub list: Browser,
    /// Where each setting was drawn: its row and its index among those
    /// shown, for clicks.
    pub spots: RefCell<Vec<(u16, u16, u16, usize)>>,
}

impl SettingsPanel {
    /// The setting shown at column `x`, row `y`, as its index among those
    /// shown.
    pub fn at(&self, x: u16, y: u16) -> Option<usize> {
        self.spots
            .borrow()
            .iter()
            .find(|(row, left, right, _)| *row == y && (*left..*right).contains(&x))
            .map(|s| s.3)
    }

    /// Draws the panel in the middle of `area`.
    pub fn draw(&self, buf: &mut Buffer, area: Rect, config: &Config, caps: &Caps) {
        self.spots.borrow_mut().clear();
        if area.width < 24 || area.height < 8 {
            return;
        }
        let lines = self.list.lines();
        let shown = self.list.shown();
        let selected = self.list.selected.min(shown.len().saturating_sub(1));
        let filter_row = self.list.filtering || !self.list.filter.is_empty();
        // The frame, the filter, the list, a rule, the chosen setting's
        // key and three lines of its description.
        let fixed = 2 + u16::from(filter_row) + 5;
        let w = area.width.saturating_sub(4).min(88);
        let h = (lines.len().max(1) as u16 + fixed).min(area.height.saturating_sub(2));
        let x0 = area.x + (area.width - w) / 2;
        let y0 = area.y + 1;
        let bg = panel_style(caps);
        let dim = bg.add_modifier(Modifier::DIM);
        let accent = accent_style(caps, bg);
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                buf[(x, y)].set_symbol(" ").set_style(bg);
            }
        }
        // The frame, the title and count in the top line, the keys in
        // the bottom one.
        let (tl, tr_, bl, br, hz, vt, lt, rt) = if caps.ascii {
            ("+", "+", "+", "+", "-", "|", "+", "+")
        } else {
            ("┌", "┐", "└", "┘", "─", "│", "├", "┤")
        };
        let bottom = y0 + h - 1;
        for x in x0 + 1..x0 + w - 1 {
            buf[(x, y0)].set_symbol(hz).set_style(dim);
            buf[(x, bottom)].set_symbol(hz).set_style(dim);
        }
        for y in y0 + 1..bottom {
            buf[(x0, y)].set_symbol(vt).set_style(dim);
            buf[(x0 + w - 1, y)].set_symbol(vt).set_style(dim);
        }
        buf[(x0, y0)].set_symbol(tl).set_style(dim);
        buf[(x0 + w - 1, y0)].set_symbol(tr_).set_style(dim);
        buf[(x0, bottom)].set_symbol(bl).set_style(dim);
        buf[(x0 + w - 1, bottom)].set_symbol(br).set_style(dim);
        let inner = (w - 4) as usize;
        let title = format!(" {} ", tr("settings-title"));
        buf.set_stringn(
            x0 + 2,
            y0,
            &title,
            inner,
            accent.add_modifier(Modifier::BOLD),
        );
        let count = format!(" {}/{} ", (selected + 1).min(shown.len()), shown.len());
        if title.width() + count.width() + 4 < w as usize {
            buf.set_stringn(
                x0 + w - 2 - count.width() as u16,
                y0,
                &count,
                count.width(),
                dim,
            );
        }
        let hints = format!(" {} ", tr("settings-hints"));
        buf.set_stringn(x0 + 2, bottom, &hints, inner, dim);
        let mut y = y0 + 1;
        if filter_row {
            let caret = if self.list.filtering { "▏" } else { "" };
            let caret = if caps.ascii && !caret.is_empty() {
                "_"
            } else {
                caret
            };
            let text = format!("/ {}{caret}", self.list.filter);
            buf.set_stringn(x0 + 2, y, &text, inner, accent);
            y += 1;
        }
        // The list, scrolled so that the chosen setting shows.
        let rule_y = bottom - 5;
        let room = rule_y.saturating_sub(y) as usize;
        let at = lines
            .iter()
            .position(|l| matches!(l, Line::Setting(i) if shown.get(selected) == Some(i)))
            .unwrap_or(0);
        let first = (at + 1).saturating_sub(room);
        let names = lines
            .iter()
            .filter_map(|l| match l {
                Line::Setting(i) => Some(settings_list::name(SPECS[*i].key).width()),
                Line::Section(_) => None,
            })
            .max()
            .unwrap_or(0)
            .min(inner / 2);
        // The settings above the first line shown.
        let mut n = lines[..first]
            .iter()
            .filter(|l| matches!(l, Line::Setting(_)))
            .count();
        if lines.is_empty() {
            buf.set_stringn(x0 + 2, y, tr("settings-empty"), inner, dim);
        }
        for line in lines.iter().skip(first).take(room) {
            match line {
                Line::Section(name) => {
                    buf.set_stringn(x0 + 2, y, name, inner, accent.add_modifier(Modifier::BOLD));
                }
                Line::Setting(i) => {
                    let spec = &SPECS[*i];
                    let style = if n == selected {
                        selected_style(caps, bg)
                    } else {
                        bg
                    };
                    for x in x0 + 1..x0 + w - 1 {
                        buf[(x, y)].set_style(style);
                    }
                    let name = settings_list::name(spec.key);
                    buf.set_stringn(x0 + 4, y, name, names, style);
                    let value = settings_list::shown(config, spec);
                    let vx = x0 + 4 + names as u16 + 2;
                    let room = (x0 + w - 2).saturating_sub(vx) as usize;
                    let value_style = if settings_list::changed(config, spec) {
                        accent_style(caps, style).add_modifier(Modifier::BOLD)
                    } else {
                        style
                    };
                    buf.set_stringn(vx, y, &value, room, value_style);
                    self.spots.borrow_mut().push((y, x0 + 1, x0 + w - 1, n));
                    n += 1;
                }
            }
            y += 1;
        }
        // The chosen setting: its key, default and description.
        for x in x0 + 1..x0 + w - 1 {
            buf[(x, rule_y)].set_symbol(hz).set_style(dim);
        }
        buf[(x0, rule_y)].set_symbol(lt).set_style(dim);
        buf[(x0 + w - 1, rule_y)].set_symbol(rt).set_style(dim);
        if let Some(spec) = shown.get(selected).map(|&i| &SPECS[i]) {
            let about = match settings_list::edit(spec) {
                Edit::File => tr("settings-in-file"),
                _ => kalem_core::tr!(
                    "settings-default",
                    value = settings_list::shown_default(spec)
                ),
            };
            let head = format!("{}  {about}", spec.key);
            buf.set_stringn(x0 + 2, rule_y + 1, &head, inner, accent);
            for (dy, text) in (2..).zip(wrap(spec.description, inner, 3)) {
                buf.set_stringn(x0 + 2, rule_y + dy, &text, inner, bg);
            }
        }
    }
}

/// `text` in lines of at most `width` columns broken between words, at
/// most `most` of them, the last ending in `…` when text is left over.
fn wrap(text: &str, width: usize, most: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.width() + 1 + word.width() > width {
            if out.len() + 1 == most {
                while line.width() + 1 > width {
                    line.pop();
                }
                line.push('…');
                out.push(line);
                return out;
            }
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn descriptions_wrap_between_words() {
        assert_eq!(super::wrap("one two three", 7, 2), vec!["one two", "three"]);
        assert_eq!(
            super::wrap("one two three four five", 7, 2),
            vec!["one two", "three…"]
        );
        assert!(super::wrap("", 7, 2).is_empty());
    }
}
