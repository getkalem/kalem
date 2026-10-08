//! The `extension` world end to end: the plugin of `tests/plugins/counter`,
//! built against `kalem-plugin`'s `kalem` namespace, registering commands,
//! a binding and subscriptions, asking questions and filling a panel in a
//! fake editor through the host. Skipped
//! where the `wasm32-unknown-unknown` target or `wasm-tools` is not
//! installed.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod common;

use kalem_script::extension::{
    Answer, CommandSpec, Editor, Event, EventKind, Extension, Grants, Level, PanelEvent, PanelSpec,
    Question, Reply, StatusOptions, VERSION, WidgetKind, WidgetTree, api,
};
use kalem_script::extension::{documents, http, process};
use kalem_script::{Error, Host, Limits};

/// What the fake editor holds.
#[derive(Debug, Default)]
struct State {
    commands: BTreeMap<String, CommandSpec>,
    bindings: BTreeMap<u64, (String, String)>,
    ran: Vec<(String, String)>,
    notes: Vec<(Level, String)>,
    questions: BTreeMap<u64, Question>,
    status: BTreeMap<String, String>,
    panels: BTreeMap<String, Option<WidgetTree>>,
    own: BTreeMap<String, String>,
    workspace: Vec<std::path::PathBuf>,
    fetched: Vec<(u64, String)>,
    spawned: Vec<(u64, String, Vec<String>, std::path::PathBuf)>,
    killed: Vec<u64>,
    /// The documents plugins write: plugin, spec, text and cursor.
    documents: BTreeMap<u64, (String, documents::DocumentSpec, String, Option<u64>)>,
    closed: Vec<u64>,
    /// The marks plugins set: plugin, then line and kind, by file.
    gutters: BTreeMap<std::path::PathBuf, (String, Vec<(u32, String)>)>,
}

#[derive(Debug, Clone, Default)]
struct Fake(Arc<Mutex<State>>);

impl Editor for Fake {
    fn add_command(&mut self, plugin: &str, spec: &CommandSpec) -> Result<(), String> {
        assert!(
            plugin == "counter" || plugin == "reach" || plugin == "run",
            "{plugin}"
        );
        let mut s = self.0.lock().unwrap();
        if s.commands.contains_key(&spec.id) {
            return Err("taken".into());
        }
        s.commands.insert(spec.id.clone(), spec.clone());
        Ok(())
    }

    fn remove_command(&mut self, id: &str) {
        self.0.lock().unwrap().commands.remove(id);
    }

    fn add_binding(
        &mut self,
        id: u64,
        keys: &str,
        command: &str,
        _when: Option<&str>,
    ) -> Result<(), String> {
        self.0
            .lock()
            .unwrap()
            .bindings
            .insert(id, (keys.into(), command.into()));
        Ok(())
    }

    fn remove_binding(&mut self, id: u64) {
        self.0.lock().unwrap().bindings.remove(&id);
    }

    fn run(&mut self, id: &str, args: &str) -> Result<(), String> {
        self.0.lock().unwrap().ran.push((id.into(), args.into()));
        Ok(())
    }

    fn notify(&mut self, _plugin: &str, message: &str, level: Level) {
        self.0.lock().unwrap().notes.push((level, message.into()));
    }

    fn ask(&mut self, _plugin: &str, request: u64, question: Question) {
        self.0.lock().unwrap().questions.insert(request, question);
    }

    fn withdraw(&mut self, request: u64) {
        self.0.lock().unwrap().questions.remove(&request);
    }

    fn set_status(&mut self, _plugin: &str, id: &str, text: &str, _options: &StatusOptions) {
        self.0.lock().unwrap().status.insert(id.into(), text.into());
    }

    fn remove_status(&mut self, _plugin: &str, id: &str) {
        self.0.lock().unwrap().status.remove(id);
    }

    fn add_panel(&mut self, _plugin: &str, spec: &PanelSpec) -> Result<(), String> {
        self.0.lock().unwrap().panels.insert(spec.id.clone(), None);
        Ok(())
    }

    fn set_panel(&mut self, id: &str, tree: &WidgetTree) {
        self.0
            .lock()
            .unwrap()
            .panels
            .insert(id.into(), Some(tree.clone()));
    }

    fn remove_panel(&mut self, id: &str) {
        self.0.lock().unwrap().panels.remove(id);
    }

