//! The file manager (design §2.7): a folder as a read-only document that
//! lists its entries, like Emacs's Dired, and the projects view, which
//! lists every project as if they were all in one folder; opening a
//! project lists its folder, and going up from there shows the projects
//! again.
//!
//! The listing is text, so the cursor, search, Vim motions and scrolling
//! work as in any document; this module keeps what each line stands for,
//! the marks and the listing options, and says how each part is styled.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};

use kalem_fs::{Entry, Kind, ListOptions, SortKey};

use crate::l10n::tr;
use crate::projects::tilde;

/// A project in the projects view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRow {
    /// Its name.
    pub name: String,
    /// Its folder.
    pub root: PathBuf,
    /// When it was last used, in seconds since 1970.
    pub used: u64,
    /// Whether the folder exists.
    pub exists: bool,
}

/// What the file manager shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Place {
    /// Every project.
    Projects,
    /// A folder.
    Dir(PathBuf),
}

/// A mark on an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    /// `*`: commands act on marked entries.
    Marked,
    /// `D`: flagged for deletion.
    Flagged,
}

impl Mark {
    fn char(self) -> char {
        match self {
            Mark::Marked => '*',
            Mark::Flagged => 'D',
        }
    }
}

/// What a line of the listing stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The folder's path, or the title of the projects view.
    Header,
    /// A message: an error, or how to add projects.
    Note,
    /// `..`, the parent folder.
    Parent,
    /// An entry of the folder or of a folder listed in it, by index.
    Entry(usize),
    /// The path of a folder listed inside the listing (`i` in Dired), by
    /// index in [`DirState::subdirs`].
    Subdir(usize),
    /// A project, by index.
    Project(usize),
}

/// How a part of a line looks; frontends choose the colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirStyle {
    /// The folder's path.
    Header,
    /// Permissions, size, time, a link's target, a project's folder.
    Detail,
    /// A folder's name.
    Dir,
    /// A symbolic link's name.
    Link,
    /// A link to nothing, a missing project, a flagged entry.
    Broken,
    /// A program.
    Executable,
    /// A dot file.
    Hidden,
    /// A marked entry.
    Marked,
    /// An entry flagged for deletion.
    Flagged,
    /// A message.
    Note,
}

/// The styled parts of a line, in bytes from its start.
type Styles = Vec<(Range<usize>, DirStyle)>;

/// The state of a file manager document.
#[derive(Debug, Clone)]
pub struct DirState {
    /// What it shows.
    pub place: Place,
    /// The project opened from the projects view: going up from its folder
    /// shows the projects again.
    pub via_projects: Option<PathBuf>,
    /// The folder shown before the projects view, to go back to.
    pub before_projects: Option<PathBuf>,
    /// The folder's entries, sorted and filtered.
    pub entries: Vec<Entry>,
    /// The parent folder (the `..` line).
    pub parent: Option<Entry>,
    /// The projects of the projects view.
    pub projects: Vec<ProjectRow>,
    /// Marks by path; they stay when the listing is read again.
    pub marks: BTreeMap<PathBuf, Mark>,
    /// Sorting and hidden files.
    pub options: ListOptions,
    /// Permissions, sizes and times shown.
    pub details: bool,
    /// Only names containing this (case ignored) are shown.
    pub filter: String,
    /// Why the folder could not be read.
    pub error: Option<String>,
    /// Folders listed below the folder's own entries, in order, as Dired
    /// inserts subdirectories.
    pub subdirs: Vec<PathBuf>,
    /// The entries of each listed folder: the folder's own first, then
    /// each of `subdirs`, with why it could not be read.
    sections: Vec<(PathBuf, Range<usize>, Option<String>)>,
    /// The folder each line is in (an index in `sections`).
    row_sections: Vec<usize>,
    rows: Vec<Row>,
    /// The path of each line when it was written (it stays right while
    /// the folder is read again, until the listing is written again).
    paths: Vec<Option<PathBuf>>,
    /// The name's bytes in each line.
    names: Vec<Range<usize>>,
    styles: Vec<Styles>,
    /// The names being edited as text (wdired), with the listing as it
    /// was.
    pub wdired: Option<Wdired>,
}

/// A listing whose names are being edited: each line as it was, cut
/// around the name, with the path of an entry's line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wdired {
    lines: Vec<(String, String, String, Option<PathBuf>)>,
}

impl DirState {
    /// The state for `place`, not read yet.
    pub fn new(place: Place, options: ListOptions, details: bool) -> DirState {
        DirState {
            place,
            via_projects: None,
            before_projects: None,
            entries: Vec::new(),
            parent: None,
            projects: Vec::new(),
            marks: BTreeMap::new(),
            options,
            details,
            filter: String::new(),
            error: None,
            subdirs: Vec::new(),
            sections: Vec::new(),
            row_sections: Vec::new(),
            rows: Vec::new(),
            paths: Vec::new(),
            names: Vec::new(),
            styles: Vec::new(),
            wdired: None,
        }
    }

    /// The folder shown, if any.
    pub fn dir(&self) -> Option<&Path> {
        match &self.place {
            Place::Dir(d) => Some(d),
            Place::Projects => None,
        }
    }

    /// Reads the folder again (marks of entries that are gone go too).
    pub fn load(&mut self) {
        let Place::Dir(dir) = &self.place else {
            self.sort_projects();
            return;
        };
        let dir = dir.clone();
        // Listed folders that are gone, or no longer inside, go.
        self.subdirs
            .retain(|d| d.starts_with(&dir) && *d != dir && d.is_dir());
        self.entries.clear();
        self.sections.clear();
        self.error = None;
        for (k, d) in std::iter::once(dir.clone())
            .chain(self.subdirs.clone())
            .enumerate()
        {
            let start = self.entries.len();
            let error = match kalem_fs::read_dir(&d, &self.options) {
                Ok(mut entries) => {
                    if !self.filter.is_empty() {
                        let f = self.filter.to_lowercase();
                        entries.retain(|e| e.name.to_lowercase().contains(&f));
                    }
                    self.entries.extend(entries);
                    None
                }
                Err(e) => Some(e.to_string()),
            };
            if k == 0 {
                self.error = error.clone();
            }
            self.sections.push((d, start..self.entries.len(), error));
        }
        self.parent = dir.parent().and_then(|p| Entry::read(p).ok()).map(|mut e| {
            e.name = "..".into();
            e
        });
        let listed: Vec<PathBuf> = self.sections.iter().map(|s| s.0.clone()).collect();
        self.marks.retain(|p, _| {
            p.parent().is_some_and(|d| listed.iter().any(|l| l == d))
                && std::fs::symlink_metadata(p).is_ok()
        });
    }

    /// The folder line `line` is in: the folder shown, or a folder listed
    /// in it (`dired-current-directory`).
    pub fn dir_at(&self, line: usize) -> Option<&Path> {
        let k = self.row_sections.get(line).copied().unwrap_or(0);
        match self.sections.get(k) {
            Some((d, _, _)) => Some(d),
            None => self.dir(),
        }
    }

    /// The line of listed folder `dir`'s path, if it is listed.
    pub fn subdir_line(&self, dir: &Path) -> Option<usize> {
        let k = self.subdirs.iter().position(|d| d == dir)?;
        self.rows.iter().position(|r| *r == Row::Subdir(k))
    }

    /// Sets the projects of the projects view.
    pub fn set_projects(&mut self, rows: Vec<ProjectRow>) {
        self.projects = rows;
        self.sort_projects();
    }

    fn sort_projects(&mut self) {
        let by_time = self.options.sort == SortKey::Time;
        self.projects.sort_by(|a, b| {
            let o = if by_time {
                b.used.cmp(&a.used)
            } else {
                a.name.to_lowercase().cmp(&b.name.to_lowercase())
            };
            if self.options.reverse { o.reverse() } else { o }
        });
    }

