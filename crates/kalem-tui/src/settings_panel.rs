//! The settings panel (`app.settings`, Ctrl+, or `SPC h v`), lazygit's
//! way: every setting in a framed list grouped by table, the chosen
//! one's description below, a key for each change; a list's or a table's
//! items in the same frame ([`kalem_core::settings_list`]).

use std::cell::RefCell;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

use kalem_core::l10n::tr;
use kalem_core::settings::{Config, SPECS};
use kalem_core::settings_list::{self, Browser, Edit, Items, Line};

use crate::caps::Caps;
use crate::panels::{accent_style, panel_style, selected_style};

/// The open settings panel.
#[derive(Debug, Default)]
pub struct SettingsPanel {
    /// The list as browsed.
    pub list: Browser,
    /// Where each setting (or, its items shown, each item) was drawn: its
    /// row, its columns and its index, for clicks.
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
        let items_of = self
            .list
            .item
            .and_then(|_| shown.get(selected))
            .map(|&i| &SPECS[i])
            .and_then(|spec| Some((spec, settings_list::items_kind(spec)?)));
        // The frame, the filter, the list (or the setting's key and its
        // items), a rule, the chosen setting's key and three lines of its
        // description.
        let rows = match items_of {
            Some((spec, _)) => settings_list::items(config, spec).len().max(1) + 1,
            None => lines.len().max(1) + usize::from(filter_row),
        }
        .max(lines.len());
        let fixed = 2 + 5;
        let w = area.width.saturating_sub(4).min(88);
        let h = (rows as u16 + fixed).min(area.height.saturating_sub(2));
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
        let hints = match items_of.map(|(_, k)| k) {
            Some(Items::Choices(_)) => tr("settings-items-choices"),
            Some(Items::Texts) => tr("settings-items-texts"),
            Some(Items::Table(_)) => tr("settings-items-table"),
            None => tr("settings-hints"),
        };
        let hints = format!(" {hints} ");
        buf.set_stringn(x0 + 2, bottom, &hints, inner, dim);
        let y = y0 + 1;
        let rule_y = bottom - 5;
        if let Some((spec, _)) = items_of {
            self.draw_items(buf, (x0, y, w, rule_y), spec, config, caps);
        } else {
            self.draw_list(buf, (x0, y, w, rule_y), config, caps, filter_row);
        }
        // The chosen setting: its key, default and description.
        for x in x0 + 1..x0 + w - 1 {
            buf[(x, rule_y)].set_symbol(hz).set_style(dim);
        }
        buf[(x0, rule_y)].set_symbol(lt).set_style(dim);
        buf[(x0 + w - 1, rule_y)].set_symbol(rt).set_style(dim);
        if let Some(spec) = shown.get(selected).map(|&i| &SPECS[i]) {
            let about = match settings_list::edit(spec) {
                Edit::Items => kalem_core::tr!(
                    "settings-items",
                    count = settings_list::items(config, spec).len()
                ),
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

    /// The list of settings in the frame `x0, y, w` above row `rule_y`,
    /// the filter first when there is one.
    fn draw_list(
        &self,
        buf: &mut Buffer,
        (x0, mut y, w, rule_y): (u16, u16, u16, u16),
        config: &Config,
        caps: &Caps,
        filter_row: bool,
    ) {
        let lines = self.list.lines();
        let shown = self.list.shown();
        let selected = self.list.selected.min(shown.len().saturating_sub(1));
        let inner = (w - 4) as usize;
        let bg = panel_style(caps);
        let dim = bg.add_modifier(Modifier::DIM);
        let accent = accent_style(caps, bg);
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
    }

    /// The items of `spec` (a list or a table) in the frame `x0, y, w`
    /// above row `rule_y`: its key, then each item, a choice marked in or
    /// out.
    fn draw_items(
        &self,
        buf: &mut Buffer,
        (x0, mut y, w, rule_y): (u16, u16, u16, u16),
        spec: &kalem_core::settings::Spec,
        config: &Config,
        caps: &Caps,
    ) {
        let inner = (w - 4) as usize;
        let bg = panel_style(caps);
        let dim = bg.add_modifier(Modifier::DIM);
        let accent = accent_style(caps, bg);
        buf.set_stringn(
            x0 + 2,
            y,
            spec.key,
            inner,
            accent.add_modifier(Modifier::BOLD),
        );
        y += 1;
        let items = settings_list::items(config, spec);
        if items.is_empty() {
            buf.set_stringn(x0 + 4, y, tr("settings-empty"), inner, dim);
            return;
        }
        let room = rule_y.saturating_sub(y) as usize;
        let selected = self.list.item.unwrap_or(0).min(items.len() - 1);
        let first = (selected + 1).saturating_sub(room);
        for (n, item) in items.iter().enumerate().skip(first).take(room) {
            let style = if n == selected {
                selected_style(caps, bg)
            } else {
                bg
            };
            for x in x0 + 1..x0 + w - 1 {
                buf[(x, y)].set_style(style);
            }
            let text = match item.on {
                Some(true) => format!("[x] {}", item.text),
                Some(false) => format!("[ ] {}", item.text),
                None => item.text.clone(),
            };
            let style = if item.on == Some(true) {
                accent_style(caps, style).add_modifier(Modifier::BOLD)
            } else {
                style
            };
            buf.set_stringn(x0 + 4, y, &text, inner.saturating_sub(2), style);
            self.spots.borrow_mut().push((y, x0 + 1, x0 + w - 1, n));
            y += 1;
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
