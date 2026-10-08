//! The document a plugin's command runs in (design §11.4's `editor`,
//! T3.1.7): a picture of it taken when the command starts, which the
//! plugin reads, and the edits it asks for, applied when it returns, in
//! order, each place moved by the edits before it, as one undo step.
//!
//! The picture lives on the thread running the command (the editor's), so
//! the host reaches it through [`with_document`] while the plugin's call
//! is on the stack; outside a plugin's command there is none.

use std::cell::RefCell;
use std::sync::Arc;

use org_edit::{ChangeKind, Transaction};
use org_model::Document;

use crate::command::{CommandError, CommandResult, EditorContext, Request};
use crate::document::DocumentState;

/// A headline as a plugin reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlineInfo {
    /// Where it starts.
    pub start: usize,
    /// Where its subtree ends.
    pub end: usize,
    /// Its level.
    pub level: usize,
    /// Its title, as written.
    pub title: String,
    /// Its TODO keyword.
    pub todo: Option<String>,
    /// The keyword is a done one.
    pub done: bool,
    /// Its priority.
    pub priority: Option<char>,
    /// Its own tags.
    pub tags: Vec<String>,
    /// Its property drawer.
    pub properties: Vec<(String, String)>,
    /// `SCHEDULED`, as written.
    pub scheduled: Option<String>,
    /// `DEADLINE`, as written.
    pub deadline: Option<String>,
    /// `CLOSED`, as written.
    pub closed: Option<String>,
    /// The headline above, by its start.
    pub parent: Option<usize>,
    /// The headlines right under it, by their starts.
    pub children: Vec<usize>,
}

/// An Org table as a plugin reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableInfo {
    /// Where it starts.
    pub start: usize,
    /// Where it ends, its `#+TBLFM` lines included.
    pub end: usize,
    /// Its data rows' fields.
    pub rows: Vec<Vec<String>>,
    /// Its formulas.
    pub formulas: Vec<String>,
}

/// An edit a plugin asked for; places are the picture's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocEdit {
    /// Text at a place, or at the cursor.
    Insert(Option<usize>, String),
    /// A range replaced.
    Replace(usize, usize, String),
    /// A headline's TODO keyword (`None` removes it).
    SetTodo(usize, Option<String>),
    /// A headline's title.
    SetTitle(usize, String),
    /// A headline's own tags.
    SetTags(usize, Vec<String>),
    /// A headline's property (`None` removes it).
    SetProperty(usize, String, Option<String>),
    /// A subtree promoted.
    Promote(usize),
    /// A subtree demoted.
    Demote(usize),
    /// A subtree moved above its sibling.
    MoveUp(usize),
    /// A subtree moved below its sibling.
    MoveDown(usize),
    /// A table's field: data row and field, from 0.
    SetCell(usize, usize, usize, String),
    /// A table recalculated.
    Recalc(usize),
    /// The undo step's name.
    Label(String),
    /// Save after the edits.
    Save,
}

/// The document as a plugin's command found it.
#[derive(Debug)]
pub struct DocView {
    /// Its file.
    pub path: Option<String>,
    /// Its mode's name.
    pub mode: String,
    /// A plain text file's language.
    pub language: Option<String>,
    /// Unsaved changes.
    pub modified: bool,
    /// Its text.
    pub text: String,
    /// The selection: anchor and cursor.
    pub selection: (usize, usize),
    /// The number of the document, when a plugin writes it
    /// ([`crate::GeneratedDoc`]).
    pub generated: Option<u64>,
    model: Option<Arc<Document>>,
}

thread_local! {
    static CURRENT: RefCell<Option<DocView>> = const { RefCell::new(None) };
    static EDITS: RefCell<Vec<DocEdit>> = const { RefCell::new(Vec::new()) };
}

/// Calls `f` with the document of the plugin's command running on this
/// thread, if one is.
pub fn with_document<R>(f: impl FnOnce(&DocView) -> R) -> Option<R> {
    CURRENT.with(|c| c.borrow().as_ref().map(f))
}

