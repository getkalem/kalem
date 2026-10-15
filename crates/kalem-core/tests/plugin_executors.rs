//! A plugin's language (plugin API 0.2.21, the `executor` export) run by
//! Babel: only once `babel.load_languages` names it, its default header
//! arguments merged first, its program run and its output read by the
//! plugin, or its value given at once; its refusal and a busy plugin said.
//! Its own process: it sets the executors' bridge.

#![cfg(unix)]

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use kalem_core::babel::plugins::{self, Answer, Call, Data, Prepared};
use kalem_core::command::{Clipboard, CommandRegistry, EditorContext};
use kalem_core::settings::{Config, Layer};

const LANGUAGE: &str = "upper";

fn register() {
    plugins::register(plugins::PluginExecutorSpec {
        plugin: "extest".into(),
        language: LANGUAGE.into(),
        headers: ":results output :mode up".into(),
    });
    plugins::set_provider(Arc::new(|spec, call| {
        assert_eq!(spec.plugin, "extest");
        match call {
            Call::Prepare(b) => {
                let mode = b
                    .headers
                    .iter()
                    .rev()
                    .find(|(k, _)| k == ":mode")
                    .map(|(_, v)| v.as_str());
                match mode {
                    Some("up") => {
                        Answer::Prepared(Prepared::Run(kalem_core::extensions::ProcessRequest {
                            program: "/usr/bin/tr".into(),
                            args: vec!["a-z".into(), "A-Z".into()],
                            cwd: b.dir.clone(),
                            stdin: Some(b.body.clone().into_bytes()),
                            env: Vec::new(),
                        }))
                    }
                    Some("vars") => Answer::Prepared(Prepared::Done(Data::Table(
                        b.vars
                            .iter()
                            .map(|(n, v)| {
                                let shown = match v {
                                    Data::Text(t) => t.clone(),
                                    Data::Items(i) => i.join(","),
                                    Data::Table(rows) => format!("{} rows", rows.len()),
                                    Data::Output(_) => String::new(),
                                };
                                Some(vec![n.clone(), shown])
                            })
                            .collect(),
                    ))),
                    Some("busy") => Answer::Busy,
                    _ => Answer::Refused("no such mode".into()),
                }
            }
            Call::Finish(b, output, status) => {
                assert_eq!(status, Some(0));
                // The `:results` words merged: the language's `output` kept.
                assert!(b.results.iter().any(|r| r == "output"), "{:?}", b.results);
                Answer::Value(Data::Output(output.to_string()))
            }
        }
    }));
}

fn run(text: &str, loaded: &[&str]) -> Result<String, String> {
    let loaded: Vec<String> = loaded.iter().map(|s| (*s).to_string()).collect();
    let pos = text.find("#+begin_src").expect("a block") + 1;
    kalem_core::babel::run_now(text, pos, &loaded, "", Path::new("/tmp")).map(|r| r.text)
}

#[test]
fn a_plugins_language_runs_as_babel_runs_its_own() {
    if !Path::new("/usr/bin/tr").exists() {
        return;
    }
    register();
    let block = "#+begin_src upper\nabc\ndef\n#+end_src\n";
    // Not loaded: Emacs's error.
    assert_eq!(
        run(block, &["shell"]),
        Err("No org-babel-execute function for upper!".into())
    );
    // Loaded: its program run, its output read as text (`:results output`
    // from the language's defaults).
    assert_eq!(
        run(block, &[LANGUAGE]).as_deref(),
        Ok("#+begin_src upper\nabc\ndef\n#+end_src\n\n#+RESULTS:\n: ABC\n: DEF\n")
    );
    // The document's header arguments after the language's: `:results
    // table` reads the output as a table (one line is not split, as in
    // Emacs).
    let table = "#+begin_src upper :results output table\na b\nc d\n#+end_src\n";
    assert!(
        run(table, &[LANGUAGE])
            .expect("run")
            .ends_with("#+RESULTS:\n| A | B |\n| C | D |\n")
    );
    // Variables given as values; the value at once, as a table.
    let vars = "#+name: t\n| a | 1 |\n|---|---|\n\n#+begin_src upper :mode vars :var n=2 :var t=t\n#+end_src\n";
    let got = run(vars, &[LANGUAGE]).expect("run");
    // Half of the column numbers: aligned right, as `org-table-align` does.
    assert!(
        got.ends_with("#+RESULTS:\n| n |      2 |\n| t | 2 rows |\n"),
        "{got}"
    );
    // The plugin's refusal, and a busy plugin.
    let refused = "#+begin_src upper :mode down\nx\n#+end_src\n";
    assert_eq!(run(refused, &[LANGUAGE]), Err("no such mode".into()));
    let busy = "#+begin_src upper :mode busy\nx\n#+end_src\n";
    assert!(run(busy, &[LANGUAGE]).is_err_and(|e| e.contains("busy")));
}

#[test]
fn the_command_writes_a_value_given_at_once() {
    register();
    let config = Config::from_layers(&[(
        Layer::User,
        None,
        "babel.confirm_evaluate = false\nbabel.load_languages = [\"upper\"]\n",
    )]);
    let text = "#+begin_src upper :mode vars :var x=5\n#+end_src\n";
    let mut doc = kalem_core::DocumentState::new(
        text,
        kalem_core::document::Metadata {
            path: None,
            mode: kalem_core::mode::DocumentMode::Org,
            line_ending: kalem_core::document::LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        },
        Arc::new(org_model::Settings::default()),
    );
    doc.selection = org_edit::Selection::caret(3);
    let reg = CommandRegistry::with_builtins();
    let mut clip = Clipboard::default();
    let mut ctx = EditorContext {
        document: Some(&mut doc),
        clipboard: &mut clip,
        config: &config,
        now: Instant::now(),
        clock: jiff::civil::date(2026, 10, 15).at(9, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    reg.execute("org.babel.execute", &mut ctx, &serde_json::json!({}))
        .expect("run");
    assert!(
        ctx.messages
            .last()
            .is_some_and(|m| m.starts_with("Code block evaluation complete")),
        "{:?}",
        ctx.messages
    );
    drop(ctx);
    assert_eq!(
        doc.text().as_str(),
        "#+begin_src upper :mode vars :var x=5\n#+end_src\n\n#+RESULTS:\n| x | 5 |\n"
    );
}
