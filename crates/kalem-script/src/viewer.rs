//! The host's side of the `document-viewer` world (design §11.13, D54),
//! generated from `kalem-plugin`'s WIT files, the one definition both
//! sides are built from (D6).
//!
//! A viewer reads only the file the host opened for it: the `file`
//! resource carries the file's path in the host and gives the plugin its
//! name, its size and the bytes it asks for.

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use wasmtime::component::{Resource, ResourceTable};

wasmtime::component::bindgen!({
    path: "../kalem-plugin/wit",
    world: "document-viewer",
    with: {
        "kalem:plugin/files.file": OpenFile,
    },
});

pub use exports::kalem::plugin::viewer as api;

/// A file the host opened for a plugin.
#[derive(Debug)]
pub struct OpenFile {
    path: PathBuf,
}

/// What a viewer plugin's store holds: the files it was given.
#[derive(Debug, Default)]
pub struct Files {
    table: ResourceTable,
}

impl Files {
    /// Hands the file at `path` to the plugin.
    pub fn open(&mut self, path: impl Into<PathBuf>) -> wasmtime::Result<Resource<OpenFile>> {
        Ok(self.table.push(OpenFile { path: path.into() })?)
    }

    fn get(&self, f: &Resource<OpenFile>) -> wasmtime::Result<&OpenFile> {
        Ok(self.table.get(f)?)
    }
}

impl kalem::plugin::files::Host for Files {}

impl kalem::plugin::files::HostFile for Files {
    fn name(&mut self, f: Resource<OpenFile>) -> String {
        self.get(&f)
            .ok()
            .and_then(|o| o.path.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    fn len(&mut self, f: Resource<OpenFile>) -> u64 {
        self.get(&f)
            .ok()
            .and_then(|o| std::fs::metadata(&o.path).ok())
            .map_or(0, |m| m.len())
    }

    fn read(&mut self, f: Resource<OpenFile>, offset: u64, len: u32) -> Vec<u8> {
        let Ok(o) = self.get(&f) else {
            return Vec::new();
        };
        let mut buf = Vec::new();
        if let Ok(mut file) = std::fs::File::open(&o.path)
            && file.seek(SeekFrom::Start(offset)).is_ok()
        {
            let _ = file.take(u64::from(len)).read_to_end(&mut buf);
        }
        buf
    }

    fn drop(&mut self, f: Resource<OpenFile>) -> wasmtime::Result<()> {
        self.table.delete(f)?;
        Ok(())
    }
}

/// A viewer plugin instantiated: its exports, typed, through the host's
/// budget and limits.
pub struct Viewer {
    instance: crate::Instance<Files>,
    api: DocumentViewer,
}

impl std::fmt::Debug for Viewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewer").finish_non_exhaustive()
    }
}

/// A document a viewer plugin opened, owned by its instance.
pub type Document = wasmtime::component::ResourceAny;

impl Viewer {
    /// Instantiates `plugin` granted the `files` interface, and nothing
    /// else.
    pub fn new(
        host: &crate::Host,
        plugin: &crate::Plugin,
        limits: crate::Limits,
    ) -> crate::Result<Viewer> {
        let mut linker = host.linker::<Files>();
        kalem::plugin::files::add_to_linker::<_, wasmtime::component::HasSelf<Files>>(
            &mut linker,
            |d: &mut crate::Data<Files>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        let mut instance = plugin.instantiate(host, &linker, Files::default(), limits)?;
        let api = instance.bindings(|store, i| DocumentViewer::new(store, i))?;
        Ok(Viewer { instance, api })
    }

    /// The plugin's description of itself.
    pub fn describe(&mut self) -> crate::Result<api::Description> {
        let v = self.api.kalem_plugin_viewer();
        self.instance.run(|s| v.call_describe(s))
    }

    /// Whether it opens the file `name` starting with `head`.
    pub fn detect(&mut self, name: &str, head: &[u8]) -> crate::Result<api::Detection> {
        let v = self.api.kalem_plugin_viewer();
        self.instance.run(|s| v.call_detect(s, name, head))
    }

    /// Opens the file at `path` in the plugin, which reads it through the
    /// handle and nothing else.
    pub fn open(&mut self, path: impl Into<PathBuf>) -> crate::Result<Result<Document, String>> {
        let file = self
            .instance
            .data_mut()
            .open(path)
            .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        let v = self.api.kalem_plugin_viewer();
        self.instance.run(|s| v.call_open(s, file))
    }

    /// Calls `f` with the plugin's `document` resource functions.
    pub fn document<R>(
        &mut self,
        f: impl FnOnce(
            api::GuestDocument<'_>,
            &mut wasmtime::Store<crate::Data<Files>>,
        ) -> wasmtime::Result<R>,
    ) -> crate::Result<R> {
        let v = self.api.kalem_plugin_viewer();
        self.instance.run(|s| f(v.document(), s))
    }
}

/// The limits of a viewer plugin unless its manifest says otherwise: a
/// viewer holds a whole file and renders pages of tens of megabytes, so
/// more than the 64 MB and 100 ms of §11.6's synchronous calls.
pub const VIEWER_LIMITS: crate::Limits = crate::Limits {
    memory: 1 << 30,
    time: std::time::Duration::from_secs(10),
};

/// A viewer plugin's component offered to Kalem as the Rust contract
/// (`kalem_viewer::Viewer`), so the views do not tell it from a bundled
/// one. Compiled (or read from the cache) the first time it is needed; a
/// shared instance answers `detect`, and each document opened gets an
/// instance of its own.
pub struct ComponentViewer {
    host: std::sync::Arc<crate::Host>,
    file: PathBuf,
    id: String,
    name: String,
    extensions: Vec<&'static str>,
    limits: crate::Limits,
    plugin: std::sync::OnceLock<Result<crate::Plugin, String>>,
    detector: std::sync::Mutex<Option<Viewer>>,
}

impl std::fmt::Debug for ComponentViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentViewer")
            .field("id", &self.id)
            .field("file", &self.file)
            .finish_non_exhaustive()
    }
}