    fn setting(&mut self, key: &str) -> Option<String> {
        (key == "editor.font_size").then(|| "14".into())
    }

    fn own_setting(&mut self, _plugin: &str, key: &str) -> Option<String> {
        self.0.lock().unwrap().own.get(key).cloned()
    }

    fn set_own_setting(&mut self, _plugin: &str, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().unwrap().own.insert(key.into(), value.into());
        Ok(())
    }

    fn workspace(&mut self) -> Vec<std::path::PathBuf> {
        self.0.lock().unwrap().workspace.clone()
    }

    fn document(&mut self) -> Option<Box<dyn kalem_script::extension::DocumentAccess + '_>> {
        None
    }

    fn fetch(&mut self, _plugin: &str, id: u64, request: http::Request) {
        self.0.lock().unwrap().fetched.push((id, request.url));
    }

    fn spawn(
        &mut self,
        _plugin: &str,
        run: u64,
        cwd: std::path::PathBuf,
        command: process::Command,
    ) {
        self.0
            .lock()
            .unwrap()
            .spawned
            .push((run, command.program, command.args, cwd));
    }

    fn kill(&mut self, run: u64) {
        self.0.lock().unwrap().killed.push(run);
    }

    fn open_document(
        &mut self,
        plugin: &str,
        spec: documents::DocumentSpec,
        text: String,
        cursor: Option<u64>,
    ) -> Result<u64, String> {
        let mut s = self.0.lock().unwrap();
        let found = s
            .documents
            .iter()
            .find(|(_, (p, d, _, _))| p == plugin && d.id == spec.id && d.key == spec.key)
            .map(|(n, _)| *n);
        let n = found.unwrap_or(s.documents.len() as u64 + s.closed.len() as u64 + 1);
        s.documents.insert(n, (plugin.into(), spec, text, cursor));
        Ok(n)
    }

    fn set_document(
        &mut self,
        plugin: &str,
        doc: u64,
        text: String,
        cursor: Option<u64>,
    ) -> Result<(), String> {
        let mut s = self.0.lock().unwrap();
        match s.documents.get_mut(&doc) {
            Some(d) if d.0 == plugin => {
                d.2 = text;
                d.3 = cursor;
                Ok(())
            }
            _ => Err(format!("The plugin has no document {doc}: closed")),
        }
    }

    fn set_gutter(
        &mut self,
        plugin: &str,
        path: std::path::PathBuf,
        marks: Vec<kalem_script::extension::decorations::LineMark>,
    ) -> Result<(), String> {
        self.0.lock().unwrap().gutters.insert(
            path,
            (
                plugin.to_string(),
                marks
                    .iter()
                    .map(|m| (m.line, format!("{:?}", m.kind)))
                    .collect(),
            ),
        );
        Ok(())
    }

    fn clear_gutter(&mut self, _plugin: &str, path: Option<std::path::PathBuf>) {
        let mut s = self.0.lock().unwrap();
        match path {
            Some(p) => {
                s.gutters.remove(&p);
            }
            None => s.gutters.clear(),
        }
    }

    fn close_document(&mut self, plugin: &str, doc: u64) {
        let mut s = self.0.lock().unwrap();
        if s.documents.get(&doc).is_some_and(|d| d.0 == plugin) {
            s.documents.remove(&doc);
            s.closed.push(doc);
        }
    }
}

/// The labels of panel `id`'s widgets, in the tree's order.
fn labels(fake: &Fake, id: &str) -> Vec<String> {
    let s = fake.0.lock().unwrap();
    let tree = s.panels[id].as_ref().expect("filled");
    tree.widgets
        .iter()
        .filter_map(|w| match &w.kind {
            WidgetKind::Label(l) => Some(l.text.clone()),
            WidgetKind::Button(b) => Some(b.label.clone()),
            WidgetKind::Item(i) => Some(i.label.clone()),
            _ => None,
        })
        .collect()
}

/// The counter plugin activated in a fake editor.
fn counter(limits: Limits) -> Option<(Extension, Fake)> {
    let bytes = common::component("counter")?;
    let host = Host::new(None).unwrap();
    let plugin = host.load(&bytes).unwrap();
    let fake = Fake::default();
    let mut ext = Extension::new(
        &host,
        &plugin,
        "counter",
        Box::new(fake.clone()),
        Grants::default(),
        limits,
    )
    .unwrap();
    ext.activate().unwrap().unwrap();
    Some((ext, fake))
}

