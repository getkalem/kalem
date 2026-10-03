//! Overlays and panels: the command palette, the find bar and the outline.

use std::ops::Range;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use unicode_width::UnicodeWidthStr;

use crate::caps::Caps;

use kalem_core::command::PickKind;
use kalem_core::line_search::LineSearch;
pub use kalem_core::palette::{PaletteItem, fuzzy};
use kalem_core::projects::{Entry, OpenFile, Picker, ProjectSearch};

/// The background of popups and the palette.
pub fn panel_style(caps: &Caps) -> Style {
    match (&caps.colors, caps.no_color) {
        (_, true) => Style::default(),
        (Some(t), false) => Style::default()
            .bg(crate::render::solid(t.bar, t))
            .fg(crate::render::rgb(t.foreground)),
        // 256 colors: a gray a little off the terminal's background.
        (None, false) if caps.dark_background() == Some(false) => Style::default()
            .bg(Color::Indexed(254))
            .fg(Color::Indexed(235)),
        (None, false) => Style::default()
            .bg(Color::Indexed(236))
            .fg(Color::Indexed(252)),
    }
}

/// The command palette, which also offers lists (documents, files,
/// projects) and searches a project.
#[derive(Debug)]
pub struct Palette {
    /// What was typed.
    pub input: String,
    /// The cursor, as characters after it ([`kalem_core::line_edit`]).
    pub back: usize,
    /// The chosen line.
    pub selected: usize,
    items: Vec<PaletteItem>,
    /// The items' order means something (a context menu): kept while
    /// nothing is typed.
    pub ordered: bool,
    /// Choosing from a list instead of commands.
    pub pick: Option<Picker>,
    /// Searching a project's files instead.
    pub search: Option<ProjectSearch>,
    /// Opened by a request for a list ([`kalem_core::command::Request::is_picker`]),
    /// which `SPC '` opens again with what is typed in it.
    pub resumable: bool,
    /// Searching the lines of open documents instead (`SPC s b`).
    pub lines: Option<LineSearch>,
    /// Where the cursor was when the line search opened: the document's
    /// index, its source in the search, and the selection's anchor and
    /// head, to go back to.
    pub origin: Option<(usize, usize, usize, usize)>,
}

impl Palette {
    /// A palette of `items`.
    pub fn new(items: Vec<PaletteItem>) -> Palette {
        Palette {
            input: String::new(),
            back: 0,
            selected: 0,
            items,
            ordered: false,
            pick: None,
            search: None,
            resumable: false,
            lines: None,
            origin: None,
        }
    }

    /// A live search of lines, from the cursor at `origin`.
    pub fn searching_lines(lines: LineSearch, origin: (usize, usize, usize, usize)) -> Palette {
        let mut p = Palette {
            input: lines.text().to_string(),
            lines: Some(lines),
            origin: Some(origin),
            ..Palette::new(Vec::new())
        };
        p.input_changed();
        p
    }

    /// The typed text changed: the searches follow it, and the list
    /// starts at its top (a line search at the cursor's line).
    pub fn input_changed(&mut self) {
        self.selected = 0;
        if let Some(s) = &mut self.search {
            s.set_text(&self.input);
        }
        if let Some(l) = &mut self.lines {
            l.set_text(&self.input);
            if let Some((_, source, _, head)) = self.origin {
                self.selected = l.nearest(source, head);
            }
        }
    }

    /// A list to choose from.
    pub fn picker(picker: Picker) -> Palette {
        Palette {
            pick: Some(picker),
            ..Palette::new(Vec::new())
        }
    }

    /// A search through a project's files.
    pub fn searching(search: ProjectSearch) -> Palette {
        Palette {
            input: search.query.text.clone(),
            search: Some(search),
            ..Palette::new(Vec::new())
        }
    }

