//! A plugin's keys in the files of a language it declares come before
//! the profile's there (as a major mode's in Emacs): Enter in a file of
//! its language is the plugin's in the Word-like profile, and the
//! profile's elsewhere. Its own process: it loads a language plugin.

use std::path::PathBuf;

use kalem_core::command::{Command, CommandHandler, CommandRegistry, CommandSource, Scope};
use kalem_core::keymap::{Keymap, Lookup, Profile};
use kalem_core::keys::KeySequence;
use kalem_core::when::{Context, Value};

fn enter(m: &Keymap, text_type: &str) -> Option<String> {
    let mut ctx = Context::default();
    ctx.set("editorMode", Value::Str("text".into()));
    ctx.set("textType", Value::Str(text_type.into()));
    match m.lookup(&KeySequence::parse("enter").expect("keys"), &ctx) {
        Lookup::Command { command, .. } => Some(command.to_string()),
        _ => None,
    }
}

#[test]
fn a_plugins_keys_in_its_languages_files() {
    let dir: PathBuf = std::env::temp_dir().join(format!("kalem-lang-keys-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lead")).expect("folder");
    std::fs::write(
        dir.join("lead/plugin.json"),
        r#"{"id": "org.example.lead", "name": "Lead", "version": "1",
            "languages": [{"id": "lead", "name": "Lead", "extensions": ["lg"]}]}"#,
    )
    .expect("manifest");
    kalem_core::languages::load_from(std::slice::from_ref(&dir));
    let mut reg = CommandRegistry::with_builtins();
    reg.register(Command {
        id: "lead.newline".into(),
        title: "New Line".into(),
        category: String::new(),
        default_keys: Vec::new(),
        when: None,
        handler: CommandHandler::Plugin("lead".into()),
        args_schema: None,
        source: CommandSource::Plugin("lead".into()),
        scope: Some(Scope::only(&["lg", "lead"])),
    })
    .expect("registered");
    kalem_core::extensions::add_binding(
        1,
        "enter",
        "lead.newline",
        Some("textType == lg || textType == lead"),
    )
    .expect("bound");
    let (m, issues) = Keymap::build(&reg, Profile::Word, &[]);
    assert!(issues.is_empty(), "{issues:#?}");
    assert_eq!(enter(&m, "lg").as_deref(), Some("lead.newline"));
    assert_eq!(enter(&m, "lead").as_deref(), Some("lead.newline"));
    // Another plain text file: the profile's Enter.
    assert_eq!(enter(&m, "txt").as_deref(), Some("edit.newline"));
    let _ = std::fs::remove_dir_all(&dir);
}
