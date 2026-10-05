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
/// Installs, once, the panic hook that tells the host a panic's message
/// and where it happened (the `diagnostics` import, wasm_todo W10): a
/// component stops at a panic with a trap that carries neither. The
/// adapter's entries call it, the host calling one of them first.
pub fn report_panics() {
    #[cfg(target_arch = "wasm32")]
    {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            std::panic::set_hook(Box::new(|info| {
                crate::viewer::kalem::plugin::diagnostics::panicked(&info.to_string());
            }));
        });
    }
}

pub fn describe(v: &dyn Viewer) -> w::Description {
    report_panics();
    w::Description {
        id: v.id().to_string(),
        name: v.name().to_string(),
        extensions: v.extensions().iter().map(|e| e.to_string()).collect(),
    }
}

/// Whether `v` opens the file `name` starting with `head`.
pub fn detect(v: &dyn Viewer, name: &str, head: &[u8]) -> w::Detection {
    report_panics();
    match v.detect(name, head) {
        Detection::No => w::Detection::No,
        Detection::Extension => w::Detection::Extension,
        Detection::Magic => w::Detection::Magic,
    }
}

/// Opens the host's `file` with `v`: the plugin reads it through the
/// handle, piece by piece as it asks.
pub fn open(v: &dyn Viewer, file: File) -> Result<w::Document, String> {
    report_panics();
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

        $crate::__kalem_grid!(__KalemViewer);
        $crate::viewer::export_viewer!(__KalemViewer);
    };
}

/// The `grid` interface's exports, with the feature `grid`.
#[cfg(feature = "grid")]
#[doc(hidden)]
#[macro_export]
macro_rules! __kalem_grid {
    ($t:ty) => {
        $crate::__kalem_grid_exports!($t);
    };
}

/// Nothing without the feature `grid`.
#[cfg(not(feature = "grid"))]
#[doc(hidden)]
#[macro_export]
macro_rules! __kalem_grid {
    ($t:ty) => {};
}

/// The grid's half of the adapter (the feature `grid`): the `grid`
/// interface over the contract's grid functions, for
/// [`export_viewer_of!`](crate::export_viewer_of).
#[cfg(feature = "grid")]
#[doc(hidden)]
pub mod grid {
    pub use kalem_viewer as kv;

    pub use crate::viewer::exports::kalem::plugin::grid as g;

    include!("grid_conv.rs");

    /// Calls `f` with the document behind the handle.
    pub fn with<R>(
        d: g::DocumentBorrow<'_>,
        f: impl FnOnce(&mut dyn kv::ViewerDocument) -> R,
    ) -> R {
        let doc = d.get::<super::Doc>();
        let mut doc = doc.0.borrow_mut();
        f(&mut **doc)
    }

    /// A span of rows or columns.
    pub fn span((start, end): (u32, u32)) -> std::ops::Range<u32> {
        start..end
    }

    /// A range as the contract's.
    pub fn range(r: (u32, u32, u32, u32)) -> [u32; 4] {
        r.conv()
    }

    /// A macro's answers as the contract's [`kv::MacroUi`]: answered in
    /// order; past them, unanswered, so the macro stops and reports the
    /// question.
    #[derive(Debug)]
    pub struct Answers(pub std::collections::VecDeque<g::MacroAnswer>);

    impl kv::MacroUi for Answers {
        fn message(&mut self, _prompt: &str, _buttons: i64, _title: &str) -> Option<i64> {
            match self.0.pop_front()? {
                g::MacroAnswer::Message(b) => Some(b),
                g::MacroAnswer::Input(_) => None,
            }
        }

        fn input(&mut self, _prompt: &str, _title: &str, _default: &str) -> Option<Option<String>> {
            match self.0.pop_front()? {
                g::MacroAnswer::Input(t) => Some(t),
                g::MacroAnswer::Message(_) => None,
            }
        }
    }
}

