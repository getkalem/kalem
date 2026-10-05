//! The host's side of the `document-viewer` world (design §11.13, D54),
//! generated from `kalem-plugin`'s WIT files, the one definition both
//! sides are built from (D6).
//!
//! A viewer reads only the file the host opened for it: the `file`
//! resource carries the file's path in the host and gives the plugin its
//! name, its size and the bytes it asks for.

use std::path::PathBuf;

use wasmtime::component::{Resource, ResourceTable};

wasmtime::component::bindgen!({
    path: "../kalem-plugin/wit",
    world: "document-viewer",
    with: {
        "kalem:plugin/files.file": OpenFile,
    },
});

pub use exports::kalem::plugin::password;
pub use exports::kalem::plugin::viewer as api;

/// The `spreadsheet-viewer` world's bindings: the `grid` interface of a
/// viewer of sheets of cells (T3.7.4), beside the same `viewer`.
mod spreadsheet {
    wasmtime::component::bindgen!({
        path: "../kalem-plugin/wit",
        world: "spreadsheet-viewer",
        with: {
            "kalem:plugin/files.file": super::OpenFile,
        },
    });
}

/// The `grid` interface, as the host calls it.
pub use spreadsheet::exports::kalem::plugin::grid;

/// The grid's types between the contract and the interface.
#[allow(unreachable_pub, dead_code)]
mod grid_conv {
    use super::grid as g;
    use kalem_viewer as kv;

    include!("../../kalem-plugin/src/grid_conv.rs");
}

/// A file the host opened for a plugin: one on disk, or bytes the host
/// holds (a workbook converted from `.ods` as it opened).
#[derive(Debug)]
pub struct OpenFile {
    file: kalem_viewer::FileHandle,
}

/// What a viewer plugin's store holds: the files it was given.
#[derive(Debug, Default)]
pub struct Files {
    table: ResourceTable,
}

impl Files {
    /// Hands the file at `path` to the plugin.
    pub fn open(&mut self, file: kalem_viewer::FileHandle) -> wasmtime::Result<Resource<OpenFile>> {
        Ok(self.table.push(OpenFile { file })?)
    }

    fn get(&self, f: &Resource<OpenFile>) -> wasmtime::Result<&OpenFile> {
        Ok(self.table.get(f)?)
    }
}

impl kalem::plugin::files::Host for Files {}

impl kalem::plugin::clock::Host for Files {
    fn now(&mut self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as i64)
    }

    fn timezone(&mut self) -> String {
        jiff::tz::TimeZone::system()
            .iana_name()
            .unwrap_or("UTC")
            .to_string()
    }

    fn random(&mut self) -> u64 {
        use std::hash::{BuildHasher, Hasher};
        use std::sync::atomic::{AtomicU64, Ordering};
        // Keys random for each process, a counter and the time hashed.
        static N: AtomicU64 = AtomicU64::new(0);
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u64(N.fetch_add(1, Ordering::Relaxed));
        h.write_i64(self.now());
        h.finish()
    }
}

impl kalem::plugin::files::HostFile for Files {
    fn name(&mut self, f: Resource<OpenFile>) -> String {
        self.get(&f)
            .map(|o| o.file.name().to_string())
            .unwrap_or_default()
    }

    fn len(&mut self, f: Resource<OpenFile>) -> u64 {
        self.get(&f)
            .ok()
            .and_then(|o| o.file.len().ok())
            .unwrap_or(0)
    }

    fn read(&mut self, f: Resource<OpenFile>, offset: u64, len: u32) -> Vec<u8> {
        self.get(&f)
            .ok()
            .and_then(|o| o.file.read_at(offset, len as usize).ok())
            .unwrap_or_default()
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
    api: api::Guest,
    /// The `grid` exports, for a viewer of sheets of cells.
    grid: Option<grid::Guest>,
    /// The `password` exports (API 0.2.2), for files protected by one.
    password: Option<password::Guest>,
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
        kalem::plugin::clock::add_to_linker::<_, wasmtime::component::HasSelf<Files>>(
            &mut linker,
            |d: &mut crate::Data<Files>| &mut d.user,
        )
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        kalem::plugin::diagnostics::add_to_linker::<
            _,
            wasmtime::component::HasSelf<crate::Diagnostics>,
        >(&mut linker, |d: &mut crate::Data<Files>| &mut d.diagnostics)
        .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        let mut instance = plugin.instantiate(host, &linker, Files::default(), limits)?;
        // A component built against another version of the API than this
        // Kalem's (a function or a record's field added since) does not
        // bind: refused, saying so, rather than a viewer without its grid.
        let stale = |e: crate::Error| {
            crate::Error::Invalid(format!(
                "built for another version of Kalem's plugin API, update the plugin ({e})"
            ))
        };
        // `viewer` alone, not the whole world: a component of an earlier
        // 0.2.x lacks the interfaces added to it since.
        let api = instance
            .bindings(|store, i| {
                let pre = i.instance_pre(&*store);
                api::GuestIndices::new(&pre)?.load(&mut *store, i)
            })
            .map_err(stale)?;
        // The interfaces beside `viewer` bound one by one, as the
        // component has them (wasm_todo W3): one it does not export is left
        // out (a component built against an earlier 0.2.x lacks those
        // added since), one it exports must match. Names are looked up
        // semver-compatibly, so `grid@0.2.0` finds a component's
        // `grid@0.2.3`.
        let grid = instance
            .bindings(|store, i| {
                let pre = i.instance_pre(&*store);
                let name = format!("kalem:plugin/grid@{}", crate::API_VERSION);
                if pre.component().get_export_index(None, &name).is_none() {
                    return Ok(None);
                }
                grid::GuestIndices::new(&pre)?
                    .load(&mut *store, i)
                    .map(Some)
            })
            .map_err(stale)?;
        let password = instance
            .bindings(|store, i| {
                let pre = i.instance_pre(&*store);
                let name = format!("kalem:plugin/password@{}", crate::API_VERSION);
                if pre.component().get_export_index(None, &name).is_none() {
                    return Ok(None);
                }
                password::GuestIndices::new(&pre)?
                    .load(&mut *store, i)
                    .map(Some)
            })
            .map_err(stale)?;
        Ok(Viewer {
            instance,
            api,
            grid,
            password,
        })
    }

    /// The plugin's description of itself.
    pub fn describe(&mut self) -> crate::Result<api::Description> {
        let v = &self.api;
        self.instance.run(|s| v.call_describe(s))
    }

    /// Whether it opens the file `name` starting with `head`.
    pub fn detect(&mut self, name: &str, head: &[u8]) -> crate::Result<api::Detection> {
        let v = &self.api;
        self.instance.run(|s| v.call_detect(s, name, head))
    }

