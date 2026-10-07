//! One running server: the process, the reader thread, requests and their
//! answers, the documents it has open and what it has told us.
//!
//! Nothing here blocks the caller on the server: requests return a
//! [`Pending`] answer that the frontend polls when the client wakes it,
//! and the server's own requests (configuration, capability registration)
//! are answered on the reader thread.

use std::collections::{HashMap, VecDeque};
use std::io::{self, BufRead, BufReader};
use std::ops::Range;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde_json::{Value, json};

use crate::position::{Encoding, range_json};
use crate::rpc;

/// How a server is started and configured.
#[derive(Debug, Clone, Default)]
pub struct ServerConfig {
    /// Its name, for messages and logs: `elixir-ls`.
    pub name: String,
    /// The program.
    pub command: PathBuf,
    /// Its arguments.
    pub args: Vec<String>,
    /// Variables added to its environment.
    pub env: Vec<(String, String)>,
    /// The root: its working directory and `rootUri`.
    pub root: PathBuf,
    /// The workspace folders (the root when empty).
    pub folders: Vec<PathBuf>,
    /// `initializationOptions`.
    pub initialization_options: Value,
    /// The settings `workspace/configuration` is answered from, and sent
    /// once with `workspace/didChangeConfiguration`.
    pub settings: Value,
    /// For servers that report their work only in their log (Expert):
    /// log lines containing one of these start it…
    pub busy_start: Vec<String>,
    /// …and these end it (as do the first diagnostics).
    pub busy_done: Vec<String>,
}

/// The progress token of work told by the log ([`ServerConfig::busy_start`]).
const LOG_WORK: &str = "kalem-log-work";

/// A server's error answer, or the client's reason there is none.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    /// The JSON-RPC code; the client's own are below −32900.
    pub code: i64,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for RpcError {}

impl RpcError {
    fn client(message: impl Into<String>) -> RpcError {
        RpcError {
            code: -32901,
            message: message.into(),
        }
    }
}

/// An answer to come.
#[derive(Debug)]
pub struct Pending {
    /// The request's id.
    pub id: i64,
    rx: Receiver<Result<Value, RpcError>>,
}

impl Pending {
    /// The answer if it has come.
    pub fn poll(&self) -> Option<Result<Value, RpcError>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(RpcError::client("server stopped"))),
        }
    }

    /// Waits up to `timeout`: the answer, or `None` while it has not come.
    pub fn wait_for(&self, timeout: Duration) -> Option<Result<Value, RpcError>> {
        match self.rx.recv_timeout(timeout) {
            Ok(r) => Some(r),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => Some(Err(RpcError::client("server stopped"))),
        }
    }

    /// Waits for the answer up to `timeout`.
    pub fn wait(&self, timeout: Duration) -> Result<Value, RpcError> {
        match self.rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => Err(RpcError::client("no answer in time")),
            Err(RecvTimeoutError::Disconnected) => Err(RpcError::client("server stopped")),
        }
    }
}

/// Something the server said that the frontend may show.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// New diagnostics for a document ([`Client::diagnostics`] has them).
    Diagnostics {
        /// The document.
        uri: String,
    },
    /// `window/showMessage`: 1 error, 2 warning, 3 info, 4 log.
    Message {
        /// The level.
        level: u8,
        /// The text.
        text: String,
    },
    /// The progress shown changed ([`Client::progress`] has it).
    Progress,
    /// The server became ready.
    Ready,
    /// The process ended.
    Exited {
        /// Its exit code, when it has one.
        code: Option<i32>,
    },
    /// The server asks the editor to apply a workspace edit
    /// (`workspace/applyEdit`): answered with [`Client::answer_apply`].
    ApplyEdit {
        /// The request's id.
        id: Value,
        /// The `WorkspaceEdit`.
        edit: Value,
    },
}

/// Called from the client's threads whenever something arrives, to wake
/// the frontend.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Diagnostics as published: the document's version they were made for
/// (when the server says), and the list.
type Published = (Option<i64>, Vec<Value>);

/// One edit of a document: bytes replaced by a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The bytes replaced, in the text before this edit.
    pub range: Range<usize>,
    /// The new text.
    pub text: String,
}

