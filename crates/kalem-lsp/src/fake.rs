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
//! project). Requests of its own, as rust-analyzer has: `fake/expand`
//! (a name and an expansion), `fake/docs` (a page's address),
//! `fake/parent` (the document's first line), `fake/join` (the first two
//! lines joined), `fake/reload` (nothing), `fake/onEnter` (a `//` comment
//! continued, as a snippet with its cursor), `fake/ssr` (`A ==>> B`: each
//! `A` of the document, in the selection when there is one, made `B`; a
//! query without `A` refused). Told it may (the client's capability
//! `experimental.fakeStatus`), it says its state in `fake/status`: half
//! working while a document has `HALF`, busy while one has `BUSY`.
//! `beside` is another server for the same files ([`serve_beside`]). In
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

/// A diagnostic on every one of `words`, with its severity; on `SPAN`,
/// from it to the end of the next line (a problem over two lines).
fn found(text: &str, words: &[(&str, i64)]) -> Vec<Value> {
    let mut list = Vec::new();
    for (word, severity) in words {
        for (at, _) in text.match_indices(word) {
            let end = if *word == "SPAN" {
                let next = text[at..].find('\n').map_or(text.len(), |i| at + i + 1);
                text[next..].find('\n').map_or(text.len(), |i| next + i)
            } else {
                at + word.len()
            };
            list.push(json!({
                "range": { "start": position(text, at, Encoding::Utf16).to_json(),
                           "end": position(text, end, Encoding::Utf16).to_json() },
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

/// A server beside another (`beside`), as a linter serves files a type
/// server serves too: a warning on every `LINT` (its source `lint`), one
/// completion (`lintword`), one code action ("Fix lint"), nothing else.
fn serve_beside() {
    let stdin = std::io::stdin();
    let mut r = BufReader::new(stdin.lock());
    let mut out = std::io::stdout();
    let mut texts: HashMap<String, String> = HashMap::new();
    let send = |out: &mut std::io::Stdout, v: Value| {
        let _ = rpc::write(out, &v);
    };
    let lint = |uri: &str, text: &str| {
        let list: Vec<Value> = text
            .match_indices("LINT")
            .map(|(at, w)| {
                json!({
                    "range": { "start": position(text, at, Encoding::Utf16).to_json(),
                               "end": position(text, at + w.len(), Encoding::Utf16).to_json() },
                    "severity": 2, "source": "lint", "message": "LINT is a lint",
                })
            })
            .collect();
        publish(&json!(real(uri)), &list)
    };
    while let Ok(Some(msg)) = rpc::read(&mut r) {
        let id = msg.get("id").cloned();
        let p = &msg["params"];
        let uri = p["textDocument"]["uri"].as_str().unwrap_or("").to_string();
        match msg["method"].as_str().unwrap_or("") {
            "initialize" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": {"capabilities": {
                    "positionEncoding": "utf-16",
                    "textDocumentSync": {"openClose": true, "change": 1},
                    "completionProvider": {},
                    "codeActionProvider": true,
                }}}),
            ),
            "textDocument/didOpen" | "textDocument/didChange" => {
                let text = p["textDocument"]["text"]
                    .as_str()
                    .or_else(|| p["contentChanges"][0]["text"].as_str())
                    .unwrap_or("")
                    .to_string();
                send(&mut out, lint(&uri, &text));
                texts.insert(uri, text);
            }
            "textDocument/completion" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": [{"label": "lintword", "sortText": "0"}]}),
            ),
            "textDocument/codeAction" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": [
                    {"title": "Fix lint", "kind": "quickfix", "edit": {"changes": {uri.clone(): []}}}
                ]}),
            ),
            "shutdown" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": null}),
            ),
            "exit" => std::process::exit(0),
            _ => {
                if let Some(id) = id
                    && msg.get("method").is_some()
                {
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "not here"}}),
                    );
                }
            }
        }
    }
}