    /// Opens the file at `path` in the plugin, which reads it through the
    /// handle and nothing else.
    pub fn open(&mut self, path: impl Into<PathBuf>) -> crate::Result<Result<Document, String>> {
        self.open_file(kalem_viewer::FileHandle::new(path))
    }

    /// Opens `file` in the plugin: a file on disk, or bytes the host holds.
    pub fn open_file(
        &mut self,
        file: kalem_viewer::FileHandle,
    ) -> crate::Result<Result<Document, String>> {
        let file = self
            .instance
            .data_mut()
            .open(file)
            .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        let v = &self.api;
        self.instance.run(|s| v.call_open(s, file))
    }

    /// Opens `file` with `password` (the `password` interface); `None`
    /// when the plugin has no such interface.
    pub fn open_file_with_password(
        &mut self,
        file: kalem_viewer::FileHandle,
        password: &str,
    ) -> crate::Result<Option<Result<Document, String>>> {
        let Some(p) = &self.password else {
            return Ok(None);
        };
        let file = self
            .instance
            .data_mut()
            .open(file)
            .map_err(|e| crate::Error::Invalid(format!("{e:#}")))?;
        self.instance
            .run(|s| p.call_open_with_password(s, file, password))
            .map(Some)
    }

    /// Calls `f` with the plugin's `document` resource functions.
    pub fn document<R>(
        &mut self,
        f: impl FnOnce(
            api::GuestDocument<'_>,
            &mut wasmtime::Store<crate::Data<Files>>,
        ) -> wasmtime::Result<R>,
    ) -> crate::Result<R> {
        let v = &self.api;
        self.instance.run(|s| f(v.document(), s))
    }

    /// Whether the plugin exports the `grid` interface.
    pub fn is_grid(&self) -> bool {
        self.grid.is_some()
    }

    /// Calls `f` with the plugin's `grid` functions; `None` when it has
    /// none.
    pub fn grid<R>(
        &mut self,
        f: impl FnOnce(&grid::Guest, &mut wasmtime::Store<crate::Data<Files>>) -> wasmtime::Result<R>,
    ) -> Option<crate::Result<R>> {
        let g = self.grid.as_ref()?;
        Some(self.instance.run(|s| f(g, s)))
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
    /// The component's bytes when it is built into Kalem (wasm_todo W5)
    /// rather than installed in a file.
    bytes: Option<&'static [u8]>,
    id: String,
    name: String,
    extensions: Vec<&'static str>,
    limits: crate::Limits,
    plugin: std::sync::Mutex<Option<Loaded>>,
    /// The instance answering `detect`, with the [`Loaded::generation`]
    /// it was made of.
    detector: std::sync::Mutex<Option<(u64, Viewer)>>,
    /// The bundled viewer this one takes the place of, used when the
    /// component cannot run (built for another version of the API).
    fallback: Option<std::sync::Arc<dyn kalem_viewer::Viewer>>,
    /// Tells the user, once, that the fallback is used.
    notice: Option<std::sync::Arc<dyn Fn(String) + Send + Sync>>,
    warned: std::sync::atomic::AtomicBool,
    /// Told when a document of this viewer stops (wasm_todo W8): Kalem
    /// counts the failures and turns a plugin that keeps failing off.
    on_stop: Option<OnStop>,
}

/// What [`ComponentViewer::with_on_stop`] calls.
pub type OnStop = std::sync::Arc<dyn Fn(&kalem_viewer::Stopped) + Send + Sync>;

/// A component as compiled, or why it could not be.
struct Loaded {
    /// When its file was last changed (none built into Kalem).
    changed: Option<std::time::SystemTime>,
    /// How many times it was read again.
    generation: u64,
    plugin: Result<crate::Plugin, String>,
}

impl std::fmt::Debug for ComponentViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentViewer")
            .field("id", &self.id)
            .field("file", &self.file)
            .field("embedded", &self.bytes.is_some())
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
            bytes: None,
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
            plugin: std::sync::Mutex::new(None),
            detector: std::sync::Mutex::new(None),
            fallback: None,
            notice: None,
            warned: std::sync::atomic::AtomicBool::new(false),
            on_stop: None,
        }
    }

    /// The component built into Kalem (`bytes`), known as `id` and `name`,
    /// opening files with `extensions`: compiled, or read from the
    /// host's cache, the first time it is needed.
    pub fn embedded(
        host: std::sync::Arc<crate::Host>,
        bytes: &'static [u8],
        id: impl Into<String>,
        name: impl Into<String>,
        extensions: &[String],
        limits: crate::Limits,
    ) -> ComponentViewer {
        ComponentViewer {
            bytes: Some(bytes),
            ..ComponentViewer::new(host, PathBuf::new(), id, name, extensions, limits)
        }
    }

    /// Whether the component is built into Kalem (else installed).
    pub fn is_embedded(&self) -> bool {
        self.bytes.is_some()
    }

    /// The plugin's name, as its manifest gives it.
    pub fn label(&self) -> &str {
        &self.name
    }

    /// With `fallback`, the bundled viewer it replaces, opening the files
    /// when the component cannot run, and `notice` telling the user once
    /// why.
    pub fn with_fallback(
        mut self,
        fallback: Option<std::sync::Arc<dyn kalem_viewer::Viewer>>,
        notice: std::sync::Arc<dyn Fn(String) + Send + Sync>,
    ) -> ComponentViewer {
        self.fallback = fallback;
        self.notice = Some(notice);
        self
    }

    /// The same viewer, calling `f` once for each of its documents that
    /// stops: a trap, the time budget or the memory limit spent.
    pub fn with_on_stop(mut self, f: OnStop) -> ComponentViewer {
        self.on_stop = Some(f);
        self
    }

    /// Says, once, that the component cannot run and why, and what opens
    /// its files instead.
    fn warn(&self, e: &kalem_viewer::ViewerError) {
        if self.warned.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        if let Some(n) = &self.notice {
            n(match &self.fallback {
                Some(_) => format!("{}: {}; the bundled viewer opens its files", self.name, e.0),
                None => format!("{}: {}", self.name, e.0),
            });
        }
    }

    /// The component, compiled or read from the cache on first use. One
    /// installed in a file is read again when the file changed (`kalem
    /// plugin dev` built it again, wasm_todo W10): the documents opened
    /// from then on run the new build, those open keep theirs.
    pub fn plugin(&self) -> Result<crate::Plugin, kalem_viewer::ViewerError> {
        self.loaded().1
    }

    /// [`ComponentViewer::plugin`] with its [`Loaded::generation`].
    fn loaded(&self) -> (u64, Result<crate::Plugin, kalem_viewer::ViewerError>) {
        let mut slot = self.plugin.lock().unwrap_or_else(|e| e.into_inner());
        let changed = match self.bytes {
            Some(_) => None,
            None => std::fs::metadata(&self.file)
                .and_then(|m| m.modified())
                .ok(),
        };
        if slot
            .as_ref()
            .is_none_or(|l| self.bytes.is_none() && l.changed != changed)
        {
            let plugin = match self.bytes {
                Some(b) => self.host.load(b),
                None => self.host.load_file(&self.file),
            }
            .map_err(|e| e.to_string());
            let generation = slot.as_ref().map_or(0, |l| l.generation + 1);
            *slot = Some(Loaded {
                changed,
                generation,
                plugin,
            });
        }
        let l = slot.as_ref().expect("loaded");
        (
            l.generation,
            l.plugin
                .clone()
                .map_err(|e| kalem_viewer::ViewerError(format!("{}: {e}", self.name))),
        )
    }

    fn instance(&self) -> Result<Viewer, kalem_viewer::ViewerError> {
        self.instance_of(self.plugin())
    }

    fn instance_of(
        &self,
        plugin: Result<crate::Plugin, kalem_viewer::ViewerError>,
    ) -> Result<Viewer, kalem_viewer::ViewerError> {
        Viewer::new(&self.host, &plugin?, self.limits).map_err(err)
    }

    /// Whether the component runs with this Kalem: compiled and bound to
    /// the plugin API, as opening a file would (`kalem plugin check`).
    pub fn check(&self) -> Result<(), kalem_viewer::ViewerError> {
        self.instance().map(|_| ())
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
        // Asked before the detector's lock: `loaded` takes the plugin's.
        let (generation, plugin) = self.loaded();
        let mut slot = self.detector.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_none_or(|(g, _)| *g != generation) {
            match self.instance_of(plugin) {
                Ok(v) => *slot = Some((generation, v)),
                Err(e) => {
                    self.warn(&e);
                    return self
                        .fallback
                        .as_ref()
                        .map_or(kalem_viewer::Detection::No, |f| f.detect(name, head));
                }
            }
        }
        let Some((_, v)) = slot.as_mut() else {
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
        self.open_document(file, None)
    }

    fn open_with_password(
        &self,
        file: kalem_viewer::FileHandle,
        password: &str,
    ) -> kalem_viewer::Result<Box<dyn kalem_viewer::ViewerDocument>> {
        self.open_document(file, Some(password))
    }
}

