//! Installing, listing and removing a plugin through the commands the
//! Plugins menu runs: the download in the background, the list offered to
//! confirm, the plugin loaded at once, its syntax highlighting files, and
//! everything undone on removal. This binary runs itself again with
//! `KALEM_CONFIG_DIR` and `KALEM_STATE_DIR` pointing at a temporary
//! folder, so no user's settings are touched.

#![allow(clippy::print_stdout)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use kalem_core::command::{Clipboard, EditorContext, Request};
use kalem_core::{CommandRegistry, palette};
use serde_json::{Value, json};

const SYNTAX: &str = "%YAML 1.2\n---\nname: Zed Test\nscope: source.zedtest\nfile_extensions: [zedtest]\ncontexts:\n  main:\n    - match: '\\bfn\\b'\n      scope: keyword.zedtest\n";

fn run(reg: &CommandRegistry, id: &str, args: Value) -> (Vec<String>, Vec<Request>) {
    let mut clip = Clipboard::default();
    let config = kalem_core::Config::default();
    let mut ctx = EditorContext {
        document: None,
        clipboard: &mut clip,
        config: &config,
        now: Instant::now(),
        clock: jiff::civil::date(2026, 10, 3).at(12, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    reg.execute(id, &mut ctx, &args)
        .unwrap_or_else(|e| panic!("{id}: {e:?}"));
    (ctx.messages, ctx.requests)
}

/// Runs the command of a chosen item.
fn choose(reg: &CommandRegistry, item: &palette::PaletteItem) -> (Vec<String>, Vec<Request>) {
    let (id, args) = palette::split_invocation(&item.id);
    run(reg, id, args)
}

fn offered() -> Vec<palette::PaletteItem> {
    let t = Instant::now();
    loop {
        if let Some(items) = kalem_core::jobs::take_offers().into_iter().next() {
            return items;
        }
        for f in kalem_core::jobs::take_finished() {
            assert!(!f.error, "{}", f.message);
        }
        assert!(t.elapsed() < Duration::from_secs(20), "nothing offered");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn child(work: &Path) {
    let plugin = work.join("zedtest");
    std::fs::create_dir_all(plugin.join("syntaxes")).unwrap();
    std::fs::write(plugin.join("syntaxes/Zed.sublime-syntax"), SYNTAX).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        json!({
            "id": "org.example.zedtest", "name": "Zed Test", "version": "1.0.0",
            "description": "A test language", "api": "^0.1", "permissions": ["subprocess"],
            "syntaxes": ["syntaxes/Zed.sublime-syntax"],
            "languages": [{"id": "zedtest", "name": "Zed Test", "extensions": ["zedtest"], "servers": ["z"]}],
            "servers": {"z": {"name": "ZedLS", "command": ["kalem-no-such-zed-ls"]}}
        })
        .to_string(),
    )
    .unwrap();
    // The interface in English, whatever the system's language: the
    // assertions read English texts.
    kalem_core::Config::from_layers(&[(
        kalem_core::settings::Layer::User,
        None,
        "[ui]\nlanguage = \"en\"\n",
    )])
    .apply_process_settings();
    let reg = CommandRegistry::with_builtins();
    let generation = kalem_highlight::generation();

    // Without a source, it asks for one.
    let (_, req) = run(&reg, "plugin.install", json!({}));
    assert!(matches!(&req[0], Request::Ask { arg, .. } if arg == "source"));

    // Cancelled: nothing installed, the staging folder gone.
    run(&reg, "plugin.install", json!({ "source": plugin }));
    let items = offered();
    assert_eq!(items.len(), 2);
    assert!(
        items[0].title.starts_with("Install Zed Test 1.0.0"),
        "{}",
        items[0].title
    );
    assert!(
        items[0]
            .category
            .contains("Runs programs on this computer: ZedLS"),
        "{}",
        items[0].category
    );
    choose(&reg, &items[1]);
    assert!(kalem_core::plugin_store::installed().is_empty());
    let staging = work.join("state/plugin-staging");
    assert_eq!(std::fs::read_dir(&staging).map_or(0, |d| d.count()), 0);
    println!("test cancel ... ok");

    // Confirmed: installed, recorded, loaded, highlighting at once.
    run(&reg, "plugin.install", json!({ "source": plugin }));
    let items = offered();
    let (msgs, _) = choose(&reg, &items[0]);
    assert!(msgs[0].starts_with("Installed Zed Test 1.0.0"), "{msgs:?}");
    let installed = kalem_core::plugin_store::installed();
    assert_eq!(installed[0].id, "org.example.zedtest");
    assert_eq!(
        installed[0].source.as_deref(),
        Some(plugin.to_str().unwrap())
    );
    assert!(
        work.join("config/plugins/org.example.zedtest/plugin.json")
            .is_file()
    );
    let record = std::fs::read_to_string(work.join("config/plugins.toml")).unwrap();
    assert!(
        record.contains("[plugins.\"org.example.zedtest\"]") && record.contains("subprocess"),
        "{record}"
    );
    assert!(kalem_highlight::generation() > generation);
    let lang = kalem_highlight::Language::find("zedtest").expect("its syntax");
    assert_eq!(
        kalem_highlight::highlight(lang, "fn x")[0][0].kind,
        kalem_highlight::Kind::Keyword
    );
    assert!(kalem_core::languages::for_path(Path::new("/a/b.zedtest"), None).is_some());
    println!("test install ... ok");

    // Listed, managed, offered again as an update.
    let (_, req) = run(&reg, "plugin.list", json!({}));
    let Request::Choose(items) = &req[0] else {
        panic!("{req:?}")
    };
    let (_, req) = choose(&reg, &items[0]);
    let Request::Choose(actions) = &req[0] else {
        panic!("{req:?}")
    };
    let titles: Vec<&str> = actions.iter().map(|a| a.title.as_str()).collect();
    assert_eq!(
        titles,
        ["Update Zed Test", "Remove Zed Test", "Show Its Folder"]
    );
    choose(&reg, &actions[0]);
    let update = offered();
    assert!(
        update[0].title.contains("(replaces 1.0.0)"),
        "{}",
        update[0].title
    );
    choose(&reg, &update[1]);
    println!("test manage ... ok");

    // A newer version in the index: found once a day, said, and shown in
    // the list.
    let index = work.join("index.json");
    std::fs::write(
        &index,
        json!({"schema": 1, "plugins": [{
            "id": "org.example.zedtest", "name": "Zed Test", "version": "2.0.0", "api": "^0.1",
            "description": "A test language", "source": plugin, "download": null, "sha256": null,
            "kind": "declarative"
        }]})
        .to_string(),
    )
    .unwrap();
    let config = kalem_core::Config::from_layers(&[(
        kalem_core::settings::Layer::User,
        None,
        // A TOML literal string: a Windows path's backslashes are not
        // escapes there.
        &format!("[plugins]\nindex = 'file://{}'\n", index.display()),
    )]);
    kalem_core::plugin_store::check_updates(&config);
    let t = Instant::now();
    let notice = loop {
        if let Some(n) = kalem_core::jobs::take_notices().into_iter().next() {
            break n.0;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "no update notice");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(notice.contains("Zed Test 2.0.0"), "{notice}");
    kalem_core::plugin_store::check_updates(&config);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        kalem_core::jobs::take_notices().is_empty(),
        "checked once a day"
    );
    let (_, req) = run(&reg, "plugin.list", json!({}));
    let Request::Choose(items) = &req[0] else {
        panic!("{req:?}")
    };
    assert!(
        items[0].category.starts_with("2.0.0 available"),
        "{}",
        items[0].category
    );
    println!("test update notice ... ok");

    // Removed after a confirmation: the plugin and its syntax gone.
    let (_, req) = choose(&reg, &actions[1]);
    let Request::Choose(confirm) = &req[0] else {
        panic!("{req:?}")
    };
    assert!(
        kalem_core::plugin_store::installed().len() == 1,
        "nothing removed before the yes"
    );
    let (msgs, _) = choose(&reg, &confirm[0]);
    assert_eq!(msgs, ["Removed Zed Test"]);
    assert!(kalem_core::plugin_store::installed().is_empty());
    assert!(kalem_highlight::Language::find("zedtest").is_none());
    assert!(kalem_core::languages::for_path(Path::new("/a/b.zedtest"), None).is_none());
    println!("test remove ... ok");
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
    if let Ok(work) = std::env::var("KALEM_PLUGIN_INSTALL_TEST") {
        child(Path::new(&work));
        return;
    }
    let work: PathBuf =
        std::env::temp_dir().join(format!("kalem-plugin-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .env("KALEM_PLUGIN_INSTALL_TEST", &work)
        .env("KALEM_CONFIG_DIR", work.join("config"))
        .env("KALEM_STATE_DIR", work.join("state"))
        .env_remove("KALEM_PLUGIN_PATH")
        .status()
        .unwrap();
    let _ = std::fs::remove_dir_all(&work);
    assert!(status.success(), "the child failed");
}
