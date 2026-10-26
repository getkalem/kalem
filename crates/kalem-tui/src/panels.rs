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
    /// The question a list's items answer, the category they all share
    /// ([`kalem_core::palette::heading`]): drawn once above what is typed,
    /// wrapped, each item then shown by its title alone.
    pub heading: Option<String>,
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
    /// The items a menu's separator comes before, drawn as a rule while
    /// nothing is typed.
    breaks: Vec<usize>,
    /// The items matching what was typed, asked for at each key and each
    /// frame.
    matched: std::cell::RefCell<kalem_core::palette::MatchCache>,
}

impl Palette {
    /// A palette of `items`.
    pub fn new(items: Vec<PaletteItem>) -> Palette {
        let (items, breaks) = kalem_core::palette::split_separators(items);
        Palette {
            breaks,
            input: String::new(),
            back: 0,
            selected: 0,
            items,
            ordered: false,
            heading: None,
            pick: None,
            search: None,
            resumable: false,
            lines: None,
            origin: None,
            matched: Default::default(),
        }
    }

    /// A list a command or a plugin offers to choose from (a question's
    /// answers, a menu): in its order, under its question.
    pub fn choice(items: Vec<PaletteItem>) -> Palette {
        let mut p = Palette::new(items);
        p.ordered = true;
        p.heading = kalem_core::palette::heading(&p.items).map(str::to_string);
        p
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
        let items = self.pick.as_ref().map_or(&self.items[..], |p| &p.items);
        let mut cache = self.matched.borrow_mut();
        let order = cache.get(items, &self.input, || match &self.pick {
            Some(p) => kalem_core::projects::matching(p, &self.input),
            None if self.ordered => kalem_core::palette::matching_ordered(items, &self.input),
            None => kalem_core::palette::matching(items, &self.input),
        });
        order.iter().map(|&i| &items[i]).collect()
    }

