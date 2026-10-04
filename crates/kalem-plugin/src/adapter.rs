//! A viewer written against the Rust contract (`kalem_viewer::Viewer`, the
//! traits bundled viewers implement) exported as a component of the
//! `document-viewer` world: [`export_viewer_of!`](crate::export_viewer_of)
//! writes the world's exports over it. One crate builds both ways: bundled
//! into a binary, or as a sandboxed component.

use std::cell::RefCell;

use kalem_viewer::{
    Detection, FileHandle, RenderRequest, Rendered, Theme, UnitKind, Viewer, ViewerDocument,
};

use crate::viewer::exports::kalem::plugin::viewer as w;
use crate::viewer::kalem::plugin::files::File;

pub use kalem_viewer;

/// A document the viewer opened, as the `document` resource.
pub struct Doc(RefCell<Box<dyn ViewerDocument>>);

impl std::fmt::Debug for Doc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Doc")
    }
}

/// The plugin's description of itself.
pub fn describe(v: &dyn Viewer) -> w::Description {
    w::Description {
        id: v.id().to_string(),
        name: v.name().to_string(),
        extensions: v.extensions().iter().map(|e| e.to_string()).collect(),
    }
}

/// Whether `v` opens the file `name` starting with `head`.
pub fn detect(v: &dyn Viewer, name: &str, head: &[u8]) -> w::Detection {
    match v.detect(name, head) {
        Detection::No => w::Detection::No,
        Detection::Extension => w::Detection::Extension,
        Detection::Magic => w::Detection::Magic,
    }
}

/// Opens the host's `file` with `v`: the plugin reads it through the
/// handle, piece by piece as it asks.
pub fn open(v: &dyn Viewer, file: File) -> Result<w::Document, String> {
    let name = file.name();
    let len = file.len();
    let handle = FileHandle::from_reader(name, len, move |offset, len| {
        file.read(offset, len.min(u32::MAX as usize) as u32)
    });
    let doc = v.open(handle).map_err(|e| e.0)?;
    Ok(w::Document::new(Doc(RefCell::new(doc))))
}

fn span(s: w::Span) -> std::ops::Range<usize> {
    s.start as usize..s.end as usize
}

fn to_span(r: std::ops::Range<usize>) -> w::Span {
    w::Span {
        start: r.start as u32,
        end: r.end as u32,
    }
}

fn rect([x, y, width, height]: [f32; 4]) -> w::Rect {
    w::Rect {
        x,
        y,
        width,
        height,
    }
}

impl w::GuestDocument for Doc {
    fn structure(&self) -> w::Structure {
        let s = self.0.borrow().structure();
        w::Structure {
            units: s
                .units
                .into_iter()
                .map(|u| w::Unit {
                    kind: match u.kind {
                        UnitKind::Image => w::UnitKind::Image,
                        UnitKind::Frame => w::UnitKind::Frame,
                        UnitKind::Page => w::UnitKind::Page,
                        UnitKind::Sheet => w::UnitKind::Sheet,
                        UnitKind::Slide => w::UnitKind::Slide,
                        UnitKind::Table => w::UnitKind::Table,
                    },
                    label: u.label,
                    duration_ms: u.duration_ms,
                })
                .collect(),
            outline: s
                .outline
                .into_iter()
                .map(|e| w::OutlineEntry {
                    title: e.title,
                    unit: e.unit as u32,
                    level: e.level,
                })
                .collect(),
        }
    }

    fn size(&self, unit: u32) -> Option<(f32, f32)> {
        self.0.borrow().size(unit as usize)
    }