#[derive(Default)]
struct State {
    ready: bool,
    /// Messages written before the server answered `initialize`.
    queued: Vec<Value>,
    capabilities: Value,
    encoding: Encoding,
    exited: bool,
}

struct Inner {
    config: ServerConfig,
    /// The settings answered to `workspace/configuration`: the
    /// configuration's at first, changed by [`Client::set_settings`].
    settings: RwLock<Value>,
    /// Messages for the writer thread, which alone writes to the
    /// server: nobody waits on a server that is slow to read.
    writer: Sender<Value>,
    child: Mutex<Child>,
    next_id: AtomicI64,
    pending: Mutex<HashMap<i64, Sender<Result<Value, RpcError>>>>,
    state: RwLock<State>,
    versions: Mutex<HashMap<String, i64>>,
    /// Diagnostics by document: the document's version they were made
    /// for (when the server says), and the list.
    diagnostics: Mutex<HashMap<String, Published>>,
    /// Counts the diagnostics published, so readers know when theirs are
    /// out of date.
    published: AtomicU64,
    /// Work in progress by token: its title (from its `begin`) and the
    /// text shown.
    progress: Mutex<Vec<(String, String, String)>>,
    events: Mutex<VecDeque<Event>>,
    log: Mutex<VecDeque<String>>,
    wake: Wake,
    shutting_down: AtomicBool,
}

/// The lines of log kept.
const LOG_LINES: usize = 2000;

impl Inner {
    fn log(&self, line: impl Into<String>) {
        let mut log = self.log.lock().expect("log");
        if log.len() == LOG_LINES {
            log.pop_front();
        }
        log.push_back(line.into());
    }

    /// Work a server tells only in its log: shown as progress from a
    /// start line to a done line, each line between as its text.
    fn log_work(&self, line: &str) {
        let c = &self.config;
        if c.busy_start.is_empty() {
            return;
        }
        let mut p = self.progress.lock().expect("progress");
        let busy = p.iter().any(|(t, ..)| t == LOG_WORK);
        if c.busy_done.iter().any(|d| line.contains(d.as_str())) {
            if busy {
                p.retain(|(t, ..)| t != LOG_WORK);
                drop(p);
                self.event(Event::Progress);
            }
            return;
        }
        let starts = c.busy_start.iter().any(|s| line.contains(s.as_str()));
        if busy || starts {
            let text: String = line.lines().next().unwrap_or("").chars().take(80).collect();
            p.retain(|(t, ..)| t != LOG_WORK);
            p.push((LOG_WORK.to_string(), String::new(), text));
            drop(p);
            self.event(Event::Progress);
        }
    }

    /// Diagnostics came: work told by the log is over.
    fn end_log_work(&self) {
        let mut p = self.progress.lock().expect("progress");
        let before = p.len();
        p.retain(|(t, ..)| t != LOG_WORK);
        if p.len() != before {
            drop(p);
            self.event(Event::Progress);
        }
    }

    fn event(&self, e: Event) {
        self.events.lock().expect("events").push_back(e);
        (self.wake)();
    }

    /// Hands `msg` to the writer thread; never blocks.
    fn send_now(&self, msg: Value) {
        if self.writer.send(msg).is_err() {
            self.log("[client] the server's input is closed");
        }
    }

    /// Writes, or queues until the server is ready; `initialize` and the
    /// answers to the server's requests are never queued.
    fn send(&self, msg: Value, urgent: bool) {
        if !urgent {
            let mut st = self.state.write().expect("state");
            if st.exited {
                return;
            }
            if !st.ready {
                queue(&mut st.queued, msg);
                return;
            }
        }
        self.send_now(msg);
    }