/// Queues an edit of the document of the plugin's command running on this
/// thread; `false` when none runs.
pub fn queue(edit: DocEdit) -> bool {
    if CURRENT.with(|c| c.borrow().is_none()) {
        return false;
    }
    EDITS.with(|e| e.borrow_mut().push(edit));
    true
}

/// Runs `f` (a plugin's command) with the picture of `doc` offered, and
/// returns its result with the edits it asked for.
pub(crate) fn during<R>(
    doc: Option<&mut DocumentState>,
    f: impl FnOnce() -> R,
) -> (R, Vec<DocEdit>) {
    let view = doc.map(DocView::of);
    let before = CURRENT.with(|c| c.replace(view));
    let outer = EDITS.with(|e| std::mem::take(&mut *e.borrow_mut()));
    let r = f();
    let edits = EDITS.with(|e| std::mem::replace(&mut *e.borrow_mut(), outer));
    CURRENT.with(|c| *c.borrow_mut() = before);
    (r, edits)
}

impl DocView {
    fn of(doc: &mut DocumentState) -> DocView {
        let model = doc.model();
        let language = match &doc.meta.mode {
            crate::DocumentMode::Text { language } => language.clone(),
            _ => None,
        };
        DocView {
            path: doc
                .meta
                .path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            mode: doc.meta.mode.name().to_string(),
            language,
            modified: doc.is_modified(),
            text: doc.text().as_str().to_string(),
            selection: (doc.selection.anchor, doc.selection.head),
            generated: doc.generated.as_ref().map(|g| g.number),
            model,
        }
    }

    /// The text of `start..end`, clamped to the text and its characters.
    pub fn text(&self, range: Option<(usize, usize)>) -> String {
        let (mut s, mut e) = range.unwrap_or((0, self.text.len()));
        e = e.min(self.text.len());
        s = s.min(e);
        while !self.text.is_char_boundary(s) {
            s -= 1;
        }
        while !self.text.is_char_boundary(e) {
            e += 1;
        }
        self.text[s..e].to_string()
    }

    /// Its headlines, in order; none outside Org.
    pub fn headlines(&self) -> Vec<HeadlineInfo> {
        let Some(m) = &self.model else {
            return Vec::new();
        };
        let entries = &m.outline().entries;
        let start = |i: usize| usize::from(entries[i].range.start());
        (0..entries.len())
            .map(|i| {
                let e = &entries[i];
                let id = org_model::EntryId(i);
                let planning = m.planning(id);
                let raw = |t: Option<org_model::time::Timestamp>| t.map(|t| t.raw);
                HeadlineInfo {
                    start: start(i),
                    end: usize::from(e.range.end()),
                    level: e.level,
                    title: e.raw_title.clone(),
                    todo: e.todo.clone(),
                    done: m.todo_state(id).is_some_and(|t| t.done),
                    priority: e.priority,
                    tags: e.local_tags.clone(),
                    properties: e.drawer.clone(),
                    scheduled: raw(planning.scheduled),
                    deadline: raw(planning.deadline),
                    closed: raw(planning.closed),
                    parent: e.parent.map(|p| start(p.0)),
                    children: e.children.iter().map(|c| start(c.0)).collect(),
                }
            })
            .collect()
    }

    /// The text under headline `start`'s line, up to its first
    /// subheadline.
    pub fn body(&self, start: usize) -> Option<String> {
        let h = self.headlines().into_iter().find(|h| h.start == start)?;
        let line_end = self.text[start..]
            .find('\n')
            .map_or(self.text.len(), |i| start + i + 1)
            .min(h.end);
        let end = h.children.first().copied().unwrap_or(h.end);
        Some(self.text.get(line_end..end.max(line_end))?.to_string())
    }

