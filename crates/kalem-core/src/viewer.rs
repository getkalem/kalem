//! The host side of the viewer contract (design §11.13, D54): the viewers
//! installed, a document opened by one ([`ViewerState`], the
//! [`DocumentMode::Viewer`](crate::DocumentMode::Viewer) kind, with no
//! text), its view (zoom, pan, turn, the unit shown) and the commands both
//! frontends run on it.
//!
//! Frontends draw [`ViewerState::bitmap`] where [`ViewerState::placement`]
//! says, after telling the state the size of its area
//! ([`ViewerState::set_area`]); the commands work in that area's pixels.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use kalem_viewer::{
    Bitmap, Edit, FileHandle, InfoField, RenderRequest, Rendered, SaveOutput, Structure, UnitKind,
    Viewer, ViewerDocument,
};

use crate::command::{
    Command, CommandHandler, CommandResult, CommandSource, EditorContext, Request,
};
use crate::keys::KeySequence;
use crate::when::WhenClause;

static VIEWERS: RwLock<Vec<Arc<dyn Viewer>>> = RwLock::new(Vec::new());

/// The last generation given out: unique over every document, so a
/// frontend's texture of one file is never taken for another's.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn next_generation() -> u64 {
    GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1
}

/// Installs a viewer (a bundled plugin; the plugin loader of T3.1.12 for
/// components). A viewer with the same identifier is replaced.
pub fn register(viewer: Arc<dyn Viewer>) {
    if let Ok(mut all) = VIEWERS.write() {
        all.retain(|v| v.id() != viewer.id());
        all.push(viewer);
    }
}

/// The viewers installed.
pub fn viewers() -> Vec<Arc<dyn Viewer>> {
    VIEWERS.read().map(|v| v.clone()).unwrap_or_default()
}

/// The viewer that opens the file named `name` starting with `head`: the
/// surest, the first installed among equals.
pub fn find(name: &str, head: &[u8]) -> Option<Arc<dyn Viewer>> {
    let mut best: Option<(kalem_viewer::Detection, Arc<dyn Viewer>)> = None;
    for v in viewers() {
        let d = v.detect(name, head);
        if d > kalem_viewer::Detection::No && best.as_ref().is_none_or(|(b, _)| d > *b) {
            best = Some((d, v));
        }
    }
    best.map(|(_, v)| v)
}

/// The viewer for the file at `path` when it is not text (design §2.6):
/// text files open in a document mode even when a viewer could show them
/// (an SVG drawing is XML).
pub fn for_file(path: &Path) -> Option<Arc<dyn Viewer>> {
    let head = FileHandle::new(path).read_at(0, 8192).ok()?;
    if !crate::mode::looks_binary(&head) {
        return None;
    }
    find(&name_of(path), &head)
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// How large the unit is shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Zoom {
    /// Whole in the area, never larger than its own size.
    Fit,
    /// This many screen pixels per pixel of the unit.
    Scale(f32),
}

/// Where a unit is drawn in its area, in the area's pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// The left edge of the whole bitmap (it may be outside the area).
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The bitmap's width as drawn.
    pub width: f32,
    /// Its height as drawn.
    pub height: f32,
    /// Screen pixels per bitmap pixel.
    pub scale: f32,
}