/// The `grid` interface's exports over [`grid::with`], for a viewer
/// exported with the feature `grid`.
#[cfg(feature = "grid")]
#[doc(hidden)]
#[macro_export]
macro_rules! __kalem_grid_exports {
    ($t:ty) => {
        impl $crate::adapter::grid::g::Guest for $t {
            fn layout(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::option::Option<$crate::adapter::grid::g::GridLayout> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.grid(unit as usize)).conv()
            }
            fn cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, rows: (u32, u32), cols: (u32, u32)) -> ::std::vec::Vec<$crate::adapter::grid::g::PlacedCell> {
                use $crate::adapter::grid::{span, Conv};
                $crate::adapter::grid::with(d, |x| x.grid_cells(unit as usize, span(rows), span(cols)))
                    .into_iter()
                    .map(|(row, col, cell)| $crate::adapter::grid::g::PlacedCell { row, col, cell: cell.conv() })
                    .collect()
            }
            fn cell_input(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32) -> ::std::string::String {
                $crate::adapter::grid::with(d, |x| x.cell_input(unit as usize, row, col))
            }
            fn set_frozen(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, rows: u32, cols: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_frozen(unit as usize, rows, cols)))
            }
            fn set_hidden(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, rows: bool, from: u32, to: u32, hidden: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_hidden(unit as usize, rows, from, to, hidden)))
            }
            fn edit_sheets(d: $crate::adapter::grid::g::DocumentBorrow<'_>, edit: $crate::adapter::grid::g::SheetEdit) -> ::std::result::Result<u32, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.edit_sheets(edit.conv())).map(|u| u as u32).map_err(|e| e.0)
            }
            fn hidden_units(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::vec::Vec<u32> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.hidden_units()).conv()
            }
            fn range_numbers(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, range: (u32, u32, u32, u32)) -> (::std::vec::Vec<f64>, u32) {
                let (v, n) = $crate::adapter::grid::with(d, |x| x.range_numbers(unit as usize, $crate::adapter::grid::range(range)));
                (v, n as u32)
            }
            fn cell_format(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32) -> ::std::option::Option<::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.cell_format(unit as usize, row, col))
            }
            fn cell_note(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32) -> ::std::option::Option<::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.cell_note(unit as usize, row, col))
            }
            fn formula_functions(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::vec::Vec<(::std::string::String, ::std::string::String)> {
                $crate::adapter::grid::with(d, |x| x.formula_functions())
            }
            fn paste_cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, from_unit: u32, from: (u32, u32, u32, u32), to_unit: u32, to_row: u32, to_col: u32, kind: $crate::adapter::grid::g::PasteKind, transpose: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::{range, Conv};
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.paste_cells((from_unit as usize, range(from)), (to_unit as usize, to_row, to_col), kind.conv(), transpose)))
            }
            fn clear_range(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), contents: bool, formats: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.clear_range(unit as usize, range(r), contents, formats)))
            }
            fn fill_formats(d: $crate::adapter::grid::g::DocumentBorrow<'_>, from_unit: u32, from: (u32, u32, u32, u32), to_unit: u32, to: (u32, u32, u32, u32)) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.fill_formats((from_unit as usize, range(from)), (to_unit as usize, range(to)))))
            }
            fn remove_duplicates(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), columns: ::std::vec::Vec<u32>, header: bool) -> ::std::result::Result<u32, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::with(d, |x| x.remove_duplicates(unit as usize, range(r), &columns, header)).map(|n| n as u32).map_err(|e| e.0)
            }
            fn cell_link(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32) -> ::std::option::Option<::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.cell_link(unit as usize, row, col))
            }
            fn set_link(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, target: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_link(unit as usize, row, col, target)))
            }
            fn defined_names(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::vec::Vec<(::std::string::String, ::std::string::String)> {
                $crate::adapter::grid::with(d, |x| x.defined_names())
            }
            fn set_defined_name(d: $crate::adapter::grid::g::DocumentBorrow<'_>, name: ::std::string::String, refers_to: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_defined_name(&name, refers_to.as_deref())))
            }
            fn recalculate(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.recalculate()))
            }
            fn set_note(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, text: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_note(unit as usize, row, col, text)))
            }
            fn set_cell(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, input: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_cell(unit as usize, row, col, &input)))
            }
            fn edit_grid(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, edit: $crate::adapter::grid::g::GridEdit) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.grid_edit(unit as usize, edit.conv())))
            }
            fn set_cell_list(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, cells: ::std::vec::Vec<(u32, u32, ::std::string::String)>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_cell_list(unit as usize, &cells)))
            }
            fn set_cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, values: ::std::vec::Vec<::std::vec::Vec<::std::string::String>>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_cells(unit as usize, row, col, &values)))
            }
            fn add_conditional_format(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), rule: $crate::adapter::grid::g::CondRule, style: $crate::adapter::grid::g::CondStyle) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::{range, Conv};
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.add_conditional_format(unit as usize, range(r), rule.conv(), style.conv())))
            }
            fn clear_conditional_formats(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: ::std::option::Option<(u32, u32, u32, u32)>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.clear_conditional_formats(unit as usize, r.conv())))
            }
            fn charts(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::vec::Vec<$crate::adapter::grid::g::Chart> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.charts(unit as usize)).conv()
            }
            fn insert_chart(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), kind: $crate::adapter::grid::g::ChartKind, title: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::{range, Conv};
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.insert_chart(unit as usize, range(r), kind.conv(), title)))
            }
            fn move_chart(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, anchor: (u32, u32, u32, u32)) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.move_chart(unit as usize, index as usize, range(anchor))))
            }
            fn set_chart_title(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, title: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_chart_title(unit as usize, index as usize, title)))
            }
            fn set_axis_title(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, axis: $crate::adapter::grid::g::ChartAxis, title: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_axis_title(unit as usize, index as usize, axis.conv(), title)))
            }
            fn set_legend(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, position: ::std::option::Option<$crate::adapter::grid::g::LegendPosition>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_legend(unit as usize, index as usize, position.conv())))
            }
            fn set_data_labels(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, labels: $crate::adapter::grid::g::DataLabels) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_data_labels(unit as usize, index as usize, labels.conv())))
            }
            fn set_axis_scale(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, scale: $crate::adapter::grid::g::AxisScale) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_axis_scale(unit as usize, index as usize, scale.conv())))
            }
            fn set_chart_kind(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, kind: $crate::adapter::grid::g::ChartKind) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_chart_kind(unit as usize, index as usize, kind.conv())))
            }
            fn set_series_kind(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, series: u32, kind: ::std::option::Option<$crate::adapter::grid::g::ChartKind>, secondary: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_series_kind(unit as usize, index as usize, series as usize, kind.conv(), secondary)))
            }
            fn set_trendline(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, series: u32, trendline: ::std::option::Option<$crate::adapter::grid::g::Trendline>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_trendline(unit as usize, index as usize, series as usize, trendline.conv())))
            }
            fn set_error_bars(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, series: u32, bars: ::std::option::Option<$crate::adapter::grid::g::ErrorBars>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_error_bars(unit as usize, index as usize, series as usize, bars.conv())))
            }
            fn set_label_cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, series: u32, range: ::std::option::Option<(u32, u32, u32, u32)>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_label_cells(unit as usize, index as usize, series as usize, range.map($crate::adapter::grid::range))))
            }
            fn move_chart_to_sheet(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, name: ::std::string::String) -> ::std::result::Result<u32, ::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.move_chart_to_sheet(unit as usize, index as usize, &name)).map(|u| u as u32).map_err(|e| e.0)
            }
            fn move_chart_to_grid(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, target: u32, anchor: (u32, u32, u32, u32)) -> ::std::result::Result<u32, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::with(d, |x| x.move_chart_to_grid(unit as usize, target as usize, range(anchor))).map(|u| u as u32).map_err(|e| e.0)
            }
            fn chart_template(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32) -> ::std::result::Result<::std::vec::Vec<u8>, ::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.chart_template(unit as usize, index as usize)).map_err(|e| e.0)
            }
            fn apply_chart_template(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, template: ::std::vec::Vec<u8>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.apply_chart_template(unit as usize, index as usize, &template)))
            }
            fn set_series_color(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, series: u32, color: ::std::option::Option<$crate::adapter::grid::g::Rgb>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_series_color(unit as usize, index as usize, series as usize, color.conv())))
            }
            fn set_point_color(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, series: u32, point: u32, color: ::std::option::Option<$crate::adapter::grid::g::Rgb>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_point_color(unit as usize, index as usize, series as usize, point as usize, color.conv())))
            }
            fn set_explosion(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, series: u32, point: ::std::option::Option<u32>, percent: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_explosion(unit as usize, index as usize, series as usize, point.conv(), percent)))
            }
            fn set_chart_area(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, background: $crate::adapter::grid::g::Paint, border: $crate::adapter::grid::g::Paint) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_chart_area(unit as usize, index as usize, background.conv(), border.conv())))
            }
            fn set_plot_area(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, background: $crate::adapter::grid::g::Paint, border: $crate::adapter::grid::g::Paint) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_plot_area(unit as usize, index as usize, background.conv(), border.conv())))
            }
            fn set_gridlines(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, lines: $crate::adapter::grid::g::Gridlines) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_gridlines(unit as usize, index as usize, lines.conv())))
            }
            fn set_axis_format(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, format: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_axis_format(unit as usize, index as usize, format)))
            }
            fn set_axis_font(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, axis: $crate::adapter::grid::g::ChartAxis, font: $crate::adapter::grid::g::AxisFont) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_axis_font(unit as usize, index as usize, axis.conv(), font.conv())))
            }
            fn set_title_font(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, font: $crate::adapter::grid::g::AxisFont) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_title_font(unit as usize, index as usize, font.conv())))
            }
            fn set_legend_font(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, font: $crate::adapter::grid::g::AxisFont) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_legend_font(unit as usize, index as usize, font.conv())))
            }
            fn delete_chart(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.delete_chart(unit as usize, index as usize)))
            }
            fn insert_pivot(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, spec: $crate::adapter::grid::g::PivotSpec) -> ::std::result::Result<u32, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.insert_pivot(unit as usize, spec.conv())).map(|u| u as u32).map_err(|e| e.0)
            }
            fn pivots(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::vec::Vec<$crate::adapter::grid::g::PivotInfo> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.pivots(unit as usize)).conv()
            }
            fn set_pivot(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, spec: $crate::adapter::grid::g::PivotSpec) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_pivot(unit as usize, index as usize, spec.conv())))
            }
            fn insert_pivot_chart(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, kind: $crate::adapter::grid::g::ChartKind) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.insert_pivot_chart(unit as usize, index as usize, kind.conv())))
            }
            fn slicers(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::vec::Vec<$crate::adapter::grid::g::Slicer> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.slicers(unit as usize)).conv()
            }
            fn insert_slicer(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, pivot: ::std::option::Option<u32>, table: ::std::option::Option<::std::string::String>, field: ::std::string::String, anchor: (u32, u32, u32, u32)) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.insert_slicer(unit as usize, pivot.map(|p| p as usize), table.as_deref(), &field, range(anchor))))
            }
            fn select_slicer(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, selected: ::std::vec::Vec<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.select_slicer(unit as usize, index as usize, &selected)))
            }
            fn delete_slicer(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.delete_slicer(unit as usize, index as usize)))
            }
            fn refresh_pivots(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.refresh_pivots()))
            }
            fn validation(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32) -> ::std::option::Option<$crate::adapter::grid::g::CellValidation> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.validation(unit as usize, row, col)).conv()
            }
            fn change_style(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), change: $crate::adapter::grid::g::StyleChange) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::{range, Conv};
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.change_style(unit as usize, range(r), change.conv())))
            }
            fn set_validation(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), validation: ::std::option::Option<$crate::adapter::grid::g::CellValidation>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::{range, Conv};
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_validation(unit as usize, range(r), validation.conv())))
            }
            fn check_input(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, input: ::std::string::String) -> ::std::option::Option<$crate::adapter::grid::g::ValidationError> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.check_input(unit as usize, row, col, &input)).conv()
            }
            fn invalid_cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, rows: (u32, u32), cols: (u32, u32)) -> ::std::vec::Vec<(u32, u32)> {
                use $crate::adapter::grid::span;
                $crate::adapter::grid::with(d, |x| x.invalid_cells(unit as usize, span(rows), span(cols)))
            }
            fn set_fill_lists(d: $crate::adapter::grid::g::DocumentBorrow<'_>, lists: ::std::vec::Vec<::std::vec::Vec<::std::string::String>>) {
                $crate::adapter::grid::with(d, |x| x.set_fill_lists(lists))
            }
            fn fill(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, source: (u32, u32, u32, u32), target: (u32, u32, u32, u32), series: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.fill(unit as usize, range(source), range(target), series)))
            }
            fn sort_range(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), key: u32, descending: bool, header: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.sort_range(unit as usize, range(r), key, descending, header)))
            }
            fn set_filter(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: ::std::option::Option<(u32, u32, u32, u32)>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_filter(unit as usize, r.conv())))
            }
            fn filter_column(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, col: u32, values: ::std::option::Option<::std::vec::Vec<::std::string::String>>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.filter_column(unit as usize, col, values)))
            }
            fn move_cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), row: u32, col: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.move_cells(unit as usize, range(r), row, col)))
            }
            fn move_cells_between(d: $crate::adapter::grid::g::DocumentBorrow<'_>, from: u32, r: (u32, u32, u32, u32), to: u32, row: u32, col: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.move_cells_between(from as usize, range(r), to as usize, row, col)))
            }
            fn clear_cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32)) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.clear_cells(unit as usize, range(r))))
            }
            fn merge_cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), center: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.merge_cells(unit as usize, range(r), center)))
            }
            fn unmerge_cells(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.unmerge_cells(unit as usize, row, col)))
            }
            fn set_wrap(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, wrap: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_wrap(unit as usize, row, col, wrap)))
            }
            fn set_row_height(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, height: f32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_row_height(unit as usize, row, height)))
            }
            fn set_col_width(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, col: u32, width: f32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_col_width(unit as usize, col, width)))
            }
            fn enter_in_range(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), at: (u32, u32), input: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.enter_in_range(unit as usize, range(r), at, &input)))
            }
            fn tables(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::vec::Vec<$crate::adapter::grid::g::TableInfo> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.tables(unit as usize)).conv()
            }
            fn create_table(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), header: bool, style: ::std::string::String) -> ::std::result::Result<::std::string::String, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::with(d, |x| x.create_table(unit as usize, range(r), header, &style)).map_err(|e| e.0)
            }
            fn set_table_totals(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, name: ::std::string::String, on: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_table_totals(unit as usize, &name, on)))
            }
            fn remove_table(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, name: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.remove_table(unit as usize, &name)))
            }
            fn sort_range_by(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), keys: ::std::vec::Vec<$crate::adapter::grid::g::SortKey>, header: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::{range, Conv};
                let keys: ::std::vec::Vec<$crate::adapter::grid::kv::SortKey> = keys.conv();
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.sort_range_by(unit as usize, range(r), &keys, header)))
            }
            fn filter_column_by(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, col: u32, rule: ::std::option::Option<$crate::adapter::grid::g::FilterRule>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.filter_column_by(unit as usize, col, rule.conv())))
            }
            fn column_filter(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, col: u32) -> ::std::option::Option<$crate::adapter::grid::g::FilterRule> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.column_filter(unit as usize, col)).conv()
            }
            fn reapply_filter(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.reapply_filter(unit as usize)))
            }
            fn outline(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> (::std::vec::Vec<(u32, u8)>, ::std::vec::Vec<(u32, u8)>) {
                $crate::adapter::grid::with(d, |x| x.outline(unit as usize))
            }
            fn set_outline(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, rows: bool, from: u32, to: u32, deeper: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_outline(unit as usize, rows, from, to, deeper)))
            }
            fn set_detail_shown(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, rows: bool, at: u32, shown: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_detail_shown(unit as usize, rows, at, shown)))
            }
            fn subtotal(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), by: u32, function: u32, columns: ::std::vec::Vec<u32>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.subtotal(unit as usize, range(r), by, function, &columns)))
            }
            fn begin_batch(d: $crate::adapter::grid::g::DocumentBorrow<'_>) {
                $crate::adapter::grid::with(d, |x| x.begin_batch())
            }
            fn end_batch(d: $crate::adapter::grid::g::DocumentBorrow<'_>) {
                $crate::adapter::grid::with(d, |x| x.end_batch())
            }
            fn conditional_ranges(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::vec::Vec<(u32, u32, u32, u32)> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.conditional_ranges(unit as usize)).conv()
            }
            fn evaluate_formulas(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, formulas: ::std::vec::Vec<::std::string::String>) -> ::std::vec::Vec<::std::option::Option<::std::string::String>> {
                $crate::adapter::grid::with(d, |x| x.evaluate_formulas(unit as usize, &formulas))
            }
            fn sheet_protection(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::option::Option<$crate::adapter::grid::g::Protection> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.sheet_protection(unit as usize)).conv()
            }
            fn protect_sheet(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, protection: ::std::option::Option<$crate::adapter::grid::g::Protection>, password: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.protect_sheet(unit as usize, protection.conv(), password.as_deref())))
            }
            fn workbook_protected(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> bool {
                $crate::adapter::grid::with(d, |x| x.workbook_protected())
            }
            fn protect_workbook(d: $crate::adapter::grid::g::DocumentBorrow<'_>, on: bool, password: ::std::option::Option<::std::string::String>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.protect_workbook(on, password.as_deref())))
            }
            fn page_setup(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::option::Option<$crate::adapter::grid::g::PageLayout> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.page_setup(unit as usize)).conv()
            }
            fn set_page_setup(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, setup: $crate::adapter::grid::g::PageLayout) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                let setup: $crate::adapter::grid::kv::PageSetup = setup.conv();
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_page_setup(unit as usize, &setup)))
            }
            fn drawings(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::vec::Vec<$crate::adapter::grid::g::Drawing> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.drawings(unit as usize)).conv()
            }
            fn drawing_image(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32) -> ::std::option::Option<::std::vec::Vec<u8>> {
                $crate::adapter::grid::with(d, |x| x.drawing_image(unit as usize, index as usize))
            }
            fn insert_picture(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, anchor: (u32, u32, u32, u32), bytes: ::std::vec::Vec<u8>, extension: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.insert_picture(unit as usize, range(anchor), &bytes, &extension)))
            }
            fn insert_shape(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, anchor: (u32, u32, u32, u32), preset: ::std::string::String, text: ::std::string::String, text_box: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.insert_shape(unit as usize, range(anchor), &preset, &text, text_box)))
            }
            fn move_drawing(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, anchor: (u32, u32, u32, u32)) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.move_drawing(unit as usize, index as usize, range(anchor))))
            }
            fn set_shape_text(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32, text: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_shape_text(unit as usize, index as usize, &text)))
            }
            fn delete_drawing(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, index: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.delete_drawing(unit as usize, index as usize)))
            }
            fn add_sparklines(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, data: (u32, u32, u32, u32), location: (u32, u32, u32, u32), kind: $crate::adapter::grid::g::SparklineKind, mark: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::{range, Conv};
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.add_sparklines(unit as usize, range(data), range(location), kind.conv(), mark)))
            }
            fn clear_sparklines(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32)) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.clear_sparklines(unit as usize, range(r))))
            }
            fn goal_seek(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, set: (u32, u32), target: f64, by: (u32, u32)) -> ::std::result::Result<::std::option::Option<f64>, ::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.goal_seek(unit as usize, set, target, by)).map_err(|e| e.0)
            }
            fn create_data_table(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), row_input: ::std::option::Option<(u32, u32)>, col_input: ::std::option::Option<(u32, u32)>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.create_data_table(unit as usize, range(r), row_input, col_input)))
            }
            fn scenarios(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::vec::Vec<$crate::adapter::grid::g::Scenario> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.scenarios(unit as usize)).conv()
            }
            fn add_scenario(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, name: ::std::string::String, cells: ::std::vec::Vec<(u32, u32)>, comment: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.add_scenario(unit as usize, &name, &cells, &comment)))
            }
            fn show_scenario(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, name: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.show_scenario(unit as usize, &name)))
            }
            fn delete_scenario(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, name: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.delete_scenario(unit as usize, &name)))
            }
            fn calc_options(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> $crate::adapter::grid::g::CalcSettings {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.calc_options()).conv()
            }
            fn set_calc_options(d: $crate::adapter::grid::g::DocumentBorrow<'_>, options: $crate::adapter::grid::g::CalcSettings) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_calc_options(options.conv())))
            }
            fn circular_references(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::vec::Vec<(u32, u32, u32)> {
                $crate::adapter::grid::with(d, |x| x.circular_references())
                    .into_iter()
                    .map(|(u, r, c)| (u as u32, r, c))
                    .collect()
            }
            fn sheet_view(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> $crate::adapter::grid::g::ViewSettings {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.sheet_view(unit as usize)).conv()
            }
            fn set_sheet_view(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, view: $crate::adapter::grid::g::ViewSettings) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_sheet_view(unit as usize, view.conv())))
            }
            fn cell_styles(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::vec::Vec<::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.cell_styles())
            }
            fn apply_cell_style(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, r: (u32, u32, u32, u32), name: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::range;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.apply_cell_style(unit as usize, range(r), &name)))
            }
            fn new_cell_style(d: $crate::adapter::grid::g::DocumentBorrow<'_>, name: ::std::string::String, unit: u32, row: u32, col: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.new_cell_style(&name, unit as usize, row, col)))
            }
            fn theme_name(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::option::Option<::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.theme_name())
            }
            fn theme_names(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::vec::Vec<::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.theme_names())
            }
            fn set_theme(d: $crate::adapter::grid::g::DocumentBorrow<'_>, name: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_theme(&name)))
            }
            fn tab_color(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::option::Option<$crate::adapter::grid::g::Rgb> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.tab_color(unit as usize)).conv()
            }
            fn set_tab_color(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, color: ::std::option::Option<$crate::adapter::grid::g::Rgb>) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.set_tab_color(unit as usize, color.conv())))
            }
            fn threads(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32) -> ::std::vec::Vec<$crate::adapter::grid::g::CommentThread> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.threads(unit as usize)).conv()
            }
            fn add_thread_comment(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, author: ::std::string::String, text: ::std::string::String, time: ::std::string::String) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.add_thread_comment(unit as usize, row, col, &author, &text, &time)))
            }
            fn resolve_thread(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, done: bool) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.resolve_thread(unit as usize, row, col, done)))
            }
            fn delete_thread_comment(d: $crate::adapter::grid::g::DocumentBorrow<'_>, unit: u32, row: u32, col: u32, index: u32) -> ::std::result::Result<::std::vec::Vec<u32>, ::std::string::String> {
                $crate::adapter::grid::changed($crate::adapter::grid::with(d, |x| x.delete_thread_comment(unit as usize, row, col, index as usize)))
            }
            fn has_history(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> bool {
                $crate::adapter::grid::with(d, |x| x.has_history())
            }
            fn undo(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::result::Result<bool, ::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.undo()).map_err(|e| e.0)
            }
            fn redo(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::result::Result<bool, ::std::string::String> {
                $crate::adapter::grid::with(d, |x| x.redo()).map_err(|e| e.0)
            }
            fn macros(d: $crate::adapter::grid::g::DocumentBorrow<'_>) -> ::std::vec::Vec<$crate::adapter::grid::g::MacroEntry> {
                use $crate::adapter::grid::Conv;
                $crate::adapter::grid::with(d, |x| x.macros()).conv()
            }
            fn run_macro(d: $crate::adapter::grid::g::DocumentBorrow<'_>, name: ::std::string::String, answers: ::std::vec::Vec<$crate::adapter::grid::g::MacroAnswer>) -> ::std::result::Result<$crate::adapter::grid::g::MacroOutcome, ::std::string::String> {
                use $crate::adapter::grid::Conv;
                let mut ui = $crate::adapter::grid::Answers(answers.into());
                $crate::adapter::grid::with(d, |x| x.run_macro(&name, &mut ui)).map(Conv::conv).map_err(|e| e.0)
            }
        }
    };
}