    fn render(&self, unit: u32, request: w::RenderRequest) -> Result<w::Bitmap, String> {
        let rgb = |c: w::Rgb| [c.r, c.g, c.b];
        let request = RenderRequest {
            scale: request.scale,
            theme: Theme {
                dark: request.theme.dark,
                background: rgb(request.theme.background),
                foreground: rgb(request.theme.foreground),
            },
        };
        let rendered = self.0.borrow_mut().render(unit as usize, request);
        match rendered.map_err(|e| e.0)? {
            Rendered::Bitmap(b) => Ok(w::Bitmap {
                width: b.width,
                height: b.height,
                rgba: b.rgba.to_vec(),
            }),
        }
    }

    fn text(&self, unit: u32) -> String {
        self.0.borrow().text(unit as usize)
    }

    fn text_rects(&self, unit: u32, s: w::Span) -> Vec<w::Rect> {
        let rects = self.0.borrow().text_rects(unit as usize, span(s));
        rects.into_iter().map(rect).collect()
    }

    fn text_at(&self, unit: u32, x: f32, y: f32) -> Option<(w::Span, w::Rect)> {
        let (r, b) = self.0.borrow().text_at(unit as usize, x, y)?;
        Some((to_span(r), rect(b)))
    }

    fn info(&self) -> Vec<w::InfoField> {
        let fields = self.0.borrow().info();
        fields
            .into_iter()
            .map(|f| w::InfoField {
                label: f.label,
                value: f.value,
            })
            .collect()
    }

    fn search(&self, query: String) -> Vec<w::Hit> {
        let hits = self.0.borrow().search(&query);
        hits.into_iter()
            .map(|(unit, r)| w::Hit {
                unit: unit as u32,
                span: to_span(r),
            })
            .collect()
    }

    fn links(&self, unit: u32) -> Vec<w::Link> {
        let links = self.0.borrow().links(unit as usize);
        links
            .into_iter()
            .map(|l| w::Link {
                rect: rect(l.rect),
                target: l.target,
            })
            .collect()
    }

    fn edits(&self, unit: u32) -> Vec<w::Edit> {
        let edits = self.0.borrow().edits(unit as usize);
        edits
            .into_iter()
            .map(|e| w::Edit {
                id: e.id,
                title: e.title,
                inverse: e.inverse,
            })
            .collect()
    }

    fn apply(&self, edit: String) -> Result<Vec<u32>, String> {
        let changed = self.0.borrow_mut().apply(&edit).map_err(|e| e.0)?;
        Ok(changed.into_iter().map(|u| u as u32).collect())
    }

    fn modified(&self) -> bool {
        self.0.borrow().modified()
    }

    fn save(&self) -> Result<w::SaveOutput, String> {
        let out = self.0.borrow_mut().save().map_err(|e| e.0)?;
        Ok(w::SaveOutput {
            bytes: out.bytes,
            losses: out.losses,
        })
    }
}

/// Exports a viewer of the Rust contract as the component's
/// `document-viewer` world: `export_viewer_of!(PdfViewer)`, the
/// expression making the viewer (evaluated for each call; a unit struct
/// costs nothing).
#[macro_export]
macro_rules! export_viewer_of {
    ($viewer:expr) => {
        #[doc(hidden)]
        pub struct __KalemViewer;

        impl $crate::viewer::exports::kalem::plugin::viewer::Guest for __KalemViewer {
            type Document = $crate::adapter::Doc;

            fn describe() -> $crate::viewer::exports::kalem::plugin::viewer::Description {
                $crate::adapter::describe(&$viewer)
            }

            fn detect(
                name: ::std::string::String,
                head: ::std::vec::Vec<u8>,
            ) -> $crate::viewer::exports::kalem::plugin::viewer::Detection {
                $crate::adapter::detect(&$viewer, &name, &head)
            }

            fn open(
                file: $crate::viewer::kalem::plugin::files::File,
            ) -> ::std::result::Result<
                $crate::viewer::exports::kalem::plugin::viewer::Document,
                ::std::string::String,
            > {
                $crate::adapter::open(&$viewer, file)
            }
        }

        $crate::viewer::export_viewer!(__KalemViewer);
    };
}