impl Placement {
    /// The part of the bitmap inside an area `w` × `h`: x, y, width and
    /// height in bitmap pixels.
    pub fn visible(&self, w: f32, h: f32) -> (f32, f32, f32, f32) {
        let x0 = (-self.x / self.scale).max(0.0);
        let y0 = (-self.y / self.scale).max(0.0);
        let x1 = ((w - self.x) / self.scale).min(self.width / self.scale);
        let y1 = ((h - self.y) / self.scale).min(self.height / self.scale);
        (x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
    }
}

/// The zoom steps, as a factor.
const ZOOM_STEP: f32 = 1.25;
/// The smallest and largest scales.
const MIN_SCALE: f32 = 0.01;
const MAX_SCALE: f32 = 64.0;

/// A file opened by a viewer, and how it is shown.
pub struct ViewerState {
    /// The viewer.
    pub viewer: Arc<dyn Viewer>,
    doc: Box<dyn ViewerDocument>,
    structure: Structure,
    /// The unit shown.
    pub unit: usize,
    /// The zoom.
    pub zoom: Zoom,
    /// The point of the bitmap at the area's center, in bitmap pixels;
    /// `None` for the middle.
    pub center: Option<(f32, f32)>,
    /// Quarter turns clockwise of the view (not of the file).
    pub rotation: u8,
    /// The information panel is shown.
    pub info: bool,
    /// The frames play.
    pub playing: bool,
    area: (f32, f32),
    undo: Vec<(String, String)>,
    redo: Vec<(String, String)>,
    generation: u64,
    cache: Option<(usize, u8, u64, Bitmap)>,
}

impl std::fmt::Debug for ViewerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ViewerState")
            .field("viewer", &self.viewer.id())
            .field("unit", &self.unit)
            .field("zoom", &self.zoom)
            .field("rotation", &self.rotation)
            .finish_non_exhaustive()
    }
}

impl ViewerState {
    /// Opens `path` with `viewer`.
    pub fn open(viewer: Arc<dyn Viewer>, path: &Path) -> Result<ViewerState, String> {
        let doc = viewer
            .open(FileHandle::new(path))
            .map_err(|e| e.to_string())?;
        let structure = doc.structure();
        if structure.units.is_empty() {
            return Err("The file has nothing to show".into());
        }
        let playing = structure.animated();
        Ok(ViewerState {
            viewer,
            doc,
            structure,
            unit: 0,
            zoom: Zoom::Fit,
            center: None,
            rotation: 0,
            info: false,
            playing,
            area: (0.0, 0.0),
            undo: Vec::new(),
            redo: Vec::new(),
            generation: next_generation(),
            cache: None,
        })
    }

    /// The document's units and outline.
    pub fn structure(&self) -> &Structure {
        &self.structure
    }