fn run(ext: &mut Extension, id: &str) -> Result<String, String> {
    ext.run_command(id, "null").unwrap()
}

#[test]
fn a_plugin_registers_its_commands_and_runs_them() {
    let Some((mut ext, fake)) = counter(Limits::default()) else {
        return;
    };
    {
        let s = fake.0.lock().unwrap();
        let count = &s.commands["counter.count"];
        assert_eq!(count.title, "Count");
        assert_eq!(count.scope.types, Some(vec!["org".to_string()]));
        assert_eq!(count.keys, ["ctrl+alt+c"]);
        assert_eq!(count.when.as_deref(), Some("!inTable"));
        assert_eq!(
            s.commands["counter.spin"].scope.except,
            ["csv"],
            "a scope with exceptions"
        );
        let bound: Vec<_> = s.bindings.values().collect();
        assert_eq!(
            bound,
            [&("space c c".to_string(), "counter.count".to_string())]
        );
    }
    assert!(ext.commands().contains("counter.count"));

    assert_eq!(run(&mut ext, "counter.count"), Ok("1".into()));
    assert_eq!(run(&mut ext, "counter.count"), Ok("2".into()), "state kept");
    assert_eq!(run(&mut ext, "counter.version"), Ok(VERSION.into()));
    assert!(run(&mut ext, "counter.unknown").is_err());

    // Another's command, through the editor.
    assert_eq!(
        ext.run_command("counter.cycle", "{\"n\":1}").unwrap(),
        Ok("null".into())
    );
    assert_eq!(
        fake.0.lock().unwrap().ran,
        [("org.todo.cycle".to_string(), "{\"n\":1}".to_string())]
    );
}

#[test]
fn the_host_refuses_what_is_not_the_plugins() {
    let Some((mut ext, fake)) = counter(Limits::default()) else {
        return;
    };
    let out = run(&mut ext, "counter.refused").unwrap();
    let errors: Vec<&str> = out.lines().collect();
    assert_eq!(errors.len(), 4, "{out}");
    assert!(errors[0].contains("not the plugin's"), "{}", errors[0]);
    assert!(errors[1].contains("no scope"), "{}", errors[1]);
    assert!(errors[2].contains("already registered"), "{}", errors[2]);
    assert!(errors[3].contains("the plugin's own"), "{}", errors[3]);
    let s = fake.0.lock().unwrap();
    assert!(!s.commands.contains_key("other.thing"));
    assert!(!s.commands.contains_key("counter.none"));
}

#[test]
fn events_reach_the_subscriptions_and_a_veto_stops_a_save() {
    let Some((mut ext, _)) = counter(Limits::default()) else {
        return;
    };
    assert!(ext.wants(EventKind::DocumentOpen));
    assert!(!ext.wants(EventKind::DocumentClose));
    for path in ["a.org", "b.org"] {
        let e = Event::DocumentOpen(api::DocumentOpened {
            document: 1,
            path: Some(path.into()),
        });
        assert!(matches!(ext.event(&e).unwrap(), Reply::Proceed));
    }
    assert_eq!(run(&mut ext, "counter.opened"), Ok("a.org,b.org".into()));

    let save = |path: &str| {
        Event::DocumentBeforeSave(api::DocumentSaved {
            document: 1,
            path: path.into(),
        })
    };
    assert!(matches!(ext.event(&save("a.org")).unwrap(), Reply::Proceed));
    match ext.event(&save("x.lock")).unwrap() {
        Reply::Veto(why) => assert_eq!(why, "a lock file"),
        Reply::Proceed => panic!("not vetoed"),
    }

    // A subscription disposed hears nothing more.
    run(&mut ext, "counter.deaf").unwrap();
    assert!(!ext.wants(EventKind::DocumentOpen));
    let e = Event::DocumentOpen(api::DocumentOpened {
        document: 2,
        path: None,
    });
    ext.event(&e).unwrap();
    assert_eq!(run(&mut ext, "counter.opened"), Ok("a.org,b.org".into()));
}

#[test]
fn deactivation_takes_back_everything() {
    let Some((mut ext, fake)) = counter(Limits::default()) else {
        return;
    };
    assert!(!fake.0.lock().unwrap().commands.is_empty());
    ext.deactivate().unwrap();
    let s = fake.0.lock().unwrap();
    assert!(s.commands.is_empty(), "{:?}", s.commands.keys());
    assert!(s.bindings.is_empty());
    assert!(ext.commands().is_empty());
    assert!(!ext.wants(EventKind::DocumentBeforeSave));
}