    fn answer(&self, id: Value, result: Result<Value, (i64, &str)>) {
        let msg = match result {
            Ok(v) => json!({"jsonrpc": "2.0", "id": id, "result": v}),
            Err((code, m)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}})
            }
        };
        self.send(msg, true);
    }

    fn setting(&self, section: Option<&str>) -> Value {
        let all = self.settings.read().expect("settings");
        let mut v = &*all;
        if let Some(section) = section.filter(|s| !s.is_empty()) {
            for key in section.split('.') {
                match v.get(key) {
                    Some(next) => v = next,
                    None => return Value::Null,
                }
            }
        }
        v.clone()
    }

    fn on_request(&self, id: Value, method: &str, params: &Value) {
        match method {
            "workspace/configuration" => {
                let items = params["items"].as_array().cloned().unwrap_or_default();
                let out: Vec<Value> = items
                    .iter()
                    .map(|i| self.setting(i["section"].as_str()))
                    .collect();
                self.answer(id, Ok(Value::Array(out)));
            }
            "workspace/workspaceFolders" => {
                self.answer(id, Ok(folders_json(&self.config)));
            }
            "client/registerCapability"
            | "client/unregisterCapability"
            | "window/workDoneProgress/create"
            | "workspace/diagnostic/refresh"
            | "workspace/semanticTokens/refresh"
            | "workspace/inlayHint/refresh"
            | "workspace/codeLens/refresh" => self.answer(id, Ok(Value::Null)),
            // Said as a message (the editor offers no choice of its
            // actions yet), and answered with none chosen.
            "window/showMessageRequest" => {
                let text = params["message"].as_str().unwrap_or_default().to_string();
                self.log(format!("[message] {text}"));
                let level = params["type"].as_u64().unwrap_or(3) as u8;
                self.event(Event::Message { level, text });
                self.answer(id, Ok(Value::Null));
            }
            "workspace/applyEdit" => {
                // The editor applies it on its timer and answers then.
                self.event(Event::ApplyEdit {
                    id,
                    edit: params["edit"].clone(),
                });
            }
            _ => self.answer(id, Err((-32601, "method not found"))),
        }
    }

    fn on_notification(&self, method: &str, params: &Value) {
        match method {
            "textDocument/publishDiagnostics" => {
                // Stored by one spelling of the URI, so a server writing
                // the drive letter or a verbatim path its own way still
                // reaches the document.
                let Some(uri) = params["uri"].as_str().map(crate::uri::normalize) else {
                    return;
                };
                let uri = uri.as_str();
                let list = params["diagnostics"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let version = params["version"].as_i64();
                self.diagnostics
                    .lock()
                    .expect("diagnostics")
                    .insert(uri.to_string(), (version, list));
                self.end_log_work();
                self.published.fetch_add(1, Ordering::Relaxed);
                self.event(Event::Diagnostics {
                    uri: uri.to_string(),
                });
            }
            "window/logMessage" => {
                let line = params["message"].as_str().unwrap_or_default().to_string();
                self.log_work(&line);
                self.log(line);
            }
            "window/showMessage" => {
                let text = params["message"].as_str().unwrap_or_default().to_string();
                self.log(format!("[message] {text}"));
                let level = params["type"].as_u64().unwrap_or(3) as u8;
                self.event(Event::Message { level, text });
            }
            "$/progress" => {
                let token = match &params["token"] {
                    Value::String(s) => s.clone(),
                    t => t.to_string(),
                };
                let v = &params["value"];
                let mut p = self.progress.lock().expect("progress");
                match v["kind"].as_str() {
                    Some("begin") | Some("report") => {
                        // The title comes with `begin` (a server may
                        // report without one); each report's message
                        // replaces the last.
                        let (title, old) = match p.iter().position(|(t, ..)| *t == token) {
                            Some(i) => {
                                let (_, title, text) = p.remove(i);
                                (title, text)
                            }
                            None => (String::new(), String::new()),
                        };
                        let title = v["title"].as_str().map_or(title, str::to_string);
                        let text = match v["message"].as_str() {
                            Some(m) if title.is_empty() => m.to_string(),
                            Some(m) => format!("{title}: {m}"),
                            None if old.is_empty() => title.clone(),
                            None => old,
                        };
                        p.push((token, title, text));
                    }
                    Some("end") => p.retain(|(t, ..)| *t != token),
                    _ => {}
                }
                drop(p);
                self.event(Event::Progress);
            }
            _ => {}
        }
    }

    fn on_response(&self, msg: &Value) {
        let Some(id) = msg["id"].as_i64() else {
            return;
        };
        let Some(tx) = self.pending.lock().expect("pending").remove(&id) else {
            return;
        };
        let r = match msg.get("error") {
            Some(e) if !e.is_null() => Err(RpcError {
                code: e["code"].as_i64().unwrap_or(0),
                message: e["message"].as_str().unwrap_or("error").to_string(),
            }),
            _ => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
        };
        let _ = tx.send(r);
        (self.wake)();
    }

    fn request(self: &Arc<Self>, method: &str, params: Value, urgent: bool) -> Pending {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        // Waiting under the state's lock: the reader marks the server
        // exited before it answers the waiting requests, so this one is
        // either refused here or answered there, never left waiting.
        let st = self.state.read().expect("state");
        if st.exited {
            drop(st);
            let _ = tx.send(Err(RpcError::client("server stopped")));
        } else {
            self.pending.lock().expect("pending").insert(id, tx);
            drop(st);
            let mut msg = message(method, params);
            msg["id"] = json!(id);
            self.send(msg, urgent);
        }
        Pending { id, rx }
    }

    fn notify(&self, method: &str, params: Value) {
        self.send(message(method, params), false);
    }
}