impl ComponentViewer {
    /// The component at `file`, known as `id` and `name`, opening files
    /// with `extensions` (from the manifest: nothing is compiled yet).
    pub fn new(
        host: std::sync::Arc<crate::Host>,
        file: impl Into<PathBuf>,
        id: impl Into<String>,
        name: impl Into<String>,
        extensions: &[String],
        limits: crate::Limits,
    ) -> ComponentViewer {
        ComponentViewer {
            host,
            file: file.into(),
            id: id.into(),
            name: name.into(),
            // Kept for the life of the program, as viewers are.
            extensions: extensions
                .iter()
                .map(|e| {
                    &*Box::leak(
                        e.trim_start_matches('.')
                            .to_ascii_lowercase()
                            .into_boxed_str(),
                    )
                })
                .collect(),
            limits,
            plugin: std::sync::OnceLock::new(),
            detector: std::sync::Mutex::new(None),
        }
    }

    /// The component, compiled or read from the cache on first use.
    pub fn plugin(&self) -> Result<&crate::Plugin, kalem_viewer::ViewerError> {
        self.plugin
            .get_or_init(|| self.host.load_file(&self.file).map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| kalem_viewer::ViewerError(format!("{}: {e}", self.name)))
    }

    fn instance(&self) -> Result<Viewer, kalem_viewer::ViewerError> {
        Viewer::new(&self.host, self.plugin()?, self.limits).map_err(err)
    }
}

fn err(e: crate::Error) -> kalem_viewer::ViewerError {
    kalem_viewer::ViewerError(e.to_string())
}

impl kalem_viewer::Viewer for ComponentViewer {
    fn id(&self) -> &str {
        &self.id
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn extensions(&self) -> &[&str] {
        &self.extensions
    }

    fn detect(&self, name: &str, head: &[u8]) -> kalem_viewer::Detection {
        let mut slot = self.detector.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            *slot = self.instance().ok();
        }
        let Some(v) = slot.as_mut() else {
            return kalem_viewer::Detection::No;
        };
        match v.detect(name, head) {
            Ok(api::Detection::Magic) => kalem_viewer::Detection::Magic,
            Ok(api::Detection::Extension) => kalem_viewer::Detection::Extension,
            Ok(api::Detection::No) => kalem_viewer::Detection::No,
            // A plugin that failed is used no more for detection.
            Err(_) => {
                *slot = None;
                kalem_viewer::Detection::No
            }
        }
    }

    fn open(
        &self,
        file: kalem_viewer::FileHandle,
    ) -> kalem_viewer::Result<Box<dyn kalem_viewer::ViewerDocument>> {
        let mut v = self.instance()?;
        let doc = v
            .open(file.path())
            .map_err(err)?
            .map_err(kalem_viewer::ViewerError)?;
        Ok(Box::new(ComponentDocument {
            v: std::sync::Mutex::new(v),
            doc,
        }))
    }
}

/// A document a component opened, in its own instance.
struct ComponentDocument {
    v: std::sync::Mutex<Viewer>,
    doc: Document,
}

impl ComponentDocument {
    /// Calls the document's `f`; a failure of the plugin (a trap, the
    /// budget) as the contract's error.
    fn call<R>(
        &self,
        f: impl FnOnce(
            api::GuestDocument<'_>,
            &mut wasmtime::Store<crate::Data<Files>>,
            Document,
        ) -> wasmtime::Result<R>,
    ) -> kalem_viewer::Result<R> {
        let doc = self.doc;
        let mut v = self.v.lock().unwrap_or_else(|e| e.into_inner());
        v.document(|d, s| f(d, s, doc)).map_err(err)
    }
}

fn span(r: std::ops::Range<usize>) -> api::Span {
    api::Span {
        start: r.start as u32,
        end: r.end as u32,
    }
}

fn rect(r: api::Rect) -> [f32; 4] {
    [r.x, r.y, r.width, r.height]
}

