//! The `extension` world end to end: the plugin of `tests/plugins/counter`,
//! built against `kalem-plugin`'s `kalem` namespace, registering commands,
//! a binding and subscriptions in a fake editor through the host. Skipped
//! where the `wasm32-unknown-unknown` target or `wasm-tools` is not
//! installed.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

mod common;

use kalem_script::extension::{
    CommandSpec, Editor, Event, EventKind, Extension, Reply, VERSION, api,
};
use kalem_script::{Error, Host, Limits};

/// What the fake editor holds.
#[derive(Debug, Default)]
struct State {
    commands: BTreeMap<String, CommandSpec>,
    bindings: BTreeMap<u64, (String, String)>,
    ran: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
struct Fake(Arc<Mutex<State>>);

impl Editor for Fake {
    fn add_command(&mut self, plugin: &str, spec: &CommandSpec) -> Result<(), String> {
        assert_eq!(plugin, "counter");
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

    fn run(&mut self, id: &str, args: &str) -> Result<String, String> {
        self.0.lock().unwrap().ran.push((id.into(), args.into()));
        Ok("\"DONE\"".into())
    }
}

/// The counter plugin activated in a fake editor.
fn counter(limits: Limits) -> Option<(Extension, Fake)> {
    let bytes = common::component("counter")?;
    let host = Host::new(None).unwrap();
    let plugin = host.load(&bytes).unwrap();
    let fake = Fake::default();
    let mut ext =
        Extension::new(&host, &plugin, "counter", Box::new(fake.clone()), limits).unwrap();
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
        Ok("\"DONE\"".into())
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
