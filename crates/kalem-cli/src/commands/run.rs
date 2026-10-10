//! `kalem run PLUGIN COMMAND [FILE...]`: batch mode (design §7.7,
//! T3.1.16). An installed extension plugin's command runs against each
//! file as the editors run it, without a window: the document it reads and
//! edits, the commands it asks for, the documents it writes printed, its
//! notices on standard error, its questions answered as a script would
//! leave them (a prompt with the text it offers, else cancelled; a choice
//! with nothing; a confirmation with no). A file the command changed is
//! saved. The folders named (a file's own) are in the plugin's workspace
//! with the projects for the run.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use kalem_core::command::{Clipboard, CommandRegistry, EditorContext, Request};
use kalem_core::extensions::{Answer, Question};
use kalem_core::{Config, DocumentState};
use serde_json::Value;

use super::Result;

/// How many rounds of answers and queued commands one run may take: a
/// plugin asking again after every answer stops there.
const ROUNDS: usize = 1000;

/// What one run of a command left.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The plugin's documents it showed, by number, in order.
    pub shown: Vec<u64>,
    /// The command or a command it asked for failed, or the plugin
    /// reported an error.
    pub failed: bool,
}

/// Runs command `command` with `args` against `doc` (none: without a
/// document) as the editors run it, then what it set going until nothing
/// is left: the commands it queued, its questions answered as batch mode
/// answers them, a save it asked for. Notices, and what batch mode cannot
/// do (open a window, a link), go to `err`.
pub fn drive(
    registry: &CommandRegistry,
    doc: Option<&mut DocumentState>,
    config: &Config,
    command: &str,
    args: &Value,
    err: &mut dyn Write,
) -> Outcome {
    let mut out = Outcome::default();
    let mut clipboard = Clipboard::default();
    let clock = jiff::Zoned::now().datetime();
    let mut ctx = EditorContext::new(doc, &mut clipboard, config, Instant::now(), clock);
    if let Err(e) = registry.execute(command, &mut ctx, args) {
        let _ = writeln!(err, "kalem run: {command}: {e}");
        out.failed = true;
    }
    for _ in 0..ROUNDS {
        let mut busy = false;
        for (request, question) in kalem_core::extensions::take_questions() {
            busy = true;
            let answer = match question {
                Question::Prompt { value, .. } => Answer::Text(value),
                Question::Confirm(_) => Answer::Confirmed(false),
                Question::Pick { .. } => Answer::Picked(Vec::new()),
            };
            kalem_core::extensions::answer(request, answer);
        }
        for (id, a) in kalem_core::extensions::take_runs() {
            busy = true;
            if let Err(e) = registry.execute(&id, &mut ctx, &a) {
                let _ = writeln!(err, "kalem run: {id}: {e}");
                out.failed = true;
            }
        }
        let mut requests = std::mem::take(&mut ctx.requests);
        requests.extend(kalem_core::extensions::take_requests());
        for r in requests {
            busy = true;
            match r {
                Request::ShowGenerated(n) if !out.shown.contains(&n) => out.shown.push(n),
                Request::ShowGenerated(_) => {}
                Request::CloseGenerated(n) => out.shown.retain(|s| *s != n),
                Request::Save => {
                    if let Ok(d) = ctx.doc()
                        && let Err(e) = d.save(Default::default(), false)
                    {
                        let _ = writeln!(err, "kalem run: not saved: {e}");
                        out.failed = true;
                    }
                }
                Request::Open { path: Some(p) } | Request::OpenAt { path: p, .. } => {
                    let _ = writeln!(err, "kalem run: {command} would open {p}");
                }
                _ => {}
            }
        }
        for (text, error) in kalem_core::jobs::take_notices() {
            let _ = writeln!(err, "{text}");
            out.failed |= error;
        }
        if !busy {
            break;
        }
    }
    out
}