#[test]
fn a_command_that_loops_is_stopped() {
    let limits = Limits {
        time: Duration::from_millis(50),
        ..Limits::default()
    };
    let Some((mut ext, fake)) = counter(limits) else {
        return;
    };
    assert!(matches!(
        ext.run_command("counter.spin", "null"),
        Err(Error::Timeout(_))
    ));
    // The instance is spent; its registrations still go.
    ext.deactivate().ok();
    assert!(fake.0.lock().unwrap().commands.is_empty());
}

#[test]
fn questions_are_answered_later() {
    let Some((mut ext, fake)) = counter(Limits::default()) else {
        return;
    };
    run(&mut ext, "counter.ask").unwrap();
    let asked: Vec<(u64, Question)> = fake
        .0
        .lock()
        .unwrap()
        .questions
        .iter()
        .map(|(r, q)| (*r, q.clone()))
        .collect();
    assert_eq!(asked.len(), 3);
    let (confirm, prompt, pick) = (asked[0].0, asked[1].0, asked[2].0);
    assert!(matches!(&asked[0].1, Question::Confirm(m) if m == "Sure?"));
    assert!(matches!(&asked[1].1, Question::Prompt { title, .. } if title == "Name?"));
    assert!(
        matches!(&asked[2].1, Question::Pick { items, options } if items.len() == 2 && options.many)
    );

    assert!(ext.answer(confirm, Answer::Confirmed(true)).unwrap());
    assert!(
        !ext.answer(confirm, Answer::Confirmed(true)).unwrap(),
        "answered once"
    );
    assert!(
        !ext.answer(999, Answer::Confirmed(true)).unwrap(),
        "never asked"
    );
    ext.answer(prompt, Answer::Text(Some("Ada".into())))
        .unwrap();
    ext.answer(pick, Answer::Picked(vec![1])).unwrap();
    let s = fake.0.lock().unwrap();
    assert_eq!(
        s.notes,
        [(Level::Info, "yes".into()), (Level::Warning, "[1]".into())]
    );
    assert_eq!(s.status.get("name").map(String::as_str), Some("Ada"));
}

#[test]
fn a_panel_is_filled_and_hears_its_widgets() {
    let Some((mut ext, fake)) = counter(Limits::default()) else {
        return;
    };
    assert_eq!(
        labels(&fake, "counter.panel"),
        ["0", "Add one", "Items", "First"]
    );
    assert!(
        ext.panel_event("counter.panel", "add", &PanelEvent::Clicked)
            .unwrap()
    );
    assert_eq!(labels(&fake, "counter.panel")[0], "1");
    assert!(
        !ext.panel_event("other.panel", "add", &PanelEvent::Clicked)
            .unwrap(),
        "not its panel"
    );

    let out = run(&mut ext, "counter.badTrees").unwrap();
    let errors: Vec<&str> = out.lines().collect();
    assert_eq!(errors.len(), 6, "{out}");
    assert!(errors.iter().all(|e| !e.is_empty()), "{out}");
    assert_eq!(labels(&fake, "counter.panel")[0], "1", "the panel kept");
    let foreign = run(&mut ext, "counter.foreignPanel").unwrap_err();
    assert!(foreign.contains("not the plugin's"), "{foreign}");
}

#[test]
fn deactivation_closes_questions_status_and_panels() {
    let Some((mut ext, fake)) = counter(Limits::default()) else {
        return;
    };
    run(&mut ext, "counter.ask").unwrap();
    let prompt = *fake.0.lock().unwrap().questions.keys().nth(1).unwrap();
    ext.answer(prompt, Answer::Text(None)).unwrap();
    assert_eq!(
        fake.0
            .lock()
            .unwrap()
            .status
            .get("name")
            .map(String::as_str),
        Some("nobody")
    );
    ext.deactivate().unwrap();
    let s = fake.0.lock().unwrap();
    assert!(
        s.questions.len() == 1,
        "only the answered one is left: {:?}",
        s.questions.keys()
    );
    assert!(s.status.is_empty());
    assert!(s.panels.is_empty());
}

