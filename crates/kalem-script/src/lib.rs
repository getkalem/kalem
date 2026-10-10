//! The WebAssembly host of Kalem's plugins (D28, T3.1.1).
//!
//! A plugin is a WebAssembly component on a WIT API. The engine is
//! wasmtime with Cranelift, chosen on the spike of T3.1.0
//! (`book/part-4/decisions/D28-plugin-abi.org`):
//!
//! - a component compiles once and is cached in the state directory, keyed
//!   by its bytes and the engine's compatibility, so a plugin's later
//!   starts load it in a fraction of a millisecond; files not used for
//!   a month go, and the folder is kept under [`CACHE_CAP`];
//! - a plugin reaches only what the host grants: its imports are resolved
//!   against a [`Linker`] that starts empty, and one importing anything
//!   else is refused, naming what it asked for;
//! - each instance has a memory limit, and each call a time budget counted
//!   by epochs (a tick every [`TICK`] while a call runs), so a plugin that
//!   loops or grows is stopped and the editor goes on;
//! - a loaded [`Plugin`] is shared by threads, each instantiating it in its
//!   own store: parsers, renderers and completers run in parallel.
//!
//! The API a plugin implements is WIT (T3.1.3); this crate is the engine
//! underneath, generic over it.

/// The version of the plugin API this host implements: the WIT package
/// `kalem:plugin` of `kalem-plugin/wit`. A released interface never
/// changes; a later version adds interfaces, so a component built against
/// an earlier `0.2.x` binds what it has (the Book, Part III, "Versions of
/// the plugin API"). 0.2.1 added the `diagnostics` import, 0.2.2 the
/// `password` export, 0.2.3 the `formats` export, 0.2.4 the `process`
/// import and the `on-process` export of the `extension` world, 0.2.5
/// the `documents` and `decorations` imports, 0.2.6 the
/// `styled-documents` import.
pub const API_VERSION: &str = "0.2.8";

/// Whether a manifest's `api` requirement (`^0.2`, `0.2`, `^0.2.1`)
/// names this host's API: the same `0.MINOR` before 1.0 (the same major
/// after), and no earlier than the version it names, as a component built
/// against a later one may import what this host lacks. A manifest
/// without one is tried.
pub fn api_compatible(requirement: Option<&str>) -> bool {
    let Some(req) = requirement.map(str::trim).filter(|r| !r.is_empty()) else {
        return true;
    };
    let nums = |v: &str| -> Vec<u64> {
        v.trim_start_matches(['^', '~', '='])
            .split('.')
            .map_while(|p| p.trim().parse().ok())
            .collect()
    };
    let (want, have) = (nums(req), nums(API_VERSION));
    let same = match (want.first(), want.get(1)) {
        (Some(0), Some(minor)) => have.first() == Some(&0) && have.get(1) == Some(minor),
        (Some(0), None) => have.first() == Some(&0),
        (Some(major), _) => have.first() == Some(major),
        (None, _) => true,
    };
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    same && (0..3)
        .map(|i| at(&have, i))
        .ge((0..3).map(|i| at(&want, i)))
}

use std::fmt;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, SystemTime};

use sha2::{Digest, Sha256};
use wasmtime::component::{Component, ComponentNamedList, Lift, Lower, TypedFunc};
use wasmtime::{Config, Engine, ResourceLimiter, Store, Trap};

pub use wasmtime::component::Linker;

pub mod extension;
pub mod viewer;

/// How often the time budget's clock ticks while a call runs.
pub const TICK: Duration = Duration::from_millis(10);

/// The most the cache of compiled components holds, in bytes: past it the
/// files used longest ago go first. A compiled component is 5 to 15 MB.
pub const CACHE_CAP: u64 = 200 << 20;

/// How long a compiled component no host used stays in the cache: one
/// built for another plugin version or engine is never used again.
const UNUSED: Duration = Duration::from_secs(30 * 86_400);

/// How long a `.tmp` file, a write to the cache that never finished, stays.
const UNFINISHED: Duration = Duration::from_secs(86_400);

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

/// The engine and where compiled components are kept.
pub struct Host {
    engine: &'static Engine,
    cache: Option<PathBuf>,
}

/// The engine every host shares: wasmtime's advice is one per process, and
/// one engine needs one clock.
fn engine() -> Result<&'static Engine> {
    static ENGINE: OnceLock<std::result::Result<Engine, String>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let mut config = Config::new();
            config.epoch_interruption(true);
            Engine::new(&config).map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| Error::Invalid(e.clone()))
}