impl kalem_viewer::ViewerDocument for ComponentDocument {
    fn structure(&self) -> kalem_viewer::Structure {
        let Ok(s) = self.call(|d, st, doc| d.call_structure(st, doc)) else {
            return kalem_viewer::Structure::default();
        };
        kalem_viewer::Structure {
            units: s
                .units
                .into_iter()
                .map(|u| kalem_viewer::Unit {
                    kind: match u.kind {
                        api::UnitKind::Image => kalem_viewer::UnitKind::Image,
                        api::UnitKind::Frame => kalem_viewer::UnitKind::Frame,
                        api::UnitKind::Page => kalem_viewer::UnitKind::Page,
                        api::UnitKind::Sheet => kalem_viewer::UnitKind::Sheet,
                        api::UnitKind::Slide => kalem_viewer::UnitKind::Slide,
                        api::UnitKind::Table => kalem_viewer::UnitKind::Table,
                    },
                    label: u.label,
                    duration_ms: u.duration_ms,
                })
                .collect(),
            outline: s
                .outline
                .into_iter()
                .map(|e| kalem_viewer::OutlineEntry {
                    title: e.title,
                    unit: e.unit as usize,
                    level: e.level,
                })
                .collect(),
        }
    }

    fn render(
        &mut self,
        unit: usize,
        request: kalem_viewer::RenderRequest,
    ) -> kalem_viewer::Result<kalem_viewer::Rendered> {
        let rgb = |[r, g, b]: [u8; 3]| api::Rgb { r, g, b };
        let request = api::RenderRequest {
            scale: request.scale,
            theme: api::Theme {
                dark: request.theme.dark,
                background: rgb(request.theme.background),
                foreground: rgb(request.theme.foreground),
            },
        };
        let b = self
            .call(|d, st, doc| d.call_render(st, doc, unit as u32, request))?
            .map_err(kalem_viewer::ViewerError)?;
        Ok(kalem_viewer::Rendered::Bitmap(kalem_viewer::Bitmap::new(
            b.width, b.height, b.rgba,
        )))
    }

    fn size(&self, unit: usize) -> Option<(f32, f32)> {
        self.call(|d, st, doc| d.call_size(st, doc, unit as u32))
            .ok()
            .flatten()
    }

    fn text(&self, unit: usize) -> String {
        self.call(|d, st, doc| d.call_text(st, doc, unit as u32))
            .unwrap_or_default()
    }

    fn text_rects(&self, unit: usize, range: std::ops::Range<usize>) -> Vec<[f32; 4]> {
        self.call(|d, st, doc| d.call_text_rects(st, doc, unit as u32, span(range)))
            .map(|r| r.into_iter().map(rect).collect())
            .unwrap_or_default()
    }

    fn text_at(&self, unit: usize, x: f32, y: f32) -> Option<(std::ops::Range<usize>, [f32; 4])> {
        let (s, r) = self
            .call(|d, st, doc| d.call_text_at(st, doc, unit as u32, x, y))
            .ok()??;
        Some((s.start as usize..s.end as usize, rect(r)))
    }

    fn info(&self) -> Vec<kalem_viewer::InfoField> {
        self.call(|d, st, doc| d.call_info(st, doc))
            .map(|f| {
                f.into_iter()
                    .map(|f| kalem_viewer::InfoField::new(f.label, f.value))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn search(&self, query: &str) -> Vec<(usize, std::ops::Range<usize>)> {
        self.call(|d, st, doc| d.call_search(st, doc, query))
            .map(|h| {
                h.into_iter()
                    .map(|h| (h.unit as usize, h.span.start as usize..h.span.end as usize))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn links(&self, unit: usize) -> Vec<kalem_viewer::Link> {
        self.call(|d, st, doc| d.call_links(st, doc, unit as u32))
            .map(|l| {
                l.into_iter()
                    .map(|l| kalem_viewer::Link {
                        rect: rect(l.rect),
                        target: l.target,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn edits(&self, unit: usize) -> Vec<kalem_viewer::Edit> {
        self.call(|d, st, doc| d.call_edits(st, doc, unit as u32))
            .map(|e| {
                e.into_iter()
                    .map(|e| kalem_viewer::Edit {
                        id: e.id,
                        title: e.title,
                        inverse: e.inverse,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn apply(&mut self, edit: &str) -> kalem_viewer::Result<Vec<usize>> {
        let changed = self
            .call(|d, st, doc| d.call_apply(st, doc, edit))?
            .map_err(kalem_viewer::ViewerError)?;
        Ok(changed.into_iter().map(|u| u as usize).collect())
    }

    fn modified(&self) -> bool {
        self.call(|d, st, doc| d.call_modified(st, doc))
            .unwrap_or(false)
    }

    fn save(&mut self) -> kalem_viewer::Result<kalem_viewer::SaveOutput> {
        let out = self
            .call(|d, st, doc| d.call_save(st, doc))?
            .map_err(kalem_viewer::ViewerError)?;
        Ok(kalem_viewer::SaveOutput {
            bytes: out.bytes,
            losses: out.losses,
        })
    }
}
