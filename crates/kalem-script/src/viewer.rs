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
