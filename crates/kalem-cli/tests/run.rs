//! Batch mode, `kalem run PLUGIN COMMAND [FILE...]` (T3.1.16): the test
//! plugin of `tests/plugins/counter` installed, started by name and its
//! commands run without a window: a document it writes printed, a file it
//! edits saved, its questions answered as batch mode answers them, its
//! notices on standard error. Skipped where the `wasm32-unknown-unknown`
//! target or `wasm-tools` is not installed.

use std::process::ExitCode;

mod test_plugins;
use test_plugins::component;

fn run(
    plugin: &str,
    command: &str,
    files: &[std::path::PathBuf],
    json: bool,
) -> (Result<ExitCode, String>, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let r = kalem_cli::run_batch(plugin, command, files, None, json, &mut out, &mut err);
    (
        r,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_plugins_command_runs_without_a_window() {
    let Some(bytes) = component("counter") else {
        return;
    };
    let root = std::env::temp_dir().join(format!("kalem-cli-run-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let plugin = root.join("config/plugins/org.test.counter");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(plugin.join("counter.wasm"), bytes).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"id": "org.test.counter", "name": "Counter", "main": "counter.wasm",
            "activation": ["onCommand:counter.count"], "limits": {"time_ms": 5000}}"#,
    )
    .unwrap();
    let notes = root.join("notes");
    std::fs::create_dir_all(&notes).unwrap();
    let file = notes.join("plan.org");
    std::fs::write(&file, "* TODO Write report\nSome text.\n** Sub item\n* TODO Call Ada :home:\n* Numbers\n| a | b |\n|---+---|\n| 1 | 2 |\n#+TBLFM: $2=$1*10\n").unwrap();
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", root.join("config"));
        std::env::set_var("KALEM_STATE_DIR", root.join("state"));
    }

    // Started by name whatever its activation; the document it writes
    // printed, with the file it ran against.
    let (r, out, err) = run(
        "counter",
        "counter.report",
        std::slice::from_ref(&file),
        false,
    );
    assert_eq!(r, Ok(ExitCode::SUCCESS), "{err}");
    assert_eq!(out, format!("count 0\nfile {}\n", file.display()));
    // Without a file, and as JSON.
    let (r, out, _) = run("org.test.counter", "counter.report", &[], true);
    assert_eq!(r, Ok(ExitCode::SUCCESS));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v[0]["kind"], "counter-report");
    assert_eq!(v[0]["text"], "count 0\nfile none\n");
    // Its edits saved: the file as the command left it.
    let (r, _, err) = run(
        "counter",
        "counter.organize",
        std::slice::from_ref(&file),
        false,
    );
    assert_eq!(r, Ok(ExitCode::SUCCESS), "{err}");
    assert!(err.contains("saved"), "{err}");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.starts_with("#+TITLE: Plan\n* DONE Write report"),
        "{text}"
    );
    // Its questions answered: no to a confirmation, nothing picked, a
    // prompt without text cancelled; its notices on standard error.
    let (r, out, err) = run("counter", "counter.ask", &[], false);
    assert_eq!(r, Ok(ExitCode::SUCCESS), "{err}");
    assert!(out.is_empty());
    assert!(err.lines().any(|l| l.ends_with(": no")), "{err}");
    // A command it does not have, a plugin not installed.
    let (r, _, _) = run("counter", "counter.nothing", &[], false);
    assert!(r.unwrap_err().contains("its commands: counter."));
    let (r, _, _) = run("absent", "absent.x", &[], false);
    assert!(r.unwrap_err().contains("org.test.counter"));
    let _ = std::fs::remove_dir_all(&root);
}
