//! A viewer's document saved under another name: its own kind by the
//! plugin's `save`, another by its `save_as` (a workbook as `.ods`), what
//! that loses told, a kind it does not write refused.

use std::sync::Arc;

use kalem_core::DocumentState;
use kalem_viewer::{
    Bitmap, Detection, FileHandle, GridLayout, RenderRequest, Rendered, Result, SaveOutput,
    Structure, Unit, UnitKind, Viewer, ViewerDocument, ViewerError,
};
use org_model::Settings;

/// A workbook viewer (it opens `.xlsx`) that writes `.ods` without the
/// password, as the xlsx plugin does.
struct Books;

/// A picture viewer that saves its own kind only, as the image viewer
/// does.
struct Pictures;

struct Doc(&'static str);

impl Viewer for Books {
    fn id(&self) -> &str {
        "books"
    }
    fn name(&self) -> &str {
        "Books"
    }
    fn extensions(&self) -> &[&str] {
        &["xlsx", "ods"]
    }
    fn detect(&self, _: &str, _: &[u8]) -> Detection {
        Detection::No
    }
    fn open(&self, _: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        Ok(Box::new(Doc("book")))
    }
}

impl Viewer for Pictures {
    fn id(&self) -> &str {
        "pictures"
    }
    fn name(&self) -> &str {
        "Pictures"
    }
    fn extensions(&self) -> &[&str] {
        &["pic"]
    }
    fn detect(&self, _: &str, _: &[u8]) -> Detection {
        Detection::No
    }
    fn open(&self, _: FileHandle) -> Result<Box<dyn ViewerDocument>> {
        Ok(Box::new(Doc("picture")))
    }
}

impl ViewerDocument for Doc {
    fn structure(&self) -> Structure {
        Structure {
            units: vec![Unit {
                kind: UnitKind::Image,
                label: "1".into(),
                duration_ms: None,
            }],
            outline: Vec::new(),
        }
    }
    fn render(&mut self, _: usize, _: RenderRequest) -> Result<Rendered> {
        Ok(Rendered::Bitmap(Bitmap::new(1, 1, vec![0; 4])))
    }
    fn text(&self, _: usize) -> String {
        String::new()
    }
    fn grid(&mut self, _: usize) -> Option<GridLayout> {
        (self.0 == "book").then(|| GridLayout {
            rows: 1,
            cols: 1,
            editable: true,
            ..GridLayout::default()
        })
    }
    fn save(&mut self) -> Result<SaveOutput> {
        Ok(SaveOutput {
            bytes: format!("{} saved", self.0).into_bytes(),
            losses: Vec::new(),
        })
    }
    fn save_as(&mut self, extension: &str) -> Result<SaveOutput> {
        if self.0 != "book" {
            return Err(ViewerError(format!(
                "Kalem does not write .{extension} files"
            )));
        }
        let losses = if extension == "ods" {
            vec!["The password to open: the .ods file is saved without encryption".to_string()]
        } else {
            Vec::new()
        };
        Ok(SaveOutput {
            bytes: format!("book as {extension}").into_bytes(),
            losses,
        })
    }
}

fn open(name: &str, viewer: Arc<dyn Viewer>) -> (DocumentState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "kalem-core-viewer-save-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, "x").unwrap();
    let d = DocumentState::viewed(&path, viewer, Arc::new(Settings::default())).unwrap();
    (d, dir)
}

#[test]
fn a_workbook_saved_as_ods_tells_what_it_loses() {
    let (mut d, dir) = open("a.xlsx", Arc::new(Books));
    let losses = d.save_as(&dir.join("b.ods"), Default::default()).unwrap();
    assert_eq!(
        losses,
        ["The password to open: the .ods file is saved without encryption"]
    );
    assert_eq!(std::fs::read(dir.join("b.ods")).unwrap(), b"book as ods");
    let losses = d.save_as(&dir.join("c.xlsm"), Default::default()).unwrap();
    assert!(losses.is_empty());
    assert_eq!(std::fs::read(dir.join("c.xlsm")).unwrap(), b"book as xlsm");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_picture_saved_as_another_kind_is_refused() {
    let (mut d, dir) = open("a.pic", Arc::new(Pictures));
    let was = d.meta.path.clone();
    let e = d
        .save_as(&dir.join("b.png"), Default::default())
        .unwrap_err();
    assert_eq!(e.to_string(), "Kalem does not write .png files");
    assert!(!dir.join("b.png").exists());
    assert_eq!(d.meta.path, was);
    // Its own kind: the plugin's save.
    assert!(
        d.save_as(&dir.join("b.PIC"), Default::default())
            .unwrap()
            .is_empty()
    );
    assert_eq!(std::fs::read(dir.join("b.PIC")).unwrap(), b"picture saved");
    std::fs::remove_dir_all(&dir).unwrap();
}