    /// The items matching the input, best first.
    pub fn matches(&self) -> Vec<&PaletteItem> {
        if self.search.is_some() || self.lines.is_some() {
            return Vec::new();
        }
        match &self.pick {
            Some(p) => kalem_core::projects::matches(p, &self.input),
            None if self.ordered => kalem_core::palette::matches_ordered(&self.items, &self.input),
            None => kalem_core::palette::matches(&self.items, &self.input),
        }
    }

    /// How many lines can be chosen.
    pub fn len(&self) -> usize {
        match (&self.search, &self.lines) {
            (Some(s), _) => s.hits.len(),
            (_, Some(l)) => l.hits.len(),
            _ => self.matches().len(),
        }
    }

    /// Whether nothing can be chosen.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The command (or item) on the chosen line.
    pub fn chosen(&self) -> Option<String> {
        self.matches().get(self.selected).map(|it| it.id.clone())
    }

    /// Draws the palette near the top of `area`.
    pub fn draw(&self, buf: &mut Buffer, area: Rect, caps: &Caps) {
        let wide = self.pick.is_some() || self.search.is_some() || self.lines.is_some();
        let w = area.width.saturating_sub(4).min(if wide { 90 } else { 70 });
        let x = area.x + (area.width - w) / 2;
        // Lines: a title, a detail and keys (or a mark).
        let lines: Vec<(String, String, String)> = match (&self.search, &self.pick, &self.lines) {
            (_, _, Some(l)) => l
                .hits
                .iter()
                .map(|h| {
                    let (text, place) = l.row(h);
                    (place, text, String::new())
                })
                .collect(),
            (Some(s), _, _) => s
                .hits
                .iter()
                .map(|h| {
                    let (at, text) = s.line(h);
                    (at, text, String::new())
                })
                .collect(),
            (None, Some(_), _) => self
                .matches()
                .iter()
                .map(|it| (it.title.clone(), it.category.clone(), it.keys.clone()))
                .collect(),
            (None, None, _) => self
                .matches()
                .iter()
                .map(|it| {
                    (
                        format!("{}: {}", it.category, it.title),
                        String::new(),
                        it.keys.clone(),
                    )
                })
                .collect(),
        };
        let h = (lines.len() as u16 + 1)
            .min(area.height.saturating_sub(2))
            .max(1);
        let bg = panel_style(caps);
        let accent = accent_style(caps, bg);
        let y0 = area.y + 1;
        for y in y0..y0 + h + 1 {
            for dx in 0..w {
                buf[(x + dx, y)].set_symbol(" ").set_style(bg);
            }
        }
        let (prompt, note) = match (&self.search, &self.pick) {
            _ if self.lines.is_some() => {
                let n = self.lines.as_ref().map_or(0, |l| l.hits.len());
                (
                    format!("{}: ", kalem_core::l10n::tr("search-lines")),
                    kalem_core::tr!("search-lines-count", count = n),
                )
            }
            (Some(s), _) => {
                let switches: Vec<String> = s
                    .switches()
                    .into_iter()
                    .map(|(l, on)| if on { format!("[{l}]") } else { l })
                    .collect();
                (
                    format!(
                        "{}: ",
                        kalem_core::tr!("search-project", project = s.name.clone()),
                    ),
                    format!("{}  {}", switches.join(" "), s.status()),
                )
            }
            (None, Some(p)) => {
                let note = if p.partial {
                    kalem_core::l10n::tr("pick-walking")
                } else if p.items.is_empty()
                    && matches!(p.kind, PickKind::Projects | PickKind::RemoveProject)
                {
                    kalem_core::l10n::tr("pick-no-projects")
                } else {
                    String::new()
                };
                (format!("{}: ", p.prompt), note)
            }
            (None, None) => ("> ".to_string(), String::new()),
        };
        // The label, then the typed text with the cursor shown in reverse.
        let (before, after) = kalem_core::line_edit::split(&self.input, self.back);
        let caret = (prompt.width() + before.width()) as u16;
        let prompt = format!("{prompt}{before}{after}");
        buf.set_stringn(
            x + 1,
            y0,
            &prompt,
            (w - 2) as usize,
            accent.add_modifier(Modifier::BOLD),
        );
        if caret + 1 < w - 1 {
            let cell = &mut buf[(x + 1 + caret, y0)];
            cell.set_style(cell.style().add_modifier(Modifier::REVERSED));
        }
        let nw = note.width() as u16;
        if !note.is_empty() && prompt.width() as u16 + nw + 4 < w {
            buf.set_stringn(
                x + w - nw - 1,
                y0,
                &note,
                nw as usize,
                bg.add_modifier(Modifier::DIM),
            );
        }
        let first = self.selected.saturating_sub(h.saturating_sub(2) as usize);
        for (n, (title, detail, keys)) in lines.iter().enumerate().skip(first).take(h as usize) {
            let y = y0 + 1 + (n - first) as u16;
            let style = if n == self.selected {
                selected_style(caps, bg)
            } else {
                bg
            };
            for dx in 0..w {
                buf[(x + dx, y)].set_style(style);
            }
            buf.set_stringn(x + 1, y, title, (w - 2) as usize, style);
            let tw = title.width() as u16;
            if !detail.is_empty() && tw + 3 < w {
                buf.set_stringn(
                    x + 2 + tw,
                    y,
                    detail,
                    (w - 3 - tw) as usize,
                    style.add_modifier(Modifier::DIM),
                );
            }
            let kw = keys.width() as u16;
            if !keys.is_empty() && tw + detail.width() as u16 + kw + 5 < w {
                buf.set_stringn(
                    x + w - kw - 1,
                    y,
                    keys,
                    kw as usize,
                    style.add_modifier(Modifier::DIM),
                );
            }
        }
    }
}