    /// The TODO keywords in force, the not-done ones first.
    pub fn todo_keywords(&self) -> Vec<String> {
        let Some(m) = &self.model else {
            return Vec::new();
        };
        let ctx = m.parse().context();
        ctx.todo_keywords
            .iter()
            .chain(&ctx.done_keywords)
            .cloned()
            .collect()
    }

    /// Its `#+KEY: value` lines.
    pub fn keywords(&self) -> Vec<(String, String)> {
        self.model
            .as_ref()
            .map(|m| m.info().keywords.clone())
            .unwrap_or_default()
    }

    /// The innermost element at `offset`: its kind, in lower case with
    /// dashes (`src-block`), and its range.
    pub fn node_at(&self, offset: usize) -> Option<(String, usize, usize)> {
        let m = self.model.as_ref()?;
        let root = m.parse().syntax();
        let at = org_syntax::TextSize::try_from(offset.min(self.text.len())).ok()?;
        let token = root.token_at_offset(at).right_biased()?;
        let node = token.parent()?;
        let kind = format!("{:?}", node.kind())
            .to_lowercase()
            .replace('_', "-");
        let r = node.text_range();
        Some((kind, usize::from(r.start()), usize::from(r.end())))
    }

    /// The table holding `offset`.
    pub fn table_at(&self, offset: usize) -> Option<TableInfo> {
        let m = self.model.as_ref()?;
        let node = table_node(m, offset)?;
        let r = node.text_range();
        let (start, end) = (usize::from(r.start()), usize::from(r.end()));
        let text = &self.text[start..end];
        let table = org_table::table::Table::parse(text);
        let rows = table
            .rows
            .iter()
            .filter_map(|r| match r {
                org_table::table::Row::Data(f) => Some(f.clone()),
                org_table::table::Row::Rule => None,
            })
            .collect();
        let formulas = text
            .lines()
            .filter_map(|l| {
                let l = l.trim_start();
                l.get(..8)
                    .filter(|k| k.eq_ignore_ascii_case("#+tblfm:"))
                    .map(|_| l[8..].trim())
            })
            .flat_map(|v| v.split("::").map(|f| f.trim().to_string()))
            .filter(|f| !f.is_empty())
            .collect();
        Some(TableInfo {
            start,
            end,
            rows,
            formulas,
        })
    }
}

/// The table node holding `offset`.
fn table_node(m: &Document, offset: usize) -> Option<org_syntax::SyntaxNode> {
    m.parse()
        .syntax()
        .descendants()
        .filter(|n| n.kind() == org_syntax::SyntaxKind::TABLE)
        .find(|n| {
            let r = n.text_range();
            usize::from(r.start()) <= offset && offset < usize::from(r.end()).max(1)
        })
}

/// Applies a plugin's edits to the document of `ctx`, as one undo step.
pub(crate) fn apply(ctx: &mut EditorContext<'_>, edits: Vec<DocEdit>) -> CommandResult {
    let label = edits
        .iter()
        .rev()
        .find_map(|e| match e {
            DocEdit::Label(l) => Some(l.clone()),
            _ => None,
        })
        .unwrap_or_else(|| "Plugin edit".into());
    let save = edits.contains(&DocEdit::Save);
    let (now, clock) = (ctx.now, ctx.clock);
    let todo_base = ctx.config.todo_settings();
    let doc = ctx.doc()?;
    doc.begin_undo_join();
    let mut done: Vec<Transaction> = Vec::new();
    let result = (|| -> CommandResult {
        for e in edits {
            let at = |p: usize| done.iter().fold(p, |p, tx| place(tx, p));
            let tx = edit_transaction(doc, &e, &label, &at, clock, &todo_base)?;
            if let Some(tx) = tx {
                doc.apply(&tx, ChangeKind::Command, now);
                done.push(tx);
            }
            // A field set: the table aligned after it.
            if let DocEdit::SetCell(t, ..) = &e {
                let p = done.iter().fold(*t, |p, tx| place(tx, p));
                let m = org(doc)?;
                let tx = org_edit::table::align_table(&m, p).map_err(err)?;
                doc.apply(&tx, ChangeKind::Command, now);
                done.push(tx);
            }
        }
        Ok(())
    })();
    doc.break_undo_group();
    result?;
    if save {
        ctx.requests.push(Request::Save);
    }
    Ok(())
}