/// A request or notification: without `params` when there are none
/// (`shutdown`, `exit`), as JSON-RPC wants them an object, a list or
/// absent, never null.
fn message(method: &str, params: Value) -> Value {
    let mut msg = json!({"jsonrpc": "2.0", "method": method});
    if !params.is_null() {
        msg["params"] = params;
    }
    msg
}

/// Queues `msg` for a server not ready yet. A document's changes then
/// carry its whole text (positions need the encoding the server has not
/// chosen yet), so only the latest text is kept: it goes into the queued
/// `didOpen` or replaces the queued change, instead of one copy of the
/// document per keystroke.
fn queue(queued: &mut Vec<Value>, msg: Value) {
    if msg["method"] == "textDocument/didChange" {
        let uri = &msg["params"]["textDocument"]["uri"];
        let changes = msg["params"]["contentChanges"].as_array();
        let whole = changes
            .and_then(|c| c.last())
            .filter(|c| c.get("range").is_none());
        if let Some(whole) = whole {
            let text = whole["text"].clone();
            let version = msg["params"]["textDocument"]["version"].clone();
            for q in queued.iter_mut().rev() {
                let same = q["params"]["textDocument"]["uri"] == *uri;
                if same && q["method"] == "textDocument/didOpen" {
                    q["params"]["textDocument"]["text"] = text;
                    q["params"]["textDocument"]["version"] = version;
                    return;
                }
                if same && q["method"] == "textDocument/didChange" {
                    *q = msg;
                    return;
                }
                if same {
                    break;
                }
            }
        }
    }
    queued.push(msg);
}

fn folders_json(config: &ServerConfig) -> Value {
    let folders: Vec<&PathBuf> = if config.folders.is_empty() {
        vec![&config.root]
    } else {
        config.folders.iter().collect()
    };
    Value::Array(
        folders
            .into_iter()
            .map(|f| {
                json!({
                    "uri": crate::uri::from_path(f),
                    "name": f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                })
            })
            .collect(),
    )
}

/// What Kalem tells servers it can do.
fn client_capabilities() -> Value {
    json!({
        "general": { "positionEncodings": ["utf-8", "utf-16"] },
        "workspace": {
            "configuration": true,
            "workspaceFolders": true,
            "didChangeConfiguration": { "dynamicRegistration": false },
            "didChangeWatchedFiles": { "dynamicRegistration": false },
            "applyEdit": true,
            "workspaceEdit": { "documentChanges": true },
        },
        "window": { "workDoneProgress": true, "showMessage": {} },
        "textDocument": {
            "synchronization": { "didSave": true, "willSave": false, "willSaveWaitUntil": false },
            "publishDiagnostics": { "relatedInformation": false, "versionSupport": true },
            "hover": { "contentFormat": ["markdown", "plaintext"] },
            "completion": {
                "completionItem": {
                    "snippetSupport": false,
                    "documentationFormat": ["markdown", "plaintext"],
                    "insertReplaceSupport": false,
                },
                "contextSupport": true,
            },
            "signatureHelp": { "signatureInformation": { "documentationFormat": ["markdown", "plaintext"] } },
            "definition": { "linkSupport": true },
            "declaration": { "linkSupport": true },
            "typeDefinition": { "linkSupport": true },
            "implementation": { "linkSupport": true },
            "references": {},
            "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
            "formatting": {},
            "rename": { "prepareSupport": false },
            "codeAction": {
                "codeActionLiteralSupport": { "codeActionKind": { "valueSet": [
                    "", "quickfix", "refactor", "refactor.extract", "refactor.inline",
                    "refactor.rewrite", "source", "source.organizeImports"
                ] } },
                "resolveSupport": { "properties": ["edit"] },
            },
        },
    })
}