    /// The listing's text; what each line is and how it looks are kept.
    pub fn render(&mut self) -> String {
        self.rows.clear();
        self.names.clear();
        self.styles.clear();
        self.row_sections.clear();
        let mut lines: Vec<String> = Vec::new();
        let section = std::cell::Cell::new(0);
        let mut push = |this: &mut DirState, line: String, row: Row, name: Range<usize>, styles| {
            lines.push(line);
            this.rows.push(row);
            this.names.push(name);
            this.styles.push(styles);
            this.row_sections.push(section.get());
        };
        match self.place.clone() {
            Place::Projects => {
                let title = format!("  {}:", tr("fm-projects"));
                let len = title.len();
                push(
                    self,
                    title,
                    Row::Header,
                    2..len - 1,
                    vec![(2..len, DirStyle::Header)],
                );
                if self.projects.is_empty() {
                    let note = format!("  {}", tr("fm-no-projects"));
                    let len = note.len();
                    push(
                        self,
                        note,
                        Row::Note,
                        2..len,
                        vec![(2..len, DirStyle::Note)],
                    );
                }
                let width = self
                    .projects
                    .iter()
                    .map(|p| p.name.chars().count())
                    .max()
                    .unwrap_or(0);
                for i in 0..self.projects.len() {
                    let p = self.projects[i].clone();
                    let mut line = String::from("  ");
                    let name = line.len()..line.len() + p.name.len();
                    line.push_str(&p.name);
                    let mut styles = vec![(
                        name.clone(),
                        if p.exists {
                            DirStyle::Dir
                        } else {
                            DirStyle::Broken
                        },
                    )];
                    if self.details {
                        line.push_str(&" ".repeat(width - p.name.chars().count() + 2));
                        let at = line.len();
                        line.push_str(&tilde(&p.root));
                        styles.push((at..line.len(), DirStyle::Detail));
                        if !p.exists {
                            let at = line.len() + 2;
                            line.push_str(&format!("  ({})", tr("project-missing")));
                            styles.push((at..line.len(), DirStyle::Broken));
                        }
                    }
                    push(self, line, Row::Project(i), name, styles);
                }
            }
            Place::Dir(dir) => {
                let mut title = format!("  {}:", tilde(&dir));
                let len = title.len();
                let mut styles = vec![(2..len, DirStyle::Header)];
                if !self.filter.is_empty() {
                    let at = title.len() + 1;
                    title.push_str(&format!(
                        " ({})",
                        crate::tr!("fm-filter", text = self.filter.clone())
                    ));
                    styles.push((at..title.len(), DirStyle::Detail));
                }
                push(self, title, Row::Header, 2..len - 1, styles);
                if let Some(e) = self.error.clone() {
                    let note = format!("  {e}");
                    let len = note.len();
                    push(
                        self,
                        note,
                        Row::Note,
                        2..len,
                        vec![(2..len, DirStyle::Broken)],
                    );
                }
                let size_width = self
                    .entries
                    .iter()
                    .chain(self.parent.as_ref())
                    .map(|e| size_text(e).len())
                    .max()
                    .unwrap_or(1);
                if let Some(p) = self.parent.clone() {
                    let (line, name, styles) = self.entry_line(&p, None, size_width);
                    push(self, line, Row::Parent, name, styles);
                }
                for k in 0..self.sections.len() {
                    let (d, range, error) = self.sections[k].clone();
                    if k > 0 {
                        // A blank line, then the folder's path, as Dired
                        // writes an inserted subdirectory.
                        push(self, String::new(), Row::Note, 0..0, Vec::new());
                        section.set(k);
                        let title = format!("  {}:", tilde(&d));
                        let len = title.len();
                        push(
                            self,
                            title,
                            Row::Subdir(k - 1),
                            2..len - 1,
                            vec![(2..len, DirStyle::Header)],
                        );
                        if let Some(e) = error {
                            let note = format!("  {e}");
                            let len = note.len();
                            push(
                                self,
                                note,
                                Row::Note,
                                2..len,
                                vec![(2..len, DirStyle::Broken)],
                            );
                        }
                    }
                    for i in range {
                        let e = self.entries[i].clone();
                        let mark = self.marks.get(&e.path).copied();
                        let (line, name, styles) = self.entry_line(&e, mark, size_width);
                        push(self, line, Row::Entry(i), name, styles);
                    }
                }
            }
        }
        self.paths = (0..self.rows.len()).map(|l| self.row_path(l)).collect();
        lines.join("\n")
    }

    /// An entry's line, the name's bytes in it and its styles.
    fn entry_line(
        &self,
        e: &Entry,
        mark: Option<Mark>,
        size_width: usize,
    ) -> (String, Range<usize>, Styles) {
        let mut line = String::new();
        let mut styles = Vec::new();
        match mark {
            Some(m) => {
                line.push(m.char());
                styles.push((
                    0..1,
                    if m == Mark::Marked {
                        DirStyle::Marked
                    } else {
                        DirStyle::Flagged
                    },
                ));
            }
            None => line.push(' '),
        }
        line.push(' ');
        if self.details {
            let at = line.len();
            line.push_str(&kalem_fs::permissions(e));
            line.push(' ');
            let size = size_text(e);
            line.push_str(&" ".repeat(size_width.saturating_sub(size.len()) + 1));
            line.push_str(&size);
            line.push_str("  ");
            line.push_str(
                &e.modified
                    .map(kalem_fs::format_time)
                    .unwrap_or_else(|| " ".repeat(16)),
            );
            styles.push((at..line.len(), DirStyle::Detail));
            line.push_str("  ");
        }
        let name = line.len()..line.len() + e.name.len();
        line.push_str(&e.name);
        let style = match (mark, &e.kind) {
            (Some(Mark::Marked), _) => DirStyle::Marked,
            (Some(Mark::Flagged), _) => DirStyle::Flagged,
            (_, Kind::Symlink { broken: true, .. }) => DirStyle::Broken,
            (_, Kind::Symlink { .. }) => DirStyle::Link,
            (_, Kind::Dir) => DirStyle::Dir,
            _ if e.is_hidden() => DirStyle::Hidden,
            _ if e.executable => DirStyle::Executable,
            _ => DirStyle::Detail,
        };
        if style != DirStyle::Detail {
            styles.push((name.clone(), style));
        }
        if e.is_dir() && !matches!(e.kind, Kind::Symlink { .. }) {
            line.push('/');
        }
        if let Kind::Symlink { target, .. } = &e.kind
            && self.details
        {
            let at = line.len();
            line.push_str(&format!(" -> {}", target.display()));
            styles.push((at..line.len(), DirStyle::Detail));
        }
        (line, name, styles)
    }

    /// What line `line` stands for.
    pub fn row(&self, line: usize) -> Option<Row> {
        self.rows.get(line).copied()
    }

    /// The name's bytes in line `line`.
    pub fn name_range(&self, line: usize) -> Option<Range<usize>> {
        if self.wdired.is_some() {
            return None;
        }
        self.names.get(line).cloned()
    }

    /// The styled parts of line `line`, in bytes from its start (none
    /// while the names are edited).
    pub fn styles(&self, line: usize) -> &[(Range<usize>, DirStyle)] {
        if self.wdired.is_some() {
            return &[];
        }
        self.styles.get(line).map_or(&[], Vec::as_slice)
    }

    /// The entry of line `line`, if it is one.
    pub fn entry(&self, line: usize) -> Option<&Entry> {
        match self.row(line)? {
            Row::Entry(i) => self.entries.get(i),
            Row::Parent => self.parent.as_ref(),
            _ => None,
        }
    }

    /// The file, folder or project of line `line`.
    pub fn path_at(&self, line: usize) -> Option<PathBuf> {
        self.paths.get(line).cloned().flatten()
    }

    fn row_path(&self, line: usize) -> Option<PathBuf> {
        match self.row(line)? {
            Row::Entry(i) => self.entries.get(i).map(|e| e.path.clone()),
            Row::Parent => self.dir().and_then(Path::parent).map(Path::to_path_buf),
            Row::Project(i) => self.projects.get(i).map(|p| p.root.clone()),
            Row::Subdir(k) => self.subdirs.get(k).cloned(),
            Row::Header | Row::Note => None,
        }
    }

    /// The line of `path`'s entry or project.
    pub fn line_of(&self, path: &Path) -> Option<usize> {
        self.rows.iter().position(|r| match r {
            Row::Entry(i) => self.entries.get(*i).is_some_and(|e| e.path == path),
            Row::Project(i) => self.projects.get(*i).is_some_and(|p| p.root == path),
            _ => false,
        })
    }

    /// The first line of an entry or project (after the header and `..`).
    pub fn first_entry_line(&self) -> Option<usize> {
        self.rows
            .iter()
            .position(|r| matches!(r, Row::Entry(_) | Row::Project(_)))
    }

    /// The entries commands act on: the marked ones, else the one of line
    /// `line`.
    pub fn targets(&self, line: usize) -> Vec<PathBuf> {
        let marked: Vec<PathBuf> = self
            .entries
            .iter()
            .filter(|e| self.marks.get(&e.path) == Some(&Mark::Marked))
            .map(|e| e.path.clone())
            .collect();
        if !marked.is_empty() {
            return marked;
        }
        match self.row(line) {
            Some(Row::Entry(i)) => self
                .entries
                .get(i)
                .map(|e| vec![e.path.clone()])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// The entries flagged for deletion, in listing order.
    pub fn flagged(&self) -> Vec<PathBuf> {
        self.entries
            .iter()
            .filter(|e| self.marks.get(&e.path) == Some(&Mark::Flagged))
            .map(|e| e.path.clone())
            .collect()
    }

    /// The name the list of open files gives the file manager: "File
    /// Manager", or "Projects" in the projects view.
    pub fn list_title(&self) -> String {
        match &self.place {
            Place::Projects => tr("fm-projects"),
            Place::Dir(_) => tr("menu-file-manager"),
        }
    }

    /// The title of the document: the folder's name, or "Projects".
    pub fn title(&self) -> String {
        match &self.place {
            Place::Projects => tr("fm-projects"),
            Place::Dir(d) => {
                let name = d.file_name().map_or_else(
                    || d.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                );
                format!("{name}/")
            }
        }
    }
}

fn size_text(e: &Entry) -> String {
    if e.is_dir() {
        "-".into()
    } else {
        kalem_fs::format_size(e.size)
    }
}

/// The listing options from the settings (`files.*`).
pub fn options_from(config: &crate::settings::Config) -> (ListOptions, bool) {
    let options = ListOptions {
        sort: SortKey::from_name(config.str("files.sort")).unwrap_or_default(),
        reverse: false,
        dirs_first: config.bool("files.directories_first"),
        hidden: config.bool("files.show_hidden"),
    };
    (options, config.bool("files.details"))
}

/// The moment `text` names for "changed since": an age (`30m`, `2h`,
/// `3d`, `1w`, before `now`), `today`, `yesterday`, or a date or time
/// (`2026-09-01`, `2026-09-01 14:30`) at `clock`'s time zone.
pub fn parse_since(
    text: &str,
    clock: jiff::civil::DateTime,
    now: std::time::SystemTime,
) -> Option<std::time::SystemTime> {
    let t = text.trim().to_lowercase();
    let day = |d: jiff::civil::Date| -> Option<std::time::SystemTime> {
        let z = d.to_zoned(jiff::tz::TimeZone::system()).ok()?;
        Some(std::time::SystemTime::from(z.timestamp()))
    };
    match t.as_str() {
        "today" | "bugün" => return day(clock.date()),
        "yesterday" | "dün" => return day(clock.date().yesterday().ok()?),
        _ => {}
    }
    if let Some(unit) = t.chars().last().filter(|c| c.is_ascii_alphabetic())
        && let Ok(n) = t[..t.len() - 1].trim().parse::<u64>()
    {
        let secs = match unit {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            'd' => 86_400,
            'w' => 7 * 86_400,
            _ => return None,
        };
        return now.checked_sub(std::time::Duration::from_secs(n * secs));
    }
    if let Ok(dt) = t.parse::<jiff::civil::DateTime>() {
        let z = dt.to_zoned(jiff::tz::TimeZone::system()).ok()?;
        return Some(std::time::SystemTime::from(z.timestamp()));
    }
    day(t.parse::<jiff::civil::Date>().ok()?)
}

/// Where `input` points, from the folder `dir`: `~` is the home folder,
/// relative paths start at `dir`.
pub fn resolve(dir: &Path, input: &str) -> PathBuf {
    let p = PathBuf::from(crate::settings::expand_home(input.trim()));
    if p.is_absolute() { p } else { dir.join(p) }
}

// Showing places.

/// Shows the projects in the file manager `doc`, the cursor on `select`;
/// the folder it showed is remembered, to go back to.
pub fn show_projects(doc: &mut DocumentState, rows: Vec<ProjectRow>, select: Option<&Path>) {
    let Some(s) = doc.dired.as_mut() else { return };
    if let Place::Dir(d) = &s.place {
        s.before_projects = Some(d.clone());
    }
    s.set_projects(rows);
    doc.visit(Place::Projects, select);
}

/// The rows of the projects view for `list`.
pub fn project_rows(list: &kalem_project::Projects) -> Vec<ProjectRow> {
    list.list
        .iter()
        .map(|p| ProjectRow {
            name: p.name.clone(),
            root: p.root.clone(),
            used: p.used,
            exists: p.root.is_dir(),
        })
        .collect()
}

// Questions and background work for file operations.

/// A question a file operation asks before it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Question {
    /// Yes or no.
    Confirm(String),
    /// A destination exists: overwrite, skip or keep both, for it or for
    /// every one left.
    Conflict {
        /// The question.
        text: String,
        /// The destination.
        path: PathBuf,
        /// Conflicts after this one.
        more: usize,
    },
}

/// An answer to a [`Question`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Go ahead.
    Yes,
    /// Stop: nothing is done.
    No,
    /// Replace this destination (merge into a folder).
    Overwrite,
    /// Leave this source alone.
    Skip,
    /// Give this one a new name.
    KeepBoth,
    /// Replace this destination and the ones after it.
    OverwriteAll,
    /// Skip this and the ones after it.
    SkipAll,
    /// Give this and the ones after it new names.
    KeepBothAll,
}