impl ComponentViewer {
    /// Opens `file` in an instance of its own, with `password` when given
    /// (as `open` when the plugin has no `password` interface).
    fn open_document(
        &self,
        file: kalem_viewer::FileHandle,
        password: Option<&str>,
    ) -> kalem_viewer::Result<Box<dyn kalem_viewer::ViewerDocument>> {
        let mut v = match self.instance() {
            Ok(v) => v,
            Err(e) => {
                self.warn(&e);
                return match (&self.fallback, password) {
                    (Some(f), Some(p)) => f.open_with_password(file, p),
                    (Some(f), None) => f.open(file),
                    (None, _) => Err(e),
                };
            }
        };
        // The handle itself: bytes the host holds reach the plugin too (a
        // workbook converted from `.ods` has no file on disk).
        let opened = match password {
            Some(p) => match v.open_file_with_password(file.clone(), p) {
                Ok(Some(r)) => Ok(r),
                Ok(None) => v.open_file(file),
                Err(e) => Err(e),
            },
            None => v.open_file(file),
        };
        let doc = match opened {
            Ok(opened) => opened.map_err(kalem_viewer::ViewerError)?,
            // It failed opening the file: counted as a stop.
            Err(e) => {
                let why = stopped_of(&e);
                tracing::error!(viewer = %self.name, error = %e, "a plugin failed opening a file");
                if let Some(hook) = &self.on_stop {
                    hook(&why);
                }
                return Err(stopped_error(&self.name, &why));
            }
        };
        Ok(Box::new(ComponentDocument {
            v: std::sync::Mutex::new(v),
            doc,
            modified: std::sync::atomic::AtomicBool::new(false),
            structure: std::sync::Mutex::new(None),
            stopped: std::sync::Mutex::new(None),
            name: self.name.clone(),
            on_stop: self.on_stop.clone(),
        }))
    }
}

use grid_conv::Conv;
use kalem_viewer as kv;

/// A document a component opened, in its own instance.
struct ComponentDocument {
    v: std::sync::Mutex<Viewer>,
    doc: Document,
    /// What `modified` last answered: an instance spent by a trap or its
    /// time can answer no more, and its unsaved edits must not then look
    /// saved (closing would not ask).
    modified: std::sync::atomic::AtomicBool,
    /// What `structure` last answered, for the same reason: an empty one
    /// has no unit the host shows.
    structure: std::sync::Mutex<Option<kalem_viewer::Structure>>,
    /// Why the instance stopped: it is called no more (wasm_todo W8).
    stopped: std::sync::Mutex<Option<kalem_viewer::Stopped>>,
    /// The viewer's name, for the errors after it stopped.
    name: String,
    on_stop: Option<OnStop>,
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
        self.run(|v| Some(v.document(|d, s| f(d, s, doc))))
            .unwrap_or_else(|| Err(kv::ViewerError("Not a grid".into())))
    }

    /// `f` on the instance, unless it stopped; its first failure (a trap,
    /// the time or the memory spent) stops it, said once in the log and
    /// to the viewer's hook.
    fn run<R>(
        &self,
        f: impl FnOnce(&mut Viewer) -> Option<crate::Result<R>>,
    ) -> Option<kalem_viewer::Result<R>> {
        // Not held while the plugin runs: `stopped` is asked at every tick.
        if let Some(why) = kalem_viewer::ViewerDocument::stopped(self) {
            return Some(Err(stopped_error(&self.name, &why)));
        }
        let mut v = self.v.lock().unwrap_or_else(|e| e.into_inner());
        match f(&mut v)? {
            Ok(r) => Some(Ok(r)),
            Err(e) => {
                let why = stopped_of(&e);
                let mut stopped = self.stopped.lock().unwrap_or_else(|e| e.into_inner());
                if stopped.is_none() {
                    tracing::error!(viewer = %self.name, error = %e, "a plugin's document stopped; it answers no more");
                    if let Some(hook) = &self.on_stop {
                        hook(&why);
                    }
                    *stopped = Some(why.clone());
                }
                Some(Err(stopped_error(&self.name, &why)))
            }
        }
    }
}

/// Why an instance stopped, from the host's error.
fn stopped_of(e: &crate::Error) -> kalem_viewer::Stopped {
    match e {
        crate::Error::Timeout(t) => kalem_viewer::Stopped::Timeout(*t),
        crate::Error::Memory(m) => kalem_viewer::Stopped::Memory(*m),
        other => kalem_viewer::Stopped::Failed(other.to_string()),
    }
}

