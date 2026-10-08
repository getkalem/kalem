//! An extension plugin for tests: commands starting and stopping programs
//! through the `process` interface (API 0.2.4), their arguments as JSON
//! (`{"program": …, "args": […], "cwd": …, "stdin": …}`), so that the
//! host's grants decide what runs, and a command telling how the last run
//! ended; and commands writing documents of their own through the
//! `documents` interface (API 0.2.5).

use std::cell::RefCell;

use kalem_plugin::decorations::{self, Mark};
use kalem_plugin::documents::{self, Spec};
use kalem_plugin::kalem::{self, Plugin, Scope};
use kalem_plugin::process::{self, Command};
use serde_json::Value;

thread_local! {
    /// How the last run ended: `STATUS STDOUT|STDERR`, or the error.
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Run;

fn command(
    id: &str,
    run: impl FnMut(&str) -> Result<String, String> + 'static,
) -> Result<(), String> {
    kalem::command(kalem::spec(id, id, Scope::all()), run).map(drop)
}

impl Plugin for Run {
    fn activate() -> Result<(), String> {
        command("run.start", |a| {
            let v: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            let mut c = Command::new(v["program"].as_str().unwrap_or_default()).args(
                v["args"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|x| x.as_str()),
            );
            if let Some(dir) = v["cwd"].as_str() {
                c = c.cwd(dir);
            }
            if let Some(input) = v["stdin"].as_str() {
                c = c.stdin(input.as_bytes().to_vec());
            }
            let id = process::run(&c, |result| {
                let s = match result {
                    Ok(e) => format!(
                        "{} {}|{}{}",
                        e.status.map_or("-".to_string(), |s| s.to_string()),
                        String::from_utf8_lossy(&e.stdout),
                        String::from_utf8_lossy(&e.stderr),
                        if e.truncated { " (cut)" } else { "" }
                    ),
                    Err(e) => format!("error {e}"),
                };
                LAST.with(|l| *l.borrow_mut() = s);
            })?;
            Ok(id.to_string())
        })?;
        command("run.kill", |a| {
            let v: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            process::kill(v["run"].as_u64().unwrap_or_default());
            Ok("null".into())
        })?;
        command("run.last", |_| Ok(LAST.with(|l| l.borrow().clone())))?;
        command("run.show", |a| {
            let v: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
            let mut spec = Spec::new(&s("id"), &s("key"), &s("title"), &s("kind"));
            if let Some(l) = v["language"].as_str() {
                spec = spec.language(l);
            }
            documents::open(&spec, &s("text"), v["cursor"].as_u64()).map(|n| n.to_string())
        })?;
        command("run.write", |a| {
            let v: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            let doc = v["doc"].as_u64().unwrap_or_default();
            documents::set(
                doc,
                v["text"].as_str().unwrap_or_default(),
                v["cursor"].as_u64(),
            )
            .map(|()| "null".into())
        })?;
        command("run.close", |a| {
            let v: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            documents::close(v["doc"].as_u64().unwrap_or_default());
            Ok("null".into())
        })?;
        command("run.mark", |a| {
            let v: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            let marks: Vec<(u32, Mark)> = v["marks"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|m| {
                    let kind = match m[1].as_str() {
                        Some("added") => Mark::Added,
                        Some("removed") => Mark::Removed,
                        _ => Mark::Changed,
                    };
                    (m[0].as_u64().unwrap_or_default() as u32, kind)
                })
                .collect();
            decorations::set_gutter(v["path"].as_str().unwrap_or_default(), &marks)
                .map(|()| "null".into())
        })?;
        command("run.unmark", |a| {
            let v: Value = serde_json::from_str(a).map_err(|e| e.to_string())?;
            decorations::clear_gutter(v["path"].as_str());
            Ok("null".into())
        })?;
        command("run.current", |_| {
            Ok(documents::current().map_or("none".into(), |n| n.to_string()))
        })?;
        Ok(())
    }
}

kalem_plugin::export_plugin!(Run);