    /// Changes whenever the pixels shown change: frontends key their
    /// textures by it.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn changed(&mut self) {
        self.generation = next_generation();
    }

    /// The unit shown, rendered and turned by the view's rotation.
    pub fn bitmap(&mut self) -> Result<Bitmap, String> {
        if let Some((u, r, g, b)) = &self.cache
            && (*u, *r, *g) == (self.unit, self.rotation, self.generation)
        {
            return Ok(b.clone());
        }
        let Rendered::Bitmap(b) = self
            .doc
            .render(self.unit, RenderRequest::default())
            .map_err(|e| e.to_string())?;
        let b = b.rotated(self.rotation);
        self.cache = Some((self.unit, self.rotation, self.generation, b.clone()));
        Ok(b)
    }

    /// The size of the bitmap shown, without rendering it again when it
    /// is cached.
    fn size(&mut self) -> (f32, f32) {
        self.bitmap()
            .map(|b| (b.width as f32, b.height as f32))
            .unwrap_or((1.0, 1.0))
    }

    /// Sets the size of the area the unit is drawn in, in the frontend's
    /// pixels (a terminal's cells times their size).
    pub fn set_area(&mut self, width: f32, height: f32) {
        self.area = (width.max(1.0), height.max(1.0));
    }

    /// The area's size.
    pub fn area(&self) -> (f32, f32) {
        self.area
    }

    /// The scale [`Zoom::Fit`] means in the current area.
    pub fn fit_scale(&mut self) -> f32 {
        let (w, h) = self.size();
        let (aw, ah) = self.area;
        (aw / w).min(ah / h).min(1.0)
    }

    /// The scale shown.
    pub fn scale(&mut self) -> f32 {
        match self.zoom {
            Zoom::Fit => self.fit_scale(),
            Zoom::Scale(s) => s,
        }
    }

    /// Where the bitmap is drawn in the area: centered where it is
    /// smaller than the area, else at [`ViewerState::center`], kept from
    /// leaving an edge empty.
    pub fn placement(&mut self) -> Placement {
        let (w, h) = self.size();
        let s = self.scale();
        let (aw, ah) = self.area;
        let (dw, dh) = (w * s, h * s);
        let (cx, cy) = self.center.unwrap_or((w / 2.0, h / 2.0));
        let along = |d: f32, a: f32, c: f32| {
            if d <= a {
                (a - d) / 2.0
            } else {
                (a / 2.0 - c * s).clamp(a - d, 0.0)
            }
        };
        Placement {
            x: along(dw, aw, cx),
            y: along(dh, ah, cy),
            width: dw,
            height: dh,
            scale: s,
        }
    }

    /// The bitmap's point at the area's center, as placed.
    fn shown_center(&mut self) -> (f32, f32) {
        let p = self.placement();
        let (aw, ah) = self.area;
        ((aw / 2.0 - p.x) / p.scale, (ah / 2.0 - p.y) / p.scale)
    }

    /// Zooms by `factor`, keeping the center where it is.
    pub fn zoom_by(&mut self, factor: f32) {
        let c = self.shown_center();
        let s = (self.scale() * factor).clamp(MIN_SCALE, MAX_SCALE);
        self.zoom = Zoom::Scale(s);
        self.center = Some(c);
    }

    /// Zooms by `factor` keeping the bitmap's point under (`x`, `y`) of
    /// the area there (the mouse wheel).
    pub fn zoom_at(&mut self, factor: f32, x: f32, y: f32) {
        let p = self.placement();
        let (bx, by) = ((x - p.x) / p.scale, (y - p.y) / p.scale);
        let s = (p.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let (aw, ah) = self.area;
        self.zoom = Zoom::Scale(s);
        self.center = Some((bx + (aw / 2.0 - x) / s, by + (ah / 2.0 - y) / s));
        // Kept inside, as placed.
        let c = self.shown_center();
        self.center = Some(c);
    }

    /// Shows the unit at its own size.
    pub fn actual_size(&mut self) {
        let c = self.shown_center();
        self.zoom = Zoom::Scale(1.0);
        self.center = Some(c);
    }

    /// Fits the unit in the area.
    pub fn fit(&mut self) {
        self.zoom = Zoom::Fit;
        self.center = None;
    }

    /// Moves the view by (`dx`, `dy`) area pixels: the picture moves the
    /// other way.
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let s = self.scale();
        let (cx, cy) = self.shown_center();
        self.center = Some((cx + dx / s, cy + dy / s));
        let c = self.shown_center();
        self.center = Some(c);
    }

    /// Turns the view by `quarters` clockwise.
    pub fn rotate(&mut self, quarters: i8) {
        self.rotation = (self.rotation as i8 + quarters).rem_euclid(4) as u8;
        self.center = None;
        if let Zoom::Scale(_) = self.zoom {
            self.zoom = Zoom::Fit;
        }
    }

    /// Whether the units are paged through (pages, sheets, slides), not
    /// played (frames) or single (a picture).
    pub fn paged(&self) -> bool {
        self.structure.units.len() > 1
            && !self
                .structure
                .units
                .iter()
                .any(|u| matches!(u.kind, UnitKind::Frame | UnitKind::Image))
    }

    /// Shows unit `unit`; false when there is none.
    pub fn go_to(&mut self, unit: usize) -> bool {
        if unit >= self.structure.units.len() || unit == self.unit {
            return false;
        }
        self.unit = unit;
        if self.paged() {
            self.center = None;
        }
        true
    }

    /// The next frame of an animation, and how long the frame shown now
    /// stays; `None` when nothing plays.
    pub fn frame_delay(&self) -> Option<u32> {
        if !self.playing || !self.structure.animated() {
            return None;
        }
        Some(
            self.structure.units[self.unit]
                .duration_ms
                .unwrap_or(100)
                .max(20),
        )
    }

    /// Shows the next frame (after [`ViewerState::frame_delay`]).
    pub fn advance_frame(&mut self) {
        let n = self.structure.units.len();
        if n > 1 {
            self.unit = (self.unit + 1) % n;
        }
    }

    /// The information panel's fields.
    pub fn info_fields(&self) -> Vec<InfoField> {
        self.doc.info()
    }

    /// The unit's text.
    pub fn text(&self) -> String {
        self.doc.text(self.unit)
    }

    /// The edits the format allows on the unit shown.
    pub fn edits(&self) -> Vec<Edit> {
        self.doc.edits(self.unit)
    }

    /// Applies edit `id`, recording its inverse for undo.
    pub fn apply(&mut self, id: &str) -> Result<(), String> {
        let edit = self
            .edits()
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| format!("No edit {id}"))?;
        self.doc.apply(id).map_err(|e| e.to_string())?;
        if let Some(inv) = edit.inverse {
            self.undo.push((inv, id.to_string()));
        } else {
            // An edit without an inverse cannot be undone past.
            self.undo.clear();
        }
        self.redo.clear();
        self.structure = self.doc.structure();
        self.changed();
        Ok(())
    }

    /// Undoes the last edit; false when there is none.
    pub fn undo(&mut self) -> Result<bool, String> {
        let Some((inverse, again)) = self.undo.pop() else {
            return Ok(false);
        };
        self.doc.apply(&inverse).map_err(|e| e.to_string())?;
        self.redo.push((inverse, again));
        self.changed();
        Ok(true)
    }

    /// Redoes the last edit undone; false when there is none.
    pub fn redo(&mut self) -> Result<bool, String> {
        let Some((inverse, again)) = self.redo.pop() else {
            return Ok(false);
        };
        self.doc.apply(&again).map_err(|e| e.to_string())?;
        self.undo.push((inverse, again));
        self.changed();
        Ok(true)
    }

    /// Whether there are edits not saved.
    pub fn modified(&self) -> bool {
        self.doc.modified()
    }

    /// The file with the edits.
    pub fn save(&mut self) -> Result<SaveOutput, String> {
        self.doc.save().map_err(|e| e.to_string())
    }

    /// What the status bar says: the size, the zoom, the unit.
    pub fn status(&mut self) -> String {
        let mut parts = Vec::new();
        if let Ok(b) = self.bitmap() {
            parts.push(format!("{} × {}", b.width, b.height));
        }
        parts.push(format!("{:.0}%", self.scale() * 100.0));
        let n = self.structure.units.len();
        if n > 1 {
            parts.push(format!("{}/{n}", self.unit + 1));
        }
        parts.join(" · ")
    }
}

