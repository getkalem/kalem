//! A fake language server for tests (of this crate and of the editor's
//! service): a test binary started again with an environment variable
//! calls [`serve`] and speaks the protocol on its standard streams.
//!
//! Behaviors: `normal`; `silent` (never answers `initialize`); `refuse`
//! (answers it with an error); `crash`
//! (exits with 3 on the first `didOpen`); `garbage` (a malformed message
//! first); `absent` (exits with 1 at once, its reason on standard error,
//! as a toolchain's proxy for a component not installed does); `pull`
//! (as rust-analyzer: the warnings on `TODO` pushed, the errors on `bad`
//! given only when asked, `textDocument/diagnostic`, "unchanged" when the
//! client has the report already; a save adds a note to them, and the
//! server asks the client to ask again); `busy` (its first two hovers
//! answered "content modified", as rust-analyzer answers while it loads a
//! project). In
//! `normal`, a change whose text contains `CRASH` exits with 4, and the
//! references of anything are the document's first line and, when the
//! folder beside the root has a `library/lib.fk`, that file's (a
//! library's source outside the project, as a standard library is).

#![allow(clippy::print_stdout)]

use std::collections::HashMap;
use std::io::{BufReader, Write};

use serde_json::{Value, json};

use crate::position::{Encoding, byte_range, position};
use crate::rpc;

/// A diagnostic on every one of `words`, with its severity.
fn found(text: &str, words: &[(&str, i64)]) -> Vec<Value> {
    let mut list = Vec::new();
    for (word, severity) in words {
        for (at, _) in text.match_indices(word) {
            list.push(json!({
                "range": { "start": position(text, at, Encoding::Utf16).to_json(),
                           "end": position(text, at + word.len(), Encoding::Utf16).to_json() },
                "severity": severity, "source": "fake", "message": format!("{word} found"),
            }));
        }
    }
    list
}

fn publish(uri: &Value, list: &[Value]) -> Value {
    json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
           "params": {"uri": uri, "diagnostics": list}})
}

/// A warning on every `TODO`, an error on every `bad`.
fn diagnostics(uri: &Value, text: &str) -> Value {
    publish(uri, &found(text, &[("TODO", 2), ("bad", 1)]))
}

/// A file's URI with links resolved, as Expert names files.
fn real(uri: &str) -> String {
    crate::uri::to_path(uri)
        .and_then(|p| std::fs::canonicalize(p).ok())
        .map_or_else(|| uri.to_string(), |p| crate::uri::from_path(&p))
}

