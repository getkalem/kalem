//! The settings panel (`app.settings`, Ctrl+, or `SPC h v`), lazygit's
//! way: every setting in a framed list grouped by table, the chosen
//! one's description below, a key for each change; a list's or a table's
//! items in the same frame; the installed plugins and each one's settings
//! as pages of their own ([`kalem_core::settings_list`]).

use std::cell::RefCell;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use unicode_width::UnicodeWidthStr;

use kalem_core::l10n::tr;
use kalem_core::settings::Config;
use kalem_core::settings_list::{self, Browser, Entry, Field, FieldKind, Row};

use crate::caps::Caps;
use crate::panels::{accent_style, panel_style, selected_style};

/// The open settings panel.
#[derive(Debug, Default)]
pub struct SettingsPanel {
    /// The list as browsed.
    pub list: Browser,
    /// Where each entry (or, its items shown, each item) was drawn: its
    /// row, its columns and its index, for clicks.
    pub spots: RefCell<Vec<(u16, u16, u16, usize)>>,
}

/// The frame's inside: its left column, first row, width, and the row of
/// the rule above the chosen entry's description.
type Frame = (u16, u16, u16, u16);

impl SettingsPanel {
    /// The entry (or item) shown at column `x`, row `y`, as its index.
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
        let rows = self.list.rows(config);
        let count = rows.iter().filter(|r| matches!(r, Row::Entry(_))).count();
        let selected = self.list.selected.min(count.saturating_sub(1));
        let current = self.list.current(config);
        let items_of = self
            .list
            .item
            .and(current.as_ref())
            .and_then(Entry::field)
            .filter(|f| settings_list::has_items(f));
        let filter_row = self.list.filtering || !self.list.filter.is_empty();
        // The frame, the filter, the list (or the setting's key and its
        // items), a rule, the chosen entry's heading and three lines of
        // its description.
        let lines = match items_of {
            Some(f) => settings_list::items(config, f).len().max(1) + 1,
            None => rows.len().max(1) + usize::from(filter_row),
        }
        .max(rows.len());
        let w = area.width.saturating_sub(4).min(88);
        let h = (lines as u16 + 7).min(area.height.saturating_sub(2));
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
        let n = format!(" {}/{} ", (selected + 1).min(count), count);
        if title.width() + n.width() + 4 < w as usize {
            buf.set_stringn(x0 + w - 2 - n.width() as u16, y0, &n, n.width(), dim);
        }
        let hints = format!(" {} ", hints(items_of));
        buf.set_stringn(x0 + 2, bottom, &hints, inner, dim);
        let rule_y = bottom - 5;
        let frame = (x0, y0 + 1, w, rule_y);
        match items_of {
            Some(f) => self.draw_items(buf, frame, f, config, caps),
            None => self.draw_list(buf, frame, &rows, selected, config, caps, filter_row),
        }
        // The chosen entry: its key and default (or what it is) and its
        // description.
        for x in x0 + 1..x0 + w - 1 {
            buf[(x, rule_y)].set_symbol(hz).set_style(dim);
        }
        buf[(x0, rule_y)].set_symbol(lt).set_style(dim);
        buf[(x0 + w - 1, rule_y)].set_symbol(rt).set_style(dim);
        if let Some(e) = &current {
            let (head, about) = e.about(config);
            buf.set_stringn(x0 + 2, rule_y + 1, &head, inner, accent);
            for (dy, text) in (2..).zip(wrap(&about, inner, 3)) {
                buf.set_stringn(x0 + 2, rule_y + dy, &text, inner, bg);
            }
        }
    }

    /// The page's lines in `frame`, the filter first when there is one,
    /// scrolled so that the chosen entry shows.
    #[allow(clippy::too_many_arguments)]
    fn draw_list(
        &self,
        buf: &mut Buffer,
        (x0, mut y, w, rule_y): Frame,
        rows: &[Row],
        selected: usize,
        config: &Config,
        caps: &Caps,
        filter_row: bool,
    ) {
        let inner = (w - 4) as usize;
        let bg = panel_style(caps);
        let dim = bg.add_modifier(Modifier::DIM);
        let accent = accent_style(caps, bg);
        if filter_row {
            let caret = match (self.list.filtering, caps.ascii) {
                (false, _) => "",
                (true, true) => "_",
                (true, false) => "▏",
            };
            let text = format!("/ {}{caret}", self.list.filter);
            buf.set_stringn(x0 + 2, y, &text, inner, accent);
            y += 1;
        }
        let room = rule_y.saturating_sub(y) as usize;
        // The line of the chosen entry.
        let mut n = 0;
        let at = rows
            .iter()
            .position(|r| {
                let entry = matches!(r, Row::Entry(_));
                let found = entry && n == selected;
                n += usize::from(entry);
                found
            })
            .unwrap_or(0);
        let first = (at + 1).saturating_sub(room);
        let names = rows
            .iter()
            .filter_map(|r| match r {
                Row::Entry(e) => Some(e.name().width()),
                Row::Heading(_) => None,
            })
            .max()
            .unwrap_or(0)
            .min(inner / 2);
        // The entries above the first line shown.
        let mut n = rows[..first]
            .iter()
            .filter(|r| matches!(r, Row::Entry(_)))
            .count();
        if rows.is_empty() {
            buf.set_stringn(x0 + 2, y, tr("settings-empty"), inner, dim);
        }
        for row in rows.iter().skip(first).take(room) {
            match row {
                Row::Heading(name) => {
                    buf.set_stringn(x0 + 2, y, name, inner, accent.add_modifier(Modifier::BOLD));
                }
                Row::Entry(e) => {
                    let style = if n == selected {
                        selected_style(caps, bg)
                    } else {
                        bg
                    };
                    for x in x0 + 1..x0 + w - 1 {
                        buf[(x, y)].set_style(style);
                    }
                    buf.set_stringn(x0 + 4, y, e.name(), names, style);
                    let vx = x0 + 4 + names as u16 + 2;
                    let room = (x0 + w - 2).saturating_sub(vx) as usize;
                    let value_style = if e.changed(config) {
                        accent_style(caps, style).add_modifier(Modifier::BOLD)
                    } else {
                        style
                    };
                    buf.set_stringn(vx, y, e.value(config), room, value_style);
                    self.spots.borrow_mut().push((y, x0 + 1, x0 + w - 1, n));
                    n += 1;
                }
            }
            y += 1;
        }
    }

    /// The items of `field` (a list or a table) in `frame`: its key, then
    /// each item, a choice marked in or out.
    fn draw_items(
        &self,
        buf: &mut Buffer,
        (x0, mut y, w, rule_y): Frame,
        field: &Field,
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
            &field.key,
            inner,
            accent.add_modifier(Modifier::BOLD),
        );
        y += 1;
        let items = settings_list::items(config, field);
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

/// The keys of the panel, for the items of `field` when shown.
pub fn hints(field: Option<&Field>) -> String {
    match field.map(|f| &f.kind) {
        Some(FieldKind::Choices(_)) => tr("settings-items-choices"),
        Some(FieldKind::Texts) => tr("settings-items-texts"),
        Some(FieldKind::Table(_)) => tr("settings-items-table"),
        _ => tr("settings-hints"),
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
