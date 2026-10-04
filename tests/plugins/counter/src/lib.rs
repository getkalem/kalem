//! An extension plugin for tests: `counter.count` counts its runs, the
//! documents opened are remembered, a save to a `.lock` file is vetoed,
//! a few commands try what the host must refuse, and others ask the user,
//! set the status bar and fill the panel `counter.panel`.

use std::cell::{Cell, RefCell};

use kalem_plugin::kalem::{self, Disposable, Event, EventKind, Plugin, Reply, Scope};
use kalem_plugin::ui::{self, Level, PanelEvent, PanelSpec, Placement, Tree, WidgetKind};

thread_local! {
    static COUNT: Cell<u32> = const { Cell::new(0) };
    static OPENED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static OPENS: RefCell<Option<Disposable>> = const { RefCell::new(None) };
}

struct Counter;

/// The panel: the count, a button adding one, a list.
fn fill_panel() -> Result<(), String> {
    let mut tree = Tree::new(WidgetKind::Column);
    tree.add(Tree::ROOT, "count", ui::label(&COUNT.get().to_string()));
    tree.add(Tree::ROOT, "add", ui::button("Add one"));
    let list = tree.add(Tree::ROOT, "list", ui::item("Items"));
    tree.add(list, "first", ui::item("First"));
    ui::set_panel("counter.panel", &tree)
}

/// Trees the host must refuse, built by hand: the messages joined.
fn bad_trees() -> String {
    use kalem_plugin::ui::{Widget, WidgetTree};
    let w = |key: &str, kind: WidgetKind, children: Vec<u32>| Widget {
        key: key.into(),
        kind,
        children,
    };
    let trees = [
        vec![],
        vec![w("", WidgetKind::Column, vec![0])],
        vec![w("", WidgetKind::Column, vec![1, 1]), w("a", ui::button("A"), vec![])],
        vec![w("", WidgetKind::Column, vec![]), w("a", ui::button("A"), vec![])],
        vec![w("", ui::label("x"), vec![1]), w("", ui::label("y"), vec![])],
        vec![
            w("", WidgetKind::Column, vec![1, 2]),
            w("same", ui::button("A"), vec![]),
            w("same", ui::button("B"), vec![]),
        ],
    ];
    trees
        .into_iter()
        .map(|widgets| {
            kalem_plugin::extension::kalem::plugin::ui::set_panel(
                "counter.panel",
                &WidgetTree { widgets },
            )
            .err()
            .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

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
            kalem::run("org.todo.cycle", args).map(|()| "null".into())
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
        // Settings: Kalem's read, its own set, read and watched.
        command("counter.settings", Scope::all(), |_| {
            let font = kalem_plugin::settings::get("editor.font_size").unwrap_or_default();
            kalem_plugin::settings::set("greeting", "\"hello\"")?;
            let own = kalem_plugin::settings::own("greeting").unwrap_or_default();
            Ok(format!("{font} {own}"))
        })?;
        kalem_plugin::settings::watch("greeting", true, |key| {
            ui::notify(&format!("{key} changed"), Level::Info)
        })?;
        kalem_plugin::settings::watch("editor.font_size", false, |key| {
            ui::notify(&format!("{key} changed"), Level::Info)
        })?;
        command("counter.ask", Scope::all(), |_| {
            ui::confirm("Sure?", |yes| {
                ui::notify(if yes { "yes" } else { "no" }, Level::Info)
            });
            ui::prompt("Name?", &ui::PromptOptions {
                value: None,
                placeholder: None,
                password: false,
            }, |name| {
                let _ = ui::status(
                    "name",
                    &name.unwrap_or_else(|| "nobody".into()),
                    &ui::status_options(),
                );
            });
            ui::quick_pick(
                &[ui::PickItem { label: "a".into(), detail: None }, ui::PickItem { label: "b".into(), detail: None }],
                &ui::PickOptions { title: None, placeholder: None, many: true },
                |picked| ui::notify(&format!("{picked:?}"), Level::Warning),
            );
            Ok("null".into())
        })?;
        command("counter.badTrees", Scope::all(), |_| Ok(bad_trees()))?;
        command("counter.foreignPanel", Scope::all(), |_| {
            ui::panel(
                PanelSpec { id: "other.panel".into(), title: String::new(), placement: Placement::Side },
                |_, _| {},
            )
            .map(drop)
            .and(Ok("null".into()))
        })?;
        ui::panel(
            PanelSpec {
                id: "counter.panel".into(),
                title: "Counter".into(),
                placement: Placement::Side,
            },
            |key, event| {
                if key == "add" && matches!(event, PanelEvent::Clicked) {
                    COUNT.set(COUNT.get() + 1);
                    let _ = fill_panel();
                }
            },
        )?;
        fill_panel()?;
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
