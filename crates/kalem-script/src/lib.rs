//! The WebAssembly host of Kalem's plugins (D28, T3.1.1).
//!
//! A plugin is a WebAssembly component on a WIT API. The engine is
//! wasmtime with Cranelift, chosen on the spike of T3.1.0
//! (`book/part-5/decisions/D28-plugin-abi.org`):
//!
//! - a component compiles once and is cached in the state directory, keyed
//!   by its bytes and the engine's compatibility, so a plugin's later
//!   starts load it in a fraction of a millisecond;
//! - a plugin reaches only what the host grants: its imports are resolved
//!   against a [`Linker`] that starts empty, and one importing anything
//!   else is refused, naming what it asked for;
//! - each instance has a memory limit, and each call a time budget counted
//!   by epochs (a tick every [`TICK`]), so a plugin that loops or grows
//!   is stopped and the editor goes on;
//! - a loaded [`Plugin`] is shared by threads, each instantiating it in its
//!   own store: parsers, renderers and completers run in parallel.
//!
//! The API a plugin implements is WIT (T3.1.3); this crate is the engine
//! underneath, generic over it.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};
use wasmtime::component::{Component, ComponentNamedList, Lift, Lower, TypedFunc};
use wasmtime::{Config, Engine, ResourceLimiter, Store, Trap};

pub use wasmtime::component::Linker;

pub mod viewer;

/// How often the time budget's clock ticks.
pub const TICK: Duration = Duration::from_millis(10);

/// What an instance may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Its memories together, in bytes.
    pub memory: usize,
    /// A call's time.
    pub time: Duration,
}

impl Default for Limits {
    /// 64 MB and 100 ms, the budget of §11's synchronous calls.
    fn default() -> Limits {
        Limits {
            memory: 64 << 20,
            time: Duration::from_millis(100),
        }
    }
}

/// Why a plugin could not be loaded, instantiated or called.
#[derive(Debug)]
pub enum Error {
    /// The bytes are not a component the engine compiles.
    Invalid(String),
    /// The component imports what the host did not grant.
    NotGranted(Vec<String>),
    /// A call ran past its time budget and was stopped.
    Timeout(Duration),
    /// The plugin asked for more memory than its limit.
    Memory(usize),
    /// The plugin trapped (a panic, an out-of-bounds access).
    Trap(String),
    /// Reading or writing the cache or the component's file.
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Invalid(e) => write!(f, "not a plugin component: {e}"),
            Error::NotGranted(names) => {
                write!(
                    f,
                    "the plugin asks for what it was not granted: {}",
                    names.join(", ")
                )
            }
            Error::Timeout(t) => write!(f, "the plugin ran past its {} ms", t.as_millis()),
            Error::Memory(m) => write!(f, "the plugin asked for more than its {} MB", m >> 20),
            Error::Trap(e) => write!(f, "the plugin failed: {e}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Io(e)
    }
}

/// The result of the host's functions.
pub type Result<T> = std::result::Result<T, Error>;

/// The engine, its clock, and where compiled components are kept.
pub struct Host {
    engine: Engine,
    cache: Option<PathBuf>,
    /// Stops the clock's thread when the host goes.
    running: Arc<AtomicBool>,
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

impl Host {
    /// A host keeping compiled components in `cache` (a folder of the
    /// state directory), or compiling them each time with `None`.
    pub fn new(cache: Option<PathBuf>) -> Result<Host> {
        let mut config = Config::new();
        config.epoch_interruption(true);
        let engine = Engine::new(&config).map_err(|e| Error::Invalid(e.to_string()))?;
        let running = Arc::new(AtomicBool::new(true));
        let (clock, on) = (engine.clone(), running.clone());
        std::thread::Builder::new()
            .name("kalem-script-clock".into())
            .spawn(move || {
                while on.load(Ordering::Relaxed) {
                    std::thread::sleep(TICK);
                    clock.increment_epoch();
                }
            })?;
        Ok(Host {
            engine,
            cache,
            running,
        })
    }

    /// The engine, for building a [`Linker`] of granted interfaces.
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// A linker granting nothing: the host adds what a plugin may use.
    pub fn linker<T: 'static>(&self) -> Linker<Data<T>> {
        Linker::new(&self.engine)
    }