/// Where `pos`, a place in the text before `tx`, is after it. Text
/// inserted right at it goes before it (the place keeps naming what
/// followed it, such as a headline's start); a replacement starting at it
/// or holding it leaves it at the replacement's start (a table rewritten
/// still starts there).
fn place(tx: &Transaction, pos: usize) -> usize {
    let mut shift: isize = 0;
    for e in &tx.edits {
        let (s, t) = (e.range.start, e.range.end);
        if pos < s {
            break;
        }
        if s == t {
            shift += e.insert.len() as isize;
            continue;
        }
        if pos < t {
            return (s as isize + shift) as usize;
        }
        shift += e.insert.len() as isize - (t - s) as isize;
    }
    (pos as isize + shift) as usize
}

fn org(doc: &mut DocumentState) -> Result<Arc<Document>, CommandError> {
    doc.model()
        .ok_or_else(|| CommandError::new("Not an Org document"))
}

fn err(e: org_edit::EditError) -> CommandError {
    CommandError::new(e.message)
}

/// The transaction of edit `e`, its places moved by `at`; `None` for what
/// changes nothing.
fn edit_transaction(
    doc: &mut DocumentState,
    e: &DocEdit,
    label: &str,
    at: &dyn Fn(usize) -> usize,
    clock: jiff::civil::DateTime,
    todo_base: &org_edit::todo::TodoSettings,
) -> Result<Option<Transaction>, CommandError> {
    let replace = |start: usize, end: usize, text: &str| {
        let mut tx = Transaction::new(label);
        tx.replace(start..end, text)
            .map_err(|_| CommandError::new("Overlapping edits"))?;
        Ok::<_, CommandError>(Some(tx))
    };
    Ok(match e {
        DocEdit::Label(_) | DocEdit::Save => None,
        DocEdit::Insert(p, text) => {
            let p = p.map_or(doc.selection.head, at).min(doc.text().len());
            replace(p, p, text)?
        }
        DocEdit::Replace(s, e, text) => {
            let (s, e) = (at(*s), at(*e));
            let len = doc.text().len();
            replace(s.min(e).min(len), e.max(s).min(len), text)?
        }
        DocEdit::SetTodo(h, state) => {
            use org_edit::todo::{TodoArg, TodoOptions, todo};
            let m = org(doc)?;
            let settings = todo_base.for_document(&m);
            let opts = TodoOptions {
                arg: TodoArg::State(state.clone().unwrap_or_default()),
                settings: &settings,
                now: clock,
                remembered_head: None,
                repeated: false,
                force_note: false,
                inhibit_note: true,
            };
            Some(todo(&m, at(*h), &opts).map_err(err)?.transaction)
        }
        DocEdit::SetTitle(h, title) => {
            let m = org(doc)?;
            let p = at(*h);
            let id = m
                .outline()
                .entry_at(p)
                .ok_or_else(|| CommandError::new("No headline"))?;
            let e = m.entry(id);
            let start = usize::from(e.range.start());
            let (s, t) = title_span(e);
            replace(start + s, start + t, title)?
        }
        DocEdit::SetTags(h, tags) => {
            let m = org(doc)?;
            Some(org_edit::tags::set_tags(&m, at(*h), tags).map_err(err)?)
        }
        DocEdit::SetProperty(h, key, value) => {
            let m = org(doc)?;
            match value {
                Some(v) => {
                    Some(org_edit::property::set_property(&m, at(*h), key, v, false).map_err(err)?)
                }
                None => org_edit::property::delete_property(&m, at(*h), key),
            }
        }
        DocEdit::Promote(h) | DocEdit::Demote(h) => {
            let m = org(doc)?;
            let text = m.parse().syntax().to_string();
            let ctx = m.parse().context();
            let f = if matches!(e, DocEdit::Promote(_)) {
                org_edit::headline::promote_subtree
            } else {
                org_edit::headline::demote_subtree
            };
            Some(f(&text, at(*h), ctx).map_err(err)?)
        }
        DocEdit::MoveUp(h) | DocEdit::MoveDown(h) => {
            let m = org(doc)?;
            let text = m.parse().syntax().to_string();
            let down = matches!(e, DocEdit::MoveDown(_));
            Some(
                org_edit::headline::move_subtree(&text, at(*h), down, m.parse().context())
                    .map_err(err)?,
            )
        }
        DocEdit::SetCell(t, row, col, value) => {
            let m = org(doc)?;
            let p = at(*t);
            let node = table_node(&m, p).ok_or_else(|| CommandError::new("No table"))?;
            let start = usize::from(node.text_range().start());
            let text = node.to_string();
            let mut table = org_table::table::Table::parse(&text);
            let line = table
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| matches!(r, org_table::table::Row::Data(_)))
                .nth(*row)
                .map(|(i, _)| i)
                .ok_or_else(|| CommandError::new(format!("The table has no row {row}")))?;
            table.set_field(line, col + 1, value.clone());
            // The table's lines, indented as its first; then aligned.
            let indent: String = text
                .chars()
                .take_while(|c| *c == ' ' || *c == '\t')
                .collect();
            let lines: String = table
                .to_org()
                .lines()
                .map(|l| format!("{indent}{l}\n"))
                .collect();
            let old: usize = text
                .split_inclusive('\n')
                .take(table.rows.len())
                .map(str::len)
                .sum();
            replace(start, start + old, &lines)?
        }
        DocEdit::Recalc(t) => {
            let m = org(doc)?;
            Some(
                org_edit::recalc::recalculate(&m, at(*t), false)
                    .map_err(err)?
                    .transaction,
            )
        }
    })
}

