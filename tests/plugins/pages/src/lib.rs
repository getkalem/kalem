//! A fake viewer: three pages of 100 × 50 pixels, page N filled with the
//! byte N and holding the text "page N", the file read through the handle
//! the host gave.

use std::cell::RefCell;

use kalem_plugin::viewer::exports::kalem::plugin::viewer::{
    Bitmap, Description, Detection, Document, Guest, GuestDocument, Hit, InfoField, Link,
    OutlineEntry, Rect, RenderRequest, SaveOutput, Span, Structure, Unit, UnitKind,
};
use kalem_plugin::viewer::kalem::plugin::files::File;

struct Pages;

struct Doc {
    file: File,
    renders: RefCell<u32>,
}

impl Guest for Pages {
    type Document = Doc;

    fn describe() -> Description {
        Description {
            id: "pages".into(),
            name: "Pages".into(),
            extensions: vec!["pages".into()],
        }
    }

    fn detect(name: String, head: Vec<u8>) -> Detection {
        if head.starts_with(b"PAGES") {
            Detection::Magic
        } else if name.ends_with(".pages") {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    fn open(file: File) -> Result<Document, String> {
        if file.read(0, 5) != b"PAGES" {
            return Err("not a pages file".into());
        }
        Ok(Document::new(Doc {
            file,
            renders: RefCell::new(0),
        }))
    }
}

impl GuestDocument for Doc {
    fn structure(&self) -> Structure {
        Structure {
            units: (1..=3)
                .map(|n| Unit {
                    kind: UnitKind::Page,
                    label: n.to_string(),
                    duration_ms: None,
                })
                .collect(),
            outline: vec![OutlineEntry {
                title: "Last".into(),
                unit: 2,
                level: 1,
            }],
        }
    }

    fn size(&self, _unit: u32) -> Option<(f32, f32)> {
        Some((100.0, 50.0))
    }

    fn render(&self, unit: u32, request: RenderRequest) -> Result<Bitmap, String> {
        *self.renders.borrow_mut() += 1;
        let (w, h) = (
            (100.0 * request.scale) as u32,
            (50.0 * request.scale) as u32,
        );
        Ok(Bitmap {
            width: w,
            height: h,
            rgba: vec![unit as u8 + 1; (w * h * 4) as usize],
        })
    }

    fn text(&self, unit: u32) -> String {
        format!("page {}", unit + 1)
    }

    fn text_rects(&self, _unit: u32, span: Span) -> Vec<Rect> {
        vec![Rect {
            x: span.start as f32 * 10.0,
            y: 30.0,
            width: (span.end - span.start) as f32 * 10.0,
            height: 10.0,
        }]
    }

    fn text_at(&self, _unit: u32, x: f32, _y: f32) -> Option<(Span, Rect)> {
        let i = (x / 10.0) as u32;
        Some((
            Span {
                start: i,
                end: i + 1,
            },
            Rect {
                x: i as f32 * 10.0,
                y: 30.0,
                width: 10.0,
                height: 10.0,
            },
        ))
    }

    fn info(&self) -> Vec<InfoField> {
        vec![
            InfoField {
                label: "Name".into(),
                value: self.file.name(),
            },
            InfoField {
                label: "Size".into(),
                value: self.file.len().to_string(),
            },
            InfoField {
                label: "Renders".into(),
                value: self.renders.borrow().to_string(),
            },
        ]
    }

    fn search(&self, query: String) -> Vec<Hit> {
        (0..3)
            .filter_map(|u| {
                let t = self.text(u);
                t.find(&query).map(|i| Hit {
                    unit: u,
                    span: Span {
                        start: i as u32,
                        end: (i + query.len()) as u32,
                    },
                })
            })
            .collect()
    }

    fn links(&self, _unit: u32) -> Vec<Link> {
        Vec::new()
    }

    fn edits(&self, _unit: u32) -> Vec<kalem_plugin::viewer::exports::kalem::plugin::viewer::Edit> {
        Vec::new()
    }

    fn apply(&self, edit: String) -> Result<Vec<u32>, String> {
        Err(format!("no edit {edit}"))
    }

    fn modified(&self) -> bool {
        false
    }

    fn save(&self) -> Result<SaveOutput, String> {
        Err("this format is not edited".into())
    }
}

/// No file of its is protected by a password: opened as `open` does.
impl kalem_plugin::viewer::exports::kalem::plugin::password::Guest for Pages {
    fn open_with_password(
        file: kalem_plugin::viewer::kalem::plugin::files::File,
        _password: String,
    ) -> Result<kalem_plugin::viewer::exports::kalem::plugin::viewer::Document, String> {
        <Pages as Guest>::open(file)
    }
}

/// No other formats: a viewer of the WIT world itself answers the
/// `formats` interface with refusals.
impl kalem_plugin::viewer::exports::kalem::plugin::formats::Guest for Pages {
    fn save_as(
        _doc: kalem_plugin::viewer::exports::kalem::plugin::formats::DocumentBorrow<'_>,
        extension: String,
    ) -> Result<kalem_plugin::viewer::exports::kalem::plugin::formats::SaveOutput, String> {
        Err(format!("no .{extension} files"))
    }

    fn new_file(
        extension: String,
        _sheets: Vec<kalem_plugin::viewer::exports::kalem::plugin::formats::Sheet>,
    ) -> Result<Vec<u8>, String> {
        Err(format!("no .{extension} files"))
    }
}

kalem_plugin::viewer::export_viewer!(Pages);
