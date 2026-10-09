//! The client against a fake server: this binary started again with
//! `KALEM_LSP_FAKE` set to a behavior (`normal`, `silent`, `crash`,
//! `garbage`).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kalem_lsp::{Client, Edit, Event, ServerConfig};
use serde_json::json;

fn start(behavior: &str, wakes: Arc<AtomicUsize>) -> Client {
    let config = ServerConfig {
        name: "fake".into(),
        command: std::env::current_exe().unwrap(),
        env: vec![("KALEM_LSP_FAKE".into(), behavior.into())],
        root: std::env::temp_dir(),
        settings: json!({"elixirLS": {"dialyzerEnabled": false}}),
        ..ServerConfig::default()
    };
    Client::start(
        config,
        Arc::new(move || {
            wakes.fetch_add(1, Ordering::Relaxed);
        }),
    )
    .unwrap()
}

fn until(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn normal() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let c = start("normal", wakes.clone());
    let uri = "file:///tmp/a.ex";
    // Sent before the server is ready: queued, then sent whole.
    c.did_open(uri, "elixir", "😀a\nb");
    until("ready", || c.is_ready());
    assert!(c.provides("hoverProvider"));
    assert!(!c.provides("inlayHintProvider"));
    // An incremental change after the emoji: UTF-16 positions.
    c.did_change(
        uri,
        "😀a\nb",
        &[Edit {
            range: 4..5,
            text: "bad".into(),
        }],
        "😀bad\nb",
    );
    until("diagnostics", || !c.diagnostics(uri).is_empty());
    let d = kalem_lsp::features::diagnostics("😀bad\nb", &c.diagnostics(uri), c.encoding());
    assert_eq!(d[0].range, 4..7);
    assert_eq!(d[0].message, "bad found");
    let h = c.request("textDocument/hover", json!({"textDocument": {"uri": uri}, "position": kalem_lsp::position::position("😀bad\nb", 5, c.encoding()).to_json()}));
    let v = h.wait(Duration::from_secs(5)).unwrap();
    assert_eq!(kalem_lsp::features::hover_text(&v).unwrap(), "at 0:3");
    until("config", || {
        c.log().iter().any(|l| l.contains("dialyzerEnabled"))
    });
    assert!(c.take_events().iter().any(|e| matches!(e, Event::Ready)));
    assert!(wakes.load(Ordering::Relaxed) > 0);
    c.shutdown();
    until("exit", || c.has_exited());
}

fn silent() {
    let c = start("silent", Arc::new(AtomicUsize::new(0)));
    let p = c.request("textDocument/hover", json!({}));
    // Never answered: the caller is not blocked, and a wait times out.
    assert!(p.poll().is_none());
    assert!(p.wait(Duration::from_millis(100)).is_err());
    let t = Instant::now();
    drop(c);
    assert!(t.elapsed() < Duration::from_secs(3));
}

fn crash() {
    let c = start("crash", Arc::new(AtomicUsize::new(0)));
    until("ready", || c.is_ready());
    c.did_open("file:///x.ex", "elixir", "x");
    until("exit", || c.has_exited());
    assert!(
        c.take_events()
            .iter()
            .any(|e| matches!(e, Event::Exited { code: Some(3) }))
    );
    let p = c.request("textDocument/hover", json!({}));
    assert!(p.wait(Duration::from_secs(1)).is_err());
}

fn log_work() {
    // Work told only in the log: progress from its start line until the
    // first diagnostics.
    let config = ServerConfig {
        name: "fake".into(),
        command: std::env::current_exe().unwrap(),
        env: vec![("KALEM_LSP_FAKE".into(), "normal".into())],
        root: std::env::temp_dir(),
        settings: json!({"elixirLS": {}}),
        busy_start: vec!["config".into()],
        busy_done: vec!["never said".into()],
        ..ServerConfig::default()
    };
    let c = Client::start(config, Arc::new(|| {})).unwrap();
    until("log work", || {
        c.progress().is_some_and(|p| p.starts_with("config"))
    });
    c.did_open("file:///tmp/w.ex", "elixir", "x");
    c.did_change(
        "file:///tmp/w.ex",
        "x",
        &[Edit {
            range: 0..1,
            text: "bad".into(),
        }],
        "bad",
    );
    until("diagnostics end it", || c.progress().is_none());
}

fn chatty() {
    let c = start("chatty", Arc::new(AtomicUsize::new(0)));
    until("ready", || c.is_ready());
    // Its standard error read past a line that is not UTF-8.
    until("stderr", || {
        c.log().iter().any(|l| l == "[stderr] after the odd byte")
    });
    assert!(c.log().iter().any(|l| l == "[stderr] caf\u{fffd} compiled"));
    // The title kept whole, each report's message after it.
    until("progress", || {
        c.progress().as_deref() == Some("Building: app: b.ex")
    });
    // A question is said, and answered with no choice.
    until("answered", || c.log().iter().any(|l| l == "chose null"));
    assert!(c.take_events().iter().any(|e| matches!(
        e,
        Event::Message { level: 2, text } if text == "Fetch the dependencies?"
    )));
    // A message after stray output is still read.
    until("after the stray output", || {
        c.log().iter().any(|l| l == "after the stray output")
    });
    // No `params` when there are none.
    let p = c.request("fake/params", serde_json::Value::Null);
    assert_eq!(p.wait(Duration::from_secs(5)).unwrap(), json!(false));
    let p = c.request("fake/params", json!({}));
    assert_eq!(p.wait(Duration::from_secs(5)).unwrap(), json!(true));
    c.shutdown();
}

fn refuse() {
    let c = start("refuse", Arc::new(AtomicUsize::new(0)));
    // Refused `initialize`: said, and the process ended, not "starting".
    until("exit", || c.has_exited());
    let events = c.take_events();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Message { level: 1, text } if text.contains("no project")))
    );
    assert!(!c.is_ready());
}

fn garbage() {
    let c = start("garbage", Arc::new(AtomicUsize::new(0)));
    until("ready", || c.is_ready());
    assert!(c.log().iter().any(|l| l.contains("malformed")));
}

fn main() {
    // nextest asks each test binary for its tests before it runs them:
    // this one is a single test, `main` (docs/ci_todo.md, C4).
    if std::env::args().any(|a| a == "--list") {
        if !std::env::args().any(|a| a == "--ignored") {
            println!("main: test");
        }
        return;
    }
    if let Ok(b) = std::env::var("KALEM_LSP_FAKE") {
        kalem_lsp::fake::serve(&b);
        return;
    }
    for (name, f) in [
        ("normal", normal as fn()),
        ("silent", silent),
        ("crash", crash),
        ("refuse", refuse),
        ("log work", log_work),
        ("garbage", garbage),
        ("chatty", chatty),
    ] {
        f();
        println!("test {name} ... ok");
    }
}