/// The bytes of a headline line where its title stands: after the stars,
/// the keyword and the priority, before the tags.
fn title_span(e: &org_model::Entry) -> (usize, usize) {
    let line = e.line.as_str();
    // After the stars, the keyword and the priority.
    let mut from = line.find(|c: char| c != '*').unwrap_or(line.len());
    let skip = |from: &mut usize, word: &str| {
        let rest = line[*from..].trim_start_matches([' ', '\t']);
        if rest.starts_with(word) {
            *from = line.len() - rest.len() + word.len();
        }
    };
    if let Some(k) = &e.todo {
        skip(&mut from, k);
    }
    if let Some(p) = e.priority {
        skip(&mut from, &format!("[#{p}]"));
    }
    let title = e.raw_title.as_str();
    if !title.is_empty()
        && let Some(i) = line[from..].find(title)
    {
        return (from + i, from + i + title.len());
    }
    // No title: before the tags, or at the end of the line.
    let trimmed = line.trim_end();
    let end = if e.local_tags.is_empty() {
        trimmed.len()
    } else {
        trimmed.rfind([' ', '\t']).unwrap_or(trimmed.len())
    }
    .max(from);
    (end, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_follow_insertions_and_stay_at_replacements() {
        let mut tx = Transaction::new("t");
        tx.insert(10, "abc").unwrap();
        tx.replace(20..25, "xy").unwrap();
        assert_eq!(place(&tx, 5), 5);
        assert_eq!(place(&tx, 10), 13, "after text inserted at it");
        assert_eq!(place(&tx, 20), 23, "a replacement's start stays its start");
        assert_eq!(place(&tx, 22), 23, "inside: the start");
        assert_eq!(place(&tx, 25), 25, "after: moved by both");
    }
}