/// The time budget's clock: one thread ticking the engine's epochs while a
/// call runs, parked while none does, so an idle editor is not woken a
/// hundred times a second. It lives as long as the process, so an
/// instance kept after its host is still stopped when it loops.
struct Clock {
    state: Mutex<Ticking>,
    wake: Condvar,
}

/// What the clock knows.
struct Ticking {
    /// Calls in flight, instantiations among them (a component's start
    /// runs its code).
    calls: usize,
    /// Whether its thread was started.
    started: bool,
    /// How many times it ticked: the tests see it rest.
    ticks: u64,
    /// How many calls began: the tests tell their own calls from the
    /// others' of the process.
    begun: u64,
}

static CLOCK: Clock = Clock {
    state: Mutex::new(Ticking {
        calls: 0,
        started: false,
        ticks: 0,
        begun: 0,
    }),
    wake: Condvar::new(),
};

impl Clock {
    fn state(&self) -> MutexGuard<'_, Ticking> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts the thread ticking `engine`, once; a host is refused when it
    /// cannot, as its calls would have no time budget.
    fn start(&'static self, engine: &'static Engine) -> std::io::Result<()> {
        let mut s = self.state();
        if !s.started {
            std::thread::Builder::new()
                .name("kalem-script-clock".into())
                .spawn(move || self.run(engine))?;
            s.started = true;
        }
        Ok(())
    }

    /// The thread: a tick every [`TICK`] while a call is in flight.
    fn run(&self, engine: &Engine) {
        let mut s = self.state();
        loop {
            s = self
                .wake
                .wait_while(s, |s| s.calls == 0)
                .unwrap_or_else(PoisonError::into_inner);
            drop(s);
            std::thread::sleep(TICK);
            engine.increment_epoch();
            s = self.state();
            s.ticks += 1;
        }
    }

    /// A call starting: the clock ticks until the guard is dropped.
    fn call(&'static self) -> Call {
        let mut s = self.state();
        s.calls += 1;
        s.begun += 1;
        if s.calls == 1 {
            self.wake.notify_one();
        }
        Call(self)
    }
}

/// A call in flight, from [`Clock::call`].
struct Call(&'static Clock);

impl Drop for Call {
    fn drop(&mut self) {
        let mut s = self.0.state();
        s.calls = s.calls.saturating_sub(1);
    }
}

impl fmt::Debug for Host {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Host")
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

impl Host {
    /// A host keeping compiled components in `cache` (a folder of the
    /// state directory), or compiling them each time with `None`. The
    /// cache is pruned on a thread of its own.
    pub fn new(cache: Option<PathBuf>) -> Result<Host> {
        let engine = engine()?;
        CLOCK.start(engine)?;
        if let Some(dir) = cache.clone() {
            // A failed spawn only leaves the pruning to the next write.
            let _ = std::thread::Builder::new()
                .name("kalem-script-prune".into())
                .spawn(move || prune(&dir, None, CACHE_CAP));
        }
        Ok(Host { engine, cache })
    }

    /// The engine, for building a [`Linker`] of granted interfaces.
    pub fn engine(&self) -> &Engine {
        self.engine
    }

    /// A linker granting nothing: the host adds what a plugin may use.
    pub fn linker<T: 'static>(&self) -> Linker<Data<T>> {
        Linker::new(self.engine)
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
            touch(&file);
            return Ok(Plugin::new(component, true));
        }
        let component = self.compile(bytes)?;
        // A failed write only costs the next start a compile.
        if let Ok(compiled) = component.serialize() {
            let _ = std::fs::create_dir_all(dir);
            // A name of the process's own: two processes compiling the
            // same component (two Kalems, or tests run each in a process)
            // never write into one file.
            let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
            if std::fs::write(&tmp, compiled).is_ok() {
                let _ = std::fs::rename(&tmp, &file);
            } else {
                // Half written on a full disk: not left to fill it more.
                let _ = std::fs::remove_file(&tmp);
            }
            // `kalem plugin dev` writes one per build: pruned as written.
            prune(dir, Some(&file), CACHE_CAP);
        }
        Ok(Plugin::new(component, false))
    }

    fn compile(&self, bytes: &[u8]) -> Result<Component> {
        Component::new(self.engine, bytes).map_err(|e| Error::Invalid(format!("{e:#}")))
    }

    /// The cache's file name for `bytes`: their hash and the engine's
    /// compatibility (its version and settings), so a file compiled by
    /// another engine is never loaded. Both hashes are the same from one
    /// toolchain to the next (`DefaultHasher` is not), so that a new Rust
    /// does not orphan every file.
    fn cache_name(&self, bytes: &[u8]) -> String {
        let mut compat = Fnv::default();
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
        //
        // Read into memory rather than mapped (`deserialize_file`): a
        // mapped file that something rewrites in place while a component
        // made from it is alive ends the process with SIGBUS; the bytes in
        // memory cannot change under the component.
        let bytes = std::fs::read(file).map_err(|e| Error::Invalid(e.to_string()))?;
        unsafe { Component::deserialize(self.engine, &bytes) }
            .map_err(|e| Error::Invalid(e.to_string()))
    }
}

