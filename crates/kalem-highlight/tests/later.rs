//! A set not cached is built on a thread of its own
//! (`register_cached_later`): a process of its own, since the set is the
//! process's.

use kalem_highlight::{Language, SyntaxSource, register_cached_later};

const LANG: &str = r#"%YAML 1.2
---
name: Later
scope: source.later
file_extensions: [later]
contexts:
  main:
    - match: '!'
      scope: keyword.operator.later
"#;

#[test]
fn a_set_not_cached_is_built_on_a_thread() {
    let dir = std::env::temp_dir().join(format!("kalem-syntax-later-{}", std::process::id()));
    let sources = vec![SyntaxSource {
        file: "Later.sublime-syntax".into(),
        text: LANG.into(),
        base_only: false,
    }];
    kalem_highlight::set_aliases(vec![("later".into(), "Later".into())]);
    let (tx, rx) = std::sync::mpsc::channel();
    let r = register_cached_later(sources.clone(), Some(dir.clone()), move |r| {
        tx.send(r).unwrap();
    });
    assert!(r.is_none(), "not cached yet");
    // Another language is found in the set in place; a plugin's waits
    // for the set built with it, and finds it told.
    assert!(Language::find("rs").is_some());
    let lang = Language::find("later").expect("the plugin's syntax");
    assert_eq!(lang.name(), "Later");
    assert_eq!(
        kalem_highlight::highlight(lang, "!")[0][0].kind,
        kalem_highlight::Kind::Operator
    );
    let built = rx.try_recv().expect("told before the lookup returned");
    assert_eq!(
        built.names,
        [("Later.sublime-syntax".to_string(), "Later".to_string())]
    );
    // Cached now: in place at once.
    let again = register_cached_later(sources, Some(dir.clone()), |_| panic!("built again"))
        .expect("cached");
    assert_eq!(again.names, built.names);
    kalem_highlight::wait();
    let _ = std::fs::remove_dir_all(&dir);
}
