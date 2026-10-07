//! An extension plugin for tests: commands starting and stopping programs
//! through the `process` interface (API 0.2.4), their arguments as JSON
//! (`{"program": …, "args": […], "cwd": …, "stdin": …}`), so that the
//! host's grants decide what runs, and a command telling how the last run
//! ended.

use std::cell::RefCell;

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
        Ok(())
    }
}

kalem_plugin::export_plugin!(Run);
