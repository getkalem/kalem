//! A fake language server for tests (of this crate and of the editor's
//! service): a test binary started again with an environment variable
//! calls [`serve`] and speaks the protocol on its standard streams.
//!
//! Behaviors: `normal`; `silent` (never answers `initialize`); `crash`
//! (exits with 3 on the first `didOpen`); `garbage` (a malformed message
//! first). In `normal`, a change whose text contains `CRASH` exits with 4.

#![allow(clippy::print_stdout)]

use std::collections::HashMap;
use std::io::{BufReader, Write};

use serde_json::{Value, json};

use crate::position::{Encoding, byte_range, position};
use crate::rpc;

fn diagnostics(uri: &Value, text: &str) -> Value {
    // A warning on every `TODO`, an error on every `bad`.
    let mut list = Vec::new();
    for (word, severity) in [("TODO", 2), ("bad", 1)] {
        for (at, _) in text.match_indices(word) {
            list.push(json!({
                "range": { "start": position(text, at, Encoding::Utf16).to_json(),
                           "end": position(text, at + word.len(), Encoding::Utf16).to_json() },
                "severity": severity, "source": "fake", "message": format!("{word} found"),
            }));
        }
    }
    json!({"jsonrpc": "2.0", "method": "textDocument/publishDiagnostics",
           "params": {"uri": uri, "diagnostics": list}})
}

/// A file's URI with links resolved, as Expert names files.
fn real(uri: &str) -> String {
    crate::uri::to_path(uri)
        .and_then(|p| std::fs::canonicalize(p).ok())
        .map_or_else(|| uri.to_string(), |p| crate::uri::from_path(&p))
}

/// Serves on standard input and output until `exit`.
pub fn serve(behavior: &str) {
    let stdin = std::io::stdin();
    let mut r = BufReader::new(stdin.lock());
    let mut out = std::io::stdout();
    let mut texts: HashMap<String, String> = HashMap::new();
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
                if behavior == "silent" {
                    continue;
                }
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {
                        "positionEncoding": "utf-16",
                        "textDocumentSync": {"openClose": true, "change": 2, "save": {"includeText": true}},
                        "hoverProvider": true,
                        "definitionProvider": true,
                        "referencesProvider": true,
                        "documentFormattingProvider": true,
                        "completionProvider": {"triggerCharacters": ["."], "resolveProvider": true},
                    }}}),
                );
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
                send(&mut out, diagnostics(&json!(real(&uri)), &text));
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
                send(&mut out, diagnostics(&json!(real(&uri)), text));
            }
            "textDocument/hover" => {
                let pos = &p["position"];
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": {"contents": {"kind": "markdown",
                    "value": format!("at {}:{}", pos["line"], pos["character"])}}}),
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
                        {"label": "goodbye/0", "kind": 3, "insertText": "goodbye()", "sortText": "2"},
                    ]}}),
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
            "shutdown" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": null}),
            ),
            "exit" => std::process::exit(0),
            _ => {
                // The answer to the configuration request: logged back.
                if id == Some(json!(900)) {
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