#[test]
fn settings_are_read_set_and_watched() {
    let Some((mut ext, fake)) = counter(Limits::default()) else {
        return;
    };
    assert_eq!(run(&mut ext, "counter.settings"), Ok("14 \"hello\"".into()));
    assert_eq!(
        fake.0
            .lock()
            .unwrap()
            .own
            .get("greeting")
            .map(String::as_str),
        Some("\"hello\"")
    );
    let mut watches = ext.watches();
    watches.sort();
    assert_eq!(
        watches,
        [
            ("editor.font_size".to_string(), false),
            ("greeting".to_string(), true)
        ]
    );
    assert!(ext.setting_changed("greeting", true).unwrap());
    assert!(
        !ext.setting_changed("greeting", false).unwrap(),
        "not Kalem's"
    );
    assert!(!ext.setting_changed("editor.theme", false).unwrap());
    assert_eq!(
        fake.0.lock().unwrap().notes,
        [(Level::Info, "greeting changed".to_string())]
    );
}

/// The reach plugin instantiated with `permissions`, its workspace `root`.
fn reach(permissions: &[&str], root: &std::path::Path) -> Option<Result<(Extension, Fake), Error>> {
    let bytes = common::component("reach")?;
    let host = Host::new(None).unwrap();
    let plugin = host.load(&bytes).unwrap();
    let fake = Fake::default();
    fake.0.lock().unwrap().workspace = vec![root.to_path_buf()];
    let ext = Extension::new(
        &host,
        &plugin,
        "reach",
        Box::new(fake.clone()),
        Grants::from_permissions(permissions),
        Limits::default(),
    );
    Some(ext.map(|mut e| {
        e.activate().unwrap().unwrap();
        (e, fake)
    }))
}

fn call(ext: &mut Extension, id: &str, args: serde_json::Value) -> Result<String, String> {
    ext.run_command(id, &args.to_string()).unwrap()
}