/// FNV-1a, a hash that is the same from one build to the next.
struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Fnv {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fnv {
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3);
        }
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// Marks a cached `file` used now: its modification time is when it was
/// last used, which the pruning goes by.
fn touch(file: &Path) {
    let _ = std::fs::File::options()
        .append(true)
        .open(file)
        .and_then(|f| f.set_modified(SystemTime::now()));
}

/// Clears the cache folder `dir`: `.tmp` files older than [`UNFINISHED`],
/// compiled components not used for [`UNUSED`], then those used longest
/// ago while the folder holds more than `cap` bytes. `keep`, the file just
/// written, stays, and so does one a host used while this ran. A failed
/// removal only costs the disk its space; a component in use is in memory,
/// not mapped, so its file may go.
fn prune(dir: &Path, keep: Option<&Path>, cap: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    let mut files = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = e.metadata() else {
            continue;
        };
        let Ok(used) = meta.modified() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let age = now.duration_since(used).unwrap_or_default();
        let kept = keep == Some(path.as_path());
        match path.extension().and_then(|x| x.to_str()) {
            Some("tmp") if age > UNFINISHED => {
                remove_unused(&path, used);
            }
            Some("cwasm") if age > UNUSED && !kept && remove_unused(&path, used) => {}
            Some("cwasm") => files.push((used, meta.len(), path)),
            _ => {}
        }
    }
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (used, len, path) in files {
        if total <= cap {
            break;
        }
        if keep != Some(path.as_path()) && remove_unused(&path, used) {
            total -= len;
        }
    }
}

/// Removes `path` unless it was used after `used`, when it was listed: a
/// host may have just loaded it.
fn remove_unused(path: &Path, used: SystemTime) -> bool {
    let unchanged = std::fs::metadata(path).and_then(|m| m.modified()).ok() == Some(used);
    unchanged && std::fs::remove_file(path).is_ok()
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
            .imports(host.engine)
            .map(|(name, _)| name.to_string())
            .collect()
    }

    /// The names it exports.
    pub fn exports(&self, host: &Host) -> Vec<String> {
        self.component
            .component_type()
            .exports(host.engine)
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
            host.engine,
            Data {
                user: data,
                limiter: Limiter {
                    memory: limits.memory,
                    refused: false,
                },
                diagnostics: Diagnostics::default(),
            },
        );
        store.limiter(|d| &mut d.limiter);
        store.epoch_deadline_trap();
        let _call = CLOCK.call();
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
    pub(crate) diagnostics: Diagnostics,
}

/// What a plugin said of itself going wrong (the `diagnostics` import):
/// the message of its panic, which the trap that follows lacks.
#[derive(Debug, Default)]
pub struct Diagnostics {
    panicked: Option<String>,
}

impl Diagnostics {
    /// The plugin panicked with `message`: kept for the trap's error, and
    /// logged.
    fn panicked(&mut self, message: String) {
        tracing::error!(%message, "a plugin panicked");
        self.panicked = Some(message);
    }
}

impl viewer::kalem::plugin::diagnostics::Host for Diagnostics {
    fn panicked(&mut self, message: String) {
        Diagnostics::panicked(self, message);
    }
}

impl extension::diagnostics::Host for Diagnostics {
    fn panicked(&mut self, message: String) {
        Diagnostics::panicked(self, message);
    }
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
    let trace = demangled(&format!("{e:#}"));
    Error::Trap(match &store.data().diagnostics.panicked {
        Some(message) => format!("{message}\n{trace}"),
        None => trace,
    })
}