/// A file operation on its way: questions, then the background job.
#[derive(Debug)]
pub struct Task {
    op: kalem_fs::Operation,
    confirm: Option<String>,
    conflicts: Vec<PathBuf>,
}

fn names_of(paths: &[PathBuf]) -> String {
    match paths {
        [one] => one.file_name().map_or_else(
            || one.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
        many => crate::tr!("fm-items", count = many.len()),
    }
}

impl Task {
    /// The task for `req`.
    pub fn new(req: &crate::command::FileOp) -> Result<Task, String> {
        use kalem_fs::OpKind;
        if req.sources.is_empty() {
            return Err(tr("fm-nothing"));
        }
        let what = names_of(&req.sources);
        let (op, confirm) = match req.kind {
            OpKind::Copy | OpKind::Move => {
                let target = req.target.as_ref().ok_or_else(|| tr("fm-no-target"))?;
                let op = kalem_fs::Operation::transfer(req.kind, &req.sources, target)
                    .map_err(|e| e.to_string())?;
                (op, None)
            }
            OpKind::Trash => (
                kalem_fs::Operation::remove(req.kind, &req.sources),
                Some(crate::tr!("fm-confirm-trash", what = what)),
            ),
            OpKind::Delete => (
                kalem_fs::Operation::remove(req.kind, &req.sources),
                Some(crate::tr!("fm-confirm-delete", what = what)),
            ),
        };
        let conflicts = kalem_fs::conflicts(&op);
        Ok(Task {
            op,
            confirm,
            conflicts,
        })
    }

    /// The next question, if any.
    pub fn question(&self) -> Option<Question> {
        if let Some(c) = &self.confirm {
            return Some(Question::Confirm(c.clone()));
        }
        let path = self.conflicts.first()?.clone();
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        Some(Question::Conflict {
            text: crate::tr!("fm-conflict", name = name),
            path,
            more: self.conflicts.len() - 1,
        })
    }

    /// Takes the answer to the current question; `false` when the task is
    /// given up.
    pub fn answer(&mut self, a: Answer) -> bool {
        use kalem_fs::Conflict;
        if self.confirm.is_some() {
            if a != Answer::Yes {
                return false;
            }
            self.confirm = None;
            return true;
        }
        let Some(path) = self.conflicts.first().cloned() else {
            return true;
        };
        let (choice, all) = match a {
            Answer::Overwrite => (Conflict::Overwrite, false),
            Answer::Skip => (Conflict::Skip, false),
            Answer::KeepBoth => (Conflict::KeepBoth, false),
            Answer::OverwriteAll => (Conflict::Overwrite, true),
            Answer::SkipAll => (Conflict::Skip, true),
            Answer::KeepBothAll => (Conflict::KeepBoth, true),
            Answer::Yes => (Conflict::Overwrite, false),
            Answer::No => return false,
        };
        if all {
            for p in self.conflicts.drain(..) {
                self.op.choices.insert(p, choice);
            }
        } else {
            self.op.choices.insert(path, choice);
            self.conflicts.remove(0);
        }
        true
    }

    /// Starts the work in the background.
    pub fn start(self) -> Running {
        let count = self.op.items.len();
        let kind = self.op.kind;
        Running {
            job: kalem_fs::Job::start(self.op),
            kind,
            count,
        }
    }
}

/// A file operation running in the background.
#[derive(Debug)]
pub struct Running {
    job: kalem_fs::Job,
    kind: kalem_fs::OpKind,
    count: usize,
}

fn verb(kind: kalem_fs::OpKind) -> &'static str {
    match kind {
        kalem_fs::OpKind::Copy => "fm-copying",
        kalem_fs::OpKind::Move => "fm-moving",
        kalem_fs::OpKind::Trash => "fm-trashing",
        kalem_fs::OpKind::Delete => "fm-deleting",
    }
}

impl Running {
    /// What the status bar says: `Copying… 45%`.
    pub fn status(&self) -> String {
        let p = self.job.progress();
        let percent = (p.fraction() * 100.0).round() as u32;
        crate::tr!(verb(self.kind), percent = percent, count = self.count)
    }

    /// The result, once finished, with the message for the user and
    /// whether it is an error.
    pub fn poll(&mut self) -> Option<(kalem_fs::Outcome, String, bool)> {
        let out = self.job.take_outcome()?;
        record(self.kind, &out);
        let msg = outcome_message(self.kind, &out);
        let error = !out.errors.is_empty();
        Some((out, msg, error))
    }

    /// Asks it to stop.
    pub fn cancel(&self) {
        self.job.cancel();
    }

    /// Waits for the end (tests).
    pub fn wait(self) -> kalem_fs::Outcome {
        let out = self.job.wait();
        record(self.kind, &out);
        out
    }
}

/// A file operation that can be taken back (T2.7e.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    /// Renamed or moved: each item's old path and new one.
    Moved(Vec<(PathBuf, PathBuf)>),
    /// Moved to the trash, from these paths.
    Trashed(Vec<PathBuf>),
}