/// The documents `numbers` as text: one alone as it is, several each
/// under its title; or as JSON.
pub(crate) fn print_documents(
    numbers: &[u64],
    json: bool,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let docs: Vec<kalem_core::extensions::Generated> = numbers
        .iter()
        .filter_map(|n| kalem_core::extensions::generated(*n))
        .collect();
    if json {
        let v: Vec<Value> = docs
            .iter()
            .map(|g| {
                serde_json::json!({
                    "id": g.id, "key": g.key, "title": g.title, "kind": g.kind, "text": g.text,
                })
            })
            .collect();
        return writeln!(out, "{}", Value::Array(v));
    }
    for (i, g) in docs.iter().enumerate() {
        if docs.len() > 1 {
            if i > 0 {
                writeln!(out)?;
            }
            writeln!(out, "== {}", g.title)?;
        }
        write!(out, "{}", g.text)?;
        if !g.text.ends_with('\n') && !g.text.is_empty() {
            writeln!(out)?;
        }
    }
    Ok(())
}

/// `kalem run`: plugin `plugin` (its manifest's ID or its short one)
/// started, and its command `command` run with `args` (JSON) once
/// without a file, or against each of `files` (a folder opens as its
/// listing); the documents it showed printed to `out`, as text or JSON.
pub fn run(
    plugin: &str,
    command: &str,
    files: &[PathBuf],
    args: Option<&str>,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<ExitCode> {
    let args: Value = match args {
        Some(a) => serde_json::from_str(a).map_err(|e| format!("--args: {e}"))?,
        None => Value::Object(Default::default()),
    };
    // The settings the editors read: the user's, and a workspace's.
    let user = kalem_core::settings::config_dir().map(|d| d.join("settings.toml"));
    let workspace = files
        .first()
        .and_then(|p| std::path::absolute(p).ok())
        .and_then(|p| {
            let dir = if p.is_dir() {
                Some(p.clone())
            } else {
                p.parent().map(Path::to_path_buf)
            };
            dir.and_then(|d| kalem_core::settings::find_workspace_settings(&d))
        });
    let config = Config::load(user.as_deref(), workspace.as_deref());
    config.apply_process_settings();
    kalem_core::extensions::set_config(&config);
    // The folders named are the plugin's to read (and, with its
    // permission, to write) as a project's are.
    for f in files {
        let f = std::path::absolute(f).unwrap_or_else(|_| f.clone());
        let dir = if f.is_dir() {
            Some(f)
        } else {
            f.parent().map(Path::to_path_buf)
        };
        if let Some(d) = dir {
            kalem_core::extensions::name_folder(d);
        }
    }
    let id = crate::extensions::load_one(plugin)?;
    for (text, _) in kalem_core::jobs::take_notices() {
        let _ = writeln!(err, "{text}");
    }
    let registry = CommandRegistry::with_builtins();
    let ours = registry
        .get(command)
        .is_some_and(|c| c.source == kalem_core::command::CommandSource::Plugin(id.clone()));
    if !ours {
        let mut theirs: Vec<String> = registry
            .commands()
            .filter(|c| c.source == kalem_core::command::CommandSource::Plugin(id.clone()))
            .map(|c| c.id.clone())
            .collect();
        theirs.sort();
        return Err(format!(
            "{plugin} has no command {command}; its commands: {}",
            theirs.join(", ")
        ));
    }
    let mut shown = Vec::new();
    let mut failed = false;
    if files.is_empty() {
        let o = drive(&registry, None, &config, command, &args, err);
        shown.extend(o.shown);
        failed |= o.failed;
    }
    for file in files {
        let mut doc = DocumentState::open(
            file,
            std::sync::Arc::new(org_model::Settings::default()),
            &org_syntax::ParseContext::default(),
        )
        .map_err(|e| format!("{}: {e}", file.display()))?;
        let o = drive(&registry, Some(&mut doc), &config, command, &args, err);
        for n in o.shown {
            if !shown.contains(&n) {
                shown.push(n);
            }
        }
        failed |= o.failed;
        // What the command changed is kept, as `kalem fmt` keeps it.
        if doc.is_modified() {
            match doc.save(Default::default(), false) {
                Ok(()) => {
                    let _ = writeln!(err, "kalem run: saved {}", file.display());
                }
                Err(e) => {
                    let _ = writeln!(err, "kalem run: {}: not saved: {e}", file.display());
                    failed = true;
                }
            }
        }
    }
    print_documents(&shown, json, out).map_err(|e| e.to_string())?;
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}
