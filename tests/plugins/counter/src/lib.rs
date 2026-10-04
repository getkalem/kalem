//! An extension plugin for tests: `counter.count` counts its runs, the
//! documents opened are remembered, a save to a `.lock` file is vetoed,
//! and a few commands try what the host must refuse.

use std::cell::{Cell, RefCell};

use kalem_plugin::kalem::{self, Disposable, Event, EventKind, Plugin, Reply, Scope};

thread_local! {
    static COUNT: Cell<u32> = const { Cell::new(0) };
    static OPENED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static OPENS: RefCell<Option<Disposable>> = const { RefCell::new(None) };
}

struct Counter;

fn command(
    id: &str,
    scope: Scope,
    run: impl FnMut(&str) -> Result<String, String> + 'static,
) -> Result<(), String> {
    kalem::command(kalem::spec(id, id, scope), run).map(drop)
}

impl Plugin for Counter {
    fn activate() -> Result<(), String> {
        let mut spec = kalem::spec("counter.count", "Count", Scope::only(&["org"]));
        spec.keys = vec!["ctrl+alt+c".into()];
        spec.when = Some("!inTable".into());
        kalem::command(spec, |_| {
            COUNT.set(COUNT.get() + 1);
            Ok(COUNT.get().to_string())
        })?;
        kalem::keymap("space c c", "counter.count", None)?;
        // Another's command, its result passed on.
        command("counter.cycle", Scope::all(), |args| {
            kalem::run("org.todo.cycle", args)
        })?;
        // What the host refuses, the messages joined.
        command("counter.refused", Scope::all(), |_| {
            let tries = [
                kalem::command(kalem::spec("other.thing", "", Scope::all()), |_| {
                    Ok("null".into())
                })
                .map(drop),
                kalem::command(kalem::spec("counter.none", "", Scope::only(&[])), |_| {
                    Ok("null".into())
                })
                .map(drop),
                kalem::command(kalem::spec("counter.count", "", Scope::all()), |_| {
                    Ok("null".into())
                })
                .map(drop),
                kalem::run("counter.count", "null").map(drop),
            ];
            Ok(tries
                .into_iter()
                .map(|t| t.err().unwrap_or_default())
                .collect::<Vec<_>>()
                .join("\n"))
        })?;
        command("counter.opened", Scope::all(), |_| {
            Ok(OPENED.with(|o| o.borrow().join(",")))
        })?;
        command("counter.deaf", Scope::all(), |_| {
            if let Some(d) = OPENS.with(|o| o.borrow_mut().take()) {
                d.dispose();
            }
            Ok("null".into())
        })?;
        command("counter.spin", Scope::except(&["csv"]), |_| {
            let mut n = 0u64;
            loop {
                n = std::hint::black_box(n + 1);
            }
        })?;
        command("counter.version", Scope::all(), |_| Ok(kalem::version()))?;
        let opens = kalem::on(EventKind::DocumentOpen, |e| {
            if let Event::DocumentOpen(d) = e {
                let path = d.path.clone().unwrap_or_else(|| d.document.to_string());
                OPENED.with(|o| o.borrow_mut().push(path));
            }
            Reply::Proceed
        })?;
        OPENS.with(|o| *o.borrow_mut() = Some(opens));
        kalem::on(EventKind::DocumentBeforeSave, |e| match e {
            Event::DocumentBeforeSave(s) if s.path.ends_with(".lock") => {
                Reply::Veto("a lock file".into())
            }
            _ => Reply::Proceed,
        })?;
        Ok(())
    }
}

kalem_plugin::export_plugin!(Counter);