    /// Loads the component in the file at `path`.
    pub fn load_file(&self, path: &Path) -> Result<Plugin> {
        self.load(&std::fs::read(path)?)
    }

    /// Loads a component: from the cache when it was compiled before by
    /// this engine with these settings, else compiled and cached.
    pub fn load(&self, bytes: &[u8]) -> Result<Plugin> {
        let Some(dir) = &self.cache else {
            return Ok(Plugin::new(self.compile(bytes)?, false));
        };
        let file = dir.join(self.cache_name(bytes));
        if file.is_file()
            && let Ok(component) = self.load_compiled(&file)
        {
            return Ok(Plugin::new(component, true));
        }
        let component = self.compile(bytes)?;
        // A failed write only costs the next start a compile.
        if let Ok(compiled) = component.serialize() {
            let _ = std::fs::create_dir_all(dir);
            let tmp = file.with_extension("tmp");
            if std::fs::write(&tmp, compiled).is_ok() {
                let _ = std::fs::rename(&tmp, &file);
            }
        }
        Ok(Plugin::new(component, false))
    }

    fn compile(&self, bytes: &[u8]) -> Result<Component> {
        Component::new(&self.engine, bytes).map_err(|e| Error::Invalid(format!("{e:#}")))
    }

    /// The cache's file name for `bytes`: their hash and the engine's
    /// compatibility (its version and settings), so a file compiled by
    /// another engine is never loaded.
    fn cache_name(&self, bytes: &[u8]) -> String {
        use std::hash::{Hash, Hasher};
        let mut compat = std::collections::hash_map::DefaultHasher::new();
        self.engine
            .precompile_compatibility_hash()
            .hash(&mut compat);
        let digest = Sha256::digest(bytes);
        let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
        format!("{hex}-{:016x}.cwasm", compat.finish())
    }

    /// A component compiled before, from the cache.
    #[allow(unsafe_code)]
    fn load_compiled(&self, file: &Path) -> Result<Component> {
        // SAFETY: wasmtime requires the file to be what its `serialize`
        // wrote, else the machine code in it runs as it is. The file is in
        // Kalem's own state directory, written by `load` above from this
        // engine's compile and named by the component's hash and this
        // engine's compatibility hash; wasmtime checks the version and
        // settings in its header again. A process able to write there can
        // change Kalem's settings and plugins as well.
        unsafe { Component::deserialize_file(&self.engine, file) }
            .map_err(|e| Error::Invalid(e.to_string()))
    }
}

/// A loaded component, shared by the threads that instantiate it.
#[derive(Clone)]
pub struct Plugin {
    component: Component,
    cached: bool,
}

impl fmt::Debug for Plugin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Plugin")
            .field("cached", &self.cached)
            .finish_non_exhaustive()
    }
}

impl Plugin {
    fn new(component: Component, cached: bool) -> Plugin {
        Plugin { component, cached }
    }

    /// Whether it came from the cache rather than the compiler.
    pub fn cached(&self) -> bool {
        self.cached
    }

    /// The names it imports: what it asks the host for.
    pub fn imports(&self, host: &Host) -> Vec<String> {
        self.component
            .component_type()
            .imports(&host.engine)
            .map(|(name, _)| name.to_string())
            .collect()
    }

    /// The names it exports.
    pub fn exports(&self, host: &Host) -> Vec<String> {
        self.component
            .component_type()
            .exports(&host.engine)
            .map(|(name, _)| name.to_string())
            .collect()
    }

    /// Instantiates it with `linker`'s grants, `data` for the host's
    /// functions and `limits`. Refused, naming them, when it imports what
    /// the linker does not grant.
    pub fn instantiate<T: Send + 'static>(
        &self,
        host: &Host,
        linker: &Linker<Data<T>>,
        data: T,
        limits: Limits,
    ) -> Result<Instance<T>> {
        let pre = linker.instantiate_pre(&self.component).map_err(|e| {
            match not_granted(&e, &self.imports(host)) {
                Some(name) => Error::NotGranted(vec![name]),
                None => Error::Invalid(format!("{e:#}")),
            }
        })?;
        let mut store = Store::new(
            &host.engine,
            Data {
                user: data,
                limiter: Limiter {
                    memory: limits.memory,
                    refused: false,
                },
            },
        );
        store.limiter(|d| &mut d.limiter);
        store.epoch_deadline_trap();
        store.set_epoch_deadline(ticks(limits.time));
        let instance = pre
            .instantiate(&mut store)
            .map_err(|e| classify(e, &store, limits))?;
        Ok(Instance {
            store,
            instance,
            limits,
        })
    }
}

