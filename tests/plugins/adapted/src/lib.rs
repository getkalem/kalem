//! A viewer of the Rust contract, as bundled viewers are written, exported
//! as a component by `kalem_plugin::export_viewer_of!`: a file of lines
//! after the magic `LINES\n`, a page a line, read through the handle.

use kalem_viewer::{
    Bitmap, Detection, FileHandle, InfoField, RenderRequest, Rendered, Result, Structure, Unit,
    UnitKind, Viewer, ViewerDocument, ViewerError,
};

pub struct Lines;

struct Doc {
    lines: Vec<String>,
    name: String,
    size: u64,
}

impl Viewer for Lines {
    fn id(&self) -> &str {
        "lines"
    }

    fn name(&self) -> &str {
        "Lines"
    }

    fn extensions(&self) -> &[&str] {
        &["lines"]
    }

    fn detect(&self, name: &str, head: &[u8]) -> Detection {
        if head.starts_with(b"LINES\n") {
            Detection::Magic
        } else if name.ends_with(".lines") {
            Detection::Extension
        } else {
            Detection::No
        }
    }

    fn open(&self, file: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        // Read a byte at a time, the handle's slowest use.
        let mut bytes = Vec::new();
        for i in 0..file.len()? {
            bytes.extend(file.read_at(i, 1)?);
        }
        let text = String::from_utf8(bytes).map_err(|e| ViewerError(e.to_string()))?;
        let Some(rest) = text.strip_prefix("LINES\n") else {
            return Err(ViewerError("not a lines file".into()));
        };
        Ok(Box::new(Doc {
            lines: rest.lines().map(str::to_string).collect(),
            name: file.name().to_string(),
            size: file.len()?,
        }))
    }
}

impl ViewerDocument for Doc {
    fn structure(&self) -> Structure {
        Structure {
            units: (0..self.lines.len())
                .map(|i| Unit {
                    kind: UnitKind::Page,
                    label: format!("{}", i + 1),
                    duration_ms: None,
                })
                .collect(),
            outline: Vec::new(),
        }
    }

    fn render(&mut self, unit: usize, request: RenderRequest) -> Result<Rendered> {
        let w = (self.lines[unit].len() as f32 * request.scale).max(1.0) as u32;
        Ok(Rendered::Bitmap(Bitmap::new(
            w,
            1,
            vec![unit as u8; w as usize * 4],
        )))
    }

    fn size(&self, unit: usize) -> Option<(f32, f32)> {
        Some((self.lines.get(unit)?.len() as f32, 1.0))
    }

    fn text(&self, unit: usize) -> String {
        self.lines.get(unit).cloned().unwrap_or_default()
    }

    fn info(&self) -> Vec<InfoField> {
        vec![
            InfoField::new("Name", self.name.clone()),
            InfoField::new("Size", self.size.to_string()),
        ]
    }
}

#[cfg(target_arch = "wasm32")]
kalem_plugin::export_viewer_of!(Lines);
