//! Lines under lines typed into where they are drawn (API 0.2.15): a
//! layer's line under a line that shows the text of a file
//! ([`LineSource`]) is edited there as a one-line input, and what is typed
//! is written back in place of that text: in the file's open document when
//! that has unsaved changes, else in the file, which the layer then reads
//! again. An embedded block typed into in the note that embeds it (the
//! graph plugin's Logseq embeds), a task renamed in a query's results.
//! Only files in Kalem's projects are written, and only where they still
//! hold the text the layer saw.
//!
//! A line that starts a block of its file's outline (API 0.2.18,
//! [`crate::layers::Outline`]) takes Enter, Tab and Shift+Tab as that
//! outline would: Enter splits the block at the cursor into a new block
//! after it and the blocks under it, Tab indents the block with them,
//! Shift+Tab takes a step of indentation off them; the block is typed into
//! on at its new place once the layer shows it ([`Resume`]). An embedded
//! block's children added and moved in the note embedding it.

use std::ops::Range;
use std::path::Path;
use std::time::{Duration, Instant};

use org_edit::{ChangeKind, Transaction};

use crate::document::DocumentState;
use crate::layers::LineSource;
use crate::line_edit::{self, LineKey};

/// A line under a line being typed into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowEdit {
    /// The start of the line of the text it is under.
    pub line: usize,
    /// Its place among the lines under that line.
    pub row: usize,
    /// The text of a file it shows.
    pub source: LineSource,
    /// What is typed.
    pub input: String,
    /// The characters after the cursor in `input` (as [`line_edit`]
    /// keeps them).
    pub back: usize,
}

impl RowEdit {
    /// The byte of `input` the cursor is at.
    pub fn cursor(&self) -> usize {
        line_edit::cursor(&self.input, self.back)
    }
}

/// What typing into a line under a line writes: `text` in place of the
/// source's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteBack {
    /// Where.
    pub source: LineSource,
    /// What.
    pub text: String,
    /// What else changes in the file, by its outline (API 0.2.18).
    pub change: Change,
}

/// What typing into a line under a line changes in its file besides the
/// line's text, when the line starts a block of the file's outline (API
/// 0.2.18, [`crate::layers::Outline`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Change {
    /// Nothing.
    #[default]
    None,
    /// A new block after the block and the blocks under it, holding this
    /// text (Enter: what followed the cursor).
    Split(String),
    /// The block and the lines under it a step in (Tab).
    Indent,
    /// The block and the lines under it a step out (Shift+Tab).
    Outdent,
}

/// How long a block Enter made or Tab moved is waited for in its layer.
const RESUME: Duration = Duration::from_secs(3);

/// A line under a line to type into once its layer shows it (API 0.2.18):
/// the block Enter made, or the one Tab or Shift+Tab moved, at its new
/// place in its file, as it holds it now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resume {
    path: std::path::PathBuf,
    line: usize,
    column: usize,
    text: String,
    /// The characters after the cursor.
    back: usize,
    until: Instant,
}