#[test]
fn files_are_reached_only_as_granted() {
    let dir = std::env::temp_dir().join(format!("kalem-reach-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("project");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    std::fs::write(root.join("a.txt"), "inside").unwrap();
    std::fs::write(dir.join("secret.txt"), "outside").unwrap();
    let root = root.canonicalize().unwrap();
    let dir = dir.canonicalize().unwrap();
    let p = |path: &std::path::Path| path.to_string_lossy().into_owned();

    // No permission: the component imports `fs`, which is not granted.
    match reach(&[], &root) {
        None => return,
        Some(Err(Error::NotGranted(names))) => {
            assert!(names.iter().any(|n| n.contains("fs")), "{names:?}")
        }
        Some(other) => panic!("{:?}", other.map(|_| ())),
    }
    let (mut ext, _) = reach(&["fs:read:workspace", "net:fetch:example.com"], &root)
        .unwrap()
        .unwrap();
    let read = |ext: &mut Extension, path: &std::path::Path| {
        call(ext, "reach.read", serde_json::json!({ "path": p(path) }))
    };
    assert_eq!(read(&mut ext, &root.join("a.txt")), Ok("inside".into()));
    assert!(
        read(&mut ext, &dir.join("secret.txt"))
            .unwrap_err()
            .contains("outside")
    );
    // `..` written in the string: `Path::join` on Windows resolves it in
    // a verbatim path (what `canonicalize` gives) before the plugin sees
    // it, so the path is built as text.
    let up = format!(
        "{}{}..{}secret.txt",
        p(&root),
        std::path::MAIN_SEPARATOR,
        '/'
    );
    let e = call(&mut ext, "reach.read", serde_json::json!({ "path": up })).unwrap_err();
    assert!(e.contains("goes up"), "{e}");
    assert!(
        call(
            &mut ext,
            "reach.read",
            serde_json::json!({ "path": "a.txt" })
        )
        .unwrap_err()
        .contains("absolute")
    );
    let listed = call(
        &mut ext,
        "reach.list",
        serde_json::json!({ "dir": p(&root) }),
    )
    .unwrap();
    assert_eq!(
        listed,
        format!("{}\n{}/", p(&root.join("a.txt")), p(&root.join("sub")))
    );
    // Reading is not writing.
    let write = |ext: &mut Extension, path: &std::path::Path| {
        call(
            ext,
            "reach.write",
            serde_json::json!({ "path": p(path), "text": "new" }),
        )
    };
    assert!(write(&mut ext, &root.join("b.txt")).is_err());
    // A link out of the project is followed, and refused.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&dir, root.join("up")).unwrap();
        assert!(read(&mut ext, &root.join("up/secret.txt")).is_err());
    }

    let (mut ext, _) = reach(&["fs:write:workspace", "net:fetch:example.com"], &root)
        .unwrap()
        .unwrap();
    assert_eq!(write(&mut ext, &root.join("new/b.txt")), Ok("null".into()));
    assert_eq!(
        std::fs::read_to_string(root.join("new/b.txt")).unwrap(),
        "new"
    );
    assert!(write(&mut ext, &dir.join("c.txt")).is_err());

    let (mut ext, _) = reach(&["fs:read:all", "net:fetch:example.com"], &root)
        .unwrap()
        .unwrap();
    assert_eq!(
        read(&mut ext, &dir.join("secret.txt")),
        Ok("outside".into())
    );
    assert!(write(&mut ext, &root.join("d.txt")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn fetches_reach_only_granted_domains_and_answer_later() {
    let root = std::env::temp_dir();
    let (mut ext, fake) = match reach(&["fs:read:workspace", "net:fetch:example.com"], &root) {
        None => return,
        Some(r) => r.unwrap(),
    };
    let fetch = |ext: &mut Extension, url: &str| {
        call(ext, "reach.fetch", serde_json::json!({ "url": url }))
    };
    assert_eq!(
        fetch(&mut ext, "https://api.example.com/v1?q=1"),
        Ok("null".into())
    );
    for refused in [
        "https://example.org/",
        "https://notexample.com/",
        "https://example.com.evil.net/",
        "ftp://example.com/",
        "https://user@evil.net/?example.com",
    ] {
        assert!(fetch(&mut ext, refused).is_err(), "{refused}");
    }
    let fetched = fake.0.lock().unwrap().fetched.clone();
    assert_eq!(fetched.len(), 1);
    let (id, url) = &fetched[0];
    assert_eq!(url, "https://api.example.com/v1?q=1");
    assert_eq!(
        call(&mut ext, "reach.last", serde_json::Value::Null),
        Ok(String::new())
    );
    let response = http::Response {
        status: 200,
        headers: Vec::new(),
        body: b"ok".to_vec(),
    };
    assert!(ext.respond(*id, Ok(response.clone())).unwrap());
    assert!(!ext.respond(*id, Ok(response)).unwrap(), "answered once");
    assert_eq!(
        call(&mut ext, "reach.last", serde_json::Value::Null),
        Ok("200 ok".into())
    );
}

#[test]
fn grants_read_from_permissions() {
    let g = Grants::from_permissions(&["fs:read:workspace", "net:fetch:Example.com", "subprocess"]);
    assert!(g.fs() && g.net() && !g.write_workspace && !g.read_all);
    assert!(g.allows_url("http://example.com:8080/x"));
    assert!(!Grants::default().fs() && !Grants::default().net());
    assert!(Grants::from_permissions(&["net:fetch:*"]).allows_url("https://any.where/"));
}

/// The run plugin instantiated with `permissions`, its workspace `root`.
fn runner(
    permissions: &[&str],
    root: &std::path::Path,
) -> Option<Result<(Extension, Fake), Error>> {
    let bytes = common::component("run")?;
    let host = Host::new(None).unwrap();
    let plugin = host.load(&bytes).unwrap();
    let fake = Fake::default();
    fake.0.lock().unwrap().workspace = vec![root.to_path_buf()];
    let ext = Extension::new(
        &host,
        &plugin,
        "run",
        Box::new(fake.clone()),
        Grants::from_permissions(permissions),
        Limits::default(),
    );
    Some(ext.map(|mut e| {
        e.activate().unwrap().unwrap();
        (e, fake)
    }))
}

fn exit(status: i32, stdout: &str) -> process::Exit {
    process::Exit {
        status: Some(status),
        stdout: stdout.as_bytes().to_vec(),
        stderr: b"warn".to_vec(),
        truncated: false,
    }
}

#[test]
fn programs_run_only_as_granted_and_end_later() {
    let dir = std::env::temp_dir().join(format!("kalem-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let root = dir.join("project");
    std::fs::create_dir_all(root.join("sub")).unwrap();
    let root = root.canonicalize().unwrap();
    let dir = dir.canonicalize().unwrap();
    let p = |path: &std::path::Path| path.to_string_lossy().into_owned();

    // No permission: the component imports `process`, which is not
    // granted; a bare `subprocess` (a language plugin's) grants nothing.
    for none in [&[][..], &["subprocess"][..]] {
        match runner(none, &root) {
            None => return,
            Some(Err(Error::NotGranted(names))) => {
                assert!(names.iter().any(|n| n.contains("process")), "{names:?}")
            }
            Some(other) => panic!("{:?}", other.map(|_| ())),
        }
    }
    let (mut ext, fake) = runner(&["subprocess:git"], &root).unwrap().unwrap();
    let start = |ext: &mut Extension, args: serde_json::Value| call(ext, "run.start", args);
    let id: u64 = start(
        &mut ext,
        serde_json::json!({ "program": "git", "args": ["status", "-z"], "cwd": p(&root.join("sub")) }),
    )
    .unwrap()
    .parse()
    .unwrap();
    // Without a folder, the first project's.
    let id2: u64 = start(&mut ext, serde_json::json!({ "program": "git" }))
        .unwrap()
        .parse()
        .unwrap();
    {
        let s = fake.0.lock().unwrap();
        assert_eq!(
            s.spawned,
            [
                (
                    id,
                    "git".to_string(),
                    vec!["status".to_string(), "-z".to_string()],
                    root.join("sub")
                ),
                (id2, "git".to_string(), vec![], root.clone()),
            ]
        );
    }
    for (args, why) in [
        (serde_json::json!({ "program": "rm" }), "not a program"),
        (
            serde_json::json!({ "program": "/usr/bin/git" }),
            "not a program",
        ),
        (
            serde_json::json!({ "program": "git", "cwd": p(&dir) }),
            "outside the projects",
        ),
        (
            serde_json::json!({ "program": "git", "cwd": "sub" }),
            "absolute",
        ),
        (
            serde_json::json!({ "program": "git", "cwd": p(&root.join("missing")) }),
            "not a folder",
        ),
    ] {
        let e = start(&mut ext, args.clone()).unwrap_err();
        assert!(e.contains(why), "{args}: {e}");
    }
    assert_eq!(
        fake.0.lock().unwrap().spawned.len(),
        2,
        "refused runs never start"
    );
    // The end arrives later, once.
    assert_eq!(
        call(&mut ext, "run.last", serde_json::Value::Null),
        Ok(String::new())
    );
    assert!(ext.process_done(id, Ok(exit(0, "clean"))).unwrap());
    assert!(
        !ext.process_done(id, Ok(exit(0, "again"))).unwrap(),
        "delivered once"
    );
    assert!(!ext.process_done(999, Ok(exit(0, "stranger"))).unwrap());
    assert_eq!(
        call(&mut ext, "run.last", serde_json::Value::Null),
        Ok("0 clean|warn".into())
    );
    assert!(ext.process_done(id2, Err("git: not found".into())).unwrap());
    assert_eq!(
        call(&mut ext, "run.last", serde_json::Value::Null),
        Ok("error git: not found".into())
    );
    assert!(ext.running().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runs_are_limited_and_stop_with_their_plugin() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let (mut ext, fake) = match runner(&["subprocess:git"], &root) {
        None => return,
        Some(r) => r.unwrap(),
    };
    let start =
        |ext: &mut Extension| call(ext, "run.start", serde_json::json!({ "program": "git" }));
    for _ in 0..kalem_script::extension::MAX_RUNS {
        start(&mut ext).unwrap();
    }
    assert!(start(&mut ext).unwrap_err().contains("running already"));
    // A kill asked by the plugin reaches the editor; the run still ends.
    let first = *ext.running().iter().next().unwrap();
    call(&mut ext, "run.kill", serde_json::json!({ "run": first })).unwrap();
    assert_eq!(fake.0.lock().unwrap().killed, [first]);
    ext.process_done(first, Ok(exit(-1, ""))).unwrap();
    start(&mut ext).unwrap();
    // Deactivated, its programs are killed and their ends dropped.
    let running = ext.running();
    assert_eq!(running.len(), kalem_script::extension::MAX_RUNS);
    ext.deactivate().unwrap();
    let killed = fake.0.lock().unwrap().killed.clone();
    assert!(running.iter().all(|r| killed.contains(r)));
    assert!(ext.running().is_empty());
}

#[test]
fn programs_are_granted_by_name() {
    let g = Grants::from_permissions(&[
        "subprocess:git",
        "subprocess",
        "subprocess:/bin/sh",
        "subprocess:",
        "subprocess:..",
        "subprocess:git-lfs",
    ]);
    assert_eq!(g.programs, ["git", "git-lfs"]);
    assert!(g.process() && !g.fs() && !g.net());
    assert!(!Grants::from_permissions(&["subprocess"]).process());
}

#[test]
fn a_plugin_writes_documents_of_its_own() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let (mut ext, fake) = match runner(&["subprocess:git"], &root) {
        None => return,
        Some(r) => r.unwrap(),
    };
    let show = |ext: &mut Extension, id: &str, key: &str, text: &str| {
        call(
            ext,
            "run.show",
            serde_json::json!({
                "id": id, "key": key, "title": "Git: org", "kind": "run-status",
                "language": "diff", "text": text, "cursor": 3,
            }),
        )
    };
    let doc: u64 = show(&mut ext, "run.status", "/org", "Head: main\n")
        .unwrap()
        .parse()
        .unwrap();
    {
        let s = fake.0.lock().unwrap();
        let (plugin, spec, text, cursor) = &s.documents[&doc];
        assert_eq!(
            (plugin.as_str(), spec.title.as_str(), spec.kind.as_str()),
            ("run", "Git: org", "run-status")
        );
        assert_eq!(spec.language.as_deref(), Some("diff"));
        assert_eq!((text.as_str(), *cursor), ("Head: main\n", Some(3)));
    }
    // The same ID and key is that document; another key, another one.
    assert_eq!(
        show(&mut ext, "run.status", "/org", "again\n"),
        Ok(doc.to_string())
    );
    let other: u64 = show(&mut ext, "run.status", "/plugins", "")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(other, doc);
    // A name that is not the plugin's is refused before the editor.
    let e = show(&mut ext, "git.status", "/org", "").unwrap_err();
    assert!(e.contains("not the plugin's"), "{e}");
    call(
        &mut ext,
        "run.write",
        serde_json::json!({ "doc": doc, "text": "Head: dev\n" }),
    )
    .unwrap();
    assert_eq!(fake.0.lock().unwrap().documents[&doc].2, "Head: dev\n");
    // Outside a document of its own, none is current.
    assert_eq!(
        call(&mut ext, "run.current", serde_json::Value::Null),
        Ok("none".into())
    );
    // Closed, a write is refused, and the plugin forgets it.
    call(&mut ext, "run.close", serde_json::json!({ "doc": doc })).unwrap();
    assert_eq!(fake.0.lock().unwrap().closed, [doc]);
    let e = call(
        &mut ext,
        "run.write",
        serde_json::json!({ "doc": doc, "text": "late" }),
    )
    .unwrap_err();
    assert!(e.contains("closed"), "{e}");
}

#[test]
fn a_plugin_marks_the_lines_of_files() {
    let root = std::env::temp_dir().canonicalize().unwrap();
    let (mut ext, fake) = match runner(&["subprocess:git"], &root) {
        None => return,
        Some(r) => r.unwrap(),
    };
    let file = root.join("marked.rs");
    let path = file.display().to_string();
    call(
        &mut ext,
        "run.mark",
        serde_json::json!({ "path": path, "marks": [[3, "added"], [7, "changed"], [0, "removed"]] }),
    )
    .unwrap();
    {
        let s = fake.0.lock().unwrap();
        let (plugin, marks) = &s.gutters[&file];
        assert_eq!(plugin, "run");
        assert_eq!(
            marks,
            &[
                (3, "MarkKind::Added".to_string()),
                (7, "MarkKind::Changed".to_string()),
                (0, "MarkKind::Removed".to_string())
            ]
        );
    }
    // A relative path, or too many marks, are refused before the editor.
    let e = call(
        &mut ext,
        "run.mark",
        serde_json::json!({ "path": "src/lib.rs", "marks": [[1, "added"]] }),
    )
    .unwrap_err();
    assert!(e.contains("absolute"), "{e}");
    let many: Vec<serde_json::Value> = (0..=kalem_script::extension::MAX_MARKS)
        .map(|n| serde_json::json!([n, "added"]))
        .collect();
    let e = call(
        &mut ext,
        "run.mark",
        serde_json::json!({ "path": path, "marks": many }),
    )
    .unwrap_err();
    assert!(e.contains("At most"), "{e}");
    call(&mut ext, "run.unmark", serde_json::json!({ "path": path })).unwrap();
    assert!(fake.0.lock().unwrap().gutters.is_empty());
}