thread_local! {
    /// The file operations done from this thread (the frontend's), the
    /// last at the end.
    static HISTORY: std::cell::RefCell<Vec<Undo>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Remembers what an operation did, to undo it later.
fn record(kind: kalem_fs::OpKind, out: &kalem_fs::Outcome) {
    let undo = match kind {
        kalem_fs::OpKind::Move => Undo::Moved(
            out.done
                .iter()
                .filter_map(|(s, d)| Some((s.clone(), d.clone()?)))
                .collect(),
        ),
        kalem_fs::OpKind::Trash => Undo::Trashed(out.done.iter().map(|(s, _)| s.clone()).collect()),
        _ => return,
    };
    if matches!(&undo, Undo::Moved(v) if v.is_empty())
        || matches!(&undo, Undo::Trashed(v) if v.is_empty())
    {
        return;
    }
    HISTORY.with(|h| {
        let mut h = h.borrow_mut();
        h.push(undo);
        if h.len() > 100 {
            h.remove(0);
        }
    });
}

fn name_of(p: &Path) -> String {
    p.file_name().map_or_else(
        || p.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Takes back the last rename, move or trashing: the paths back in place
/// and the message, or why it cannot (the operation is kept then).
pub fn undo_last() -> Result<(Vec<PathBuf>, String), String> {
    let undo = HISTORY
        .with(|h| h.borrow_mut().pop())
        .ok_or_else(|| tr("fm-nothing-to-undo"))?;
    let result = match &undo {
        Undo::Moved(items) => {
            // Everything checked before anything moves: each old path free
            // (or one this undo frees), each new one there.
            let back: Vec<(PathBuf, PathBuf)> =
                items.iter().map(|(a, b)| (b.clone(), a.clone())).collect();
            let freed: std::collections::HashSet<&PathBuf> =
                items.iter().map(|(_, to)| to).collect();
            if let Some((from, _)) = items.iter().find(|(from, to)| {
                (std::fs::symlink_metadata(from).is_ok() && !freed.contains(from))
                    || std::fs::symlink_metadata(to).is_err()
            }) {
                Err(crate::tr!("fm-undo-blocked", name = name_of(from)))
            } else {
                let (done, error) = rename_all(&back);
                match error {
                    Some(e) => {
                        if !done.is_empty() {
                            // What moved back is done; the rest stays to undo.
                            let left: Vec<(PathBuf, PathBuf)> = items
                                .iter()
                                .filter(|(from, _)| !done.iter().any(|(_, d)| d == from))
                                .cloned()
                                .collect();
                            HISTORY.with(|h| h.borrow_mut().push(Undo::Moved(left)));
                            return Err(e);
                        }
                        Err(e)
                    }
                    None => Ok((
                        items.iter().map(|(from, _)| from.clone()).collect(),
                        crate::tr!("fm-undone-moved", count = items.len()),
                    )),
                }
            }
        }
        Undo::Trashed(paths) => kalem_fs::restore(paths).map(|()| {
            (
                paths.clone(),
                crate::tr!("fm-undone-trashed", count = paths.len()),
            )
        }),
    };
    if result.is_err() {
        HISTORY.with(|h| h.borrow_mut().push(undo));
    }
    result
}

/// The names of `doc`'s listing made editable (wdired).
pub fn edit_names(doc: &mut DocumentState) -> Result<(), String> {
    let text = doc.text().as_str().to_string();
    let s = doc
        .dired
        .as_deref_mut()
        .ok_or_else(|| tr("fm-not-listing"))?;
    if s.dir().is_none() {
        return Err(tr("fm-not-listing"));
    }
    let lines = text
        .split('\n')
        .enumerate()
        .map(|(i, line)| {
            let path = match s.rows.get(i) {
                Some(Row::Entry(_)) => s.paths.get(i).cloned().flatten(),
                _ => None,
            };
            match (s.names.get(i), &path) {
                (Some(r), Some(_)) if r.end <= line.len() => (
                    line[..r.start].to_string(),
                    line[r.clone()].to_string(),
                    line[r.end..].to_string(),
                    path,
                ),
                _ => (line.to_string(), String::new(), String::new(), None),
            }
        })
        .collect();
    s.wdired = Some(Wdired { lines });
    Ok(())
}

/// The renames the edited names ask for, checked: every line kept, no
/// name empty, no two the same, none onto a file that stays.
fn planned_renames(w: &Wdired, text: &str) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let new: Vec<&str> = text.split('\n').collect();
    if new.len() != w.lines.len() {
        return Err(tr("fm-wdired-lines"));
    }
    let mut plan = Vec::new();
    for ((prefix, name, suffix, path), line) in w.lines.iter().zip(&new) {
        let Some(path) = path else {
            if prefix != line {
                return Err(tr("fm-wdired-lines"));
            }
            continue;
        };
        let middle = line
            .strip_prefix(prefix.as_str())
            .and_then(|l| l.strip_suffix(suffix.as_str()))
            .ok_or_else(|| tr("fm-wdired-lines"))?;
        if middle == name {
            continue;
        }
        if middle.trim().is_empty() {
            return Err(crate::tr!("fm-wdired-empty", name = name.as_str()));
        }
        let parent = path.parent().unwrap_or(Path::new(""));
        plan.push((path.clone(), resolve(parent, middle)));
    }
    let sources: std::collections::HashSet<&PathBuf> = plan.iter().map(|(s, _)| s).collect();
    let mut seen = std::collections::HashSet::new();
    for (_, to) in &plan {
        if !seen.insert(to) {
            return Err(crate::tr!("fm-wdired-duplicate", name = name_of(to)));
        }
        if std::fs::symlink_metadata(to).is_ok() && !sources.contains(to) {
            return Err(crate::tr!("fm-exists", name = to.display().to_string()));
        }
        if !to.parent().is_some_and(Path::is_dir) {
            return Err(crate::tr!(
                "fm-wdired-no-folder",
                name = to.display().to_string()
            ));
        }
    }
    Ok(plan)
}

/// Renames each `from` to its `to` at once: each to a free temporary
/// name beside it first, so names can swap. What was renamed, and the
/// first error (its item left or put back where it was).
fn rename_all(plan: &[(PathBuf, PathBuf)]) -> (Vec<(PathBuf, PathBuf)>, Option<String>) {
    let mut parked: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (i, (from, _)) in plan.iter().enumerate() {
        let tmp = from.with_file_name(format!(".kalem-rename-{}-{i}", std::process::id()));
        if let Err(e) = kalem_fs::move_path(from, &tmp) {
            for (tmp, from) in parked.iter().rev() {
                let _ = kalem_fs::move_path(tmp, from);
            }
            return (Vec::new(), Some(e.to_string()));
        }
        parked.push((tmp, from.clone()));
    }
    let mut done = Vec::new();
    let mut error = None;
    for ((tmp, from), (_, to)) in parked.iter().zip(plan) {
        match kalem_fs::move_path(tmp, to) {
            Ok(()) => done.push((from.clone(), to.clone())),
            Err(e) => {
                let _ = kalem_fs::move_path(tmp, from);
                error.get_or_insert(e.to_string());
            }
        }
    }
    (done, error)
}

/// Applies the edited names of `doc`'s listing: every rename at once
/// (through temporary names, so names can swap), the listing read again.
/// Returns how many were renamed.
pub fn commit_names(doc: &mut DocumentState) -> Result<usize, String> {
    let text = doc.text().as_str().to_string();
    let s = doc
        .dired
        .as_deref_mut()
        .ok_or_else(|| tr("fm-not-listing"))?;
    let w = s.wdired.as_ref().ok_or_else(|| tr("fm-not-listing"))?;
    let plan = planned_renames(w, &text)?;
    let (done, error) = rename_all(&plan);
    let n = done.len();
    let first = done.first().map(|(_, to)| to.clone());
    if !done.is_empty() {
        HISTORY.with(|h| h.borrow_mut().push(Undo::Moved(done)));
    }
    s.wdired = None;
    s.load();
    doc.show_listing(first.as_deref());
    match error {
        Some(e) => Err(e),
        None => Ok(n),
    }
}

/// Leaves editing the names, the listing as it is on disk.
pub fn abort_names(doc: &mut DocumentState) {
    if let Some(s) = doc.dired.as_deref_mut() {
        s.wdired = None;
    }
    doc.refresh_listing();
}

/// The message for what an operation did.
pub fn outcome_message(kind: kalem_fs::OpKind, out: &kalem_fs::Outcome) -> String {
    if let Some((p, e)) = out.errors.first() {
        let name = p.file_name().map_or_else(
            || p.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        return crate::tr!(
            "fm-failed",
            name = name,
            error = e.clone(),
            count = out.errors.len()
        );
    }
    let id = match kind {
        kalem_fs::OpKind::Copy => "fm-copied",
        kalem_fs::OpKind::Move => "fm-moved",
        kalem_fs::OpKind::Trash => "fm-trashed",
        kalem_fs::OpKind::Delete => "fm-deleted",
    };
    let mut msg = crate::tr!(id, count = out.done.len());
    if !out.skipped.is_empty() {
        msg.push_str(&format!(
            " ({})",
            crate::tr!("fm-skipped", count = out.skipped.len())
        ));
    }
    if out.cancelled {
        msg.push_str(&format!(" ({})", tr("fm-cancelled")));
    }
    msg
}

// Commands.

use crate::command::{
    Command, CommandError, CommandHandler, CommandResult, CommandSource, EditorContext,
    FileManagerRequest, FileOp, Request,
};
use crate::document::DocumentState;
use crate::keys::KeySequence;
use crate::when::WhenClause;
use serde_json::Value;

const IN_LISTING: &str = "editorMode == directory && !wdired";
const EDITING_NAMES: &str = "editorMode == directory && wdired";

fn cmd(
    id: &str,
    title: &str,
    keys: &[&str],
    when: Option<&str>,
    handler: fn(&mut EditorContext<'_>, &Value) -> CommandResult,
) -> Command {
    Command {
        id: id.into(),
        title: title.into(),
        category: "Files".into(),
        default_keys: keys
            .iter()
            .map(|k| KeySequence::parse(k).expect("valid default key"))
            .collect(),
        when: when.map(|w| WhenClause::parse(w).expect("valid when-clause")),
        handler: CommandHandler::Native(handler),
        args_schema: None,
        scope: None,
        source: CommandSource::Builtin,
    }
}

/// The active document if it is a file manager.
fn listing<'a>(ctx: &'a mut EditorContext<'_>) -> Result<&'a mut DocumentState, CommandError> {
    match ctx.document.as_deref_mut() {
        Some(d) if d.dired.is_some() => Ok(d),
        _ => Err(CommandError::new(tr("fm-not-listing"))),
    }
}

fn state(doc: &DocumentState) -> &DirState {
    doc.dired.as_deref().expect("a file manager")
}

fn state_mut(doc: &mut DocumentState) -> &mut DirState {
    doc.dired.as_deref_mut().expect("a file manager")
}

fn cursor_line(doc: &DocumentState) -> usize {
    doc.text().line_of(doc.selection.head)
}

/// The lines of the selection, or the cursor's line.
fn selected_lines(doc: &DocumentState) -> Range<usize> {
    let s = doc.selection;
    let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
    let first = doc.text().line_of(a);
    let last = doc.text().line_of(b);
    // A selection ending at a line's start leaves that line out.
    let last = if b > a && doc.text().line_range(last).start == b && last > first {
        last - 1
    } else {
        last
    };
    first..last + 1
}

fn arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, CommandError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::new(format!("Missing argument `{key}`")))
}

/// The folder of the cursor's line: the folder shown, or a folder listed
/// in it.
fn the_dir(doc: &DocumentState) -> Result<PathBuf, CommandError> {
    state(doc)
        .dir_at(cursor_line(doc))
        .map(Path::to_path_buf)
        .ok_or_else(|| CommandError::new(tr("fm-in-projects")))
}

/// Lists the folder at the cursor below the listing (Dired's `i`), or
/// goes to it if it is listed already.
fn insert_subdir(ctx: &mut EditorContext<'_>, _: &Value) -> CommandResult {
    let doc = listing(ctx)?;
    let line = cursor_line(doc);
    let s = state(doc);
    let dir = match (s.row(line), s.entry(line)) {
        (Some(Row::Entry(_)), Some(e)) if e.is_dir() => e.path.clone(),
        _ => return Err(CommandError::new(tr("fm-not-a-folder"))),
    };
    if !s.subdirs.contains(&dir) {
        let s = state_mut(doc);
        s.subdirs.push(dir.clone());
        s.load();
        doc.show_listing(None);
    }
    if let Some(l) = state(doc).subdir_line(&dir) {
        let r = doc.text().line_range(l);
        let column = state(doc).name_range(l).map_or(0, |n| n.start);
        doc.selection = org_edit::Selection::caret((r.start + column).min(r.end));
    }
    Ok(())
}

/// Takes the listed folder the cursor is in out of the listing; the
/// cursor goes to its line in its parent.
fn remove_subdir(ctx: &mut EditorContext<'_>, _: &Value) -> CommandResult {
    let doc = listing(ctx)?;
    let line = cursor_line(doc);
    let s = state(doc);
    let dir = s.dir_at(line).map(Path::to_path_buf);
    let Some(dir) = dir.filter(|d| s.subdirs.contains(d)) else {
        return Err(CommandError::new(tr("fm-no-subdir")));
    };
    let s = state_mut(doc);
    // Folders listed inside it go with it.
    s.subdirs.retain(|d| !d.starts_with(&dir));
    s.load();
    doc.selection = org_edit::Selection::caret(0);
    doc.show_listing(Some(&dir));
    Ok(())
}

/// Opens the entry at the cursor: a folder or project is listed, a file
/// opens as a document.
fn open(ctx: &mut EditorContext<'_>, _: &Value) -> CommandResult {
    let doc = listing(ctx)?;
    let line = cursor_line(doc);
    let s = state(doc);
    match s.row(line) {
        Some(Row::Parent) => return up(ctx, &Value::Null),
        Some(Row::Entry(_)) => {
            let e = s.entry(line).cloned().expect("an entry");
            if e.is_dir() {
                doc.visit(Place::Dir(e.path), None);
            } else {
                ctx.requests.push(Request::Open {
                    path: Some(e.path.display().to_string()),
                });
            }
        }
        Some(Row::Subdir(k)) => {
            let d = s.subdirs[k].clone();
            doc.visit(Place::Dir(d), None);
        }
        Some(Row::Project(i)) => {
            let p = s.projects[i].clone();
            if !p.exists {
                return Err(CommandError::new(crate::tr!(
                    "msg-project-missing",
                    name = p.name
                )));
            }
            state_mut(doc).via_projects = Some(p.root.clone());
            doc.visit(Place::Dir(p.root), None);
        }
        _ => {}
    }
    Ok(())
}

/// The cursor on the name of the line `by` lines down (up when
/// negative).
fn step(ctx: &mut EditorContext<'_>, by: isize) -> CommandResult {
    let doc = listing(ctx)?;
    let line = cursor_line(doc);
    let last = doc.text().line_count().saturating_sub(1);
    let target = line.saturating_add_signed(by).min(last);
    let column = state(doc).name_range(target).map_or(0, |r| r.start);
    let r = doc.text().line_range(target);
    doc.selection = org_edit::Selection::caret((r.start + column).min(r.end));
    Ok(())
}

/// The parent folder; from a project opened in the projects view, the
/// projects.
fn up(ctx: &mut EditorContext<'_>, _: &Value) -> CommandResult {
    let doc = listing(ctx)?;
    let s = state(doc);
    // In a listed folder: its line in its parent's listing.
    if let Some(d) = s.dir_at(cursor_line(doc)).map(Path::to_path_buf)
        && s.subdirs.contains(&d)
    {
        doc.show_listing(Some(&d));
        return Ok(());
    }
    let Place::Dir(d) = s.place.clone() else {
        return Ok(());
    };
    if s.via_projects.as_deref() == Some(d.as_path()) {
        ctx.requests
            .push(Request::FileManager(FileManagerRequest::Projects {
                select: Some(d),
            }));
        return Ok(());
    }
    if let Some(parent) = d.parent() {
        doc.visit(Place::Dir(parent.to_path_buf()), Some(&d));
    }
    Ok(())
}

/// Sets `mark` on the entries of the selected lines (the cursor's line),
/// then moves to the next line.
fn set_mark(ctx: &mut EditorContext<'_>, mark: Option<Mark>) -> CommandResult {
    let doc = listing(ctx)?;
    let lines = selected_lines(doc);
    let many = lines.len() > 1;
    let s = state_mut(doc);
    for l in lines.clone() {
        if let Some(Row::Entry(i)) = s.row(l) {
            let p = s.entries[i].path.clone();
            match mark {
                Some(m) => {
                    s.marks.insert(p, m);
                }
                None => {
                    s.marks.remove(&p);
                }
            }
        }
    }
    let next = if many {
        lines.end.saturating_sub(1)
    } else {
        lines.end
    };
    let next = next.min(doc.text().line_count().saturating_sub(1));
    let r = doc.text().line_range(next);
    doc.selection = org_edit::Selection::caret(r.start);
    doc.show_listing(None);
    // The cursor on the next line's name.
    let target = next;
    if let Some(n) = state(doc).name_range(target) {
        let r = doc.text().line_range(target);
        doc.selection = org_edit::Selection::caret((r.start + n.start).min(r.end));
    }
    Ok(())
}

fn mark_where(ctx: &mut EditorContext<'_>, f: &dyn Fn(&Entry) -> bool) -> CommandResult {
    let doc = listing(ctx)?;
    let s = state_mut(doc);
    let mut n = 0;
    for e in &s.entries {
        if f(e) {
            s.marks.insert(e.path.clone(), Mark::Marked);
            n += 1;
        }
    }
    doc.show_listing(None);
    ctx.messages.push(crate::tr!("fm-marked", count = n));
    Ok(())
}

fn file_op(
    ctx: &mut EditorContext<'_>,
    kind: kalem_fs::OpKind,
    sources: Vec<PathBuf>,
    target: Option<PathBuf>,
) -> CommandResult {
    if sources.is_empty() {
        return Err(CommandError::new(tr("fm-nothing")));
    }
    ctx.requests.push(Request::FileOp(FileOp {
        kind,
        sources,
        target,
    }));
    Ok(())
}

fn transfer(ctx: &mut EditorContext<'_>, args: &Value, kind: kalem_fs::OpKind) -> CommandResult {
    let doc = listing(ctx)?;
    let dir = the_dir(doc)?;
    let sources = state(doc).targets(cursor_line(doc));
    let target = resolve(&dir, arg(args, "target")?);
    file_op(ctx, kind, sources, Some(target))
}

/// After a change made here: the listing again, the cursor on `select`.
fn changed(doc: &mut DocumentState, select: Option<&Path>) {
    if let Some(s) = doc.dired.as_mut() {
        s.load();
    }
    doc.show_listing(select);
}

fn names(doc: &DocumentState, full: bool) -> String {
    let s = state(doc);
    s.targets(cursor_line(doc))
        .iter()
        .map(|p| {
            if full {
                p.display().to_string()
            } else {
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The argument schemas of the file manager's commands.
pub(crate) fn schemas() -> Vec<(&'static str, Value)> {
    let one = |name: &str| {
        serde_json::json!({
            "type": "object",
            "properties": { name: { "type": "string" } },
            "required": [name],
        })
    };
    vec![
        ("dired.copy", one("target")),
        ("dired.move", one("target")),
        ("dired.mkdir", one("name")),
        ("dired.newFile", one("name")),
        ("dired.symlink", one("link")),
        ("dired.chmod", one("mode")),
        ("dired.markRegexp", one("regexp")),
        ("dired.markExtension", one("extension")),
        ("dired.markChangedSince", one("since")),
        ("dired.filter", one("text")),
    ]
}

/// What the prompt for argument `name` of the file manager's command
/// `id` starts with.
pub(crate) fn argument_default(id: &str, name: &str, doc: &DocumentState) -> Option<String> {
    let s = doc.dired.as_deref()?;
    let dir = s.dir_at(cursor_line(doc))?;
    let sep = std::path::MAIN_SEPARATOR;
    let targets = s.targets(cursor_line(doc));
    Some(match (id, name) {
        ("dired.move", "target") if targets.len() == 1 => tilde(&targets[0]),
        ("dired.copy" | "dired.move", "target") => format!("{}{sep}", tilde(dir)),
        ("dired.symlink", "link") => targets
            .first()
            .map(|t| {
                let mut s = tilde(t);
                s.push_str(".link");
                s
            })
            .unwrap_or_default(),
        ("dired.chmod", "mode") => doc
            .dired
            .as_deref()
            .and_then(|s| s.entry(cursor_line(doc)))
            .map(|e| format!("{:o}", e.mode & 0o7777))
            .unwrap_or_default(),
        ("dired.filter", "text") => s.filter.clone(),
        _ => return None,
    })
}

/// The file manager's commands.
pub(crate) fn commands() -> Vec<Command> {
    use kalem_fs::OpKind;
    vec![
        // Opening the file manager.
        cmd(
            "dired.jump",
            "File Manager",
            &["ctrl+alt+d"],
            None,
            |ctx, _| {
                // From the file manager, the same key goes back to the
                // document.
                let listing = ctx.document.as_deref().is_some_and(|d| d.dired.is_some());
                ctx.requests.push(Request::FileManager(if listing {
                    FileManagerRequest::Leave
                } else {
                    FileManagerRequest::Dir { dir: None }
                }));
                Ok(())
            },
        ),
        cmd(
            "dired.projectRoot",
            "Project Folder in the File Manager",
            &[],
            Some("inProject"),
            |ctx, _| {
                ctx.requests
                    .push(Request::FileManager(FileManagerRequest::ProjectRoot));
                Ok(())
            },
        ),
        cmd(
            "dired.projects",
            "Projects View",
            &["ctrl+alt+shift+d"],
            None,
            |ctx, _| {
                // In the projects view: back to the folder shown before.
                if let Some(doc) = ctx.document.as_deref_mut()
                    && let Some(s) = doc.dired.as_deref()
                    && s.place == Place::Projects
                {
                    if let Some(back) = s.before_projects.clone() {
                        doc.visit(Place::Dir(back), None);
                    }
                    return Ok(());
                }
                ctx.requests
                    .push(Request::FileManager(FileManagerRequest::Projects {
                        select: None,
                    }));
                Ok(())
            },
        ),
        cmd(
            "dired.cancel",
            "Stop File Operations",
            &[],
            None,
            |ctx, _| {
                ctx.requests.push(Request::CancelFileOps);
                Ok(())
            },
        ),
        // Moving around.
        cmd("dired.open", "Open", &[], Some(IN_LISTING), open),
        cmd("dired.up", "Parent Folder", &[], Some(IN_LISTING), up),
        cmd(
            "dired.insertSubdir",
            "List Folder Here",
            &[],
            Some(IN_LISTING),
            insert_subdir,
        ),
        cmd(
            "dired.removeSubdir",
            "Remove Listed Folder",
            &[],
            Some(IN_LISTING),
            remove_subdir,
        ),
        cmd(
            "dired.next",
            "Next Line",
            &[],
            Some(IN_LISTING),
            |ctx, _| step(ctx, 1),
        ),
        cmd(
            "dired.first",
            "First Entry",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let first = state(doc).first_entry_line().unwrap_or(0);
                let line = cursor_line(doc);
                step(ctx, first as isize - line as isize)
            },
        ),
        cmd(
            "dired.previous",
            "Previous Line",
            &[],
            Some(IN_LISTING),
            |ctx, _| step(ctx, -1),
        ),
        cmd(
            "dired.refresh",
            "Read Again",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                if state(doc).place == Place::Projects {
                    let select = state(doc).path_at(cursor_line(doc));
                    ctx.requests
                        .push(Request::FileManager(FileManagerRequest::Projects {
                            select,
                        }));
                } else {
                    doc.refresh_listing();
                }
                Ok(())
            },
        ),
        cmd(
            "dired.close",
            "Close File Manager",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                ctx.requests.push(Request::Close);
                Ok(())
            },
        ),
        // How the listing looks.
        cmd(
            "dired.toggleDetails",
            "Show or Hide Details",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let s = state_mut(doc);
                s.details = !s.details;
                doc.show_listing(None);
                Ok(())
            },
        ),
        cmd(
            "dired.toggleHidden",
            "Show or Hide Dot Files",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let s = state_mut(doc);
                s.options.hidden = !s.options.hidden;
                let on = s.options.hidden;
                changed(doc, None);
                ctx.messages.push(tr(if on {
                    "fm-hidden-shown"
                } else {
                    "fm-hidden-hidden"
                }));
                Ok(())
            },
        ),
        cmd(
            "dired.sort",
            "Sort By Next Order",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let s = state_mut(doc);
                s.options.sort = if s.place == Place::Projects {
                    if s.options.sort == SortKey::Name {
                        SortKey::Time
                    } else {
                        SortKey::Name
                    }
                } else {
                    s.options.sort.next()
                };
                let key = s.options.sort.name();
                changed(doc, None);
                ctx.messages.push(crate::tr!(
                    "fm-sorted",
                    order = tr(&format!("fm-sort-{key}"))
                ));
                Ok(())
            },
        ),
        cmd(
            "dired.reverse",
            "Reverse Order",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let s = state_mut(doc);
                s.options.reverse = !s.options.reverse;
                changed(doc, None);
                Ok(())
            },
        ),
        cmd(
            "dired.filter",
            "Filter by Name",
            &[],
            Some(IN_LISTING),
            |ctx, args| {
                let text = arg(args, "text")?.trim().to_string();
                let doc = listing(ctx)?;
                state_mut(doc).filter = text;
                changed(doc, None);
                Ok(())
            },
        ),
        // Marks.
        cmd("dired.mark", "Mark", &[], Some(IN_LISTING), |ctx, _| {
            set_mark(ctx, Some(Mark::Marked))
        }),
        cmd("dired.unmark", "Unmark", &[], Some(IN_LISTING), |ctx, _| {
            set_mark(ctx, None)
        }),
        cmd(
            "dired.flag",
            "Flag for Deletion",
            &[],
            Some(IN_LISTING),
            |ctx, _| set_mark(ctx, Some(Mark::Flagged)),
        ),
        cmd(
            "dired.unmarkAll",
            "Unmark All",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                state_mut(doc).marks.clear();
                doc.show_listing(None);
                Ok(())
            },
        ),
        cmd(
            "dired.toggleMarks",
            "Toggle Marks",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let s = state_mut(doc);
                let paths: Vec<PathBuf> = s.entries.iter().map(|e| e.path.clone()).collect();
                for p in paths {
                    match s.marks.get(&p) {
                        Some(Mark::Marked) => {
                            s.marks.remove(&p);
                        }
                        None => {
                            s.marks.insert(p, Mark::Marked);
                        }
                        Some(Mark::Flagged) => {}
                    }
                }
                doc.show_listing(None);
                Ok(())
            },
        ),
        cmd(
            "dired.markRegexp",
            "Mark by Regular Expression",
            &[],
            Some(IN_LISTING),
            |ctx, args| {
                let re = regex::Regex::new(arg(args, "regexp")?)
                    .map_err(|e| CommandError::new(e.to_string()))?;
                mark_where(ctx, &|e: &Entry| re.is_match(&e.name))
            },
        ),
        cmd(
            "dired.markDirectories",
            "Mark Folders",
            &[],
            Some(IN_LISTING),
            |ctx, _| mark_where(ctx, &|e: &Entry| e.is_dir()),
        ),
        cmd(
            "dired.markExtension",
            "Mark by Extension",
            &[],
            Some(IN_LISTING),
            |ctx, args| {
                let ext = arg(args, "extension")?
                    .trim()
                    .trim_start_matches('.')
                    .to_lowercase();
                mark_where(ctx, &|e: &Entry| !e.is_dir() && e.extension() == ext)
            },
        ),
        cmd(
            "dired.markChangedSince",
            "Mark Changed Since",
            &[],
            Some(IN_LISTING),
            |ctx, args| {
                let text = arg(args, "since")?;
                let since = parse_since(text, ctx.clock, std::time::SystemTime::now())
                    .ok_or_else(|| CommandError::new(crate::tr!("fm-bad-since", text = text)))?;
                mark_where(ctx, &|e: &Entry| e.modified.is_some_and(|m| m >= since))
            },
        ),
        // Operations.
        cmd(
            "dired.executeFlagged",
            "Delete Flagged",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let flagged = state(doc).flagged();
                file_op(ctx, OpKind::Trash, flagged, None)
            },
        ),
        cmd(
            "dired.delete",
            "Move to Trash",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let t = state(doc).targets(cursor_line(doc));
                file_op(ctx, OpKind::Trash, t, None)
            },
        ),
        cmd(
            "dired.deletePermanently",
            "Delete for Good",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let t = state(doc).targets(cursor_line(doc));
                file_op(ctx, OpKind::Delete, t, None)
            },
        ),
        cmd(
            "dired.copy",
            "Copy To",
            &[],
            Some(IN_LISTING),
            |ctx, args| transfer(ctx, args, OpKind::Copy),
        ),
        cmd(
            "dired.move",
            "Rename or Move To",
            &[],
            Some(IN_LISTING),
            |ctx, args| transfer(ctx, args, OpKind::Move),
        ),
        cmd(
            "dired.editNames",
            "Edit Names",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                edit_names(doc).map_err(CommandError::new)?;
                ctx.messages.push(tr("fm-wdired-help"));
                Ok(())
            },
        ),
        cmd(
            "dired.commitNames",
            "Apply Edited Names",
            &[],
            Some(EDITING_NAMES),
            |ctx, _| {
                let doc = listing(ctx)?;
                let n = commit_names(doc).map_err(CommandError::new)?;
                ctx.messages.push(crate::tr!("fm-renamed", count = n));
                Ok(())
            },
        ),
        cmd(
            "dired.abortNames",
            "Discard Edited Names",
            &[],
            Some(EDITING_NAMES),
            |ctx, _| {
                abort_names(listing(ctx)?);
                Ok(())
            },
        ),
        cmd(
            "dired.undo",
            "Undo File Operation",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let (paths, msg) = undo_last().map_err(CommandError::new)?;
                changed(doc, paths.first().map(PathBuf::as_path));
                ctx.messages.push(msg);
                Ok(())
            },
        ),
        cmd(
            "dired.mkdir",
            "New Folder",
            &[],
            Some(IN_LISTING),
            |ctx, args| {
                let doc = listing(ctx)?;
                let path = resolve(&the_dir(doc)?, arg(args, "name")?);
                kalem_fs::mkdir(&path).map_err(|e| CommandError::new(e.to_string()))?;
                changed(doc, Some(&path));
                Ok(())
            },
        ),
        cmd(
            "dired.newFile",
            "New File",
            &[],
            Some(IN_LISTING),
            |ctx, args| {
                let doc = listing(ctx)?;
                let name = arg(args, "name")?;
                let path = resolve(&the_dir(doc)?, name);
                // A name ending with a slash makes a folder.
                if name.trim_end().ends_with(['/', std::path::MAIN_SEPARATOR]) {
                    kalem_fs::mkdir(&path).map_err(|e| CommandError::new(e.to_string()))?;
                    changed(doc, Some(&path));
                    return Ok(());
                }
                if std::fs::symlink_metadata(&path).is_ok() {
                    return Err(CommandError::new(crate::tr!(
                        "fm-exists",
                        name = path.display().to_string()
                    )));
                }
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| CommandError::new(e.to_string()))?;
                }
                kalem_fs::touch(&path).map_err(|e| CommandError::new(e.to_string()))?;
                changed(doc, Some(&path));
                Ok(())
            },
        ),
        cmd(
            "dired.symlink",
            "Symbolic Link",
            &[],
            Some(IN_LISTING),
            |ctx, args| {
                let doc = listing(ctx)?;
                let dir = the_dir(doc)?;
                let target = state(doc)
                    .entry(cursor_line(doc))
                    .map(|e| e.path.clone())
                    .ok_or_else(|| CommandError::new(tr("fm-nothing")))?;
                let link = resolve(&dir, arg(args, "link")?);
                kalem_fs::symlink(&target, &link).map_err(|e| CommandError::new(e.to_string()))?;
                changed(doc, Some(&link));
                Ok(())
            },
        ),
        cmd(
            "dired.chmod",
            "Change Permissions",
            &[],
            Some(IN_LISTING),
            |ctx, args| {
                let mode = u32::from_str_radix(arg(args, "mode")?.trim(), 8)
                    .map_err(|_| CommandError::new(tr("fm-bad-mode")))?;
                let doc = listing(ctx)?;
                for p in state(doc).targets(cursor_line(doc)) {
                    kalem_fs::chmod(&p, mode).map_err(|e| CommandError::new(e.to_string()))?;
                }
                changed(doc, None);
                Ok(())
            },
        ),
        cmd("dired.touch", "Touch", &[], Some(IN_LISTING), |ctx, _| {
            let doc = listing(ctx)?;
            for p in state(doc).targets(cursor_line(doc)) {
                kalem_fs::touch(&p).map_err(|e| CommandError::new(e.to_string()))?;
            }
            changed(doc, None);
            Ok(())
        }),
        cmd(
            "dired.copyName",
            "Copy Names",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let text = names(doc, false);
                ctx.messages.push(text.clone());
                ctx.requests.push(Request::CopyText(text));
                Ok(())
            },
        ),
        cmd(
            "dired.copyPath",
            "Copy Paths",
            &[],
            Some(IN_LISTING),
            |ctx, _| {
                let doc = listing(ctx)?;
                let text = names(doc, true);
                ctx.messages.push(text.clone());
                ctx.requests.push(Request::CopyText(text));
                Ok(())
            },
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{Clipboard, CommandRegistry};
    use serde_json::json;
    use std::sync::Arc;
    use std::time::Instant;

    fn tree(name: &str, files: &[&str]) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-dired-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for f in files {
            let p = d.join(f);
            if f.ends_with('/') {
                std::fs::create_dir_all(p).unwrap();
            } else {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(p, "x").unwrap();
            }
        }
        d.canonicalize().unwrap()
    }

    fn run(doc: &mut DocumentState, id: &str, args: Value) -> (CommandResult, Vec<Request>) {
        let reg = CommandRegistry::with_builtins();
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(doc),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        let r = reg.execute(id, &mut ctx, &args);
        let requests = std::mem::take(&mut ctx.requests);
        (r, requests)
    }

    fn line(doc: &DocumentState) -> String {
        let t = doc.text();
        t.as_str()[t.line_range(t.line_of(doc.selection.head))].to_string()
    }

    fn goto(doc: &mut DocumentState, name: &str) {
        let at = doc
            .text()
            .as_str()
            .find(&format!(" {name}"))
            .expect("listed")
            + 1;
        doc.move_cursor(at, false);
    }

    #[test]
    fn listing_and_navigation() {
        let d = tree("nav", &["b.org", "a.txt", "sub/inner.org", ".dot"]);
        let mut doc = DocumentState::open(
            &d,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(doc.meta.mode, crate::DocumentMode::Directory);
        let text = doc.text().as_str().to_string();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].ends_with(':'), "{text}");
        assert!(lines[1].ends_with(" ../"), "{text}");
        assert!(lines[2].ends_with(" sub/"), "{text}");
        assert!(
            lines[3].ends_with(" a.txt") && lines[4].ends_with(" b.org"),
            "{text}"
        );
        assert!(!text.contains(".dot"));
        // The cursor starts on the first entry's name.
        assert!(line(&doc).ends_with("sub/"));
        assert_eq!(
            &doc.text().as_str()[doc.selection.head..doc.selection.head + 3],
            "sub"
        );
        // Typing does nothing.
        doc.insert_text("zz", Instant::now());
        assert_eq!(doc.text().as_str(), text);
        assert!(!doc.is_modified());
        // Into the folder and back up, the cursor on it.
        run(&mut doc, "dired.open", json!({})).0.unwrap();
        assert!(doc.text().as_str().contains(" inner.org"));
        assert_eq!(doc.meta.path.as_deref(), Some(d.join("sub").as_path()));
        run(&mut doc, "dired.up", json!({})).0.unwrap();
        assert!(line(&doc).ends_with("sub/"));
        // A file opens as a document.
        goto(&mut doc, "b.org");
        let (_, req) = run(&mut doc, "dired.open", json!({}));
        assert_eq!(
            req,
            vec![Request::Open {
                path: Some(d.join("b.org").display().to_string())
            }]
        );
        // Dot files and details.
        run(&mut doc, "dired.toggleHidden", json!({})).0.unwrap();
        assert!(doc.text().as_str().contains(" .dot"));
        run(&mut doc, "dired.toggleDetails", json!({})).0.unwrap();
        assert!(
            doc.text()
                .as_str()
                .lines()
                .nth(2)
                .unwrap()
                .starts_with("  sub/")
        );
        assert!(
            line(&doc).ends_with("b.org"),
            "the cursor stays on its entry"
        );
    }

    #[test]
    fn changed_since() {
        use std::time::{Duration, SystemTime};
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let now = SystemTime::now();
        assert_eq!(
            parse_since("2h", clock, now),
            now.checked_sub(Duration::from_secs(7200))
        );
        assert_eq!(
            parse_since(" 1W ", clock, now),
            now.checked_sub(Duration::from_secs(7 * 86_400))
        );
        let day = parse_since("2026-09-28", clock, now).unwrap();
        assert_eq!(parse_since("today", clock, now), Some(day));
        assert_eq!(
            parse_since("yesterday", clock, now),
            parse_since("2026-09-27", clock, now)
        );
        assert!(parse_since("2026-09-28 14:30", clock, now).unwrap() > day);
        assert!(parse_since("soon", clock, now).is_none());
        assert!(parse_since("3x", clock, now).is_none());
        // Marks the entries changed since then.
        let d = tree("since", &["old.txt", "new.txt"]);
        let old = std::fs::File::options()
            .write(true)
            .open(d.join("old.txt"))
            .unwrap();
        old.set_modified(now - Duration::from_secs(10 * 86_400))
            .unwrap();
        let mut doc = DocumentState::open(
            &d,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        run(&mut doc, "dired.markChangedSince", json!({"since": "1d"}))
            .0
            .unwrap();
        assert_eq!(state(&doc).targets(0), vec![d.join("new.txt")]);
        assert!(
            run(&mut doc, "dired.markChangedSince", json!({"since": "?"}))
                .0
                .is_err()
        );
    }

    #[test]
    fn listed_subdirectories() {
        let d = tree(
            "subdirs",
            &[
                "top.org",
                "sub/inner.org",
                "sub/deeper/x.txt",
                "other/o.txt",
            ],
        );
        let mut doc = DocumentState::open(
            &d,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        // Only a folder can be listed.
        goto(&mut doc, "top.org");
        assert!(run(&mut doc, "dired.insertSubdir", json!({})).0.is_err());
        goto(&mut doc, "sub/");
        run(&mut doc, "dired.insertSubdir", json!({})).0.unwrap();
        let text = doc.text().as_str().to_string();
        let sub = format!("  {}:", crate::projects::tilde(&d.join("sub")));
        assert!(text.contains(&format!("\n\n{sub}\n")), "{text}");
        assert!(
            line(&doc).starts_with(&sub),
            "the cursor on the folder's path"
        );
        assert!(
            text.contains(" inner.org") && text.contains(" deeper/"),
            "{text}"
        );
        // Inside it: its entries are entries, its folder is theirs.
        goto(&mut doc, "inner.org");
        let l = cursor_line(&doc);
        assert_eq!(state(&doc).dir_at(l), Some(d.join("sub").as_path()));
        let (_, req) = run(&mut doc, "dired.open", json!({}));
        assert_eq!(
            req,
            vec![Request::Open {
                path: Some(d.join("sub/inner.org").display().to_string())
            }]
        );
        run(&mut doc, "dired.mark", json!({})).0.unwrap();
        assert_eq!(
            state(&doc).targets(cursor_line(&doc)),
            vec![d.join("sub/inner.org")]
        );
        // A new file goes into the folder of the cursor's line.
        goto(&mut doc, "deeper/");
        run(&mut doc, "dired.newFile", json!({"name": "made.txt"}))
            .0
            .unwrap();
        assert!(d.join("sub/made.txt").is_file());
        assert!(doc.text().as_str().contains(" made.txt"));
        // Nested, then listing it again only goes there.
        goto(&mut doc, "deeper/");
        run(&mut doc, "dired.insertSubdir", json!({})).0.unwrap();
        assert_eq!(state(&doc).subdirs.len(), 2);
        goto(&mut doc, "deeper/");
        run(&mut doc, "dired.insertSubdir", json!({})).0.unwrap();
        assert_eq!(state(&doc).subdirs.len(), 2);
        // Marks and listed folders stay when the listing is read again.
        doc.refresh_listing();
        assert_eq!(state(&doc).subdirs.len(), 2);
        assert!(doc.text().as_str().contains("* "));
        // Up from a listed folder: its line in the parent's listing.
        goto(&mut doc, "x.txt");
        run(&mut doc, "dired.up", json!({})).0.unwrap();
        assert!(line(&doc).ends_with(" deeper/"), "{}", line(&doc));
        // Removing `sub` takes the folders listed inside it too.
        goto(&mut doc, "inner.org");
        run(&mut doc, "dired.removeSubdir", json!({})).0.unwrap();
        assert!(state(&doc).subdirs.is_empty());
        assert!(line(&doc).ends_with(" sub/"));
        assert!(!doc.text().as_str().contains("inner.org"));
        assert!(run(&mut doc, "dired.removeSubdir", json!({})).0.is_err());
        // A folder removed on disk goes from the listing.
        goto(&mut doc, "other/");
        run(&mut doc, "dired.insertSubdir", json!({})).0.unwrap();
        std::fs::remove_dir_all(d.join("other")).unwrap();
        doc.refresh_listing();
        assert!(state(&doc).subdirs.is_empty());
        // Another place starts without listed folders.
        goto(&mut doc, "sub/");
        run(&mut doc, "dired.insertSubdir", json!({})).0.unwrap();
        goto(&mut doc, "sub/");
        run(&mut doc, "dired.open", json!({})).0.unwrap();
        assert!(state(&doc).subdirs.is_empty());
    }

    /// Replaces `old` in the listing's text with `new`, as typing would.
    fn edit(doc: &mut DocumentState, old: &str, new: &str) {
        let at = doc.text().as_str().find(old).expect("in the listing");
        let mut tx = org_edit::Transaction::new("Edit");
        tx.replace(at..at + old.len(), new).unwrap();
        doc.apply(&tx, org_edit::ChangeKind::Typing, std::time::Instant::now());
    }

    #[test]
    fn editable_names() {
        let d = tree("wdired", &["a.txt", "b.txt", "c.txt", "dir/"]);
        let mut doc = DocumentState::open(
            &d,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        // A listing ignores edits until its names are made editable.
        edit(&mut doc, "a.txt", "zzz");
        assert!(doc.text().as_str().contains("a.txt"));
        run(&mut doc, "dired.editNames", json!({})).0.unwrap();
        assert!(doc.when_context().get("wdired").is_some());
        // The file manager's keys are text now.
        let reg = CommandRegistry::with_builtins();
        let applies = |doc: &DocumentState, id: &str| {
            reg.get(id)
                .unwrap()
                .when
                .as_ref()
                .is_none_or(|w| w.eval(&doc.when_context()))
        };
        assert!(!applies(&doc, "dired.mark") && applies(&doc, "dired.commitNames"));
        // A swap, a rename and a move into a folder, at once.
        edit(&mut doc, "a.txt", "b.txt#");
        edit(&mut doc, " b.txt\n", " a.txt\n");
        edit(&mut doc, "b.txt#", "b.txt");
        edit(&mut doc, "c.txt", "dir/c2.txt");
        std::fs::write(d.join("a.txt"), "A").unwrap();
        std::fs::write(d.join("b.txt"), "B").unwrap();
        run(&mut doc, "dired.commitNames", json!({})).0.unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "B");
        assert_eq!(std::fs::read_to_string(d.join("b.txt")).unwrap(), "A");
        assert!(d.join("dir/c2.txt").is_file() && !d.join("c.txt").exists());
        assert!(state(&doc).wdired.is_none() && !doc.is_modified());
        // Undo takes them all back.
        run(&mut doc, "dired.undo", json!({})).0.unwrap();
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "A");
        assert!(d.join("c.txt").is_file());
        // Refused before anything changes: two the same, an empty name,
        // a line removed; the edit stays to fix.
        for (old, new) in [("a.txt", "b.txt"), ("a.txt", ""), (" a.txt\n", "")] {
            doc.refresh_listing();
            run(&mut doc, "dired.editNames", json!({})).0.unwrap();
            edit(&mut doc, old, new);
            assert!(run(&mut doc, "dired.commitNames", json!({})).0.is_err());
            assert!(state(&doc).wdired.is_some());
            assert!(d.join("a.txt").is_file() && d.join("b.txt").is_file());
            run(&mut doc, "dired.abortNames", json!({})).0.unwrap();
            assert!(doc.text().as_str().contains(" a.txt\n"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn undo_renames_and_moves() {
        let d = tree("undo", &["a.txt", "dir/"]);
        let mut doc = DocumentState::open(
            &d,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        let mv = |from: PathBuf, to: PathBuf| {
            let op = crate::command::FileOp {
                kind: kalem_fs::OpKind::Move,
                sources: vec![from],
                target: Some(to),
            };
            Task::new(&op).unwrap().start().wait();
        };
        mv(d.join("a.txt"), d.join("b.txt"));
        mv(d.join("b.txt"), d.join("dir"));
        assert!(d.join("dir/b.txt").exists());
        doc.refresh_listing();
        run(&mut doc, "dired.undo", json!({})).0.unwrap();
        assert!(d.join("b.txt").exists() && !d.join("dir/b.txt").exists());
        assert!(line(&doc).ends_with("b.txt"), "{}", line(&doc));
        // Not over a file put in the way; the operation stays to undo.
        std::fs::write(d.join("a.txt"), "").unwrap();
        assert!(run(&mut doc, "dired.undo", json!({})).0.is_err());
        std::fs::remove_file(d.join("a.txt")).unwrap();
        run(&mut doc, "dired.undo", json!({})).0.unwrap();
        assert!(d.join("a.txt").exists() && !d.join("b.txt").exists());
        assert!(run(&mut doc, "dired.undo", json!({})).0.is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn marks_and_operations() {
        let d = tree("marks", &["a.txt", "b.txt", "c.org", "dir/"]);
        let mut doc = DocumentState::open(
            &d,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        goto(&mut doc, "a.txt");
        run(&mut doc, "dired.mark", json!({})).0.unwrap();
        assert!(line(&doc).ends_with("b.txt"), "marking moves down");
        assert!(doc.text().as_str().contains("\n* "));
        run(&mut doc, "dired.flag", json!({})).0.unwrap();
        let s = doc.dired.as_deref().unwrap();
        assert_eq!(s.targets(0), vec![d.join("a.txt")]);
        assert_eq!(s.flagged(), vec![d.join("b.txt")]);
        let (_, req) = run(&mut doc, "dired.executeFlagged", json!({}));
        assert!(
            matches!(&req[..], [Request::FileOp(op)] if op.kind == kalem_fs::OpKind::Trash && op.sources == vec![d.join("b.txt")])
        );
        run(&mut doc, "dired.unmarkAll", json!({})).0.unwrap();
        run(
            &mut doc,
            "dired.markExtension",
            json!({"extension": ".txt"}),
        )
        .0
        .unwrap();
        run(&mut doc, "dired.markRegexp", json!({"regexp": "^c"}))
            .0
            .unwrap();
        assert_eq!(doc.dired.as_deref().unwrap().targets(0).len(), 3);
        // Copy the marked entries into the folder, with a conflict.
        std::fs::write(d.join("dir/a.txt"), "old").unwrap();
        let (_, req) = run(&mut doc, "dired.copy", json!({"target": "dir"}));
        let [Request::FileOp(op)] = &req[..] else {
            panic!("{req:?}")
        };
        let mut task = Task::new(op).unwrap();
        assert!(matches!(
            task.question(),
            Some(Question::Conflict { more: 0, .. })
        ));
        assert!(task.answer(Answer::KeepBoth));
        assert_eq!(task.question(), None);
        let out = task.start().wait();
        assert_eq!(out.done.len(), 3);
        assert!(d.join("dir/a (2).txt").exists() && d.join("dir/c.org").exists());
        // New folder, then the listing shows it with the cursor on it.
        run(&mut doc, "dired.mkdir", json!({"name": "made"}))
            .0
            .unwrap();
        assert!(line(&doc).ends_with("made/"));
        // New file; a name ending with a slash makes a folder.
        run(&mut doc, "dired.newFile", json!({"name": "new.org"}))
            .0
            .unwrap();
        assert!(line(&doc).ends_with(" new.org") && d.join("new.org").is_file());
        run(&mut doc, "dired.newFile", json!({"name": "made2/"}))
            .0
            .unwrap();
        assert!(line(&doc).ends_with("made2/") && d.join("made2").is_dir());
        assert!(
            run(&mut doc, "dired.newFile", json!({"name": "new.org"}))
                .0
                .is_err()
        );
        // Trash asks first; "no" gives up.
        let mut t = Task::new(&FileOp {
            kind: kalem_fs::OpKind::Trash,
            sources: vec![d.join("a.txt")],
            target: None,
        })
        .unwrap();
        assert!(matches!(t.question(), Some(Question::Confirm(q)) if q.contains("a.txt")));
        assert!(!t.answer(Answer::No));
    }

    #[test]
    fn projects_view() {
        let d = tree("projects", &["one/x.org", "two/y.org"]);
        let rows = vec![
            ProjectRow {
                name: "two".into(),
                root: d.join("two"),
                used: 2,
                exists: true,
            },
            ProjectRow {
                name: "one".into(),
                root: d.join("one"),
                used: 1,
                exists: true,
            },
            ProjectRow {
                name: "gone".into(),
                root: d.join("gone"),
                used: 3,
                exists: false,
            },
        ];
        let mut doc = DocumentState::open(
            &d.join("one"),
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        show_projects(&mut doc, rows, None);
        let text = doc.text().as_str().to_string();
        let names: Vec<&str> = text
            .lines()
            .skip(1)
            .map(|l| l.split_whitespace().next().unwrap())
            .collect();
        assert_eq!(names, vec!["gone", "one", "two"], "{text}");
        assert!(text.contains("missing"));
        assert_eq!(doc.meta.path, None);
        // A project opens like a folder; up from its root shows the projects.
        goto(&mut doc, "two");
        run(&mut doc, "dired.open", json!({})).0.unwrap();
        assert!(doc.text().as_str().contains(" y.org"));
        let (_, req) = run(&mut doc, "dired.up", json!({}));
        assert_eq!(
            req,
            vec![Request::FileManager(
                crate::command::FileManagerRequest::Projects {
                    select: Some(d.join("two"))
                }
            )]
        );
        // A missing project cannot be opened.
        let rows = doc.dired.as_deref().unwrap().projects.clone();
        show_projects(&mut doc, rows, Some(&d.join("gone")));
        assert!(line(&doc).contains("gone"));
        assert!(run(&mut doc, "dired.open", json!({})).0.is_err());
        // The projects view toggles back to the folder shown before.
        run(&mut doc, "dired.projects", json!({})).0.unwrap();
        assert_eq!(doc.meta.path.as_deref(), Some(d.join("two").as_path()));
    }
}