/// A running server.
pub struct Client {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("name", &self.inner.config.name)
            .field("root", &self.inner.config.root)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// Starts the server and sends `initialize`; returns at once. Messages
    /// sent before the server answers wait in a queue.
    pub fn start(config: ServerConfig, wake: Wake) -> io::Result<Client> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .current_dir(&config.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (k, v) in &config.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn()?;
        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");
        let (writer, outbox) = mpsc::channel::<Value>();
        let inner = Arc::new(Inner {
            settings: RwLock::new(config.settings.clone()),
            config,
            writer,
            child: Mutex::new(child),
            next_id: AtomicI64::new(1),
            pending: Mutex::new(HashMap::new()),
            state: RwLock::new(State::default()),
            versions: Mutex::new(HashMap::new()),
            diagnostics: Mutex::new(HashMap::new()),
            published: AtomicU64::new(0),
            progress: Mutex::new(Vec::new()),
            events: Mutex::new(VecDeque::new()),
            log: Mutex::new(VecDeque::new()),
            wake,
            shutting_down: AtomicBool::new(false),
        });
        let name = inner.config.name.clone();
        {
            // The writer holds the client weakly, so the client ending
            // (its sender dropped) ends the thread.
            let weak = Arc::downgrade(&inner);
            std::thread::Builder::new()
                .name(format!("lsp-out:{name}"))
                .spawn(move || {
                    let mut w = std::io::BufWriter::new(stdin);
                    while let Ok(msg) = outbox.recv() {
                        if let Err(e) = rpc::write(&mut w, &msg) {
                            if let Some(inner) = weak.upgrade() {
                                inner.log(format!("[client] write failed: {e}"));
                            }
                            break;
                        }
                    }
                })?;
        }
        {
            let inner = Arc::clone(&inner);
            std::thread::Builder::new()
                .name(format!("lsp-err:{name}"))
                .spawn(move || {
                    // Read to its end whatever it holds: a line that is
                    // not UTF-8 (a compiler's message in another locale)
                    // must not close the pipe the server still writes to.
                    let mut r = BufReader::new(stderr);
                    let mut line = Vec::new();
                    while matches!(r.read_until(b'\n', &mut line), Ok(n) if n > 0) {
                        let text = String::from_utf8_lossy(&line);
                        inner.log(format!("[stderr] {}", text.trim_end_matches(['\r', '\n'])));
                        line.clear();
                    }
                })?;
        }
        let init = inner.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": { "name": "Kalem", "version": env!("CARGO_PKG_VERSION") },
                "rootUri": crate::uri::from_path(&inner.config.root),
                "rootPath": inner.config.root.to_string_lossy(),
                "workspaceFolders": folders_json(&inner.config),
                "initializationOptions": inner.config.initialization_options,
                "capabilities": client_capabilities(),
            }),
            true,
        );
        {
            let inner = Arc::clone(&inner);
            std::thread::Builder::new()
                .name(format!("lsp:{name}"))
                .spawn(move || reader(inner, stdout, init))?;
        }
        Ok(Client { inner })
    }

    /// Ends the process (a server that never answered `initialize`); its
    /// exit comes as [`Event::Exited`].
    pub fn kill(&self) {
        let _ = self.inner.child.lock().expect("child").kill();
    }

    /// Answers the server's `workspace/applyEdit` request `id`.
    pub fn answer_apply(&self, id: Value, applied: bool, reason: Option<&str>) {
        let mut result = json!({ "applied": applied });
        if let Some(r) = reason {
            result["failureReason"] = json!(r);
        }
        self.inner.answer(id, Ok(result));
    }

    /// The settings the server has.
    pub fn settings(&self) -> Value {
        self.inner.settings.read().expect("settings").clone()
    }

    /// Changes the server's settings: kept for its `workspace/configuration`
    /// requests and sent with `workspace/didChangeConfiguration` (queued
    /// until it is ready). Nothing is sent when they are the same.
    pub fn set_settings(&self, settings: Value) {
        {
            let mut s = self.inner.settings.write().expect("settings");
            if *s == settings {
                return;
            }
            *s = settings.clone();
        }
        self.notify(
            "workspace/didChangeConfiguration",
            json!({ "settings": settings }),
        );
    }

    /// The server's name.
    pub fn name(&self) -> &str {
        &self.inner.config.name
    }

    /// Its configuration.
    pub fn config(&self) -> &ServerConfig {
        &self.inner.config
    }

    /// It answered `initialize` and has not exited.
    pub fn is_ready(&self) -> bool {
        let st = self.inner.state.read().expect("state");
        st.ready && !st.exited
    }

    /// The process has ended.
    pub fn has_exited(&self) -> bool {
        self.inner.state.read().expect("state").exited
    }

    /// The server's capabilities (null before it is ready).
    pub fn capabilities(&self) -> Value {
        self.inner.state.read().expect("state").capabilities.clone()
    }

    /// The server has the provider `name` (`hoverProvider`): present and
    /// not `false`.
    pub fn provides(&self, name: &str) -> bool {
        let st = self.inner.state.read().expect("state");
        !matches!(
            st.capabilities.get(name),
            None | Some(Value::Null) | Some(Value::Bool(false))
        )
    }

    /// The position encoding agreed on.
    pub fn encoding(&self) -> Encoding {
        self.inner.state.read().expect("state").encoding
    }

    /// Sends a request.
    pub fn request(&self, method: &str, params: Value) -> Pending {
        self.inner.request(method, params, false)
    }

    /// Tells the server a request's answer is no longer wanted.
    pub fn cancel(&self, pending: &Pending) {
        if self
            .inner
            .pending
            .lock()
            .expect("pending")
            .remove(&pending.id)
            .is_some()
        {
            self.inner
                .notify("$/cancelRequest", json!({ "id": pending.id }));
        }
    }

    /// Sends a notification.
    pub fn notify(&self, method: &str, params: Value) {
        self.inner.notify(method, params);
    }

    /// The events since the last call.
    pub fn take_events(&self) -> Vec<Event> {
        self.inner
            .events
            .lock()
            .expect("events")
            .drain(..)
            .collect()
    }

    /// The diagnostics last published for `uri`.
    pub fn diagnostics(&self, uri: &str) -> Vec<Value> {
        self.inner
            .diagnostics
            .lock()
            .expect("diagnostics")
            .get(&crate::uri::normalize(uri))
            .map(|(_, l)| l.clone())
            .unwrap_or_default()
    }

    /// Every document's published diagnostics, by URI (the project's files
    /// the server checks, open or not).
    pub fn all_published(&self) -> Vec<(String, Vec<Value>)> {
        self.inner
            .diagnostics
            .lock()
            .expect("diagnostics")
            .iter()
            .filter(|(_, (_, l))| !l.is_empty())
            .map(|(k, (_, l))| (k.clone(), l.clone()))
            .collect()
    }

    /// How many times diagnostics were published: a reader's copy is up
    /// to date while this has not moved.
    pub fn published(&self) -> u64 {
        self.inner.published.load(Ordering::Relaxed)
    }

    /// The diagnostics of `uri` were made for an older version of it than
    /// the one sent since: their places may have moved. Servers that do not
    /// say the version are taken at their word.
    pub fn diagnostics_stale(&self, uri: &str) -> bool {
        let made_for = self
            .inner
            .diagnostics
            .lock()
            .expect("diagnostics")
            .get(&crate::uri::normalize(uri))
            .and_then(|(v, _)| *v);
        let now = self
            .inner
            .versions
            .lock()
            .expect("versions")
            .get(uri)
            .copied();
        matches!((made_for, now), (Some(a), Some(b)) if a < b)
    }

    /// The work the server reports in progress, for the status bar.
    pub fn progress(&self) -> Option<String> {
        let p = self.inner.progress.lock().expect("progress");
        p.iter()
            .rev()
            .map(|(.., t)| t)
            .find(|t| !t.is_empty())
            .cloned()
    }

    /// The log: the server's log messages and its standard error.
    pub fn log(&self) -> Vec<String> {
        self.inner
            .log
            .lock()
            .expect("log")
            .iter()
            .cloned()
            .collect()
    }

    /// Opens a document.
    pub fn did_open(&self, uri: &str, language_id: &str, text: &str) {
        self.inner
            .versions
            .lock()
            .expect("versions")
            .insert(uri.to_string(), 1);
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": { "uri": uri, "languageId": language_id, "version": 1, "text": text }
            }),
        );
    }

    /// A document changed from `before` by `edits` (in order) to `after`.
    /// Sent as ranges when the server takes them, else whole.
    pub fn did_change(&self, uri: &str, before: &str, edits: &[Edit], after: &str) {
        let version = {
            let mut v = self.inner.versions.lock().expect("versions");
            let Some(n) = v.get_mut(uri) else { return };
            *n += 1;
            *n
        };
        let (incremental, enc) = {
            let st = self.inner.state.read().expect("state");
            let sync = &st.capabilities["textDocumentSync"];
            let kind = sync
                .as_i64()
                .or_else(|| sync["change"].as_i64())
                .unwrap_or(1);
            (st.ready && kind == 2, st.encoding)
        };
        let changes = if incremental && edits.len() == 1 {
            // One edit (the editor's usual): its range in `before` as it
            // is, without a copy of the document.
            let e = &edits[0];
            let r = e.range.start.min(before.len())..e.range.end.min(before.len());
            vec![json!({ "range": range_json(before, r, enc), "text": e.text })]
        } else if incremental && !edits.is_empty() && edits.len() <= 8 {
            let mut text = before.to_string();
            let mut out = Vec::new();
            for e in edits {
                let r = e.range.start.min(text.len())..e.range.end.min(text.len());
                out.push(json!({ "range": range_json(&text, r.clone(), enc), "text": e.text }));
                text.replace_range(r, &e.text);
            }
            debug_assert_eq!(text, after);
            out
        } else {
            vec![json!({ "text": after })]
        };
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri, "version": version },
                "contentChanges": changes,
            }),
        );
    }

    /// A document was saved.
    pub fn did_save(&self, uri: &str, text: &str) {
        let include = {
            let st = self.inner.state.read().expect("state");
            st.capabilities["textDocumentSync"]["save"]["includeText"]
                .as_bool()
                .unwrap_or(false)
        };
        let mut params = json!({ "textDocument": { "uri": uri } });
        if include {
            params["text"] = json!(text);
        }
        self.notify("textDocument/didSave", params);
    }

    /// A document was closed.
    pub fn did_close(&self, uri: &str) {
        if self
            .inner
            .versions
            .lock()
            .expect("versions")
            .remove(uri)
            .is_some()
        {
            self.notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": uri } }),
            );
        }
        self.inner
            .diagnostics
            .lock()
            .expect("diagnostics")
            .remove(&crate::uri::normalize(uri));
        self.inner.published.fetch_add(1, Ordering::Relaxed);
    }

    /// The documents open in it.
    pub fn open_documents(&self) -> usize {
        self.inner.versions.lock().expect("versions").len()
    }

    /// Asks the server to stop, waits a moment, and ends the process.
    pub fn shutdown(&self) {
        if self.inner.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        if self.is_ready() {
            let p = self.inner.request("shutdown", Value::Null, false);
            let _ = p.wait(Duration::from_millis(1500));
            self.inner.notify("exit", Value::Null);
        }
        let mut child = self.inner.child.lock().expect("child");
        for _ in 0..20 {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn reader(inner: Arc<Inner>, stdout: std::process::ChildStdout, init: Pending) {
    let mut r = BufReader::new(stdout);
    let mut init = Some(init);
    loop {
        let msg = match rpc::read(&mut r) {
            Ok(Some(m)) => m,
            Ok(None) => break,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                inner.log(format!("[client] malformed message: {e}"));
                continue;
            }
            Err(e) => {
                inner.log(format!("[client] read failed: {e}"));
                break;
            }
        };
        let method = msg.get("method").and_then(Value::as_str);
        let id = msg.get("id").cloned();
        match (method, id) {
            (Some(m), Some(id)) => inner.on_request(id, m, &msg["params"]),
            (Some(m), None) => inner.on_notification(m, &msg["params"]),
            (None, Some(_)) => inner.on_response(&msg),
            (None, None) => {}
        }
        if let Some(p) = &init
            && let Some(answer) = p.poll()
        {
            init = None;
            match answer {
                Ok(v) => initialized(&inner, &v),
                Err(e) => {
                    inner.log(format!("[client] initialize failed: {e}"));
                    inner.event(Event::Message {
                        level: 1,
                        text: format!("{} did not start: {}", inner.config.name, e.message),
                    });
                    // A server that cannot start is ended, not left
                    // "starting": its exit follows as for a crash.
                    let _ = inner.child.lock().expect("child").kill();
                }
            }
        }
    }
    // The output closes as the process ends; its exit status follows a
    // moment later.
    let mut code = None;
    for _ in 0..40 {
        match inner.child.lock().expect("child").try_wait() {
            Ok(Some(status)) => {
                code = status.code();
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }
    {
        let mut st = inner.state.write().expect("state");
        st.exited = true;
        st.queued.clear();
    }
    for (_, tx) in inner.pending.lock().expect("pending").drain() {
        let _ = tx.send(Err(RpcError::client("server stopped")));
    }
    inner.progress.lock().expect("progress").clear();
    if !inner.shutting_down.load(Ordering::SeqCst) {
        inner.log(format!("[client] the server exited ({code:?})"));
    }
    inner.event(Event::Exited { code });
}

fn initialized(inner: &Inner, answer: &Value) {
    let caps = answer["capabilities"].clone();
    let encoding = Encoding::from_name(caps["positionEncoding"].as_str().unwrap_or("utf-16"));
    // The state stays locked while the queue is written, so nothing sent
    // meanwhile overtakes it.
    let mut st = inner.state.write().expect("state");
    st.capabilities = caps;
    st.encoding = encoding;
    st.ready = true;
    let queued = std::mem::take(&mut st.queued);
    // Handed to the writer in order, under the lock: nothing sent
    // meanwhile overtakes the queue, and nothing here waits on the server.
    inner.send_now(json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}));
    let settings = inner.settings.read().expect("settings").clone();
    if !settings.is_null() {
        inner.send_now(json!({
            "jsonrpc": "2.0",
            "method": "workspace/didChangeConfiguration",
            "params": { "settings": settings },
        }));
    }
    for m in queued {
        inner.send_now(m);
    }
    drop(st);
    inner.event(Event::Ready);
}

#[cfg(test)]
mod queue_tests {
    use super::*;

    #[test]
    fn one_text_per_document_before_ready() {
        let open = json!({"method": "textDocument/didOpen", "params": {"textDocument": {"uri": "a", "version": 1, "text": "x"}}});
        let change = |v: i64, t: &str| {
            json!({"method": "textDocument/didChange",
            "params": {"textDocument": {"uri": "a", "version": v}, "contentChanges": [{"text": t}]}})
        };
        let mut q = Vec::new();
        queue(&mut q, open);
        for v in 2..100 {
            queue(&mut q, change(v, &format!("text {v}")));
        }
        assert_eq!(q.len(), 1);
        assert_eq!(q[0]["params"]["textDocument"]["text"], "text 99");
        assert_eq!(q[0]["params"]["textDocument"]["version"], 99);
        // After a save, changes queue again, but one at a time.
        queue(
            &mut q,
            json!({"method": "textDocument/didSave", "params": {"textDocument": {"uri": "a"}}}),
        );
        queue(&mut q, change(100, "y"));
        queue(&mut q, change(101, "z"));
        assert_eq!(q.len(), 3);
        assert_eq!(q[2]["params"]["contentChanges"][0]["text"], "z");
    }
}