/// The import the linker lacks, from wasmtime's refusal: "component
/// imports … `NAME`, but a matching implementation was not found in the
/// linker" names the first one missing (or granted with another type),
/// checked against the component's own imports.
fn not_granted(e: &wasmtime::Error, imports: &[String]) -> Option<String> {
    e.chain().find_map(|cause| {
        let text = cause.to_string();
        if !text.contains("a matching implementation was not found") {
            return None;
        }
        let (_, rest) = text.split_once('`')?;
        let (name, _) = rest.split_once('`')?;
        imports.iter().find(|i| *i == name).cloned()
    })
}

/// The ticks of `time`, one more so that a call is never cut short.
fn ticks(time: Duration) -> u64 {
    (time.as_millis() / TICK.as_millis()).max(1) as u64 + 1
}

/// What an instance's store holds: the host's data and the limiter.
#[derive(Debug)]
pub struct Data<T> {
    /// The host's data, for the functions it grants.
    pub user: T,
    limiter: Limiter,
}

/// The memory limit, remembering that it refused, to tell a plugin out of
/// memory from one that failed otherwise.
#[derive(Debug)]
struct Limiter {
    memory: usize,
    refused: bool,
}

impl ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if desired > self.memory {
            self.refused = true;
            return Ok(false);
        }
        Ok(true)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        Ok(desired <= 100_000)
    }
}

/// A trap told apart: the time budget, the memory limit, or the plugin's
/// own failure.
fn classify<T>(e: wasmtime::Error, store: &Store<Data<T>>, limits: Limits) -> Error {
    if e.downcast_ref::<Trap>() == Some(&Trap::Interrupt) {
        return Error::Timeout(limits.time);
    }
    if store.data().limiter.refused {
        return Error::Memory(limits.memory);
    }
    Error::Trap(format!("{e:#}"))
}

/// A plugin instantiated in its own store.
pub struct Instance<T: 'static> {
    store: Store<Data<T>>,
    instance: wasmtime::component::Instance,
    limits: Limits,
}

impl<T: 'static> fmt::Debug for Instance<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Instance")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl<T: Send + 'static> Instance<T> {
    /// Calls the exported function `name` with `params`, within the time
    /// budget. After a timeout, a memory refusal or a trap the instance is
    /// spent: the caller drops it and instantiates the plugin again.
    pub fn call<P, R>(&mut self, name: &str, params: P) -> Result<R>
    where
        P: ComponentNamedList + Lower + Send + Sync,
        R: ComponentNamedList + Lift + Send + Sync,
    {
        let f: TypedFunc<P, R> = self
            .instance
            .get_typed_func(&mut self.store, name)
            .map_err(|e| Error::Invalid(format!("{name}: {e:#}")))?;
        self.store.set_epoch_deadline(ticks(self.limits.time));
        let out = f.call(&mut self.store, params);
        out.map_err(|e| classify(e, &self.store, self.limits))
    }

    /// Typed bindings of the instance (a world's `bindgen!` exports),
    /// made from the instance as instantiated.
    pub fn bindings<B>(
        &mut self,
        make: impl FnOnce(&mut Store<Data<T>>, &wasmtime::component::Instance) -> wasmtime::Result<B>,
    ) -> Result<B> {
        make(&mut self.store, &self.instance).map_err(|e| Error::Invalid(format!("{e:#}")))
    }

    /// Runs `f` on the store, a call of typed bindings, within the time
    /// budget, its failures told apart as [`Instance::call`]'s are.
    pub fn run<R>(
        &mut self,
        f: impl FnOnce(&mut Store<Data<T>>) -> wasmtime::Result<R>,
    ) -> Result<R> {
        self.store.set_epoch_deadline(ticks(self.limits.time));
        f(&mut self.store).map_err(|e| classify(e, &self.store, self.limits))
    }

    /// The host's data.
    pub fn data(&self) -> &T {
        &self.store.data().user
    }

    /// The host's data, to change.
    pub fn data_mut(&mut self) -> &mut T {
        &mut self.store.data_mut().user
    }
}