/// Text in the theme's accent color on `base` (the link color), bold
/// without colors.
pub fn accent_style(caps: &Caps, base: Style) -> Style {
    match (&caps.colors, caps.no_color) {
        (_, true) => base.add_modifier(Modifier::BOLD),
        (Some(t), false) => base.fg(crate::render::rgb(t.link)),
        (None, false) => base.fg(Color::Cyan),
    }
}

/// A chosen line on `base`: the selection color, reversed without
/// colors.
pub fn selected_style(caps: &Caps, base: Style) -> Style {
    match (&caps.colors, caps.no_color) {
        (_, true) => base.add_modifier(Modifier::REVERSED),
        (Some(t), false) => base.bg(crate::render::solid(t.selection, t)),
        (None, false) => base.bg(Color::Indexed(24)).fg(Color::White),
    }
}

/// Where each document is in the list of open files, and where the
/// commands it offers are.
pub type FileSpots = (Vec<(Rect, usize)>, Vec<(Rect, &'static str)>);

/// The folder tree the list of open files shows: its lines, the active
/// document's file, and where each line is drawn (its row and index).
#[derive(Debug)]
pub struct FolderView<'a> {
    /// The lines.
    pub rows: &'a [kalem_core::projects::TreeRow],
    /// The active document's file.
    pub current: Option<&'a std::path::Path>,
    /// Filled with where each drawn line is.
    pub spots: std::cell::RefCell<Vec<(Rect, usize)>>,
}

/// The list of open files: in a column on the left (`top` false) or on
/// one line at the top, with the file manager and the projects. Returns
/// where each document is (its row, or its columns at the top, and its
/// index) and where the two commands are.
#[allow(clippy::too_many_arguments)]
pub fn draw_files(
    buf: &mut Buffer,
    area: Rect,
    entries: &[Entry],
    files: &[OpenFile],
    active: usize,
    top: bool,
    tree: &FolderView<'_>,
    caps: &Caps,
) -> FileSpots {
    let bg = panel_style(caps);
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_symbol(" ").set_style(bg);
        }
    }
    let mut spots = Vec::new();
    let title = |f: &OpenFile| {
        if f.modified {
            format!("{} •", f.title)
        } else {
            f.title.clone()
        }
    };
    if top {
        let mut x = area.x + 1;
        for e in entries {
            if x >= area.right() {
                break;
            }
            let room = area.right().saturating_sub(x) as usize;
            match e {
                Entry::Project { name, .. } => {
                    let label = format!("{name}:");
                    buf.set_stringn(
                        x,
                        area.y,
                        &label,
                        room,
                        accent_style(caps, bg).add_modifier(Modifier::DIM),
                    );
                    x += label.width() as u16 + 1;
                }
                Entry::File { index, .. } => {
                    let label = format!(" {} ", title(&files[*index]));
                    let style = if *index == active {
                        selected_style(caps, bg).add_modifier(Modifier::BOLD)
                    } else {
                        bg
                    };
                    buf.set_stringn(x, area.y, &label, room, style);
                    let w = (label.width() as u16).min(room as u16);
                    spots.push((Rect::new(x, area.y, w, 1), *index));
                    x += w + 1;
                }
            }
        }
        // The file manager and the projects, at the right end.
        let mut actions = Vec::new();
        let mut right = area.right().saturating_sub(1);
        for (id, label) in ACTIONS.iter().rev() {
            let label = format!(" {} ", kalem_core::l10n::tr(label));
            let w = label.width() as u16;
            if right < x + w {
                break;
            }
            right -= w;
            buf.set_stringn(right, area.y, &label, w as usize, accent_style(caps, bg));
            actions.push((Rect::new(right, area.y, w, 1), *id));
            right = right.saturating_sub(1);
        }
        return (spots, actions);
    }
    let w = area.width.saturating_sub(1);
    let mut y = area.y;
    buf.set_stringn(
        area.x + 1,
        y,
        kalem_core::l10n::tr("open-files"),
        w as usize,
        bg.add_modifier(Modifier::DIM),
    );
    y += 1;
    for e in entries {
        if y >= area.bottom() {
            break;
        }
        match e {
            Entry::Project { name, .. } => {
                let glyph = if caps.ascii { "v" } else { "▾" };
                buf.set_stringn(
                    area.x + 1,
                    y,
                    format!("{glyph} {name}"),
                    w as usize,
                    accent_style(caps, bg).add_modifier(Modifier::BOLD),
                );
            }
            Entry::File { index, nested } => {
                let indent = if *nested { 3 } else { 1 };
                let style = if *index == active {
                    selected_style(caps, bg)
                } else {
                    bg
                };
                for x in area.x..area.x + w {
                    buf[(x, y)].set_style(style);
                }
                buf.set_stringn(
                    area.x + indent,
                    y,
                    title(&files[*index]),
                    w.saturating_sub(indent) as usize,
                    style,
                );
                spots.push((Rect::new(area.x, y, w, 1), *index));
            }
        }
        y += 1;
    }
    // The project's folder tree, below the files, above the actions.
    let bottom = area.bottom().saturating_sub(ACTIONS.len() as u16 + 1);
    if !tree.rows.is_empty() && y + 2 < bottom {
        y += 1;
        buf.set_stringn(
            area.x + 1,
            y,
            kalem_core::l10n::tr("folder-tree"),
            w as usize,
            bg.add_modifier(Modifier::DIM),
        );
        y += 1;
        let room = (bottom - y) as usize;
        let current = tree
            .rows
            .iter()
            .position(|r| Some(r.path.as_path()) == tree.current);
        // The current file stays in view.
        let first = current.map_or(0, |c| (c + 1).saturating_sub(room));
        let (closed, open) = if caps.ascii {
            (">", "v")
        } else {
            ("▸", "▾")
        };
        for (i, r) in tree.rows.iter().enumerate().skip(first).take(room) {
            let indent = 1 + 2 * r.depth as u16;
            let style = if Some(i) == current {
                selected_style(caps, bg)
            } else if r.dir {
                accent_style(caps, bg)
            } else {
                bg
            };
            for x in area.x..area.x + w {
                buf[(x, y)].set_style(style);
            }
            let glyph = match (r.dir, r.open) {
                (true, true) => open,
                (true, false) => closed,
                _ => " ",
            };
            buf.set_stringn(
                area.x + indent.min(w),
                y,
                format!("{glyph} {}", r.name),
                w.saturating_sub(indent) as usize,
                style,
            );
            tree.spots
                .borrow_mut()
                .push((Rect::new(area.x, y, w, 1), i));
            y += 1;
        }
    }
    // The file manager and the projects, at the bottom.
    let mut actions = Vec::new();
    let glyph = if caps.ascii { ">" } else { "▸" };
    for (i, (id, label)) in ACTIONS.iter().enumerate() {
        let row = area.bottom().saturating_sub((ACTIONS.len() - i) as u16);
        if row <= y || row >= area.bottom() {
            continue;
        }
        buf.set_stringn(
            area.x + 1,
            row,
            format!("{glyph} {}", kalem_core::l10n::tr(label)),
            w.saturating_sub(1) as usize,
            accent_style(caps, bg),
        );
        actions.push((Rect::new(area.x, row, w, 1), *id));
    }
    // A thin border on the right.
    let bar = if caps.ascii { "|" } else { "│" };
    for y in area.top()..area.bottom() {
        buf[(area.right() - 1, y)]
            .set_symbol(bar)
            .set_style(bg.add_modifier(Modifier::DIM));
    }
    (spots, actions)
}