/// Serves on standard input and output until `exit`.
pub fn serve(behavior: &str) {
    if behavior == "beside" {
        serve_beside();
        return;
    }
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
        &[("TODO", 2), ("bad", 1), ("SPAN", 3)]
    };
    let mut saved = false;
    let mut busy = if behavior == "busy" { 2 } else { 0 };
    // `fake/status`: whether the client asked for it, and the last sent.
    let mut tells_status = false;
    let mut told: Option<(bool, bool)> = None;
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
                tells_status = p["capabilities"]["experimental"]["fakeStatus"] == true;
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
                let state = (text.contains("HALF"), text.contains("BUSY"));
                if tells_status && told != Some(state) {
                    told = Some(state);
                    let (half, working) = state;
                    send(
                        &mut out,
                        json!({"jsonrpc": "2.0", "method": "fake/status", "params": {
                            "health": if half { "warning" } else { "ok" },
                            "quiescent": !working,
                            "message": if half { "fake: half working" } else { "" },
                        }}),
                    );
                }
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
            "fake/expand" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result":
                    {"name": "greet!", "expansion": "fn greet() {}"}}),
            ),
            "fake/docs" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": "https://example.org/docs/greet"}),
            ),
            "fake/parent" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": [{"uri": uri,
                    "range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}}}]}),
            ),
            "fake/join" => {
                let text = texts.get(&uri).cloned().unwrap_or_default();
                let result = match text.find('\n') {
                    Some(nl) => json!([{
                        "range": {"start": position(&text, nl, Encoding::Utf16).to_json(),
                                  "end": position(&text, nl + 1, Encoding::Utf16).to_json()},
                        "newText": " ",
                    }]),
                    None => json!([]),
                };
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": result}),
                );
            }
            "fake/reload" => send(
                &mut out,
                json!({"jsonrpc": "2.0", "id": id, "result": null}),
            ),
            "fake/onEnter" => {
                let text = texts.get(&uri).cloned().unwrap_or_default();
                let at = json!({"start": p["position"], "end": p["position"]});
                let result = match byte_range(&text, &at, Encoding::Utf16) {
                    Some(b) => {
                        let bol = text[..b.start].rfind('\n').map_or(0, |i| i + 1);
                        let line = &text[bol..b.start];
                        let indent: String = line.chars().take_while(|c| *c == ' ').collect();
                        if line.trim_start().starts_with("//") {
                            json!([{"range": at, "newText": format!("\n{indent}// $0"),
                                    "insertTextFormat": 2}])
                        } else {
                            Value::Null
                        }
                    }
                    None => Value::Null,
                };
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": result}),
                );
            }
            "fake/ssr" => {
                let text = texts.get(&uri).cloned().unwrap_or_default();
                let query = p["query"].as_str().unwrap_or("");
                let within = p["selections"]
                    .as_array()
                    .and_then(|s| s.first())
                    .and_then(|r| byte_range(&text, r, Encoding::Utf16))
                    .unwrap_or(0..text.len());
                let answer = match query.split_once(" ==>> ") {
                    Some((from, to)) if !from.is_empty() => {
                        let edits: Vec<Value> = text
                            .match_indices(from)
                            .filter(|(at, _)| within.start <= *at && at + from.len() <= within.end)
                            .map(|(at, _)| {
                                json!({"range": {
                                    "start": position(&text, at, Encoding::Utf16).to_json(),
                                    "end": position(&text, at + from.len(), Encoding::Utf16).to_json()},
                                    "newText": to})
                            })
                            .collect();
                        json!({"jsonrpc": "2.0", "id": id, "result": {"changes": {uri: edits}}})
                    }
                    _ => json!({"jsonrpc": "2.0", "id": id, "error":
                        {"code": -32603, "message": "Parse error: nothing to search for"}}),
                };
                send(&mut out, answer);
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
            // A text it cannot read: no edits, as `null` (rust-analyzer's
            // answer when rustfmt fails).
            "textDocument/formatting"
                if texts.get(&uri).is_some_and(|t| t.contains("UNREADABLE")) =>
            {
                send(
                    &mut out,
                    json!({"jsonrpc": "2.0", "id": id, "result": null}),
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
