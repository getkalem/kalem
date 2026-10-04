//! An extension plugin for tests: commands reading, writing and listing
//! files and fetching URLs, their arguments as JSON (`{"path": …}`), so
//! that the host's grants decide what succeeds.

use std::cell::RefCell;

use kalem_plugin::kalem::{self, Plugin, Scope};
use kalem_plugin::{fs, net};
use serde_json::Value;

thread_local! {
    /// The last response: its status and body, or the error.
    static LAST: RefCell<String> = const { RefCell::new(String::new()) };
}

struct Reach;

fn arg(args: &str, name: &str) -> String {
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v[name].as_str().map(str::to_string))
        .unwrap_or_default()
}

fn command(
    id: &str,
    run: impl FnMut(&str) -> Result<String, String> + 'static,
) -> Result<(), String> {
    kalem::command(kalem::spec(id, id, Scope::all()), run).map(drop)
}

impl Plugin for Reach {
    fn activate() -> Result<(), String> {
        command("reach.read", |a| fs::read(&arg(a, "path")))?;
        command("reach.write", |a| {
            fs::write(&arg(a, "path"), &arg(a, "text")).map(|()| "null".into())
        })?;
        command("reach.list", |a| fs::list(&arg(a, "dir")).map(|l| l.join("\n")))?;
        command("reach.fetch", |a| {
            net::fetch(&net::get(&arg(a, "url")), |r| {
                let s = match r {
                    Ok(r) => format!("{} {}", r.status, String::from_utf8_lossy(&r.body)),
                    Err(e) => format!("error {e}"),
                };
                LAST.with(|l| *l.borrow_mut() = s);
            })
            .map(|()| "null".into())
        })?;
        // The last response, returned and shown.
        command("reach.last", |_| {
            let last = LAST.with(|l| l.borrow().clone());
            kalem_plugin::ui::notify(&last, kalem_plugin::ui::Level::Info);
            Ok(last)
        })?;
        Ok(())
    }
}

kalem_plugin::export_plugin!(Reach);
