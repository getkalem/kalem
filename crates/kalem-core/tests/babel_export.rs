//! Babel on export (`org-babel-exp-process-buffer`): the blocks whose
//! `:exports` is `results` or `both` run first, on a copy, as a run by hand
//! does and with the export's `:eval` values; a language not loaded is
//! left with Emacs's warning; the export reads the copy and the document is
//! not changed. Both the command line's way (all at once, asked on the
//! terminal) and the editor's (a question, a run beside the editor, then
//! the export again). Its own process: the editor's way queues commands.

#![cfg(unix)]

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kalem_core::babel::export::{self, Settings};
use kalem_core::command::{Clipboard, CommandRegistry, EditorContext, Request};
use kalem_core::settings::{Config, Layer};

fn sh_installed() -> bool {
    Path::new("/bin/sh").exists()
}

const DOC: &str = "\
* Notes

#+begin_src sh
echo code only
#+end_src

#+begin_src sh :exports results
echo results
#+end_src

#+begin_src sh :exports both :eval never-export
echo refused
#+end_src

#+begin_src python :exports both
print(1)
#+end_src

#+begin_src sh :exports both
echo both
#+end_src
";

#[test]
fn the_command_lines_way() {
    if !sh_installed() {
        return;
    }
    let settings = Settings {
        loaded: vec!["shell".into()],
        python: String::new(),
        confirm: true,
    };
    let mut asked = Vec::new();
    let mut said = Vec::new();
    // Yes to the first question, no to the second.
    let mut answers = vec![Some(false), Some(true)];
    let out = export::evaluate_now(
        DOC,
        None,
        None,
        &settings,
        &mut |q| {
            asked.push(q.to_string());
            answers.pop().flatten().map(Some).unwrap_or(None)
        },
        &mut |m| said.push(m),
    );
    assert_eq!(asked.len(), 2, "{asked:?}");
    assert!(asked[0].starts_with("Evaluate this sh code block"));
    // The first ran; the second was refused by its answer.
    assert!(
        out.contains("echo results\n#+end_src\n\n#+RESULTS:\n: results\n"),
        "{out}"
    );
    assert!(!out.contains(": both"), "{out}");
    assert!(!out.contains("code only\n#+end_src\n\n#+RESULTS"), "{out}");
    assert_eq!(
        said,
        [
            "Evaluation of this sh code block is disabled.",
            "org-export: No org-babel-execute function for python.  Not updating exported results.",
            "Evaluation of this sh code block is aborted.",
        ]
    );
    // Nothing asked where it cannot ask: a no.
    let none = export::evaluate_now(DOC, None, None, &settings, &mut |_| None, &mut |_| {});
    assert_eq!(none, DOC);
}

fn doc_at(path: &Path) -> kalem_core::DocumentState {
    kalem_core::DocumentState::open(
        path,
        Arc::new(org_model::Settings::default()),
        &org_syntax::ParseContext::default(),
    )
    .expect("open")
}

fn run(
    reg: &CommandRegistry,
    config: &Config,
    doc: &mut kalem_core::DocumentState,
    id: &str,
    args: serde_json::Value,
) -> Vec<Request> {
    let mut clip = Clipboard::default();
    let mut ctx = EditorContext {
        document: Some(doc),
        clipboard: &mut clip,
        config,
        now: Instant::now(),
        clock: jiff::civil::date(2026, 10, 15).at(9, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    reg.execute(id, &mut ctx, &args).expect(id);
    ctx.requests
}

#[test]
fn the_editors_way() {
    if !sh_installed() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("kalem-babel-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("folder");
    let file = dir.join("n.org");
    std::fs::write(&file, DOC).expect("file");
    let config =
        Config::from_layers(&[(Layer::User, None, "babel.load_languages = [\"shell\"]\n")]);
    let reg = CommandRegistry::with_builtins();
    let mut doc = doc_at(&file);
    let html = dir.join("n.html");
    // The export asks first, and writes nothing yet.
    let requests = run(
        &reg,
        &config,
        &mut doc,
        "export.html",
        serde_json::json!({}),
    );
    let Some(Request::Ask { command, args, .. }) = requests.first() else {
        panic!("{requests:?}");
    };
    assert_eq!(command, "org.babel.exportStep");
    assert!(!html.exists());
    let job = args["job"].clone();
    // A yes runs the block beside the editor; its end queues the next step.
    let mut requests = run(
        &reg,
        &config,
        &mut doc,
        "org.babel.exportStep",
        serde_json::json!({ "job": job, "answer": "yes" }),
    );
    let mut rounds = 0;
    let export = loop {
        rounds += 1;
        assert!(rounds < 200, "the export never ended");
        if let Some(Request::Run { command, args }) = requests
            .iter()
            .find(|r| matches!(r, Request::Run { command, .. } if command == "export.html"))
        {
            break (command.clone(), args.clone());
        }
        if let Some(Request::Ask { args, .. }) =
            requests.iter().find(|r| matches!(r, Request::Ask { .. }))
        {
            // The second block's question: yes too.
            requests = run(
                &reg,
                &config,
                &mut doc,
                "org.babel.exportStep",
                serde_json::json!({ "job": args["job"], "answer": "yes" }),
            );
            continue;
        }
        std::thread::sleep(Duration::from_millis(20));
        requests = Vec::new();
        for (id, args) in kalem_core::extensions::take_runs() {
            if id == "org.babel.exportStep" {
                requests.extend(run(&reg, &config, &mut doc, &id, args));
            }
        }
    };
    // The export again, with the copy: both blocks' results in the page,
    // the document as it was.
    run(&reg, &config, &mut doc, &export.0, export.1);
    let page = std::fs::read_to_string(&html).expect("exported");
    assert!(
        page.contains("<pre class=\"example\">\nresults\n</pre>"),
        "{page}"
    );
    assert!(
        page.contains("<pre class=\"example\">\nboth\n</pre>"),
        "{page}"
    );
    assert!(!page.contains("echo results"), "{page}");
    assert!(page.contains("echo both"), "{page}");
    assert_eq!(doc.text().as_str(), DOC);
    assert_eq!(std::fs::read_to_string(&file).expect("file"), DOC);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn off_or_nothing_to_run_exports_at_once() {
    let dir = std::env::temp_dir().join(format!("kalem-babel-export-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("folder");
    let file = dir.join("n.org");
    std::fs::write(&file, DOC).expect("file");
    let reg = CommandRegistry::with_builtins();
    let mut doc = doc_at(&file);
    // `export.use_babel` off: no question, the page written.
    let off = Config::from_layers(&[(
        Layer::User,
        None,
        "babel.load_languages = [\"shell\"]\nexport.use_babel = false\n",
    )]);
    let requests = run(&reg, &off, &mut doc, "export.html", serde_json::json!({}));
    assert!(!requests.iter().any(|r| matches!(r, Request::Ask { .. })));
    assert!(dir.join("n.html").exists());
    std::fs::remove_file(dir.join("n.html")).expect("remove");
    // No language loaded: none runs, the page written at once.
    let requests = run(
        &reg,
        &Config::default(),
        &mut doc,
        "export.html",
        serde_json::json!({}),
    );
    assert!(!requests.iter().any(|r| matches!(r, Request::Ask { .. })));
    assert!(dir.join("n.html").exists());
    let _ = std::fs::remove_dir_all(&dir);
}