impl WriteBack {
    /// The file's name, for messages.
    pub fn name(&self) -> String {
        self.source
            .path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// The bytes of `text`, a file's, that hold the source's text, when it
    /// still holds it there.
    pub fn range_in(&self, text: &str) -> Option<Range<usize>> {
        let s = &self.source;
        let start = if s.line == 0 {
            0
        } else {
            text.match_indices('\n')
                .nth(s.line - 1)
                .map(|(i, _)| i + 1)?
        };
        let mut end = text[start..].find('\n').map_or(text.len(), |i| start + i);
        if text[start..end].ends_with('\r') {
            end -= 1;
        }
        let from = start.checked_add(s.column)?;
        (from <= end && text.is_char_boundary(from) && text[from..end] == s.text)
            .then_some(from..end)
    }

    /// Why it is not written: the file no longer holds the text.
    fn changed(&self) -> String {
        crate::tr!("msg-row-changed", file = self.name())
    }

    /// The replacements that write it into `text`, a file's, in order:
    /// the line's text, then what its outline changes. None when the file
    /// no longer holds the text there, or the block may not move so.
    pub fn edits(&self, text: &str) -> Option<Vec<(Range<usize>, String)>> {
        let r = self.range_in(text)?;
        let mut out = vec![(r, self.text.clone())];
        let Some(o) = self.source.outline.as_ref() else {
            return Some(out);
        };
        let line = self.source.line;
        // Where the file's lines start, and its line feed.
        let starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .filter(|&i| i < text.len())
            .collect();
        let nl = if text.contains("\r\n") { "\r\n" } else { "\n" };
        let end = o.end.min(starts.len()).max(line + 1);
        let block = starts.get(line..end.min(starts.len())).unwrap_or(&[]);
        match &self.change {
            Change::None => {}
            Change::Split(after) => {
                let new = format!("{}{after}", o.bullet);
                match starts.get(end) {
                    Some(&at) => out.push((at..at, format!("{new}{nl}"))),
                    None if text.is_empty() || text.ends_with('\n') => {
                        out.push((text.len()..text.len(), format!("{new}{nl}")));
                    }
                    None => out.push((text.len()..text.len(), format!("{nl}{new}"))),
                }
            }
            Change::Indent => {
                for &at in block {
                    out.push((at..at, o.step.clone()));
                }
            }
            Change::Outdent => {
                if o.step.is_empty() || !text[*block.first()?..].starts_with(&o.step) {
                    return None;
                }
                for &at in block {
                    if text[at..].starts_with(&o.step) {
                        out.push((at..at + o.step.len(), String::new()));
                    }
                }
            }
        }
        out.sort_by_key(|(r, _)| (r.start, !r.is_empty()));
        // A plugin's outline that overlaps the line's text: not written.
        if out.windows(2).any(|w| w[0].0.end > w[1].0.start) {
            return None;
        }
        Some(out)
    }

    /// Writes it into the file: read, checked, written.
    pub fn write_file(&self) -> Result<(), String> {
        let path = &self.source.path;
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let edits = self.edits(&text).ok_or_else(|| self.changed())?;
        let mut new = String::with_capacity(text.len() + self.text.len() + 16);
        let mut at = 0;
        for (r, t) in &edits {
            new.push_str(&text[at..r.start]);
            new.push_str(t);
            at = r.end;
        }
        new.push_str(&text[at..]);
        crate::files::write(path, new.as_bytes(), crate::files::SaveOptions::default())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Writes it into `doc`, the file's open document, as a step of its
    /// own to undo.
    pub fn write_into(&self, doc: &mut DocumentState, now: Instant) -> Result<(), String> {
        let edits = self
            .edits(doc.text().as_str())
            .ok_or_else(|| self.changed())?;
        let mut tx = Transaction::new("Edit Line Under Line");
        for (r, t) in edits {
            tx.edit(r, t);
        }
        doc.apply(&tx, ChangeKind::Command, now);
        Ok(())
    }
}

/// Whether a layer's line may write `path`: a file in the plugins'
/// workspace (Kalem's projects), as `fs:write:workspace` reaches.
pub fn writable(path: &Path) -> bool {
    let Ok(p) = dunce::canonicalize(path) else {
        return false;
    };
    crate::extensions::workspace()
        .iter()
        .any(|root| dunce::canonicalize(root).is_ok_and(|r| p.starts_with(r)))
}

impl DocumentState {
    /// The places of the lines under the line starting at byte `line`
    /// that may be typed into.
    pub fn editable_rows(&self, line: usize) -> Vec<usize> {
        self.overlays().map_or_else(Vec::new, |o| {
            o.below(line)
                .iter()
                .enumerate()
                .filter(|(_, l)| l.source.is_some())
                .map(|(i, _)| i)
                .collect()
        })
    }

    /// Starts typing into line `row` under the line starting at byte
    /// `line`: its source's text, the cursor at its end, the document's at
    /// the end of that line. Whether it may be typed into.
    pub fn start_row_edit(&mut self, line: usize, row: usize) -> bool {
        let source = self
            .overlays()
            .and_then(|o| o.below(line).get(row).and_then(|l| l.source.clone()));
        let Some(source) = source else {
            return false;
        };
        let end = self.text().line_range(self.text().line_of(line)).end;
        self.move_cursor(end, false);
        self.row_edit = Some(RowEdit {
            line,
            row,
            input: source.text.clone(),
            source,
            back: 0,
        });
        true
    }

    /// A key of the line being typed into.
    pub fn row_edit_key(&mut self, key: LineKey) {
        if let Some(r) = &mut self.row_edit {
            line_edit::apply(&mut r.input, &mut r.back, key);
        }
    }

    /// Text typed into the line being typed into, a line feed a space.
    pub fn row_edit_insert(&mut self, text: &str) {
        if let Some(r) = &mut self.row_edit {
            line_edit::insert(&mut r.input, r.back, &text.replace(['\n', '\r'], " "));
        }
    }

    /// Ends typing into a line under a line: what to write, when `keep`
    /// and something was changed.
    pub fn end_row_edit(&mut self, keep: bool) -> Option<WriteBack> {
        let r = self.row_edit.take()?;
        (keep && r.input != r.source.text).then_some(WriteBack {
            source: r.source,
            text: r.input,
            change: Change::None,
        })
    }

    /// Ends typing into a line under a line, keeping what was typed, and
    /// goes on to the one before (`down` false) or after under the same
    /// line that may be typed into, or back to the text past the first or
    /// last: what to write.
    pub fn next_row_edit(&mut self, down: bool) -> Option<WriteBack> {
        let (line, row) = self.row_edit.as_ref().map(|r| (r.line, r.row))?;
        let write = self.end_row_edit(true);
        let rows = self.editable_rows(line);
        let next = if down {
            rows.into_iter().find(|r| *r > row)
        } else {
            rows.into_iter().rev().find(|r| *r < row)
        };
        if let Some(n) = next {
            self.start_row_edit(line, n);
        }
        write
    }

    /// Enter while a line under a line is typed into: what was typed
    /// written; when the line starts a block of its file's outline, the
    /// block split at the cursor into a new block after it and the blocks
    /// under it, which is typed into next once the layer shows it.
    pub fn row_edit_enter(&mut self) -> Option<WriteBack> {
        let Some(o) = self.row_edit.as_ref()?.source.outline.clone() else {
            return self.end_row_edit(true);
        };
        let r = self.row_edit.take()?;
        let at = r.cursor();
        let after = r.input[at..].to_string();
        self.row_resume = Some(Resume {
            path: r.source.path.clone(),
            line: o.end.max(r.source.line + 1),
            column: o.bullet.len(),
            text: after.clone(),
            back: after.chars().count(),
            until: Instant::now() + RESUME,
        });
        Some(WriteBack {
            text: r.input[..at].to_string(),
            source: r.source,
            change: Change::Split(after),
        })
    }

    /// Tab (`out` false) or Shift+Tab while a line under a line is typed
    /// into: when the line starts a block of its file's outline, the block
    /// and the lines under it a step in or out with what was typed, typed
    /// into on at its new place; nothing where it may not move so. A line
    /// that starts no block goes on to the line after or before, as Down
    /// and Up.
    pub fn row_edit_tab(&mut self, out: bool) -> Option<WriteBack> {
        let r = self.row_edit.as_ref()?;
        let Some(o) = r.source.outline.clone() else {
            return self.next_row_edit(!out);
        };
        let may = if out { o.outdent } else { o.indent };
        let column = if out {
            r.source.column.checked_sub(o.step.len())
        } else {
            Some(r.source.column + o.step.len())
        };
        let (true, Some(column)) = (may && !o.step.is_empty(), column) else {
            return None;
        };
        let r = self.row_edit.take()?;
        self.row_resume = Some(Resume {
            path: r.source.path.clone(),
            line: r.source.line,
            column,
            text: r.input.clone(),
            back: r.back,
            until: Instant::now() + RESUME,
        });
        Some(WriteBack {
            source: r.source,
            text: r.input,
            change: if out { Change::Outdent } else { Change::Indent },
        })
    }

    /// Whether a block Enter made or Tab moved is waited for in its layer
    /// (the editors look again soon).
    pub fn waiting_row_edit(&self) -> bool {
        self.row_resume.is_some()
    }

    /// Types into the block Enter made or Tab moved once the layer shows
    /// it where its file now holds it, the cursor where it was; given up
    /// after some seconds. Whether it started.
    pub fn resume_row_edit(&mut self) -> bool {
        let Some(r) = self.row_resume.clone() else {
            return false;
        };
        if self.row_edit.is_some() || Instant::now() > r.until {
            self.row_resume = None;
            return false;
        }
        let Some(o) = self.overlays() else {
            return false;
        };
        let found = o.below.iter().find_map(|b| {
            b.lines
                .iter()
                .position(|l| {
                    l.source.as_ref().is_some_and(|s| {
                        s.path == r.path
                            && s.line == r.line
                            && s.column == r.column
                            && s.text == r.text
                    })
                })
                .map(|row| (b.line, row))
        });
        let Some((line, row)) = found else {
            return false;
        };
        self.row_resume = None;
        if !self.start_row_edit(line, row) {
            return false;
        }
        if let Some(e) = &mut self.row_edit {
            e.back = r.back.min(e.input.chars().count());
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(line: usize, column: usize, text: &str) -> LineSource {
        LineSource {
            path: std::env::temp_dir().join("kalem-row-edit-none.md"),
            line,
            column,
            text: text.into(),
            outline: None,
        }
    }

    #[test]
    fn written_where_the_file_still_holds_the_text() {
        let file = "- a\n- TODO buy milk\r\n\t- c\n";
        let wb = WriteBack {
            source: source(1, 2, "TODO buy milk"),
            text: "DONE buy bread".into(),
            change: Change::None,
        };
        assert_eq!(wb.range_in(file), Some(6..19));
        // Another text there, or no such line: not written.
        let moved = WriteBack {
            source: source(2, 3, "c"),
            ..wb.clone()
        };
        assert_eq!(moved.range_in(file), Some(24..25));
        let gone = WriteBack {
            source: source(1, 2, "TODO buy eggs"),
            ..wb.clone()
        };
        assert_eq!(gone.range_in(file), None);
        let past = WriteBack {
            source: source(9, 0, ""),
            ..wb.clone()
        };
        assert_eq!(past.range_in(file), None);
        // A file written: only that text changes.
        let dir = std::env::temp_dir().join(format!("kalem-row-edit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("page.md");
        std::fs::write(&path, file).unwrap();
        let wb = WriteBack {
            source: LineSource {
                path: path.clone(),
                ..wb.source
            },
            ..wb
        };
        wb.write_file().unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "- a\n- DONE buy bread\r\n\t- c\n"
        );
        assert!(wb.write_file().is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn outlined(line: usize, column: usize, text: &str, end: usize, bullet: &str) -> LineSource {
        LineSource {
            outline: Some(crate::layers::Outline {
                end,
                bullet: bullet.into(),
                step: "\t".into(),
                indent: true,
                outdent: true,
            }),
            ..source(line, column, text)
        }
    }

    /// The file `text` as `wb` writes it.
    fn written(wb: &WriteBack, text: &str) -> Option<String> {
        let mut new = String::new();
        let mut at = 0;
        for (r, t) in wb.edits(text)? {
            new.push_str(&text[at..r.start]);
            new.push_str(&t);
            at = r.end;
        }
        new.push_str(&text[at..]);
        Some(new)
    }

    #[test]
    fn a_block_split_indented_and_outdented_in_its_file() {
        let file = "- one\n\t- child\n- two\n";
        let wb = |source: LineSource, text: &str, change: Change| WriteBack {
            source,
            text: text.into(),
            change,
        };
        // Enter in "one" after "on": "on" stays, a new block with "e"
        // after the block and its child.
        let split = wb(
            outlined(0, 2, "one", 2, "- "),
            "on",
            Change::Split("e".into()),
        );
        assert_eq!(
            written(&split, file).as_deref(),
            Some("- on\n\t- child\n- e\n- two\n")
        );
        // At the file's end, with and without its last line feed; in
        // its line feeds.
        let last = wb(
            outlined(2, 2, "two", 3, "- "),
            "two",
            Change::Split(String::new()),
        );
        assert_eq!(
            written(&last, file).as_deref(),
            Some("- one\n\t- child\n- two\n- \n")
        );
        assert_eq!(
            written(&last, "- one\n\t- child\n- two").as_deref(),
            Some("- one\n\t- child\n- two\n- ")
        );
        assert_eq!(
            written(&split, "- one\r\n\t- child\r\n- two\r\n").as_deref(),
            Some("- on\r\n\t- child\r\n- e\r\n- two\r\n")
        );
        // Tab: the block and its child a step in; Shift+Tab: out again.
        let tab = wb(outlined(0, 2, "one", 2, "- "), "one", Change::Indent);
        let indented = written(&tab, file).unwrap();
        assert_eq!(indented, "\t- one\n\t\t- child\n- two\n");
        let back = wb(outlined(0, 3, "one", 2, "\t- "), "one", Change::Outdent);
        assert_eq!(written(&back, &indented).as_deref(), Some(file));
        // A block with no step to take off, or a file changed: none.
        let none = wb(outlined(0, 2, "one", 2, "- "), "one", Change::Outdent);
        assert_eq!(written(&none, file), None);
        assert_eq!(written(&tab, "- other\n"), None);
    }
}