/// The error of a call to viewer `name`'s document that stopped: short,
/// the detail being the log's.
fn stopped_error(name: &str, why: &kalem_viewer::Stopped) -> kalem_viewer::ViewerError {
    kalem_viewer::ViewerError(match why {
        kalem_viewer::Stopped::Failed(_) => format!("{name} stopped: it failed"),
        kalem_viewer::Stopped::Timeout(t) => {
            format!("{name} stopped: it ran past its {} s", t.as_secs())
        }
        kalem_viewer::Stopped::Memory(m) => {
            format!("{name} stopped: it ran out of its {} MB", m >> 20)
        }
    })
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
    fn stopped(&self) -> Option<kalem_viewer::Stopped> {
        self.stopped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn structure(&self) -> kalem_viewer::Structure {
        let Ok(s) = self.call(|d, st, doc| d.call_structure(st, doc)) else {
            let last = self.structure.lock().unwrap_or_else(|e| e.into_inner());
            return last.clone().unwrap_or_default();
        };
        let s = kalem_viewer::Structure {
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
        };
        *self.structure.lock().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
        s
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
        use std::sync::atomic::Ordering::Relaxed;
        match self.call(|d, st, doc| d.call_modified(st, doc)) {
            Ok(m) => {
                self.modified.store(m, Relaxed);
                m
            }
            Err(_) => self.modified.load(Relaxed),
        }
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

    // The grid (T3.7.4): through the plugin's `grid` exports when it has
    // them; a viewer without has none, as the contract's defaults say.

    fn grid(&mut self, unit: usize) -> Option<kv::GridLayout> {
        self.g(|g, s, d| g.call_layout(s, d, unit as u32))
            .ok()
            .flatten()
            .map(Conv::conv)
    }

    fn grid_cells(
        &mut self,
        unit: usize,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32, kv::GridCell)> {
        self.g(|g, s, d| {
            g.call_cells(
                s,
                d,
                unit as u32,
                (rows.start, rows.end),
                (cols.start, cols.end),
            )
        })
        .map(|c| {
            c.into_iter()
                .map(|p| (p.row, p.col, p.cell.conv()))
                .collect()
        })
        .unwrap_or_default()
    }

    fn cell_input(&mut self, unit: usize, row: u32, col: u32) -> String {
        self.g(|g, s, d| g.call_cell_input(s, d, unit as u32, row, col))
            .unwrap_or_default()
    }

    fn set_frozen(&mut self, unit: usize, rows: u32, cols: u32) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_frozen(s, d, unit as u32, rows, cols))
    }

    fn set_hidden(
        &mut self,
        unit: usize,
        rows: bool,
        from: u32,
        to: u32,
        hidden: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_hidden(s, d, unit as u32, rows, from, to, hidden))
    }

    fn edit_sheets(&mut self, edit: kv::SheetEdit) -> kv::Result<usize> {
        let edit: grid::SheetEdit = edit.conv();
        self.g(|g, s, d| g.call_edit_sheets(s, d, &edit))?
            .map(|u| u as usize)
            .map_err(kv::ViewerError)
    }

    fn hidden_units(&mut self) -> Vec<usize> {
        self.g(|g, s, d| g.call_hidden_units(s, d))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn range_numbers(&mut self, unit: usize, range: [u32; 4]) -> (Vec<f64>, usize) {
        self.g(|g, s, d| g.call_range_numbers(s, d, unit as u32, range.conv()))
            .map(|(v, n)| (v, n as usize))
            .unwrap_or_default()
    }

    fn cell_format(&mut self, unit: usize, row: u32, col: u32) -> Option<String> {
        self.g(|g, s, d| g.call_cell_format(s, d, unit as u32, row, col))
            .ok()
            .flatten()
    }

    fn cell_note(&mut self, unit: usize, row: u32, col: u32) -> Option<String> {
        self.g(|g, s, d| g.call_cell_note(s, d, unit as u32, row, col))
            .ok()
            .flatten()
    }

    fn formula_functions(&mut self) -> Vec<(String, String)> {
        self.g(|g, s, d| g.call_formula_functions(s, d))
            .unwrap_or_default()
    }

    fn paste_cells(
        &mut self,
        from: (usize, [u32; 4]),
        to: (usize, u32, u32),
        kind: kv::PasteKind,
        transpose: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_paste_cells(
                s,
                d,
                from.0 as u32,
                from.1.conv(),
                to.0 as u32,
                to.1,
                to.2,
                kind.conv(),
                transpose,
            )
        })
    }

    fn clear_range(
        &mut self,
        unit: usize,
        range: [u32; 4],
        contents: bool,
        formats: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_clear_range(s, d, unit as u32, range.conv(), contents, formats))
    }

    fn fill_formats(
        &mut self,
        from: (usize, [u32; 4]),
        to: (usize, [u32; 4]),
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_fill_formats(s, d, from.0 as u32, from.1.conv(), to.0 as u32, to.1.conv())
        })
    }

    fn remove_duplicates(
        &mut self,
        unit: usize,
        range: [u32; 4],
        columns: &[u32],
        header: bool,
    ) -> kv::Result<usize> {
        self.g(|g, s, d| {
            g.call_remove_duplicates(s, d, unit as u32, range.conv(), columns, header)
        })?
        .map(|n| n as usize)
        .map_err(kv::ViewerError)
    }

    fn cell_link(&mut self, unit: usize, row: u32, col: u32) -> Option<String> {
        self.g(|g, s, d| g.call_cell_link(s, d, unit as u32, row, col))
            .ok()
            .flatten()
    }

    fn set_link(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        target: Option<String>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_link(s, d, unit as u32, row, col, target.as_deref()))
    }

    fn defined_names(&mut self) -> Vec<(String, String)> {
        self.g(|g, s, d| g.call_defined_names(s, d))
            .unwrap_or_default()
    }

    fn set_defined_name(&mut self, name: &str, refers_to: Option<&str>) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_defined_name(s, d, name, refers_to))
    }

    fn recalculate(&mut self) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_recalculate(s, d))
    }

    fn set_note(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        text: Option<String>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_note(s, d, unit as u32, row, col, text.as_deref()))
    }

    fn set_cell(&mut self, unit: usize, row: u32, col: u32, input: &str) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_cell(s, d, unit as u32, row, col, input))
    }

    fn grid_edit(&mut self, unit: usize, edit: kv::GridEdit) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_edit_grid(s, d, unit as u32, edit.conv()))
    }

    fn set_cell_list(
        &mut self,
        unit: usize,
        cells: &[(u32, u32, String)],
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_cell_list(s, d, unit as u32, cells))
    }

    fn set_cells(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        values: &[Vec<String>],
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_cells(s, d, unit as u32, row, col, values))
    }

    fn add_conditional_format(
        &mut self,
        unit: usize,
        range: [u32; 4],
        rule: kv::CondRule,
        style: kv::CondStyle,
    ) -> kv::Result<Vec<usize>> {
        let rule: grid::CondRule = rule.conv();
        self.ch(|g, s, d| {
            g.call_add_conditional_format(s, d, unit as u32, range.conv(), &rule, style.conv())
        })
    }

    fn clear_conditional_formats(
        &mut self,
        unit: usize,
        range: Option<[u32; 4]>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_clear_conditional_formats(s, d, unit as u32, range.conv()))
    }

    fn charts(&mut self, unit: usize) -> Vec<kv::Chart> {
        self.g(|g, s, d| g.call_charts(s, d, unit as u32))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn insert_chart(
        &mut self,
        unit: usize,
        range: [u32; 4],
        kind: kv::ChartKind,
        title: Option<String>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_insert_chart(
                s,
                d,
                unit as u32,
                range.conv(),
                kind.conv(),
                title.as_deref(),
            )
        })
    }

    fn move_chart(
        &mut self,
        unit: usize,
        index: usize,
        anchor: [u32; 4],
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_move_chart(s, d, unit as u32, index as u32, anchor.conv()))
    }

    fn set_chart_title(
        &mut self,
        unit: usize,
        index: usize,
        title: Option<String>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_chart_title(s, d, unit as u32, index as u32, title.as_deref()))
    }

    fn set_axis_title(
        &mut self,
        unit: usize,
        index: usize,
        axis: kv::ChartAxis,
        title: Option<String>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_axis_title(
                s,
                d,
                unit as u32,
                index as u32,
                axis.conv(),
                title.as_deref(),
            )
        })
    }

    fn set_legend(
        &mut self,
        unit: usize,
        index: usize,
        position: Option<kv::LegendPosition>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_legend(s, d, unit as u32, index as u32, position.conv()))
    }

    fn set_data_labels(
        &mut self,
        unit: usize,
        index: usize,
        labels: kv::DataLabels,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_data_labels(s, d, unit as u32, index as u32, labels.conv()))
    }

    fn set_axis_scale(
        &mut self,
        unit: usize,
        index: usize,
        scale: kv::AxisScale,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_axis_scale(s, d, unit as u32, index as u32, scale.conv()))
    }

    fn set_chart_kind(
        &mut self,
        unit: usize,
        index: usize,
        kind: kv::ChartKind,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_chart_kind(s, d, unit as u32, index as u32, kind.conv()))
    }

    fn set_series_kind(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        kind: Option<kv::ChartKind>,
        secondary: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_series_kind(
                s,
                d,
                unit as u32,
                index as u32,
                series as u32,
                kind.conv(),
                secondary,
            )
        })
    }

    fn set_trendline(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        trendline: Option<kv::Trendline>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_trendline(
                s,
                d,
                unit as u32,
                index as u32,
                series as u32,
                trendline.conv(),
            )
        })
    }

    fn set_error_bars(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        bars: Option<kv::ErrorBars>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_error_bars(s, d, unit as u32, index as u32, series as u32, bars.conv())
        })
    }

    fn set_label_cells(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        range: Option<[u32; 4]>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_label_cells(
                s,
                d,
                unit as u32,
                index as u32,
                series as u32,
                range.map(|r| r.conv()),
            )
        })
    }

    fn move_chart_to_sheet(&mut self, unit: usize, index: usize, name: &str) -> kv::Result<usize> {
        self.g(|g, s, d| g.call_move_chart_to_sheet(s, d, unit as u32, index as u32, name))?
            .map(|u| u as usize)
            .map_err(kv::ViewerError)
    }

    fn move_chart_to_grid(
        &mut self,
        unit: usize,
        target: usize,
        anchor: [u32; 4],
    ) -> kv::Result<usize> {
        self.g(|g, s, d| {
            g.call_move_chart_to_grid(s, d, unit as u32, target as u32, anchor.conv())
        })?
        .map(|u| u as usize)
        .map_err(kv::ViewerError)
    }

    fn chart_template(&mut self, unit: usize, index: usize) -> kv::Result<Vec<u8>> {
        self.g(|g, s, d| g.call_chart_template(s, d, unit as u32, index as u32))?
            .map_err(kv::ViewerError)
    }

    fn apply_chart_template(
        &mut self,
        unit: usize,
        index: usize,
        template: &[u8],
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_apply_chart_template(s, d, unit as u32, index as u32, template))
    }

    fn set_series_color(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        color: Option<[u8; 3]>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_series_color(s, d, unit as u32, index as u32, series as u32, color.conv())
        })
    }

    fn set_point_color(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        point: usize,
        color: Option<[u8; 3]>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_point_color(
                s,
                d,
                unit as u32,
                index as u32,
                series as u32,
                point as u32,
                color.conv(),
            )
        })
    }

    fn set_explosion(
        &mut self,
        unit: usize,
        index: usize,
        series: usize,
        point: Option<usize>,
        percent: u32,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_explosion(
                s,
                d,
                unit as u32,
                index as u32,
                series as u32,
                point.conv(),
                percent,
            )
        })
    }

    fn set_chart_area(
        &mut self,
        unit: usize,
        index: usize,
        background: kv::Paint,
        border: kv::Paint,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_chart_area(
                s,
                d,
                unit as u32,
                index as u32,
                background.conv(),
                border.conv(),
            )
        })
    }

    fn set_plot_area(
        &mut self,
        unit: usize,
        index: usize,
        background: kv::Paint,
        border: kv::Paint,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_plot_area(
                s,
                d,
                unit as u32,
                index as u32,
                background.conv(),
                border.conv(),
            )
        })
    }

    fn set_gridlines(
        &mut self,
        unit: usize,
        index: usize,
        lines: kv::Gridlines,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_gridlines(s, d, unit as u32, index as u32, lines.conv()))
    }

    fn set_axis_format(
        &mut self,
        unit: usize,
        index: usize,
        format: Option<String>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_set_axis_format(s, d, unit as u32, index as u32, format.as_deref())
        })
    }

    fn set_axis_font(
        &mut self,
        unit: usize,
        index: usize,
        axis: kv::ChartAxis,
        font: kv::AxisFont,
    ) -> kv::Result<Vec<usize>> {
        let font: grid::AxisFont = font.conv();
        self.ch(|g, s, d| g.call_set_axis_font(s, d, unit as u32, index as u32, axis.conv(), &font))
    }

    fn set_title_font(
        &mut self,
        unit: usize,
        index: usize,
        font: kv::AxisFont,
    ) -> kv::Result<Vec<usize>> {
        let font: grid::AxisFont = font.conv();
        self.ch(|g, s, d| g.call_set_title_font(s, d, unit as u32, index as u32, &font))
    }

    fn set_legend_font(
        &mut self,
        unit: usize,
        index: usize,
        font: kv::AxisFont,
    ) -> kv::Result<Vec<usize>> {
        let font: grid::AxisFont = font.conv();
        self.ch(|g, s, d| g.call_set_legend_font(s, d, unit as u32, index as u32, &font))
    }

    fn delete_chart(&mut self, unit: usize, index: usize) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_delete_chart(s, d, unit as u32, index as u32))
    }

    fn insert_pivot(&mut self, unit: usize, spec: kv::PivotSpec) -> kv::Result<usize> {
        let spec: grid::PivotSpec = spec.conv();
        self.g(|g, s, d| g.call_insert_pivot(s, d, unit as u32, &spec))?
            .map(|u| u as usize)
            .map_err(kv::ViewerError)
    }

    fn pivots(&mut self, unit: usize) -> Vec<kv::PivotInfo> {
        self.g(|g, s, d| g.call_pivots(s, d, unit as u32))
            .map(|v| v.conv())
            .unwrap_or_default()
    }

    fn set_pivot(
        &mut self,
        unit: usize,
        index: usize,
        spec: kv::PivotSpec,
    ) -> kv::Result<Vec<usize>> {
        let spec: grid::PivotSpec = spec.conv();
        self.ch(|g, s, d| g.call_set_pivot(s, d, unit as u32, index as u32, &spec))
    }

    fn insert_pivot_chart(
        &mut self,
        unit: usize,
        index: usize,
        kind: kv::ChartKind,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_insert_pivot_chart(s, d, unit as u32, index as u32, kind.conv()))
    }

    fn slicers(&mut self, unit: usize) -> Vec<kv::Slicer> {
        self.g(|g, s, d| g.call_slicers(s, d, unit as u32))
            .map(|v| v.conv())
            .unwrap_or_default()
    }

    fn insert_slicer(
        &mut self,
        unit: usize,
        pivot: Option<usize>,
        table: Option<&str>,
        field: &str,
        anchor: [u32; 4],
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_insert_slicer(
                s,
                d,
                unit as u32,
                pivot.map(|p| p as u32),
                table,
                field,
                anchor.conv(),
            )
        })
    }

    fn select_slicer(
        &mut self,
        unit: usize,
        index: usize,
        selected: &[String],
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_select_slicer(s, d, unit as u32, index as u32, selected))
    }

    fn delete_slicer(&mut self, unit: usize, index: usize) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_delete_slicer(s, d, unit as u32, index as u32))
    }

    fn refresh_pivots(&mut self) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_refresh_pivots(s, d))
    }

    fn validation(&mut self, unit: usize, row: u32, col: u32) -> Option<kv::Validation> {
        self.g(|g, s, d| g.call_validation(s, d, unit as u32, row, col))
            .ok()
            .flatten()
            .map(Conv::conv)
    }

    fn change_style(
        &mut self,
        unit: usize,
        range: [u32; 4],
        change: kv::StyleChange,
    ) -> kv::Result<Vec<usize>> {
        let change: grid::StyleChange = change.conv();
        self.ch(|g, s, d| g.call_change_style(s, d, unit as u32, range.conv(), &change))
    }

    fn set_validation(
        &mut self,
        unit: usize,
        range: [u32; 4],
        validation: Option<kv::Validation>,
    ) -> kv::Result<Vec<usize>> {
        let validation: Option<grid::CellValidation> = validation.conv();
        self.ch(|g, s, d| {
            g.call_set_validation(s, d, unit as u32, range.conv(), validation.as_ref())
        })
    }

    fn check_input(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        input: &str,
    ) -> Option<kv::ValidationError> {
        self.g(|g, s, d| g.call_check_input(s, d, unit as u32, row, col, input))
            .ok()
            .flatten()
            .map(Conv::conv)
    }

    fn invalid_cells(
        &mut self,
        unit: usize,
        rows: std::ops::Range<u32>,
        cols: std::ops::Range<u32>,
    ) -> Vec<(u32, u32)> {
        self.g(|g, s, d| {
            g.call_invalid_cells(
                s,
                d,
                unit as u32,
                (rows.start, rows.end),
                (cols.start, cols.end),
            )
        })
        .unwrap_or_default()
    }

    fn set_fill_lists(&mut self, lists: Vec<Vec<String>>) {
        let _ = self.g(|g, s, d| g.call_set_fill_lists(s, d, &lists));
    }

    fn fill(
        &mut self,
        unit: usize,
        source: [u32; 4],
        target: [u32; 4],
        series: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_fill(s, d, unit as u32, source.conv(), target.conv(), series))
    }

    fn sort_range(
        &mut self,
        unit: usize,
        range: [u32; 4],
        key: u32,
        descending: bool,
        header: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_sort_range(s, d, unit as u32, range.conv(), key, descending, header)
        })
    }

    fn set_filter(&mut self, unit: usize, range: Option<[u32; 4]>) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_filter(s, d, unit as u32, range.conv()))
    }

    fn filter_column(
        &mut self,
        unit: usize,
        col: u32,
        values: Option<Vec<String>>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_filter_column(s, d, unit as u32, col, values.as_deref()))
    }

    fn move_cells(
        &mut self,
        unit: usize,
        range: [u32; 4],
        row: u32,
        col: u32,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_move_cells(s, d, unit as u32, range.conv(), row, col))
    }

    fn move_cells_between(
        &mut self,
        from: usize,
        range: [u32; 4],
        to: usize,
        row: u32,
        col: u32,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_move_cells_between(s, d, from as u32, range.conv(), to as u32, row, col)
        })
    }

    fn clear_cells(&mut self, unit: usize, range: [u32; 4]) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_clear_cells(s, d, unit as u32, range.conv()))
    }

    fn merge_cells(
        &mut self,
        unit: usize,
        range: [u32; 4],
        center: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_merge_cells(s, d, unit as u32, range.conv(), center))
    }

    fn unmerge_cells(&mut self, unit: usize, row: u32, col: u32) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_unmerge_cells(s, d, unit as u32, row, col))
    }

    fn set_wrap(&mut self, unit: usize, row: u32, col: u32, wrap: bool) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_wrap(s, d, unit as u32, row, col, wrap))
    }

    fn set_row_height(&mut self, unit: usize, row: u32, height: f32) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_row_height(s, d, unit as u32, row, height))
    }

    fn set_col_width(&mut self, unit: usize, col: u32, width: f32) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_col_width(s, d, unit as u32, col, width))
    }

    fn enter_in_range(
        &mut self,
        unit: usize,
        range: [u32; 4],
        at: (u32, u32),
        input: &str,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_enter_in_range(s, d, unit as u32, range.conv(), at, input))
    }

    fn tables(&mut self, unit: usize) -> Vec<kv::TableInfo> {
        self.g(|g, s, d| g.call_tables(s, d, unit as u32))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn create_table(
        &mut self,
        unit: usize,
        range: [u32; 4],
        header: bool,
        style: &str,
    ) -> kv::Result<String> {
        self.g(|g, s, d| g.call_create_table(s, d, unit as u32, range.conv(), header, style))?
            .map_err(kv::ViewerError)
    }

    fn set_table_totals(&mut self, unit: usize, name: &str, on: bool) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_table_totals(s, d, unit as u32, name, on))
    }

    fn remove_table(&mut self, unit: usize, name: &str) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_remove_table(s, d, unit as u32, name))
    }

    fn sort_range_by(
        &mut self,
        unit: usize,
        range: [u32; 4],
        keys: &[kv::SortKey],
        header: bool,
    ) -> kv::Result<Vec<usize>> {
        let keys: Vec<grid::SortKey> = keys.to_vec().conv();
        self.ch(|g, s, d| g.call_sort_range_by(s, d, unit as u32, range.conv(), &keys, header))
    }

    fn filter_column_by(
        &mut self,
        unit: usize,
        col: u32,
        rule: Option<kv::FilterRule>,
    ) -> kv::Result<Vec<usize>> {
        let rule: Option<grid::FilterRule> = rule.conv();
        self.ch(|g, s, d| g.call_filter_column_by(s, d, unit as u32, col, rule.as_ref()))
    }

    fn column_filter(&mut self, unit: usize, col: u32) -> Option<kv::FilterRule> {
        self.g(|g, s, d| g.call_column_filter(s, d, unit as u32, col))
            .ok()
            .flatten()
            .map(Conv::conv)
    }

    fn reapply_filter(&mut self, unit: usize) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_reapply_filter(s, d, unit as u32))
    }

    fn outline(&mut self, unit: usize) -> kv::Outline {
        self.g(|g, s, d| g.call_outline(s, d, unit as u32))
            .unwrap_or_default()
    }

    fn set_outline(
        &mut self,
        unit: usize,
        rows: bool,
        from: u32,
        to: u32,
        deeper: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_outline(s, d, unit as u32, rows, from, to, deeper))
    }

    fn set_detail_shown(
        &mut self,
        unit: usize,
        rows: bool,
        at: u32,
        shown: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_detail_shown(s, d, unit as u32, rows, at, shown))
    }

    fn subtotal(
        &mut self,
        unit: usize,
        range: [u32; 4],
        by: u32,
        function: u32,
        columns: &[u32],
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_subtotal(s, d, unit as u32, range.conv(), by, function, columns))
    }

    fn begin_batch(&mut self) {
        let _ = self.g(|g, s, d| g.call_begin_batch(s, d));
    }

    fn end_batch(&mut self) {
        let _ = self.g(|g, s, d| g.call_end_batch(s, d));
    }

    fn conditional_ranges(&mut self, unit: usize) -> Vec<[u32; 4]> {
        self.g(|g, s, d| g.call_conditional_ranges(s, d, unit as u32))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn evaluate_formulas(&mut self, unit: usize, formulas: &[String]) -> Vec<Option<String>> {
        self.g(|g, s, d| g.call_evaluate_formulas(s, d, unit as u32, formulas))
            .unwrap_or_else(|_| vec![None; formulas.len()])
    }

    fn sheet_protection(&mut self, unit: usize) -> Option<kv::SheetProtection> {
        self.g(|g, s, d| g.call_sheet_protection(s, d, unit as u32))
            .ok()
            .flatten()
            .map(Conv::conv)
    }

    fn protect_sheet(
        &mut self,
        unit: usize,
        protection: Option<kv::SheetProtection>,
        password: Option<&str>,
    ) -> kv::Result<Vec<usize>> {
        let protection: Option<grid::Protection> = protection.conv();
        self.ch(|g, s, d| g.call_protect_sheet(s, d, unit as u32, protection, password))
    }

    fn workbook_protected(&mut self) -> bool {
        self.g(|g, s, d| g.call_workbook_protected(s, d))
            .unwrap_or(false)
    }

    fn protect_workbook(&mut self, on: bool, password: Option<&str>) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_protect_workbook(s, d, on, password))
    }

    fn page_setup(&mut self, unit: usize) -> Option<kv::PageSetup> {
        self.g(|g, s, d| g.call_page_setup(s, d, unit as u32))
            .ok()
            .flatten()
            .map(Conv::conv)
    }

    fn set_page_setup(&mut self, unit: usize, setup: &kv::PageSetup) -> kv::Result<Vec<usize>> {
        let setup: grid::PageLayout = setup.clone().conv();
        self.ch(|g, s, d| g.call_set_page_setup(s, d, unit as u32, &setup))
    }

    fn drawings(&mut self, unit: usize) -> Vec<kv::Drawing> {
        self.g(|g, s, d| g.call_drawings(s, d, unit as u32))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn drawing_image(&mut self, unit: usize, index: usize) -> Option<Vec<u8>> {
        self.g(|g, s, d| g.call_drawing_image(s, d, unit as u32, index as u32))
            .ok()
            .flatten()
    }

    fn insert_picture(
        &mut self,
        unit: usize,
        anchor: [u32; 4],
        bytes: &[u8],
        extension: &str,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_insert_picture(s, d, unit as u32, anchor.conv(), bytes, extension))
    }

    fn insert_shape(
        &mut self,
        unit: usize,
        anchor: [u32; 4],
        preset: &str,
        text: &str,
        text_box: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_insert_shape(s, d, unit as u32, anchor.conv(), preset, text, text_box)
        })
    }

    fn move_drawing(
        &mut self,
        unit: usize,
        index: usize,
        anchor: [u32; 4],
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_move_drawing(s, d, unit as u32, index as u32, anchor.conv()))
    }

    fn set_shape_text(&mut self, unit: usize, index: usize, text: &str) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_shape_text(s, d, unit as u32, index as u32, text))
    }

    fn delete_drawing(&mut self, unit: usize, index: usize) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_delete_drawing(s, d, unit as u32, index as u32))
    }

    fn add_sparklines(
        &mut self,
        unit: usize,
        data: [u32; 4],
        location: [u32; 4],
        kind: kv::SparklineKind,
        mark: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_add_sparklines(
                s,
                d,
                unit as u32,
                data.conv(),
                location.conv(),
                kind.conv(),
                mark,
            )
        })
    }

    fn clear_sparklines(&mut self, unit: usize, range: [u32; 4]) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_clear_sparklines(s, d, unit as u32, range.conv()))
    }

    fn goal_seek(
        &mut self,
        unit: usize,
        set: (u32, u32),
        target: f64,
        by: (u32, u32),
    ) -> kv::Result<Option<f64>> {
        self.g(|g, s, d| g.call_goal_seek(s, d, unit as u32, set, target, by))?
            .map_err(kv::ViewerError)
    }

    fn create_data_table(
        &mut self,
        unit: usize,
        range: [u32; 4],
        row_input: Option<(u32, u32)>,
        col_input: Option<(u32, u32)>,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_create_data_table(s, d, unit as u32, range.conv(), row_input, col_input)
        })
    }

    fn scenarios(&mut self, unit: usize) -> Vec<kv::Scenario> {
        self.g(|g, s, d| g.call_scenarios(s, d, unit as u32))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn add_scenario(
        &mut self,
        unit: usize,
        name: &str,
        cells: &[(u32, u32)],
        comment: &str,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_add_scenario(s, d, unit as u32, name, cells, comment))
    }

    fn show_scenario(&mut self, unit: usize, name: &str) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_show_scenario(s, d, unit as u32, name))
    }

    fn delete_scenario(&mut self, unit: usize, name: &str) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_delete_scenario(s, d, unit as u32, name))
    }

    fn calc_options(&mut self) -> kv::CalcOptions {
        self.g(|g, s, d| g.call_calc_options(s, d))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn set_calc_options(&mut self, options: kv::CalcOptions) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_calc_options(s, d, options.conv()))
    }

    fn circular_references(&mut self) -> Vec<(usize, u32, u32)> {
        self.g(|g, s, d| g.call_circular_references(s, d))
            .map(|v| v.into_iter().map(|(u, r, c)| (u as usize, r, c)).collect())
            .unwrap_or_default()
    }

    fn sheet_view(&mut self, unit: usize) -> kv::SheetView {
        self.g(|g, s, d| g.call_sheet_view(s, d, unit as u32))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn set_sheet_view(&mut self, unit: usize, view: kv::SheetView) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_sheet_view(s, d, unit as u32, view.conv()))
    }

    fn cell_styles(&mut self) -> Vec<String> {
        self.g(|g, s, d| g.call_cell_styles(s, d))
            .unwrap_or_default()
    }

    fn apply_cell_style(
        &mut self,
        unit: usize,
        range: [u32; 4],
        name: &str,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_apply_cell_style(s, d, unit as u32, range.conv(), name))
    }

    fn new_cell_style(
        &mut self,
        name: &str,
        unit: usize,
        row: u32,
        col: u32,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_new_cell_style(s, d, name, unit as u32, row, col))
    }

    fn theme_name(&mut self) -> Option<String> {
        self.g(|g, s, d| g.call_theme_name(s, d)).ok().flatten()
    }

    fn theme_names(&mut self) -> Vec<String> {
        self.g(|g, s, d| g.call_theme_names(s, d))
            .unwrap_or_default()
    }

    fn set_theme(&mut self, name: &str) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_theme(s, d, name))
    }

    fn tab_color(&mut self, unit: usize) -> Option<[u8; 3]> {
        self.g(|g, s, d| g.call_tab_color(s, d, unit as u32))
            .ok()
            .flatten()
            .map(Conv::conv)
    }

    fn set_tab_color(&mut self, unit: usize, color: Option<[u8; 3]>) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_set_tab_color(s, d, unit as u32, color.map(Conv::conv)))
    }

    fn threads(&mut self, unit: usize) -> Vec<kv::CommentThread> {
        self.g(|g, s, d| g.call_threads(s, d, unit as u32))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    fn add_thread_comment(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        author: &str,
        text: &str,
        time: &str,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| {
            g.call_add_thread_comment(s, d, unit as u32, row, col, author, text, time)
        })
    }

    fn resolve_thread(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        done: bool,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_resolve_thread(s, d, unit as u32, row, col, done))
    }

    fn delete_thread_comment(
        &mut self,
        unit: usize,
        row: u32,
        col: u32,
        index: usize,
    ) -> kv::Result<Vec<usize>> {
        self.ch(|g, s, d| g.call_delete_thread_comment(s, d, unit as u32, row, col, index as u32))
    }

    fn has_history(&self) -> bool {
        self.g(|g, s, d| g.call_has_history(s, d)).unwrap_or(false)
    }

    fn undo(&mut self) -> kv::Result<bool> {
        self.g(|g, s, d| g.call_undo(s, d))?
            .map_err(kv::ViewerError)
    }

    fn redo(&mut self) -> kv::Result<bool> {
        self.g(|g, s, d| g.call_redo(s, d))?
            .map_err(kv::ViewerError)
    }

    fn macros(&mut self) -> Vec<kv::MacroEntry> {
        self.g(|g, s, d| g.call_macros(s, d))
            .map(Conv::conv)
            .unwrap_or_default()
    }

    /// Runs the macro with the answers given so far; a question the
    /// plugin stopped at is put to `ui`, and answered, the macro runs again
    /// from the start with it (the contract's replay, D54).
    fn run_macro(&mut self, name: &str, ui: &mut dyn kv::MacroUi) -> kv::Result<kv::MacroOutcome> {
        let mut answers: Vec<grid::MacroAnswer> = Vec::new();
        loop {
            let out: kv::MacroOutcome = self
                .g(|g, s, d| g.call_run_macro(s, d, name, &answers))?
                .map_err(kv::ViewerError)?
                .conv();
            let answer = match &out.question {
                None => return Ok(out),
                Some(kv::MacroQuestion::Message {
                    prompt,
                    buttons,
                    title,
                }) => ui
                    .message(prompt, *buttons, title)
                    .map(grid::MacroAnswer::Message),
                Some(kv::MacroQuestion::Input {
                    prompt,
                    title,
                    default,
                }) => ui
                    .input(prompt, title, default)
                    .map(grid::MacroAnswer::Input),
            };
            match answer {
                Some(a) => answers.push(a),
                // `ui` cannot answer now: the question goes to the host.
                None => return Ok(out),
            }
        }
    }
}

impl ComponentDocument {
    /// Calls the plugin's grid function `f`; an error when the plugin has
    /// no grid or failed.
    fn g<R>(
        &self,
        f: impl FnOnce(
            &grid::Guest,
            &mut wasmtime::Store<crate::Data<Files>>,
            Document,
        ) -> wasmtime::Result<R>,
    ) -> kv::Result<R> {
        let doc = self.doc;
        self.run(|v| v.grid(|g, s| f(g, s, doc)))
            .unwrap_or_else(|| Err(kv::ViewerError("Not a grid".into())))
    }

    /// [`ComponentDocument::g`] for a change: the units to draw again.
    fn ch(
        &self,
        f: impl FnOnce(
            &grid::Guest,
            &mut wasmtime::Store<crate::Data<Files>>,
            Document,
        ) -> wasmtime::Result<Result<Vec<u32>, String>>,
    ) -> kv::Result<Vec<usize>> {
        grid_conv::unchanged(self.g(f)?)
    }
}