/// The files of `path`'s folder that `viewer` opens, by name, `path`
/// among them.
pub fn siblings(path: &Path, viewer: &dyn Viewer) -> Vec<PathBuf> {
    let Some(dir) = path.parent() else {
        return vec![path.to_path_buf()];
    };
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_file()
                        && p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                            viewer
                                .extensions()
                                .contains(&e.to_ascii_lowercase().as_str())
                        })
                })
                .collect()
        })
        .unwrap_or_default();
    if !files.iter().any(|p| p == path) {
        files.push(path.to_path_buf());
    }
    files.sort_by_key(|p| name_of(p).to_lowercase());
    files
}

/// The PNG of `bitmap`, for the clipboard and `kalem view --to png`.
pub fn png(bitmap: &Bitmap) -> Result<Vec<u8>, String> {
    let img = image::RgbaImage::from_raw(bitmap.width, bitmap.height, bitmap.rgba.to_vec())
        .ok_or("a broken bitmap")?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

const IN_VIEWER: &str = "editorMode == viewer";

fn cmd(
    id: &str,
    title: &str,
    keys: &[&str],
    when: &str,
    handler: fn(&mut EditorContext<'_>, &serde_json::Value) -> CommandResult,
) -> Command {
    Command {
        id: id.into(),
        title: title.into(),
        category: "Viewer".into(),
        default_keys: keys
            .iter()
            .map(|k| KeySequence::parse(k).expect("valid default key"))
            .collect(),
        when: Some(WhenClause::parse(when).expect("valid when-clause")),
        handler: CommandHandler::Native(handler),
        args_schema: None,
        source: CommandSource::Builtin,
        scope: Some(crate::command::Scope::only(&["viewer"])),
    }
}

/// Runs `f` on the active document's viewer.
fn with(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(&mut ViewerState) -> Result<(), String>,
) -> CommandResult {
    let Some(v) = ctx
        .document
        .as_deref_mut()
        .and_then(|d| d.viewer.as_deref_mut())
    else {
        return Ok(());
    };
    if let Err(e) = f(v) {
        ctx.messages.push(e);
    }
    Ok(())
}

/// A tenth of the area, the step of the arrow keys.
fn step(v: &ViewerState, horizontal: bool) -> f32 {
    let (w, h) = v.area();
    (if horizontal { w } else { h } / 10.0).max(1.0)
}

fn pan(ctx: &mut EditorContext<'_>, dx: f32, dy: f32) -> CommandResult {
    with(ctx, |v| {
        let (sx, sy) = (step(v, true), step(v, false));
        v.pan(dx * sx, dy * sy);
        Ok(())
    })
}

/// The next or previous file of the folder, or unit of a paged document.
fn turn(ctx: &mut EditorContext<'_>, delta: i64, files: bool) -> CommandResult {
    let Some(doc) = ctx.document.as_deref_mut() else {
        return Ok(());
    };
    if let Some(v) = doc.viewer.as_deref_mut()
        && !files
        && v.paged()
    {
        let n = v.structure().units.len() as i64;
        let u = v.unit as i64 + delta;
        if (0..n).contains(&u) {
            v.go_to(u as usize);
            return Ok(());
        }
    }
    if let Err(e) = doc.viewer_step_file(delta) {
        ctx.messages.push(e);
    }
    Ok(())
}

/// Copies the picture shown (Copy in a viewer).
pub(crate) fn copy(ctx: &mut EditorContext<'_>) -> CommandResult {
    let Some(doc) = ctx.document.as_deref_mut() else {
        return Ok(());
    };
    let path = doc.meta.path.clone();
    let Some(v) = doc.viewer.as_deref_mut() else {
        return Ok(());
    };
    let png = v
        .bitmap()
        .and_then(|b| png(&b))
        .map_err(crate::command::CommandError::new)?;
    ctx.requests.push(Request::CopyImage { png, path });
    Ok(())
}

/// The viewer's commands.
pub(crate) fn commands() -> Vec<Command> {
    let mut all = vec![
        cmd(
            "viewer.zoomIn",
            "Zoom In",
            &["=", "+"],
            IN_VIEWER,
            |ctx, _| {
                with(ctx, |v| {
                    v.zoom_by(ZOOM_STEP);
                    Ok(())
                })
            },
        ),
        cmd("viewer.zoomOut", "Zoom Out", &["-"], IN_VIEWER, |ctx, _| {
            with(ctx, |v| {
                v.zoom_by(1.0 / ZOOM_STEP);
                Ok(())
            })
        }),
        cmd(
            "viewer.fit",
            "Fit to Window",
            &["0", "f"],
            IN_VIEWER,
            |ctx, _| {
                with(ctx, |v| {
                    v.fit();
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.actualSize",
            "Actual Size",
            &["1"],
            IN_VIEWER,
            |ctx, _| {
                with(ctx, |v| {
                    v.actual_size();
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.panLeft",
            "Pan Left",
            &["left", "h"],
            IN_VIEWER,
            |ctx, _| pan(ctx, -1.0, 0.0),
        ),
        cmd(
            "viewer.panRight",
            "Pan Right",
            &["right", "l"],
            IN_VIEWER,
            |ctx, _| pan(ctx, 1.0, 0.0),
        ),
        cmd(
            "viewer.panUp",
            "Pan Up",
            &["up", "k"],
            IN_VIEWER,
            |ctx, _| pan(ctx, 0.0, -1.0),
        ),
        cmd(
            "viewer.panDown",
            "Pan Down",
            &["down", "j"],
            IN_VIEWER,
            |ctx, _| pan(ctx, 0.0, 1.0),
        ),
        cmd(
            "viewer.rotateRight",
            "Rotate View Right",
            &["r"],
            IN_VIEWER,
            |ctx, _| {
                with(ctx, |v| {
                    v.rotate(1);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.rotateLeft",
            "Rotate View Left",
            &["shift+r"],
            IN_VIEWER,
            |ctx, _| {
                with(ctx, |v| {
                    v.rotate(-1);
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.next",
            "Next",
            &["n", "pagedown"],
            IN_VIEWER,
            |ctx, _| turn(ctx, 1, false),
        ),
        cmd(
            "viewer.previous",
            "Previous",
            &["p", "pageup"],
            IN_VIEWER,
            |ctx, _| turn(ctx, -1, false),
        ),
        cmd(
            "viewer.nextFile",
            "Next File in Folder",
            &["shift+n"],
            IN_VIEWER,
            |ctx, _| turn(ctx, 1, true),
        ),
        cmd(
            "viewer.previousFile",
            "Previous File in Folder",
            &["shift+p"],
            IN_VIEWER,
            |ctx, _| turn(ctx, -1, true),
        ),
        cmd("viewer.first", "First", &["home"], IN_VIEWER, |ctx, _| {
            with(ctx, |v| {
                v.go_to(0);
                Ok(())
            })
        }),
        cmd(
            "viewer.last",
            "Last",
            &["end", "shift+g"],
            IN_VIEWER,
            |ctx, _| {
                with(ctx, |v| {
                    let n = v.structure().units.len();
                    v.go_to(n.saturating_sub(1));
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.togglePlay",
            "Play or Pause",
            &["."],
            "editorMode == viewer && viewerAnimated",
            |ctx, _| {
                with(ctx, |v| {
                    v.playing = !v.playing;
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.info",
            "Show Information",
            &["i"],
            IN_VIEWER,
            |ctx, _| {
                with(ctx, |v| {
                    v.info = !v.info;
                    Ok(())
                })
            },
        ),
        cmd(
            "viewer.copy",
            "Copy Picture",
            &["y"],
            IN_VIEWER,
            |ctx, _| copy(ctx),
        ),
        cmd(
            "viewer.insertLink",
            "Insert Link at Point",
            &["shift+l"],
            IN_VIEWER,
            |ctx, _| {
                if let Some(path) = ctx.document.as_deref().and_then(|d| d.meta.path.clone()) {
                    ctx.requests.push(Request::InsertLink(path));
                }
                Ok(())
            },
        ),
        cmd(
            "viewer.edit",
            "Edit the File",
            &[],
            "editorMode == viewer && viewerEditable",
            |ctx, args| {
                let Some(id) = args
                    .get("edit")
                    .and_then(|e| e.as_str())
                    .map(str::to_string)
                else {
                    return Err(crate::command::CommandError {
                        message: "Which edit?".into(),
                    });
                };
                with(ctx, |v| v.apply(&id))
            },
        ),
    ];
    for c in &mut all {
        if c.id == "viewer.edit" {
            c.args_schema = Some(serde_json::json!({
                "type": "object",
                "properties": { "edit": { "type": "string" } },
                "required": ["edit"],
            }));
        }
    }
    all
}

#[cfg(test)]
mod tests {
    use super::*;
    use kalem_viewer::{Detection, Result as VResult, Unit};

    /// Numbered pages 100 × 50, each one color.
    #[derive(Debug)]
    struct Pages(usize);

    struct PagesDoc(usize, u8);

    impl Viewer for Pages {
        fn id(&self) -> &str {
            "pages"
        }
        fn name(&self) -> &str {
            "Pages"
        }
        fn extensions(&self) -> &[&str] {
            &["pages"]
        }
        fn detect(&self, name: &str, head: &[u8]) -> Detection {
            if head.starts_with(b"PAGES\0") {
                Detection::Magic
            } else if name.ends_with(".pages") {
                Detection::Extension
            } else {
                Detection::No
            }
        }
        fn open(&self, _file: FileHandle) -> VResult<Box<dyn ViewerDocument>> {
            Ok(Box::new(PagesDoc(self.0, 0)))
        }
    }

    impl ViewerDocument for PagesDoc {
        fn structure(&self) -> Structure {
            Structure {
                units: (0..self.0)
                    .map(|i| Unit {
                        kind: UnitKind::Page,
                        label: format!("{}", i + 1),
                        duration_ms: None,
                    })
                    .collect(),
                outline: Vec::new(),
            }
        }
        fn render(&mut self, unit: usize, _: RenderRequest) -> VResult<Rendered> {
            let px = [unit as u8, self.1, 0, 255];
            Ok(Rendered::Bitmap(Bitmap::new(100, 50, px.repeat(100 * 50))))
        }
        fn text(&self, unit: usize) -> String {
            format!("page {}", unit + 1)
        }
    }

    fn state(n: usize) -> ViewerState {
        let dir = std::env::temp_dir();
        ViewerState::open(Arc::new(Pages(n)), &dir.join("x.pages")).unwrap()
    }

    #[test]
    fn fit_and_zoom() {
        let mut v = state(1);
        v.set_area(400.0, 400.0);
        // Never larger than its own size when fitting.
        assert_eq!(v.scale(), 1.0);
        let p = v.placement();
        assert_eq!((p.x, p.y, p.width, p.height), (150.0, 175.0, 100.0, 50.0));
        v.set_area(50.0, 50.0);
        assert_eq!(v.scale(), 0.5);
        v.zoom_by(4.0);
        assert_eq!(v.scale(), 2.0);
        // 200 × 100 in a 50 × 50 area, centered on the middle.
        let p = v.placement();
        assert_eq!((p.x, p.y), (-75.0, -25.0));
        v.pan(1000.0, 0.0);
        let p = v.placement();
        // Panned to the right edge, no further.
        assert_eq!(p.x, 50.0 - 200.0);
        assert_eq!(p.visible(50.0, 50.0), (75.0, 12.5, 25.0, 25.0));
        v.fit();
        assert_eq!(v.scale(), 0.5);
    }

    #[test]
    fn the_wheel_zooms_at_the_mouse() {
        let mut v = state(1);
        v.set_area(100.0, 50.0);
        // The point under the mouse (the bitmap's (25, 25)) stays there.
        v.zoom_at(2.0, 25.0, 25.0);
        let p = v.placement();
        assert_eq!(p.scale, 2.0);
        assert_eq!(
            ((25.0 - p.x) / p.scale, (25.0 - p.y) / p.scale),
            (25.0, 25.0)
        );
    }

    #[test]
    fn rotation_turns_the_bitmap() {
        let mut v = state(1);
        v.rotate(1);
        let b = v.bitmap().unwrap();
        assert_eq!((b.width, b.height), (50, 100));
        v.rotate(-2);
        assert_eq!(v.rotation, 3);
    }

    #[test]
    fn pages_are_paged() {
        let mut v = state(3);
        assert!(v.paged());
        assert!(v.go_to(2));
        assert!(!v.go_to(3));
        assert_eq!(v.bitmap().unwrap().rgba[0], 2);
        assert_eq!(v.text(), "page 3");
        v.set_area(100.0, 50.0);
        assert_eq!(v.status(), "100 × 50 · 100% · 3/3");
    }

    #[test]
    fn detection_prefers_magic() {
        register(Arc::new(Pages(1)));
        assert!(find("a.pages", b"PAGES\0").is_some());
        assert!(find("a.txt", b"PAGES\0").is_some());
        assert!(find("a.txt", b"hello").is_none());
    }
}