/// Serves on standard input and output until `exit`.
pub fn serve(behavior: &str) {
    if behavior == "absent" {
        let mut err = std::io::stderr();
        let _ = writeln!(
            err,
            "error: 'fake' is not installed for the toolchain 'test'"
        );
        let _ = writeln!(err, "help: run `fake install` to install it");
        std::process::exit(1);
    }
    let stdin = std::io::stdin();
    let mut r = BufReader::new(stdin.lock());
    let mut out = std::io::stdout();
    let mut texts: HashMap<String, String> = HashMap::new();
    let mut root: Option<std::path::PathBuf> = None;
    // `pull`: the words pushed, and whether a save has been seen.
    let pushed: &[(&str, i64)] = if behavior == "pull" {
        &[("TODO", 2)]
    } else {
        &[("TODO", 2), ("bad", 1)]
    };
    let mut saved = false;
    let mut busy = if behavior == "busy" { 2 } else { 0 };
    let send = |out: &mut std::io::Stdout, v: Value| {
        let _ = rpc::write(out, &v);
    };
    if behavior == "garbage" {
        let _ = out.write_all(b"Content-Length: 5\r\n\r\n{nope");
        let _ = out.flush();
    }
    while let Ok(Some(msg)) = rpc::read(&mut r) {
        let id = msg.get("id").cloned();
        let method = msg["method"].as_str().unwrap_or("");
        let p = &msg["params"];
        let uri = p["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        match method {
            "initialize" => {
                root = p["rootUri"].as_str().and_then(crate::uri::to_path);
                if behavior == "silent" {
                    continue;
                }
                if behavior == "refuse" {
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32603, "message": "no project"}}),
                    );
                    continue;
                }
                let mut answer = json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {
                    "positionEncoding": "utf-16",
                    "textDocumentSync": {"openClose": true, "change": 2, "save": {"includeText": true}},
                    "hoverProvider": true,
                    "definitionProvider": true,
                    "referencesProvider": true,
                    "documentFormattingProvider": true,
                    "completionProvider": {"triggerCharacters": ["."], "resolveProvider": true},
                    "signatureHelpProvider": {"triggerCharacters": ["(", ","]},
                    "renameProvider": true,
                    "codeActionProvider": true,
                    "executeCommandProvider": {"commands": ["fake.cmd"]},
                }}});
                if behavior == "pull" {
                    answer["result"]["capabilities"]["diagnosticProvider"] =
                        json!({"interFileDependencies": false, "workspaceDiagnostics": false});
                }
                send(&mut out, answer);
                if behavior == "chatty" {
                    // A line of its standard error that is not UTF-8,
                    // then one that is: both reach the log.
                    let mut err = std::io::stderr();
                    let _ = err.write_all(b"caf\xe9 compiled\nafter the odd byte\n");
                    let _ = err.flush();
                    // Work whose title has a colon in it.
                    for value in [
                        json!({"kind": "begin", "title": "Building: app"}),
                        json!({"kind": "report", "message": "a.ex"}),
                        json!({"kind": "report", "message": "b.ex"}),
                    ] {
                        send(
                            &mut out,
                            json!({"jsonrpc": "2.0", "method": "$/progress",
                                "params": {"token": "build", "value": value}}),
                        );
                    }
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "id": 902, "method": "window/showMessageRequest",
                            "params": {"type": 2, "message": "Fetch the dependencies?",
                                "actions": [{"title": "Yes"}]}}),
                    );
                    // Stray output with no newline before a message.
                    let _ = out.write_all(b"warning: stray");
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "method": "window/logMessage",
                            "params": {"type": 3, "message": "after the stray output"}}),
                    );
                }
                // Asks for its settings, as ElixirLS does.
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": 900, "method": "workspace/configuration",
                    "params": {"items": [{"section": "elixirLS"}]}}),
                );
            }
            "textDocument/didOpen" => {
                if behavior == "crash" {
                    std::process::exit(3);
                }
                let text = p["textDocument"]["text"].as_str().unwrap_or("").to_string();
                send(&mut out, publish(&json!(real(&uri)), &found(&text, pushed)));
                texts.insert(uri, text);
            }
            "textDocument/didChange" => {
                let text = texts.entry(uri.clone()).or_default();
                for c in p["contentChanges"].as_array().into_iter().flatten() {
                    match c.get("range") {
                        Some(range) => {
                            if let Some(b) = byte_range(text, range, Encoding::Utf16) {
                                text.replace_range(b, c["text"].as_str().unwrap_or(""));
                            }
                        }
                        None => *text = c["text"].as_str().unwrap_or("").to_string(),
                    }
                }
                if text.contains("CRASH") {
                    std::process::exit(4);
                }
                // A file of the project that is not open, reported as the
                // project is checked.
                if text.contains("PROJECT")
                    && let Some(path) = crate::uri::to_path(&real(&uri))
                {
                    let other = crate::uri::from_path(&path.with_file_name("other.fk"));
                    send(&mut out, diagnostics(&json!(other), "bad"));
                }
                send(&mut out, publish(&json!(real(&uri)), &found(text, pushed)));
            }
            "textDocument/diagnostic" => {
                let text = texts.get(&uri).cloned().unwrap_or_default();
                let mut items = found(&text, &[("bad", 1)]);
                if saved {
                    items.push(json!({
                        "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}},
                        "severity": 3, "source": "fake", "message": "saved",
                    }));
                }
                let report = {
                    use std::hash::{Hash, Hasher};
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (&text, saved).hash(&mut h);
                    format!("{:x}", h.finish())
                };
                let result = if p["previousResultId"].as_str() == Some(report.as_str()) {
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "method": "window/logMessage",
                            "params": {"type": 3, "message": "diagnostics unchanged"}}),
                    );
                    json!({"kind": "unchanged", "resultId": report})
                } else {
                    json!({"kind": "full", "resultId": report, "items": items})
                };
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": result}),
                );
            }
            "textDocument/didSave" => {
                if behavior == "pull" {
                    saved = true;
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "id": 903, "method": "workspace/diagnostic/refresh"}),
                    );
                }
            }
            "textDocument/hover" if busy > 0 => {
                busy -= 1;
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32801, "message": "content modified"}}),
                );
            }
            "textDocument/hover" => {
                let pos = &p["position"];
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": {"contents": {"kind": "markdown",
                    "value": format!("at {}:{}", pos["line"], pos["character"])}}}),
                );
            }
            "textDocument/references" => {
                let line = |uri: &str| {
                    json!({"uri": uri, "range": {"start": {"line": 0, "character": 0},
                                                  "end": {"line": 0, "character": 1}}})
                };
                let mut list = vec![line(&uri)];
                let library = root
                    .as_deref()
                    .and_then(std::path::Path::parent)
                    .map(|d| d.join("library/lib.fk"))
                    .filter(|p| p.is_file());
                if let Some(lib) = library {
                    list.push(line(&crate::uri::from_path(&lib)));
                }
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": list}),
                );
            }
            "textDocument/definition" => {
                // The first line of the same document.
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": [{"uri": uri,
                    "range": {"start": {"line": 0, "character": 2}, "end": {"line": 0, "character": 3}}}]}),
                );
            }
            "textDocument/formatting" => {
                // Runs of spaces become one.
                let text = texts.get(&uri).cloned().unwrap_or_default();
                let mut edits = Vec::new();
                let b = text.as_bytes();
                let mut i = 0;
                while i < b.len() {
                    if b[i] == b' ' && b.get(i + 1) == Some(&b' ') {
                        let mut j = i + 1;
                        while b.get(j) == Some(&b' ') {
                            j += 1;
                        }
                        edits.push(json!({"range": {"start": position(&text, i + 1, Encoding::Utf16).to_json(),
                            "end": position(&text, j, Encoding::Utf16).to_json()}, "newText": ""}));
                        i = j;
                    } else {
                        i += 1;
                    }
                }
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": edits}),
                );
            }
            "textDocument/completion" => {
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": {"isIncomplete": false, "items": [
                        {"label": "greet/1", "kind": 3, "detail": "def greet(name)", "insertText": "greet(${1:name})", "insertTextFormat": 2, "sortText": "1",
                         "documentation": {"kind": "markdown", "value": "Greets `name`."}},
                        {"label": "goodbye/0", "kind": 3, "insertText": "goodbye()", "sortText": "2",
                         "additionalTextEdits": [{"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "use Bye\n"}]},
                    ]}}),
                );
            }
            "workspace/didChangeWatchedFiles" => {
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "method": "window/logMessage",
                        "params": {"type": 3, "message": format!("watched {}", p["changes"])}}),
                );
            }
            "workspace/didChangeConfiguration" => {
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "method": "window/logMessage",
                        "params": {"type": 3, "message": format!("settings {}", p["settings"])}}),
                );
            }
            "textDocument/rename" => {
                // Every `bad` of the document, and the first of a file
                // beside it that is not open.
                let text = texts.get(&uri).cloned().unwrap_or_default();
                let name = p["newName"].as_str().unwrap_or("x");
                let edits: Vec<Value> = text
                    .match_indices("bad")
                    .map(|(at, _)| {
                        json!({"range": {
                        "start": position(&text, at, Encoding::Utf16).to_json(),
                        "end": position(&text, at + 3, Encoding::Utf16).to_json()},
                        "newText": name})
                    })
                    .collect();
                let mut changes = serde_json::Map::new();
                changes.insert(uri.clone(), json!(edits));
                if let Some(path) = crate::uri::to_path(&real(&uri)) {
                    let other = crate::uri::from_path(&path.with_file_name("other.fk"));
                    changes.insert(
                        other,
                        json!([{"range": {"start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 3}}, "newText": name}]),
                    );
                }
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": {"changes": changes}}),
                );
            }
            "textDocument/codeAction" => {
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": [
                        {"title": "Mark the start", "kind": "quickfix", "edit": {"changes": {uri.clone(): [
                            {"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}}, "newText": "@"}]}}},
                        {"title": "Run a command", "command": "fake.cmd", "arguments": [uri.clone()]}
                    ]}),
                );
            }
            "workspace/executeCommand" => {
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": null}),
                );
                // Its edit asked of the editor, as servers do.
                let target = p["arguments"][0].clone();
                let mut changes = serde_json::Map::new();
                if let Some(t) = target.as_str() {
                    changes.insert(
                        t.to_string(),
                        json!([{"range": {"start": {"line": 0, "character": 0},
                        "end": {"line": 0, "character": 0}}, "newText": "#"}]),
                    );
                }
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": 901, "method": "workspace/applyEdit",
                        "params": {"edit": {"changes": changes}}}),
                );
            }
            "textDocument/signatureHelp" => {
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": {
                        "signatures": [{"label": "greet(name, greeting)", "documentation": "Greets.",
                            "parameters": [{"label": "name", "documentation": "Who."}, {"label": "greeting"}]}],
                        "activeSignature": 0, "activeParameter": 0}}),
                );
            }
            "completionItem/resolve" => {
                let mut item = p.clone();
                let label = p["label"].as_str().unwrap_or("").to_string();
                item["documentation"] =
                    json!({"kind": "markdown", "value": format!("Docs of {label}.")});
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": item}),
                );
            }
            // Whether a request came with `params`.
            "fake/params" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": msg.get("params").is_some()}),
            ),
            "shutdown" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": null}),
            ),
            "exit" => std::process::exit(0),
            _ => {
                // The answer to the configuration request: logged back.
                if id == Some(json!(902)) {
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "method": "window/logMessage",
                            "params": {"type": 3, "message": format!("chose {}", msg["result"])}}),
                    );
                } else if id == Some(json!(901)) {
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "method": "window/logMessage",
                            "params": {"type": 3, "message": format!("applied {}", msg["result"]["applied"])}}),
                    );
                } else if id == Some(json!(900)) {
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "method": "window/logMessage",
                        "params": {"type": 3, "message": format!("config {}", msg["result"])}}),
                    );
                } else if let Some(id) = id
                    && !method.is_empty()
                {
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "unknown"}}),
                    );
                }
            }
        }
    }
}