/// What the list of open files offers besides the documents: the file
/// manager and the projects view.
const ACTIONS: [(&str, &str); 2] = [
    ("dired.jump", "menu-file-manager"),
    ("dired.projects", "menu-projects-view"),
];

/// The find bar.
#[derive(Debug, Clone, Default)]
pub struct Find {
    /// The search.
    pub query: String,
    /// The replacement, in find and replace.
    pub replacement: Option<String>,
    /// Typing goes to the replacement.
    pub on_replacement: bool,
    /// Where the cursor was when the search started.
    pub origin: usize,
    /// The matches.
    pub matches: Vec<Range<usize>>,
    /// The query is a regular expression (Alt+R).
    pub regex: bool,
    /// Why the regular expression does not compile.
    pub error: Option<String>,
}

impl Find {
    /// The search options.
    pub fn options(&self) -> kalem_core::find::FindOptions {
        kalem_core::find::FindOptions { regex: self.regex }
    }

    /// The status line text.
    pub fn line(&self, current: Option<&Range<usize>>) -> String {
        let n = current
            .and_then(|c| self.matches.iter().position(|m| m == c))
            .map_or(0, |i| i + 1);
        self.line_counted(format!("{n}/{}", self.matches.len()))
    }

    /// The status line text with `count` as the matches' count (a
    /// viewer's search counts its own).
    pub fn line_counted(&self, count: String) -> String {
        let count = match &self.error {
            Some(e) => format!("  {e}"),
            None if self.query.is_empty() => String::new(),
            None => format!("  {count}"),
        };
        let find = kalem_core::l10n::tr(if self.regex {
            "find-regex-label"
        } else {
            "find-label"
        });
        let replace = kalem_core::l10n::tr("replace-label");
        match &self.replacement {
            Some(r) => format!("{find}: {}  {replace}: {}{count}", self.query, r),
            None => format!("{find}: {}{count}", self.query),
        }
    }
}