    /// The list was replaced or changed in place: its matches are found
    /// again.
    pub fn items_changed(&mut self) {
        self.matched.get_mut().clear();
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

    /// Draws the palette near the top of `area` (nothing where it does not
    /// fit).
    pub fn draw(&self, buf: &mut Buffer, area: Rect, caps: &Caps) {
        if area.width < 8 || area.height < 3 {
            return;
        }
        // A command list's detail stands at the right of its row, cut
        // first: the title is what is chosen ("Install Rust 0.1.2" before
        // what it brings). A file's folder follows its name.
        let apart = self.pick.is_none() && self.search.is_none() && self.lines.is_none();
        let details = apart && self.items.iter().any(|it| !it.detail.is_empty());
        let wide = !apart || details;
        let w = area.width.saturating_sub(4).min(if wide { 90 } else { 70 });
        let x = area.x + (area.width - w) / 2;
        // The question over the list, wrapped, in the lines that leave
        // room for what is typed and two answers; where none do, before
        // each answer as before.
        let most = (area.height as usize).saturating_sub(6).min(8);
        let heading = match &self.heading {
            Some(h) if most > 0 => wrap_text(h, (w - 2) as usize, (w - 2) as usize, most),
            _ => Vec::new(),
        };
        // Lines: a title, a detail and keys (or a mark), made only for the
        // rows shown.
        let matches = self.matches();
        let count = match (&self.search, &self.lines) {
            (_, Some(l)) => l.hits.len(),
            (Some(s), _) => s.hits.len(),
            _ => matches.len(),
        };
        let line = |n: usize| -> (String, String, String) {
            match (&self.search, &self.pick, &self.lines) {
                (_, _, Some(l)) => {
                    let (text, place) = l.row(&l.hits[n]);
                    (place, text, String::new())
                }
                (Some(s), _, _) => {
                    let (at, text) = s.line(&s.hits[n]);
                    (at, text, String::new())
                }
                (None, Some(_), _) => {
                    let it = matches[n];
                    (it.title.clone(), it.category.clone(), it.keys.clone())
                }
                // A choice without a category (a TODO keyword, a refile
                // target) shows alone, and so does one under its list's
                // heading.
                (None, None, _) => {
                    let it = matches[n];
                    let title = if heading.is_empty() {
                        it.label()
                    } else {
                        it.title.clone()
                    };
                    (title, it.detail.clone(), it.keys.clone())
                }
            }
        };
        // Rows: the lines, with a rule where a menu's group ends while
        // nothing is typed (`None`).
        let rules = self.ordered
            && self.input.trim().is_empty()
            && self.pick.is_none()
            && self.search.is_none()
            && self.lines.is_none();
        let mut rows: Vec<Option<usize>> = Vec::with_capacity(count);
        for n in 0..count {
            if rules && self.breaks.binary_search(&n).is_ok() {
                rows.push(None);
            }
            rows.push(Some(n));
        }
        let h = (rows.len() as u16 + 1)
            .min(area.height.saturating_sub(2 + heading.len() as u16))
            .max(1);
        let bg = panel_style(caps);
        let accent = accent_style(caps, bg);
        let y0 = area.y + 1;
        // The line of what is typed, under the heading.
        let top = y0 + heading.len() as u16;
        for y in y0..top + h + 1 {
            for dx in 0..w {
                buf[(x + dx, y)].set_symbol(" ").set_style(bg);
            }
        }
        for (i, l) in heading.iter().enumerate() {
            buf.set_stringn(
                x + 1,
                y0 + i as u16,
                l,
                (w - 2) as usize,
                bg.add_modifier(Modifier::BOLD),
            );
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
            top,
            &prompt,
            (w - 2) as usize,
            accent.add_modifier(Modifier::BOLD),
        );
        if caret + 1 < w - 1 {
            let cell = &mut buf[(x + 1 + caret, top)];
            cell.set_style(cell.style().add_modifier(Modifier::REVERSED));
        }
        let nw = note.width() as u16;
        if !note.is_empty() && prompt.width() as u16 + nw + 4 < w {
            buf.set_stringn(
                x + w - nw - 1,
                top,
                &note,
                nw as usize,
                bg.add_modifier(Modifier::DIM),
            );
        }
        let at = rows
            .iter()
            .position(|r| *r == Some(self.selected))
            .unwrap_or(0);
        let first = at.saturating_sub(h.saturating_sub(2) as usize);
        for (r, row) in rows.iter().enumerate().skip(first).take(h as usize) {
            let y = top + 1 + (r - first) as u16;
            let Some(n) = *row else {
                let rule = "─".repeat((w - 2) as usize);
                buf.set_stringn(
                    x + 1,
                    y,
                    &rule,
                    (w - 2) as usize,
                    bg.add_modifier(Modifier::DIM),
                );
                continue;
            };
            let (title, detail, keys) = &line(n);
            let style = if n == self.selected {
                selected_style(caps, bg)
            } else {
                bg
            };
            for dx in 0..w {
                buf[(x + dx, y)].set_style(style);
            }
            if apart && !detail.is_empty() {
                // The title whole while it leaves the detail three tenths
                // of the row; the detail in what is left, at the right.
                let mut room = (w - 2) as usize;
                if !keys.is_empty() && keys.width() + 12 < room {
                    room -= keys.width() + 1;
                    buf.set_stringn(
                        x + w - keys.width() as u16 - 1,
                        y,
                        keys,
                        keys.width(),
                        style.add_modifier(Modifier::DIM),
                    );
                }
                let least = detail.width().min(room * 3 / 10);
                let tw = title.width().min(room.saturating_sub(least + 2));
                buf.set_stringn(x + 1, y, title, tw, style);
                let left = room.saturating_sub(tw + 2);
                let dw = detail.width().min(left);
                buf.set_stringn(
                    x + 1 + (room - dw) as u16,
                    y,
                    detail,
                    left,
                    style.add_modifier(Modifier::DIM),
                );
                continue;
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
pub type FileSpots = (Vec<(Rect, usize)>, Vec<(Rect, String)>);

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
/// one line at the top, with the file manager, the projects and the
/// plugins' `buttons` (each a command and its title, the git plugin's
/// Git). Returns where each document is (its row, or its columns at the
/// top, and its index) and where the commands are.
#[allow(clippy::too_many_arguments)]
pub fn draw_files(
    buf: &mut Buffer,
    area: Rect,
    entries: &[Entry],
    files: &[OpenFile],
    active: usize,
    top: bool,
    tree: &FolderView<'_>,
    buttons: &[(String, String)],
    caps: &Caps,
) -> FileSpots {
    let actions_shown: Vec<(String, String)> = ACTIONS
        .iter()
        .map(|(id, label)| (id.to_string(), kalem_core::l10n::tr(label)))
        .chain(buttons.iter().cloned())
        .collect();
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
        // The file manager, the projects and the plugins' buttons, at
        // the right end.
        let mut actions = Vec::new();
        let mut right = area.right().saturating_sub(1);
        for (id, label) in actions_shown.iter().rev() {
            let label = format!(" {label} ");
            let w = label.width() as u16;
            if right < x + w {
                break;
            }
            right -= w;
            buf.set_stringn(right, area.y, &label, w as usize, accent_style(caps, bg));
            actions.push((Rect::new(right, area.y, w, 1), id.clone()));
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
    let bottom = area.bottom().saturating_sub(actions_shown.len() as u16 + 1);
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
    // The file manager, the projects and the plugins' buttons, at the
    // bottom.
    let mut actions = Vec::new();
    let glyph = if caps.ascii { ">" } else { "▸" };
    for (i, (id, label)) in actions_shown.iter().enumerate() {
        let row = area
            .bottom()
            .saturating_sub((actions_shown.len() - i) as u16);
        if row <= y || row >= area.bottom() {
            continue;
        }
        buf.set_stringn(
            area.x + 1,
            row,
            format!("{glyph} {label}"),
            w.saturating_sub(1) as usize,
            accent_style(caps, bg),
        );
        actions.push((Rect::new(area.x, row, w, 1), id.clone()));
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
        // A terminal too narrow for the panel ([`OutlinePanel::width`]
        // gives it no column): nothing to draw.
        if area.width == 0 || area.height == 0 {
            return;
        }
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

/// The most lines one widget of a plugin's panel takes; a text that needs
/// more ends in "…" on the last.
pub const PANEL_WIDGET_LINES: usize = 4;

/// The width of a plugin's panel at the side of a screen `width` wide:
/// wider than the outline's on a wide screen, for its longer lines.
pub fn plugin_panel_width(width: u16) -> u16 {
    (width / 3).clamp(16, 48).min(width.saturating_sub(10))
}

/// `text` in at most `max` lines, the first `first` columns wide and the
/// others `rest`, broken at its spaces where it can (a word wider than a
/// line where the line ends) and at its line feeds. A text that needs
/// more lines ends in "…" on the last.
pub fn wrap_text(text: &str, first: usize, rest: usize, max: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    let room = |n: usize| if n == 0 { first } else { rest }.max(1);
    // The bytes of `s` that fill `w` columns: one character at least.
    let split = |s: &str, w: usize| -> usize {
        let mut used = 0;
        for (i, c) in s.char_indices() {
            used += c.width().unwrap_or(0);
            if used > w {
                return if i == 0 { c.len_utf8() } else { i };
            }
        }
        s.len()
    };
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        // Spaces that would start a line going on from the one before go.
        let mut wrapped = false;
        for word in para.split_inclusive(' ') {
            let mut word = if wrapped && line.is_empty() {
                word.trim_start_matches(' ')
            } else {
                word
            };
            if line.width() + word.trim_end_matches(' ').width() <= room(lines.len()) {
                line.push_str(word);
                continue;
            }
            if !line.is_empty() {
                lines.push(line.trim_end().to_string());
                line.clear();
                wrapped = true;
                word = word.trim_start_matches(' ');
            }
            while word.trim_end_matches(' ').width() > room(lines.len()) {
                let at = split(word, room(lines.len()));
                lines.push(word[..at].to_string());
                wrapped = true;
                word = &word[at..];
            }
            line.push_str(word);
        }
        lines.push(line.trim_end().to_string());
    }
    let max = max.max(1);
    if lines.len() > max {
        lines.truncate(max);
        let room = room(max - 1);
        let last = &mut lines[max - 1];
        while !last.is_empty() && last.width() + 1 > room {
            last.pop();
        }
        let kept = last.trim_end().len();
        last.truncate(kept);
        last.push('…');
    }
    lines
}

/// A piece of a line of a plugin's panel: widget `widget`'s text, or a
/// line of it, from column `x` of the panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelPiece {
    /// The widget, by index into the panel's widgets.
    pub widget: usize,
    /// Its first column, from the panel's left edge.
    pub x: u16,
    /// Its text.
    pub text: String,
}

/// The lines of a plugin's panel under its title, for a panel `width`
/// columns wide: each widget indented by its depth, a row's widgets side
/// by side. A text that does not fit goes on over the lines below, at
/// its widget's indent (an entry's and a checkbox's past their arrow or
/// box), [`PANEL_WIDGET_LINES`] at most; a row's widget that does not fit
/// beside those before it starts a line. The last column stays free.
pub fn plugin_panel_lines(
    panel: &kalem_core::extensions::Panel,
    width: u16,
) -> Vec<Vec<PanelPiece>> {
    use kalem_core::extensions::{WidgetKind, widget_text};
    let right = width.saturating_sub(1);
    let mut out = Vec::new();
    for (i, depth) in panel.lines() {
        let w = &panel.widgets[i];
        let start = 1 + (depth as u16 * 2).min(width / 2);
        let parts: Vec<usize> = match w.kind {
            WidgetKind::Row => w.children.iter().map(|&c| c as usize).collect(),
            _ => vec![i],
        };
        let mut line = Vec::new();
        let mut x = start;
        for j in parts {
            let kind = &panel.widgets[j].kind;
            let text = widget_text(&panel.widgets[j]);
            if text.is_empty() {
                continue;
            }
            if x > start && x + text.width() as u16 > right {
                out.push(std::mem::take(&mut line));
                x = start;
            }
            if x + 1 > right {
                break;
            }
            let first = (right - x) as usize;
            // Past the arrow or the box, when that leaves room.
            let hang = match kind {
                WidgetKind::Item {
                    expanded: Some(_), ..
                }
                | WidgetKind::Checkbox { .. } => text.find(' ').map_or(0, |p| text[..=p].width()),
                _ => 0,
            };
            let hang = if hang + 8 <= first { hang } else { 0 };
            let lines = wrap_text(&text, first, first - hang, PANEL_WIDGET_LINES);
            let n = lines.len();
            for (k, t) in lines.into_iter().enumerate() {
                let at = if k == 0 {
                    x
                } else {
                    out.push(std::mem::take(&mut line));
                    x + hang as u16
                };
                if k + 1 == n {
                    x = at + t.width() as u16 + 2;
                }
                line.push(PanelPiece {
                    widget: j,
                    x: at,
                    text: t,
                });
            }
        }
        out.push(line);
    }
    out
}

/// A plugin's panel (`kalem_core::extensions`, D11) as the terminal shows
/// it: how far it is scrolled, the widget chosen by keys while it has
/// them, and where each widget a click acts on was drawn.
#[derive(Debug, Clone, Default)]
pub struct PluginPanelView {
    /// Keys go to the panel.
    pub focus: bool,
    /// The widget chosen by keys, by key.
    pub selected: Option<String>,
    /// The first line shown under the title.
    pub top: usize,
    /// The chosen widget is brought into view at the next drawing.
    pub follow: bool,
    /// Where the panel was drawn last; empty while it is not shown.
    pub area: Rect,
    /// Its lines under the title, at the last drawing.
    lines: usize,
    /// How many of them show.
    height: usize,
    /// Each widget's first and last line, by index.
    places: Vec<(usize, usize, usize)>,
    /// Where each widget a user acts on was drawn, by index: a rectangle
    /// for each of its lines.
    spots: Vec<(Rect, usize)>,
}

impl PluginPanelView {
    /// The widget a user acts on drawn at (`x`, `y`), by index.
    pub fn at(&self, x: u16, y: u16) -> Option<usize> {
        let p = ratatui::layout::Position::new(x, y);
        self.spots
            .iter()
            .find(|(r, _)| r.contains(p))
            .map(|&(_, i)| i)
    }

    /// Scrolls by `by` lines, down when positive.
    pub fn scroll(&mut self, by: isize) {
        let last = self.lines.saturating_sub(self.height);
        self.top = self.top.saturating_add_signed(by).min(last);
    }

    /// The lines a page scrolls by.
    pub fn page(&self) -> usize {
        self.height.saturating_sub(1).max(1)
    }

    /// Widget `i`'s first and last line at the last drawing.
    pub fn place(&self, i: usize) -> Option<(usize, usize)> {
        self.places
            .iter()
            .find(|p| p.0 == i)
            .map(|&(_, first, last)| (first, last))
    }

    /// Whether line `n` shows.
    pub fn shows(&self, n: usize) -> bool {
        n >= self.top && n < self.top + self.height
    }

    /// Draws `panel` in `area`: its title, then its lines
    /// ([`plugin_panel_lines`]) from `top`, a mark in the last column
    /// where more lines are above or below.
    pub fn draw(
        &mut self,
        buf: &mut Buffer,
        area: Rect,
        panel: &kalem_core::extensions::Panel,
        caps: &Caps,
    ) {
        use kalem_core::extensions::{TextStyle, WidgetKind};
        self.area = area;
        self.spots.clear();
        self.places.clear();
        let base = panel_style(caps);
        for y in area.y..area.bottom() {
            buf.set_stringn(
                area.x,
                y,
                " ".repeat(area.width as usize),
                area.width as usize,
                base,
            );
        }
        let title = if self.focus {
            accent_style(caps, base)
        } else {
            base
        };
        buf.set_stringn(
            area.x + 1,
            area.y,
            &panel.title,
            area.width.saturating_sub(2) as usize,
            title.add_modifier(Modifier::BOLD),
        );
        let red = match (&caps.colors, caps.no_color) {
            (Some(t), false) => base.fg(crate::render::rgb(t.todo)),
            (None, false) => base.fg(Color::Red),
            _ => base,
        };
        let style = |kind: &WidgetKind| match kind {
            WidgetKind::Label { style, .. } => match style {
                TextStyle::Strong | TextStyle::Heading => base.add_modifier(Modifier::BOLD),
                TextStyle::Emphasis => base.add_modifier(Modifier::ITALIC),
                TextStyle::Muted => base.add_modifier(Modifier::DIM),
                TextStyle::Error => red,
                TextStyle::Normal | TextStyle::Code => base,
            },
            WidgetKind::Button { .. } | WidgetKind::Input { .. } => accent_style(caps, base),
            WidgetKind::Item { selected: true, .. } => base.add_modifier(Modifier::REVERSED),
            WidgetKind::Separator => base.add_modifier(Modifier::DIM),
            _ => base,
        };
        let lines = plugin_panel_lines(panel, area.width);
        for (n, line) in lines.iter().enumerate() {
            for piece in line {
                match self.places.iter_mut().find(|p| p.0 == piece.widget) {
                    Some(p) => p.2 = n,
                    None => self.places.push((piece.widget, n, n)),
                }
            }
        }
        self.lines = lines.len();
        self.height = area.height.saturating_sub(2) as usize;
        let chosen = self
            .selected
            .as_ref()
            .and_then(|k| panel.widgets.iter().position(|w| &w.key == k));
        if std::mem::take(&mut self.follow)
            && let Some((first, last)) = chosen.and_then(|i| self.place(i))
        {
            if first < self.top {
                self.top = first;
            } else if last >= self.top + self.height {
                self.top = (last + 1).saturating_sub(self.height).min(first);
            }
        }
        self.top = self.top.min(self.lines.saturating_sub(self.height));
        let chosen = chosen.filter(|_| self.focus);
        let acted_on = |kind: &WidgetKind| {
            matches!(
                kind,
                WidgetKind::Button { .. }
                    | WidgetKind::Input { .. }
                    | WidgetKind::Checkbox { .. }
                    | WidgetKind::Item { .. }
            )
        };
        let end = area.right().saturating_sub(1);
        for (n, line) in lines.iter().enumerate().skip(self.top).take(self.height) {
            let y = area.y + 2 + (n - self.top) as u16;
            let alone = line.len() == 1;
            for piece in line {
                let kind = &panel.widgets[piece.widget].kind;
                let mut st = style(kind);
                let x = area.x + piece.x;
                let mut to = x + piece.text.width() as u16;
                // Alone on its line, a widget reaches the panel's edge.
                if alone {
                    to = to.max(end);
                }
                if chosen == Some(piece.widget) {
                    st = selected_style(caps, st);
                    buf.set_stringn(x, y, " ".repeat((to - x) as usize), (end - x) as usize, st);
                }
                buf.set_stringn(x, y, &piece.text, end.saturating_sub(x) as usize, st);
                if acted_on(kind) && to > x {
                    self.spots.push((Rect::new(x, y, to - x, 1), piece.widget));
                }
            }
        }
        let dim = base.add_modifier(Modifier::DIM);
        if self.top > 0 && self.height > 0 {
            buf.set_string(end, area.y + 2, if caps.ascii { "^" } else { "▲" }, dim);
        }
        if self.top + self.height < self.lines && self.height > 0 {
            let y = area.y + 1 + self.height as u16;
            buf.set_string(end, y, if caps.ascii { "v" } else { "▼" }, dim);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kalem_core::extensions::{Panel, Widget, WidgetKind};

    fn item(label: &str, detail: Option<&str>, expanded: Option<bool>) -> WidgetKind {
        WidgetKind::Item {
            label: label.into(),
            detail: detail.map(str::to_string),
            expanded,
            selected: false,
        }
    }

    fn button(label: &str) -> WidgetKind {
        WidgetKind::Button {
            label: label.into(),
            command: None,
        }
    }

    fn panel(widgets: Vec<(&str, WidgetKind, Vec<u32>)>) -> Panel {
        Panel {
            plugin: "p".into(),
            id: "p.refs".into(),
            title: "Backlinks".into(),
            bottom: false,
            widgets: widgets
                .into_iter()
                .map(|(key, kind, children)| Widget {
                    key: key.into(),
                    kind,
                    children,
                })
                .collect(),
        }
    }

    fn texts(lines: &[Vec<PanelPiece>]) -> Vec<Vec<(usize, u16, &str)>> {
        lines
            .iter()
            .map(|l| l.iter().map(|p| (p.widget, p.x, p.text.as_str())).collect())
            .collect()
    }

    #[test]
    fn a_long_text_wraps_at_its_spaces_and_ends_in_an_ellipsis() {
        let r = "First try of Editor on the Book's plugins chapter";
        assert_eq!(
            wrap_text(r, 18, 18, 4),
            ["First try of", "Editor on the", "Book's plugins", "chapter"]
        );
        assert_eq!(wrap_text(r, 60, 60, 4), [r]);
        // Past the lines allowed: the last ends in an ellipsis.
        assert_eq!(
            wrap_text("one two three four five six", 9, 9, 2),
            ["one two", "three…"]
        );
        assert_eq!(wrap_text("one two three", 7, 7, 1), ["one tw…"]);
        // A word wider than a line is cut; the lines after the first are
        // as wide as asked.
        assert_eq!(
            wrap_text("a verylongwordindeed b", 6, 4, 9),
            ["a", "very", "long", "word", "inde", "ed b"]
        );
        // Its own line feeds kept; spaces kept inside a line, dropped at
        // the start of one going on from the line before.
        assert_eq!(wrap_text("a\nb", 9, 9, 4), ["a", "b"]);
        assert_eq!(
            wrap_text("▾ Linked references  7", 30, 28, 4),
            ["▾ Linked references  7"]
        );
        assert_eq!(
            wrap_text("Linked  references", 7, 7, 4),
            ["Linked", "referen", "ces"]
        );
        // Wide characters take two columns.
        assert_eq!(wrap_text("日本語の文", 6, 6, 4), ["日本語", "の文"]);
    }

    #[test]
    fn a_panels_lines_wrap_at_their_widgets_indent() {
        let p = panel(vec![
            ("", WidgetKind::Column, vec![1, 2]),
            (
                "1",
                item("Linked references", Some("2"), Some(true)),
                vec![3],
            ),
            ("row", WidgetKind::Row, vec![5, 6]),
            ("1.0", item("Book", Some("1"), Some(true)), vec![4]),
            (
                "1.0.0",
                item(
                    "First try of Editor on the Book's plugins chapter",
                    None,
                    None,
                ),
                vec![],
            ),
            ("refresh", button("Refresh"), vec![]),
            ("search", button("Search the graph"), vec![]),
        ]);
        assert_eq!(
            texts(&plugin_panel_lines(&p, 24)),
            [
                vec![(1, 1, "▾ Linked references  2")],
                vec![(3, 3, "▾ Book  1")],
                vec![(4, 5, "First try of")],
                vec![(4, 5, "Editor on the")],
                vec![(4, 5, "Book's plugins")],
                vec![(4, 5, "chapter")],
                // A row's widget that does not fit beside the one before
                // starts a line.
                vec![(5, 1, "[ Refresh ]")],
                vec![(6, 1, "[ Search the graph ]")],
            ]
        );
        // Wide enough, each text on a line, the row's widgets side by side.
        assert_eq!(
            texts(&plugin_panel_lines(&p, 60)),
            [
                vec![(1, 1, "▾ Linked references  2")],
                vec![(3, 3, "▾ Book  1")],
                vec![(4, 5, "First try of Editor on the Book's plugins chapter")],
                vec![(5, 1, "[ Refresh ]"), (6, 14, "[ Search the graph ]")],
            ]
        );
        // An entry's text goes on past its arrow.
        assert_eq!(
            texts(&plugin_panel_lines(&p, 16))[..3],
            [
                vec![(1, 1, "▾ Linked")],
                vec![(1, 3, "references")],
                vec![(1, 3, "2")]
            ]
        );
    }

    #[test]
    fn a_panel_scrolls_and_maps_clicks_to_every_line_of_a_widget() {
        let mut widgets = vec![("", WidgetKind::Column, (1..=12).collect::<Vec<u32>>())];
        let labels: Vec<String> = (1..=12)
            .map(|n| format!("Reference {n} which is long enough to wrap"))
            .collect();
        for (n, l) in labels.iter().enumerate() {
            widgets.push((
                ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"][n],
                item(l, None, None),
                vec![],
            ));
        }
        let p = panel(widgets);
        let caps = Caps::full();
        let area = Rect::new(0, 0, 24, 10);
        let mut buf = Buffer::empty(area);
        let mut v = PluginPanelView::default();
        v.draw(&mut buf, area, &p, &caps);
        let row = |buf: &Buffer, y: u16| -> String {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        };
        // Each reference over two lines: the first on rows 2 and 3, the
        // last column marking more lines below.
        assert_eq!(row(&buf, 2).trim_end(), " Reference 1 which is");
        assert_eq!(row(&buf, 3).trim_end(), " long enough to wrap");
        assert_eq!(row(&buf, 9).trim_end(), " long enough to wrap   ▼");
        // A click on either line, anywhere to the panel's edge, is the
        // first reference's; on the title, none.
        assert_eq!(v.at(3, 2), Some(1));
        assert_eq!(v.at(20, 3), Some(1));
        assert_eq!(v.at(1, 4), Some(2));
        assert_eq!(v.at(1, 0), None);
        // Scrolled by three lines: the second reference's second line on
        // top, a mark for the lines above; clicks follow.
        v.scroll(3);
        v.draw(&mut buf, area, &p, &caps);
        assert_eq!(row(&buf, 2).trim_end(), " long enough to wrap   ▲");
        assert_eq!(v.at(1, 2), Some(2));
        assert_eq!(v.at(1, 3), Some(3));
        // Never past the last line.
        v.scroll(100);
        v.draw(&mut buf, area, &p, &caps);
        assert_eq!(v.top, 24 - 8);
        assert_eq!(row(&buf, 9).trim_end(), " long enough to wrap");
        // The widget chosen by keys, brought into view.
        v.selected = Some("a".into());
        v.follow = true;
        v.draw(&mut buf, area, &p, &caps);
        assert_eq!(v.top, 0);
        v.selected = Some("l".into());
        v.follow = true;
        v.draw(&mut buf, area, &p, &caps);
        assert_eq!(v.top, 16);
        assert_eq!(v.place(12), Some((22, 23)));
    }
}
