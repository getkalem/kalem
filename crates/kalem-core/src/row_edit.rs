//! Lines under lines typed into where they are drawn (API 0.2.15): a
//! layer's line under a line that shows the text of a file
//! ([`LineSource`]) is edited there as a one-line input, and what is typed
//! is written back in place of that text: in the file's open document when
//! that has unsaved changes, else in the file, which the layer then reads
//! again. An embedded block typed into in the note that embeds it (the
//! graph plugin's Logseq embeds), a task renamed in a query's results.
//! Only files in Kalem's projects are written, and only where they still
//! hold the text the layer saw.

use std::ops::Range;
use std::path::Path;
use std::time::Instant;

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

    /// Writes it into the file: read, checked, written.
    pub fn write_file(&self) -> Result<(), String> {
        let path = &self.source.path;
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let r = self.range_in(&text).ok_or_else(|| self.changed())?;
        let mut new = String::with_capacity(text.len() + self.text.len());
        new.push_str(&text[..r.start]);
        new.push_str(&self.text);
        new.push_str(&text[r.end..]);
        crate::files::write(path, new.as_bytes(), crate::files::SaveOptions::default())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Writes it into `doc`, the file's open document, as a step of its
    /// own to undo.
    pub fn write_into(&self, doc: &mut DocumentState, now: Instant) -> Result<(), String> {
        let r = self
            .range_in(doc.text().as_str())
            .ok_or_else(|| self.changed())?;
        let mut tx = Transaction::new("Edit Line Under Line");
        tx.edit(r, self.text.clone());
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
        }
    }

    #[test]
    fn written_where_the_file_still_holds_the_text() {
        let file = "- a\n- TODO buy milk\r\n\t- c\n";
        let wb = WriteBack {
            source: source(1, 2, "TODO buy milk"),
            text: "DONE buy bread".into(),
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
}