pub use kalem_core::view::OutlineItem;

/// The outline panel.
#[derive(Debug, Clone, Default)]
pub struct OutlinePanel {
    /// Keys go to the panel.
    pub focus: bool,
    /// The chosen heading.
    pub selected: usize,
    /// The headings.
    pub items: Vec<OutlineItem>,
    /// The text version the headings are for.
    pub version: u64,
    top: usize,
    rows: Vec<(u16, usize)>,
}

impl OutlinePanel {
    /// A focused panel with these headings.
    pub fn new(items: Vec<OutlineItem>, selected: usize, version: u64) -> OutlinePanel {
        OutlinePanel {
            focus: true,
            selected,
            items,
            version,
            ..OutlinePanel::default()
        }
    }

    /// The panel's width for a screen `width` wide.
    pub fn width(width: u16) -> u16 {
        (width / 3).clamp(16, 36).min(width.saturating_sub(10))
    }

    /// The heading at screen row `y`.
    pub fn at_row(&self, y: u16) -> Option<usize> {
        self.rows.iter().find(|(r, _)| *r == y).map(|(_, i)| *i)
    }

    /// Draws the panel; `current` is the heading holding the cursor.
    pub fn draw(&mut self, buf: &mut Buffer, area: Rect, current: Option<usize>, caps: &Caps) {
        self.rows.clear();
        let h = area.height as usize;
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + h {
            self.top = self.selected + 1 - h;
        }
        let border = if caps.ascii { "|" } else { "│" };
        let dim = Style::default().add_modifier(Modifier::DIM);
        for y in area.y..area.bottom() {
            buf.set_string(area.right() - 1, y, border, dim);
        }
        let w = area.width.saturating_sub(2) as usize;
        for (n, it) in self.items.iter().enumerate().skip(self.top).take(h) {
            let y = area.y + (n - self.top) as u16;
            let mut style = if Some(n) == current {
                Style::default().add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            if n == self.selected && self.focus {
                style = style.add_modifier(Modifier::REVERSED);
            }
            let indent = "  ".repeat(it.level.saturating_sub(1).min(6));
            let mut x = area.x;
            buf.set_stringn(x, y, &indent, w, style);
            x += indent.width() as u16;
            if let Some((kw, done)) = &it.todo {
                let st = match (&caps.colors, caps.no_color) {
                    (_, true) => style,
                    (Some(t), false) => {
                        style.fg(crate::render::rgb(if *done { t.done } else { t.todo }))
                    }
                    (None, false) => style.fg(if *done { Color::Green } else { Color::Red }),
                };
                let t = format!("{kw} ");
                buf.set_stringn(x, y, &t, w.saturating_sub((x - area.x) as usize), st);
                x += t.width() as u16;
            }
            buf.set_stringn(
                x,
                y,
                &it.title,
                w.saturating_sub((x - area.x) as usize),
                style,
            );
            self.rows.push((y, n));
        }
    }
}

/// A which-key panel at the bottom of `area`: the keys that may follow,
/// in columns.
pub fn draw_which_key(buf: &mut Buffer, area: Rect, items: &[(String, String)], caps: &Caps) {
    if items.is_empty() || area.height < 3 {
        return;
    }
    let bg = panel_style(caps);
    let key_style = accent_style(caps, bg).add_modifier(Modifier::BOLD);
    let widest = items
        .iter()
        .map(|(k, l)| k.width() + l.width() + 3)
        .max()
        .unwrap_or(10)
        .min(40) as u16;
    let cols = (area.width.saturating_sub(2) / widest.max(1)).max(1);
    let rows = (items.len() as u16).div_ceil(cols).min(area.height - 1);
    let y0 = area.bottom() - rows - 1;
    for y in y0..area.bottom() {
        for x in area.left()..area.right() {
            buf[(x, y)].set_symbol(" ").set_style(bg);
        }
    }
    let rule = if caps.ascii { "-" } else { "─" };
    for x in area.left()..area.right() {
        buf[(x, y0)]
            .set_symbol(rule)
            .set_style(bg.add_modifier(Modifier::DIM));
    }
    for (n, (k, l)) in items.iter().enumerate() {
        let (c, r) = (n as u16 / rows, n as u16 % rows);
        if c >= cols {
            break;
        }
        let x = area.x + 1 + c * widest;
        let y = y0 + 1 + r;
        if y >= area.bottom() {
            continue;
        }
        buf.set_stringn(x, y, k, widest as usize, key_style);
        let kx = x + k.width() as u16 + 1;
        buf.set_stringn(
            kx,
            y,
            format!("{} {l}", if caps.ascii { ">" } else { "→" }),
            widest.saturating_sub(k.width() as u16 + 1) as usize,
            bg,
        );
    }
}