/// A trap's backtrace with the Rust functions' names readable
/// (`<std::time::SystemTime>::now` for `_RNvMs5_NtCs…3now`), when the
/// component kept them (wasm_todo W10).
fn demangled(trace: &str) -> String {
    trace
        .lines()
        .map(|line| {
            let Some(at) = line.find(".wasm!").map(|i| i + ".wasm!".len()) else {
                return line.to_string();
            };
            let rest = &line[at..];
            let end = rest.find(": ").unwrap_or(rest.len());
            match rustc_demangle::try_demangle(&rest[..end]) {
                Ok(d) => format!("{}{d:#}{}", &line[..at], &rest[end..]),
                Err(_) => line.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
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
        let _call = CLOCK.call();
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
        let _call = CLOCK.call();
        self.store.set_epoch_deadline(ticks(self.limits.time));
        self.store.data_mut().diagnostics.panicked = None;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_traps_rust_functions_read() {
        let trace = "error while executing at wasm backtrace:\n    \
             8: 0x45cd78 - kalem_plugin_xlsx.wasm!_RNvMs5_NtCs4BkbFkxXoBt_3std4timeNtB5_10SystemTime3now\n    \
             13: 0x25583f - kalem_plugin_xlsx.wasm!kalem:plugin/grid@0.2.0#protect-sheet: wasm trap: unreachable";
        let read = super::demangled(trace);
        assert!(
            read.contains("kalem_plugin_xlsx.wasm!<std::time::SystemTime>::now\n"),
            "{read}"
        );
        assert!(
            read.ends_with("#protect-sheet: wasm trap: unreachable"),
            "{read}"
        );
    }

    #[test]
    fn api_requirements() {
        assert!(super::api_compatible(Some("^0.2")));
        assert!(super::api_compatible(Some("0.2.1")));
        assert!(super::api_compatible(None));
        assert!(!super::api_compatible(Some("^0.1")));
        assert!(!super::api_compatible(Some("^1.0")));
        // Built against a later 0.2.x: it may import what this host lacks.
        assert!(!super::api_compatible(Some("^0.2.9")));
    }

    /// An empty folder of the test's own.
    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kalem-script-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A file of `len` bytes in `dir`, used last `days` ago.
    fn used(dir: &Path, name: &str, len: usize, days: u64) -> PathBuf {
        let f = dir.join(name);
        std::fs::write(&f, vec![0; len]).unwrap();
        let at = SystemTime::now() - Duration::from_secs(days * 86_400);
        std::fs::File::options()
            .append(true)
            .open(&f)
            .unwrap()
            .set_modified(at)
            .unwrap();
        f
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn the_cache_forgets_what_is_not_used() {
        let dir = folder("prune-age");
        used(&dir, "month.cwasm", 10, 31);
        used(&dir, "weeks.cwasm", 10, 29);
        used(&dir, "unfinished.tmp", 10, 2);
        used(&dir, "writing.tmp", 10, 0);
        used(&dir, "other.txt", 10, 365);
        let just = used(&dir, "just-written.cwasm", 10, 40);
        prune(&dir, Some(&just), CACHE_CAP);
        assert_eq!(
            names(&dir),
            [
                "just-written.cwasm",
                "other.txt",
                "weeks.cwasm",
                "writing.tmp"
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_cache_keeps_under_its_cap_the_files_used_last() {
        let dir = folder("prune-cap");
        for (name, days) in [("a", 5), ("b", 1), ("c", 3), ("d", 2), ("e", 4)] {
            used(&dir, &format!("{name}.cwasm"), 100, days);
        }
        // The file just written stays, used longest ago as its time says.
        prune(&dir, Some(&dir.join("a.cwasm")), 250);
        // 500 bytes: e, c and d go, used before b; 200 are left.
        assert_eq!(names(&dir), ["a.cwasm", "b.cwasm"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_clock_rests_between_calls() {
        const SPIN: &str = r#"
            (component
              (core module $m (func (export "spin") (loop $l br $l)))
              (core instance $i (instantiate $m))
              (func (export "spin") (canon lift (core func $i "spin"))))
        "#;
        let host = Host::new(None).unwrap();
        let limits = Limits {
            time: Duration::from_millis(50),
            ..Limits::default()
        };
        let mut i = host
            .load(SPIN.as_bytes())
            .unwrap()
            .instantiate(&host, &host.linker::<()>(), (), limits)
            .unwrap();
        let before = CLOCK.state().ticks;
        let out = i.call::<(), ()>("spin", ());
        assert!(matches!(out, Err(Error::Timeout(_))), "{out:?}");
        assert!(CLOCK.state().ticks > before, "it ticks while a call runs");
        // The tick under way when the call ended.
        std::thread::sleep(TICK * 3);
        // It rests while no call runs. The clock is the process's, and the
        // other tests' calls tick it too: a window in which one ran is
        // tried again (one landing in the window failed CI on macOS).
        let rested = (0..50).any(|_| {
            let (ticks, begun, calls) = {
                let s = CLOCK.state();
                (s.ticks, s.begun, s.calls)
            };
            std::thread::sleep(TICK * 10);
            let s = CLOCK.state();
            calls == 0 && s.calls == 0 && s.begun == begun && s.ticks == ticks
        });
        assert!(rested, "and rests while none does");
    }
}
