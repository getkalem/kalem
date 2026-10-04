//! A fake spreadsheet of the Rust contract: one sheet whose cells are
//! typed text, a chart, a conditional format and a validation it keeps,
//! and a macro asking two questions; exported through the adapter with
//! the feature `grid`.

use std::collections::BTreeMap;

use kalem_viewer::{
    AxisFont, Chart, ChartKind, ChartSeries, CondRule, CondStyle, Detection, FileHandle,
    GridCell, GridLayout, MacroEntry, MacroOutcome, MacroQuestion, MacroUi, Paint, RenderRequest,
    Rendered, Result, Structure, Unit, UnitKind, Validation, Viewer, ViewerDocument, ViewerError,
};

struct Sheets;

#[derive(Default)]
struct Book {
    cells: BTreeMap<(u32, u32), String>,
    charts: Vec<Chart>,
    formats: Vec<([u32; 4], CondRule, CondStyle)>,
    validation: Option<Validation>,
    undo: Vec<BTreeMap<(u32, u32), String>>,
}

impl Viewer for Sheets {
    fn id(&self) -> &str {
        "sheet"
    }

    fn name(&self) -> &str {
        "Sheet"
    }

    fn extensions(&self) -> &[&str] {
        &["sheet"]
    }

    fn detect(&self, name: &str, _head: &[u8]) -> Detection {
        if name.ends_with(".sheet") {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        let text = String::from_utf8(file.read_all()?).map_err(|e| ViewerError(e.to_string()))?;
        let mut book = Book::default();
        // A line per row, cells split by tabs.
        for (r, line) in text.lines().enumerate() {
            for (c, cell) in line.split('\t').enumerate() {
                if !cell.is_empty() {
                    book.cells.insert((r as u32, c as u32), cell.to_string());
                }
            }
        }
        Ok(Box::new(book))
    }
}

impl ViewerDocument for Book {
    fn structure(&self) -> Structure {
        Structure {
            units: vec![Unit {
                kind: UnitKind::Sheet,
                label: "Sheet1".into(),
                duration_ms: None,
            }],
            outline: Vec::new(),
        }
    }

    fn render(&mut self, _unit: usize, _request: RenderRequest) -> Result<Rendered> {
        Err(ViewerError("a grid is not rendered".into()))
    }

    fn text(&self, _unit: usize) -> String {
        self.cells.values().cloned().collect::<Vec<_>>().join(" ")
    }

    fn grid(&mut self, _unit: usize) -> Option<GridLayout> {
        let rows = self.cells.keys().map(|k| k.0 + 1).max().unwrap_or(0);
        let cols = self.cells.keys().map(|k| k.1 + 1).max().unwrap_or(0);
        Some(GridLayout {
            rows,
            cols,
            max_rows: 1000,
            max_cols: 26,
            widths: vec![10.0; cols as usize],
            default_width: 8.43,
            heights: vec![(0, 20.0)],
            default_height: 15.0,
            merged: vec![[0, 0, 0, 1]],
            frozen: (1, 0),
            editable: true,
            filter: Some([0, 0, rows.saturating_sub(1), 1]),
            ..GridLayout::default()
        })
    }

    fn grid_cells(
        &mut self,
        _unit: usize,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32, GridCell)> {
        self.cells
            .iter()
            .filter(|((r, c), _)| rows.contains(r) && cols.contains(c))
            .map(|(&(r, c), t)| {
                let numeric = t.parse::<f64>().is_ok();
                (
                    r,
                    c,
                    GridCell {
                        text: t.clone(),
                        numeric,
                        bold: r == 0,
                        fill: (r == 0).then_some([200, 220, 255]),
                        bar: numeric.then_some((500, [0, 128, 0])),
                        borders: [Some([0, 0, 0]), None, Some([1, 2, 3]), None],
                        border_thick: [true, false, false, true],
                        ..GridCell::default()
                    },
                )
            })
            .collect()
    }

    fn cell_input(&mut self, _unit: usize, row: u32, col: u32) -> String {
        self.cells.get(&(row, col)).cloned().unwrap_or_default()
    }

    fn set_cell(&mut self, unit: usize, row: u32, col: u32, input: &str) -> Result<Vec<usize>> {
        if input.starts_with('!') {
            return Err(ViewerError(format!("`{input}` is refused")));
        }
        self.undo.push(self.cells.clone());
        self.cells.insert((row, col), input.to_string());
        Ok(vec![unit])
    }

    fn has_history(&self) -> bool {
        true
    }

    fn undo(&mut self) -> Result<bool> {
        match self.undo.pop() {
            Some(c) => {
                self.cells = c;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn insert_chart(
        &mut self,
        _unit: usize,
        range: [u32; 4],
        kind: ChartKind,
        title: Option<String>,
    ) -> Result<Vec<usize>> {
        let values = (range[0] + 1..=range[2])
            .map(|r| self.cells.get(&(r, range[1] + 1)).and_then(|t| t.parse().ok()))
            .collect();
        self.charts.push(Chart {
            kind,
            title,
            categories: vec!["a".into(), "b".into()],
            series: vec![ChartSeries {
                name: "Values".into(),
                values,
                point_colors: vec![(1, [255, 0, 0])],
                ..ChartSeries::default()
            }],
            anchor: [0, 3, 10, 8],
            background: Paint::Color([250, 250, 250]),
            title_font: AxisFont {
                size: Some(14.0),
                bold: true,
                ..AxisFont::default()
            },
            ..Chart::default()
        });
        Ok(vec![0])
    }

    fn charts(&mut self, _unit: usize) -> Vec<Chart> {
        self.charts.clone()
    }

    fn add_conditional_format(
        &mut self,
        _unit: usize,
        range: [u32; 4],
        rule: CondRule,
        style: CondStyle,
    ) -> Result<Vec<usize>> {
        self.formats.push((range, rule, style));
        Ok(vec![0])
    }

    fn cell_format(&mut self, _unit: usize, row: u32, col: u32) -> Option<String> {
        // What was given, read back: the last conditional format's rule.
        self.formats
            .iter()
            .rev()
            .find(|(r, _, _)| r[0] <= row && row <= r[2] && r[1] <= col && col <= r[3])
            .map(|(_, rule, style)| format!("{rule:?} {style:?}"))
    }

    fn set_validation(
        &mut self,
        _unit: usize,
        _range: [u32; 4],
        validation: Option<Validation>,
    ) -> Result<Vec<usize>> {
        self.validation = validation;
        Ok(vec![0])
    }

    fn validation(&mut self, _unit: usize, _row: u32, _col: u32) -> Option<Validation> {
        self.validation.clone()
    }

    fn macros(&mut self) -> Vec<MacroEntry> {
        vec![MacroEntry {
            name: "Module1.Ask".into(),
            event: false,
        }]
    }

    fn run_macro(&mut self, _name: &str, ui: &mut dyn MacroUi) -> Result<MacroOutcome> {
        let mut out = MacroOutcome::default();
        let Some(button) = ui.message("Go on?", 4, "Ask") else {
            out.question = Some(MacroQuestion::Message {
                prompt: "Go on?".into(),
                buttons: 4,
                title: "Ask".into(),
            });
            return Ok(out);
        };
        let Some(name) = ui.input("Name?", "Ask", "Ada") else {
            out.question = Some(MacroQuestion::Input {
                prompt: "Name?".into(),
                title: "Ask".into(),
                default: "Ada".into(),
            });
            return Ok(out);
        };
        out.output = vec![format!("{button} {}", name.unwrap_or_default())];
        out.changed = true;
        Ok(out)
    }
}

kalem_plugin::export_viewer_of!(Sheets);
