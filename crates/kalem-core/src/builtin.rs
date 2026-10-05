//! The built-in commands: every command of `org-edit`, and undo and redo.
//! Their default keys are those of the Word-like profile (§7.3); other
//! profiles come from keymaps.

use serde_json::Value;

use crate::command::{
    Command, CommandError, CommandHandler, CommandResult, CommandSource, DocumentsRequest,
    EditorContext, PickKind, ProjectRequest, Request,
};
use crate::keys::KeySequence;
use crate::layout::{Axis, PaneOp};
use crate::when::WhenClause;
use crate::workspaces::WorkspaceOp;

type Handler = fn(&mut EditorContext<'_>, &Value) -> CommandResult;

fn cmd(
    id: &str,
    title: &str,
    category: &str,
    keys: &[&str],
    when: Option<&str>,
    handler: Handler,
) -> Command {
    Command {
        id: id.into(),
        title: title.into(),
        category: category.into(),
        default_keys: literal_keys(keys),
        when: when.map(literal_when),
        handler: CommandHandler::Native(handler),
        args_schema: None,
        source: CommandSource::Builtin,
        scope: None,
    }
}

/// The default keys of a built-in command, written in the source. One
/// that does not parse is a bug the test `every_builtin_parses` catches;
/// a release leaves it out rather than failing at start.
pub(crate) fn literal_keys(keys: &[&str]) -> Vec<KeySequence> {
    keys.iter()
        .filter_map(|k| {
            let parsed = KeySequence::parse(k);
            debug_assert!(parsed.is_some(), "invalid default key `{k}`");
            parsed
        })
        .collect()
}

/// The when-clause of a built-in command, written in the source. One that
/// does not parse is a bug the test `every_builtin_parses` catches; a
/// release disables the command (`false`) rather than failing at start.
pub(crate) fn literal_when(w: &str) -> WhenClause {
    WhenClause::parse(w).unwrap_or_else(|e| {
        debug_assert!(false, "invalid when-clause `{w}`: {e:?}");
        WhenClause::Const(false)
    })
}

/// `c` with an explicit scope.
fn scoped(mut c: Command, scope: crate::command::Scope) -> Command {
    c.scope = Some(scope);
    c
}

/// The scope of a built-in command (§11.2): Org's editing commands serve
/// Org text (not the source blocks in it), the line commands of plain
/// text every type but Org, the file manager's the file manager, and the
/// rest (files, views, search, the palette, exports) every type.
pub(crate) fn default_scope(c: &Command) -> crate::command::Scope {
    use crate::command::Scope;
    use crate::when::{Value as W, WhenClause};
    // The clauses of a top-level `&&` chain.
    fn requires(w: &WhenClause, f: &dyn Fn(&WhenClause) -> bool) -> bool {
        match w {
            WhenClause::And(a, b) => requires(a, f) || requires(b, f),
            w => f(w),
        }
    }
    let org = |w: &WhenClause| matches!(w, WhenClause::Eq(k, W::Str(v)) if k == "editorMode" && v == "org");
    let plain = |w: &WhenClause| matches!(w, WhenClause::Ne(k, W::Str(v)) if k == "editorMode" && v == "org");
    let dired = |w: &WhenClause| matches!(w, WhenClause::Eq(k, W::Str(v)) if k == "editorMode" && v == "directory");
    // Whole documents and files: every type at the cursor, their when
    // keeps them to Org documents.
    let document = ["export.", "file.", "app.", "view.", "stats.", "project."];
    match &c.when {
        _ if document.iter().any(|p| c.id.starts_with(p)) => Scope::all(),
        Some(w) if requires(w, &dired) => Scope::only(&["directory"]),
        Some(w) if requires(w, &org) => Scope::only(&["org"]),
        Some(w) if requires(w, &plain) => Scope::except(&["org"]),
        _ => Scope::all(),
    }
}

/// The schema of an object with these properties (`name`, JSON type,
/// required).
fn object(props: &[(&str, &str, bool)]) -> Value {
    let mut properties = serde_json::Map::new();
    for (name, ty, _) in props {
        properties.insert((*name).into(), serde_json::json!({ "type": ty }));
    }
    let required: Vec<&str> = props.iter().filter(|p| p.2).map(|p| p.0).collect();
    serde_json::json!({ "type": "object", "properties": properties, "required": required })
}

/// The argument schemas of the built-in commands that take arguments.
fn schemas() -> Vec<(&'static str, Value)> {
    vec![
        (
            "org.headline.setLevel",
            object(&[("level", "integer", true)]),
        ),
        (
            "org.headline.sort",
            object(&[
                ("by", "string", true),
                ("property", "string", false),
                ("withCase", "boolean", false),
            ]),
        ),
        ("org.todo.set", object(&[("state", "string", true)])),
        ("org.priority.set", object(&[("priority", "string", true)])),
        (
            "org.property.set",
            object(&[("key", "string", true), ("value", "string", true)]),
        ),
        ("org.tags.set", object(&[("tags", "array", true)])),
        ("org.tags.toggle", object(&[("tag", "string", true)])),
        (
            "list.cycleBullet",
            object(&[("bullet", "string", false), ("previous", "boolean", false)]),
        ),
        (
            "list.toggleCheckbox",
            object(&[("presence", "boolean", false)]),
        ),
        ("list.insertItem", object(&[("checkbox", "boolean", false)])),
        (
            "table.recalculate",
            object(&[("iterate", "boolean", false)]),
        ),
        (
            "table.sortRows",
            object(&[("by", "string", true), ("withCase", "boolean", false)]),
        ),
        (
            "file.open",
            object(&[("path", "string", false), ("prompt", "boolean", false)]),
        ),
        ("file.scratch", object(&[("project", "boolean", false)])),
        ("plugin.install", object(&[("source", "string", false)])),
        (
            "file.openWithPassword",
            object(&[("path", "string", true), ("password", "string", false)]),
        ),
        ("plugin.installGitHub", object(&[("link", "string", false)])),
        ("code.rename", object(&[("name", "string", false)])),
        ("code.applyEdit", object(&[("id", "integer", true)])),
        ("code.dropEdit", object(&[("id", "integer", true)])),
        ("code.runAction", object(&[("id", "integer", true)])),
        (
            "plugin.confirmInstall",
            object(&[("staging", "string", true), ("source", "string", false)]),
        ),
        (
            "plugin.cancelInstall",
            object(&[("staging", "string", true)]),
        ),
        ("plugin.manage", object(&[("id", "string", true)])),
        ("plugin.remove", object(&[("id", "string", true)])),
        ("plugin.removeConfirmed", object(&[("id", "string", true)])),
        ("file.rename", object(&[("target", "string", false)])),
        ("app.terminal", object(&[("project", "boolean", false)])),
        ("settings.set", object(&[("key", "string", true)])),
        (
            "project.shellCommand",
            object(&[("command", "string", true)]),
        ),
        (
            "search.lines",
            object(&[
                ("all", "boolean", false),
                ("headings", "boolean", false),
                ("word", "boolean", false),
            ]),
        ),
        (
            "search.folder",
            object(&[("path", "string", false), ("ask", "boolean", false)]),
        ),
        ("file.copy", object(&[("target", "string", false)])),
        ("org.property.delete", object(&[("key", "string", true)])),
        ("org.cite.insert", object(&[("key", "string", false)])),
        ("org.insert.drawer", object(&[("name", "string", true)])),
        ("org.caption.set", object(&[("caption", "string", true)])),
        ("edit.gotoLine", object(&[("line", "integer", true)])),
        ("lines.sort", object(&[("reverse", "boolean", false)])),
        ("file.reopenWithEncoding", {
            let mut s = object(&[("encoding", "string", true)]);
            s["properties"]["encoding"]["enum"] = serde_json::json!(crate::files::COMMON_ENCODINGS);
            s
        }),
        ("file.saveWithEncoding", {
            let mut s = object(&[("encoding", "string", true)]);
            s["properties"]["encoding"]["enum"] = serde_json::json!(crate::files::COMMON_ENCODINGS);
            s
        }),
        (
            "stats.setDocumentTarget",
            object(&[("words", "string", true)]),
        ),
        (
            "stats.setSectionTarget",
            object(&[("words", "string", true)]),
        ),
        (
            crate::refile::REFILE,
            object(&[("target", "integer", false)]),
        ),
        ("org.name.set", object(&[("name", "string", true)])),
        (
            crate::affiliated::REFERENCE,
            object(&[("target", "string", false)]),
        ),
        ("org.schedule", {
            // `format: date`: frontends offer a date picker.
            let mut s = object(&[("date", "string", true)]);
            s["properties"]["date"]["format"] = Value::from("date");
            s
        }),
        ("org.deadline", {
            let mut s = object(&[("date", "string", true)]);
            s["properties"]["date"]["format"] = Value::from("date");
            s
        }),
        ("file.import", object(&[("file", "string", true)])),
        ("org.note.add", object(&[("note", "string", false)])),
        ("org.footnote.new", object(&[("label", "string", false)])),
        ("project.remove", object(&[("path", "string", false)])),
        ("project.add", object(&[("path", "string", false)])),
        ("project.rename", object(&[("name", "string", true)])),
        ("table.import", object(&[("file", "string", true)])),
        ("table.export", object(&[("file", "string", true)])),
        (
            "table.setFormula",
            object(&[("formula", "string", true), ("field", "boolean", false)]),
        ),
        (
            "org.insert.link",
            object(&[("link", "string", true), ("description", "string", false)]),
        ),
        ("org.insert.block", object(&[("type", "string", true)])),
        (
            "org.insert.timestamp",
            object(&[
                ("withTime", "boolean", false),
                ("inactive", "boolean", false),
            ]),
        ),
        ("csv.sortFile", object(&[("reverse", "boolean", false)])),
        (
            "csv.setField",
            object(&[
                ("row", "integer", false),
                ("column", "integer", false),
                ("value", "string", true),
            ]),
        ),
        (
            "csv.replaceInColumn",
            object(&[("find", "string", true), ("replace", "string", false)]),
        ),
        ("csv.filter", object(&[("text", "string", true)])),
        ("csv.sortView", object(&[("reverse", "boolean", false)])),
        ("csv.setDelimiter", object(&[("delimiter", "string", true)])),
        (
            "app.newWorkbook",
            object(&[("path", "string", false), ("replace", "boolean", false)]),
        ),
        (
            "app.newFromTemplate",
            object(&[
                ("template", "string", false),
                ("path", "string", false),
                ("replace", "boolean", false),
            ]),
        ),
        (
            "csv.openAsWorkbook",
            object(&[
                ("delimiter", "string", false),
                ("encoding", "string", false),
                ("decimal", "string", false),
                ("column types", "string", false),
                ("path", "string", false),
                ("replace", "boolean", false),
            ]),
        ),
        ("csv.setQuote", object(&[("quote", "string", true)])),
        (
            "csv.splitColumn",
            object(&[
                ("separator", "string", true),
                ("confirmed", "boolean", false),
            ]),
        ),
        ("bookmark.set", object(&[("name", "string", false)])),
        ("session.save", object(&[("name", "string", false)])),
        (
            "markdown.table.sort",
            object(&[("reverse", "boolean", false)]),
        ),
        (
            "markdown.insert.link",
            object(&[("bare", "boolean", false)]),
        ),
        (
            "markdown.frontMatter.set",
            object(&[("key", "string", true), ("value", "string", true)]),
        ),
        (
            "markdown.insert.image",
            object(&[("path", "string", false)]),
        ),
        (
            "markdown.frontMatter.delete",
            object(&[("key", "string", true)]),
        ),
        ("insert.text", object(&[("text", "string", true)])),
        ("pane.focus", object(&[("dir", "string", true)])),
        ("workspace.newNamed", object(&[("name", "string", true)])),
        ("workspace.rename", object(&[("name", "string", true)])),
        ("workspace.switch", object(&[("index", "integer", true)])),
        ("workspace.load", object(&[("name", "string", false)])),
        (
            "workspace.deleteSaved",
            object(&[("name", "string", false)]),
        ),
        ("pane.move", object(&[("dir", "string", true)])),
        (
            "pane.resize",
            object(&[("axis", "string", false), ("by", "integer", false)]),
        ),
        ("pane.rotate", object(&[("back", "boolean", false)])),
        ("session.saveAs", object(&[("name", "string", true)])),
        ("session.restore", object(&[("name", "string", false)])),
        ("session.restoreNamed", object(&[("name", "string", false)])),
        ("bookmark.goto", object(&[("name", "string", true)])),
        ("bookmark.delete", object(&[("name", "string", false)])),
        (
            "csv.joinColumns",
            object(&[
                ("separator", "string", true),
                ("confirmed", "boolean", false),
            ]),
        ),
        ("csv.sortFileBy", object(&[("columns", "string", true)])),
        ("csv.goToCell", object(&[("cell", "string", true)])),
        ("csv.selectColumn", object(&[("column", "integer", false)])),
        ("csv.selectRow", object(&[("row", "integer", false)])),
        (
            "csv.setColumnWidth",
            object(&[("width", "string", true), ("column", "integer", false)]),
        ),
        (
            "csv.autosizeColumn",
            object(&[("column", "integer", false)]),
        ),
        ("csv.sumColumn", object(&[("insert", "boolean", false)])),
        (
            "bib.sortView",
            object(&[("column", "string", false), ("reverse", "boolean", false)]),
        ),
        (
            "bib.setField",
            object(&[("field", "string", true), ("value", "string", true)]),
        ),
        (
            "bib.newEntry",
            object(&[("type", "string", false), ("key", "string", true)]),
        ),
        ("latex.nextProblem", object(&[("at", "integer", false)])),
        (
            "latex.section.setLevel",
            object(&[("level", "integer", true)]),
        ),
        (
            "latex.insert.figure",
            object(&[
                ("path", "string", false),
                ("width", "string", false),
                ("caption", "string", false),
            ]),
        ),
        (
            "latex.insert.table",
            object(&[("columns", "integer", false), ("rows", "integer", false)]),
        ),
        ("latex.insert.citation", object(&[("key", "string", false)])),
        (
            "file.newFromTemplate",
            object(&[("template", "string", false), ("path", "string", false)]),
        ),
        ("view.setMode", {
            let mut s = object(&[("mode", "string", true)]);
            // A mode, or a language (text with its highlighting).
            s["properties"]["mode"]["examples"] =
                serde_json::json!(crate::settings::DOCUMENT_MODES);
            s["properties"]["mode"]["description"] =
                Value::from("org, markdown, csv, latex, text, or a language such as python");
            s
        }),
        ("org.insert.date", {
            // `format: date`: frontends offer a date picker.
            let mut s = object(&[("date", "string", true), ("inactive", "boolean", false)]);
            s["properties"]["date"]["format"] = Value::from("date");
            s
        }),
        (
            "table.create",
            object(&[("columns", "integer", false), ("rows", "integer", false)]),
        ),
        ("table.insertRow", object(&[("below", "boolean", false)])),
        ("table.insertHline", object(&[("above", "boolean", false)])),
    ]
}

/// The footnote layout of the settings (`org.footnote_section`).
fn footnote_settings(config: &crate::settings::Config) -> org_edit::footnote::FootnoteSettings {
    let section = config.str("org.footnote_section").trim().to_string();
    org_edit::footnote::FootnoteSettings {
        section: (!section.is_empty()).then_some(section),
        ..Default::default()
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, CommandError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::new(crate::tr!("msg-missing-argument", name = key)))
}

fn arg_bool(args: &Value, key: &str) -> bool {
    args.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A path typed by the user: relative to the document's directory.
fn resolve_path(doc: &crate::document::DocumentState, file: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(file.trim());
    let p = match p.strip_prefix("~") {
        Ok(rest) => std::env::var_os("HOME")
            .map_or(p.to_path_buf(), |h| std::path::PathBuf::from(h).join(rest)),
        Err(_) => p.to_path_buf(),
    };
    if p.is_absolute() {
        return p;
    }
    doc.meta
        .path
        .as_ref()
        .and_then(|d| d.parent())
        .map_or(p.clone(), |d| d.join(&p))
}

const ORG: &str = "editorMode == org";
const TABLE: &str = "editorMode == org && inTable";
const LIST: &str = "editorMode == org && inList";

/// The text and parse context of a model, for the commands that take text.
fn text_of(d: &org_model::Document) -> String {
    d.parse().syntax().to_string()
}

fn todo(ctx: &mut EditorContext<'_>, arg: org_edit::todo::TodoArg) -> CommandResult {
    use org_edit::todo::*;
    let (clock, base) = (ctx.clock, ctx.config.todo_settings());
    let mut pending = None;
    ctx.org(|d, p, _| {
        let settings = base.for_document(d);
        let opts = TodoOptions {
            arg,
            settings: &settings,
            now: clock,
            remembered_head: None,
            repeated: false,
            force_note: false,
            inhibit_note: false,
        };
        todo(d, p, &opts).map(|o| {
            pending = o.note;
            o.transaction
        })
    })?;
    match pending {
        Some(n) => ask_note(ctx, &n),
        None => Ok(()),
    }
}

/// Asks for the note a log entry waits for (the `*Org Note*` buffer):
/// `org.note.add` runs with the answer; cancelled, nothing is logged.
fn ask_note(ctx: &mut EditorContext<'_>, note: &org_edit::todo::PendingNote) -> CommandResult {
    request(
        ctx,
        Request::Ask {
            command: "org.note.add".into(),
            args: serde_json::json!({
                "heading": note.heading,
                "purpose": note.purpose.name(),
                "state": note.state,
                "previous": note.previous_state,
                "time": note.time.to_string(),
            }),
            arg: "note".into(),
        },
    )
}

/// `org.note.add`: the log entry a command left for its note, or with no
/// such entry `org-add-note` on the entry at the cursor.
fn add_note(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    use org_edit::todo::{NotePurpose, PendingNote, add_note, store_log_note};
    let Some(content) = args.get("note").and_then(Value::as_str) else {
        return request(
            ctx,
            Request::Ask {
                command: "org.note.add".into(),
                args: args.clone(),
                arg: "note".into(),
            },
        );
    };
    let content = content.to_string();
    let pending = args
        .get("purpose")
        .and_then(Value::as_str)
        .and_then(NotePurpose::from_name)
        .map(|purpose| PendingNote {
            heading: args.get("heading").and_then(Value::as_u64).unwrap_or(0) as usize,
            purpose,
            state: args
                .get("state")
                .and_then(Value::as_str)
                .map(str::to_string),
            previous_state: args
                .get("previous")
                .and_then(Value::as_str)
                .map(str::to_string),
            time: args
                .get("time")
                .and_then(Value::as_str)
                .and_then(|t| t.parse().ok())
                .unwrap_or(ctx.clock),
        });
    let (clock, base) = (ctx.clock, ctx.config.todo_settings());
    ctx.org(|d, p, _| {
        let settings = base.for_document(d);
        match &pending {
            Some(n) => Ok(store_log_note(d, p, n, &content, &settings)),
            None => add_note(d, p, &content, &settings, clock),
        }
    })
}

/// A date typed for a planning line, and a repeater or warning at its end
/// (`2026-10-05 +1w`, `friday .+2d`).
fn planning_input(input: &str) -> (&str, Option<String>) {
    let t = input.trim();
    let is_part = |w: &str| {
        let body = w.trim_start_matches(['.', '+', '-', '/']);
        w.len() > body.len()
            && body.len() > 1
            && body[..body.len() - 1].bytes().all(|b| b.is_ascii_digit())
            && matches!(
                body.as_bytes()[body.len() - 1],
                b'h' | b'd' | b'w' | b'm' | b'y'
            )
    };
    let words: Vec<&str> = t.split_whitespace().collect();
    let n = words.iter().rev().take_while(|w| is_part(w)).count().min(2);
    if n == 0 || n == words.len() {
        return (t, None);
    }
    let at = t.rfind(words[words.len() - n]).unwrap_or(t.len());
    (t[..at].trim_end(), Some(words[words.len() - n..].join(" ")))
}

/// `org-schedule` and `org-deadline`.
fn planning(
    ctx: &mut EditorContext<'_>,
    kind: org_edit::todo::Planning,
    args: &Value,
    remove: bool,
) -> CommandResult {
    use org_edit::todo::{PlanningChange, schedule};
    let change = if remove {
        PlanningChange::Remove
    } else {
        let input = arg_str(args, "date")?.to_string();
        let (date, repeater) = planning_input(&input);
        let (dt, with_time) = crate::dates::parse(date, ctx.clock)
            .ok_or_else(|| CommandError::new(crate::tr!("msg-not-a-date", input = &input)))?;
        PlanningChange::Set(dt, with_time, repeater)
    };
    let (clock, base) = (ctx.clock, ctx.config.todo_settings());
    let mut message = String::new();
    let mut pending = None;
    ctx.org(|d, p, _| {
        let (t, m, n) = schedule(d, p, kind, &change, &base.for_document(d), clock)?;
        message = m;
        pending = n;
        Ok(t)
    })?;
    ctx.messages.push(message);
    match pending {
        Some(n) => ask_note(ctx, &n),
        None => Ok(()),
    }
}

fn priority(ctx: &mut EditorContext<'_>, a: org_edit::todo::PriorityAction) -> CommandResult {
    let base = ctx.config.todo_settings();
    ctx.org(|d, p, _| org_edit::todo::priority(d, p, a, false, &base.for_document(d)))
}

fn emphasis(ctx: &mut EditorContext<'_>, kind: org_edit::emphasis::Emphasis) -> CommandResult {
    ctx.org(|d, p, m| {
        let (s, e) = m.map_or((p, p), |m| (m.min(p), m.max(p)));
        if s == e {
            org_edit::emphasis::emphasize(d, p, None, Some(kind.marker()))
        } else {
            org_edit::emphasis::toggle_emphasis(d, s, e, kind)
        }
    })
}

/// Every built-in command.
pub(crate) fn commands() -> Vec<Command> {
    let mut schemas = schemas();
    schemas.extend(crate::dired::schemas());
    let mut all = plain_commands();
    all.extend(crate::dired::commands());
    all.extend(crate::viewer::commands());
    all.extend(crate::extensions::core_commands());
    all.extend(csv_commands());
    all.extend(markdown_commands());
    all.extend(bib_commands());
    all.push(scoped(
        cmd(
            "latex.build",
            "Build PDF",
            "LaTeX",
            &["f5"],
            None,
            |ctx, _| latex_build(ctx),
        ),
        crate::command::Scope::only(&["latex"]),
    ));
    all.push(scoped(
        cmd(
            "latex.ignoreBuildOutputs",
            "Ignore Build Outputs in Git",
            "LaTeX",
            &[],
            None,
            |ctx, _| {
                let doc = ctx
                    .document
                    .as_deref()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-no-document")))?;
                let Some(path) = doc.meta.path.clone() else {
                    return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
                };
                let path = std::path::absolute(&path).unwrap_or(path);
                let root = crate::latex_view::find_root(&path, doc.text().as_str());
                let out = ctx.config.str("latex.output_directory").trim().to_string();
                let out = (!out.is_empty()).then(|| std::path::PathBuf::from(out));
                let in_git = root
                    .parent()
                    .is_some_and(|d| d.ancestors().any(|a| a.join(".git").exists()));
                if !in_git {
                    return Err(CommandError::new(crate::l10n::tr("msg-not-in-git")));
                }
                let message = match crate::latex_build::ignore_build_outputs(&root, out.as_deref())
                    .map_err(CommandError::new)?
                {
                    (0, _) => crate::l10n::tr("msg-outputs-ignored-already"),
                    (n, file) => crate::tr!(
                        "msg-ignored-outputs",
                        count = n,
                        path = file.display().to_string()
                    ),
                };
                ctx.messages.push(message);
                Ok(())
            },
        ),
        crate::command::Scope::only(&["latex"]),
    ));
    all.push(scoped(
        cmd(
            "latex.showInPdf",
            "Show in PDF",
            "LaTeX",
            &[],
            None,
            |ctx, _| {
                // The built PDF at the page where the cursor's line is
                // typeset, by the build's SyncTeX file.
                let doc = ctx
                    .document
                    .as_deref()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-no-document")))?;
                let Some(path) = doc.meta.path.clone() else {
                    return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
                };
                let path = std::path::absolute(&path).unwrap_or(path);
                let line = doc.text().line_of(doc.selection.head) + 1;
                let root = crate::latex_view::find_root(&path, doc.text().as_str());
                let out = ctx.config.str("latex.output_directory").trim().to_string();
                let dir = root
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default();
                let dir = if out.is_empty() { dir } else { dir.join(out) };
                let stem = root
                    .file_stem()
                    .map(std::path::PathBuf::from)
                    .unwrap_or_default();
                let pdf = dir.join(stem).with_extension("pdf");
                if !pdf.is_file() {
                    return Err(CommandError::new(crate::l10n::tr("msg-no-pdf-yet")));
                }
                let place =
                    crate::synctex::Synctex::cached(&pdf).and_then(|st| st.forward(&path, line));
                if place.is_none() {
                    ctx.messages.push(crate::l10n::tr("msg-no-synctex"));
                }
                // A paged file's "line" is the page, its "column" the
                // height of the line's middle on it, in points.
                ctx.requests.push(Request::OpenAt {
                    path: pdf.display().to_string(),
                    line: place.as_ref().map_or(1, |p| p.page) as u64,
                    column: place
                        .as_ref()
                        .map_or(0, |p| (p.y + p.height / 2.0).round().max(0.0) as usize),
                });
                Ok(())
            },
        ),
        crate::command::Scope::only(&["latex"]),
    ));
    all.extend(latex_commands());
    all.extend(code_commands());
    all.extend(plugin_commands());
    for c in &mut all {
        c.args_schema = schemas
            .iter()
            .find(|(id, _)| *id == c.id)
            .map(|(_, s)| s.clone());
    }
    all
}

/// After leaving a field: the table's formulas again, when
/// `org.table_auto_recalc` asks for it and the table has any. Errors stay
/// quiet; F9 reports them.
fn auto_recalc(ctx: &mut EditorContext<'_>) {
    if !ctx.config.bool("org.table_auto_recalc") {
        return;
    }
    let _ = ctx.org(|d, p, _| {
        let r = org_edit::recalc::recalculate(d, p, false)?;
        Ok(r.transaction)
    });
}

/// Exports the active document with `backend` beside its file
/// (`#+EXPORT_FILE_NAME` names another), with unsaved changes too; with
/// `subtree`, only the subtree at the cursor (its `EXPORT_FILE_NAME`
/// property names the file).
fn export_doc(
    ctx: &mut EditorContext<'_>,
    backend: &dyn org_export::Backend,
    extension: &str,
    subtree: bool,
) -> CommandResult {
    export_doc_to(ctx, backend, extension, subtree).map(|_| ())
}

/// [`export_doc`], returning the file written.
fn export_doc_to(
    ctx: &mut EditorContext<'_>,
    backend: &dyn org_export::Backend,
    extension: &str,
    subtree: bool,
) -> Result<std::path::PathBuf, CommandError> {
    let doc = ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let path = std::path::absolute(&path).unwrap_or(path);
    let text = doc.text().as_str().to_string();
    let subtree = subtree.then_some(doc.selection.head);
    let settings = org_export::Settings {
        body_only: ctx.config.bool("export.body_only"),
        input_file: Some(path.clone()),
        now: None,
        subtree,
        math: Some(crate::math::export_renderer()),
        options: (ctx.config.str("export.math") == "svg").then(|| "tex:svg".to_string()),
    };
    let out = org_export::export(&text, backend, &settings).map_err(CommandError::new)?;
    let target = org_export::output_file_name_for(&text, &path, extension, subtree);
    std::fs::write(&target, out).map_err(|e| CommandError::new(e.to_string()))?;
    ctx.messages.push(crate::tr!(
        "msg-exported",
        path = target.display().to_string()
    ));
    if ctx.config.bool("export.open_after") {
        ctx.requests
            .push(Request::OpenLink(crate::input::LinkAction::Url(file_url(
                &target,
            ))));
    }
    Ok(target)
}

/// The LaTeX back-end, as `ox-latex` writes.
const LATEX: org_export::Latex = org_export::Latex {
    source_lines: false,
};

/// Exports the active document as LaTeX with `%% org:LINE` comments
/// beside its file, then compiles it to PDF in the background
/// (`crate::pdf`); the result, or LaTeX's first error at its Org line,
/// shows when it ends.
fn export_pdf(ctx: &mut EditorContext<'_>, subtree: bool) -> CommandResult {
    pdf_then(ctx, subtree, false)
}

/// Compiles the document (or the subtree at the cursor) to a PDF in the
/// background; then opens it (`export.open_after`) or, with `print`,
/// hands it to the system's print dialog.
fn pdf_then(ctx: &mut EditorContext<'_>, subtree: bool, print: bool) -> CommandResult {
    let doc = ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let path = std::path::absolute(&path).unwrap_or(path);
    let text = doc.text().as_str().to_string();
    let engine = crate::pdf::Engine::from_keyword(
        org_syntax::parse(&text)
            .keywords()
            .iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case("LATEX_COMPILER"))
            .map(|(_, v)| v.as_str()),
    );
    let search = std::env::var_os("PATH").unwrap_or_default();
    let tool = crate::pdf::detect_preferring(engine, &search, ctx.config.str("export.pdf_engine"))
        .ok_or_else(|| CommandError::new(crate::pdf::missing(engine, &search)))?;
    let subtree = subtree.then_some(doc.selection.head);
    let settings = org_export::Settings {
        body_only: false,
        input_file: Some(path.clone()),
        now: None,
        subtree,
        math: None,
        options: None,
    };
    let out = org_export::export(&text, &LATEX_LINES, &settings).map_err(CommandError::new)?;
    let tex = org_export::output_file_name_for(&text, &path, ".tex", subtree);
    std::fs::write(&tex, out).map_err(|e| CommandError::new(e.to_string()))?;
    let open_after = ctx.config.bool("export.open_after");
    ctx.messages.push(crate::l10n::tr("msg-compiling-pdf"));
    crate::jobs::spawn(crate::l10n::tr("msg-compiling-pdf"), move || {
        let compiled = crate::pdf::compile(&tool, engine, &tex);
        let pdf = compiled.as_ref().ok().and_then(|c| c.pdf.clone());
        let mut done = pdf_result(&path, compiled, open_after && !print);
        if print && !done.error {
            done.open = pdf.map(crate::input::LinkAction::Print);
        }
        done
    });
    Ok(())
}

/// What compiling a PDF gave, for the user.
fn pdf_result(
    org: &std::path::Path,
    compiled: Result<crate::pdf::Compiled, String>,
    open_after: bool,
) -> crate::jobs::Finished {
    let name = org
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let c = match compiled {
        Ok(c) => c,
        Err(e) => {
            return crate::jobs::Finished {
                message: crate::tr!("msg-pdf-failed", error = e),
                error: true,
                open: None,
            };
        }
    };
    let errors: Vec<&crate::pdf::Problem> = c.problems.iter().filter(|p| p.error).collect();
    let warnings = c.problems.len() - errors.len();
    if let Some(first) = errors.first() {
        let place = match first.org_line {
            Some(n) => format!("{name}:{n}"),
            None => name,
        };
        return crate::jobs::Finished {
            message: crate::tr!(
                "msg-pdf-error",
                place = place,
                error = first.message.clone(),
                count = errors.len() - 1
            ),
            error: true,
            open: None,
        };
    }
    match c.pdf {
        Some(pdf) => crate::jobs::Finished {
            message: crate::tr!(
                "msg-pdf-done",
                path = pdf.display().to_string(),
                count = warnings
            ),
            error: false,
            open: open_after.then(|| crate::input::LinkAction::Url(file_url(&pdf))),
        },
        None => crate::jobs::Finished {
            message: crate::tr!("msg-pdf-failed", error = "no PDF".to_string()),
            error: true,
            open: None,
        },
    }
}

/// Writes the active document as `format` through pandoc, in the
/// background, beside its file (or where `#+EXPORT_FILE_NAME` says).
fn export_pandoc(ctx: &mut EditorContext<'_>, format: crate::pandoc::Format) -> CommandResult {
    let doc = ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let path = std::path::absolute(&path).unwrap_or(path);
    let text = doc.text().as_str().to_string();
    let search = std::env::var_os("PATH").unwrap_or_default();
    let pandoc = crate::pandoc::find_with(ctx.config.str("export.pandoc_path"), &search)
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-pandoc")))?;
    let target = org_export::output_file_name_for(&text, &path, format.extension(), None);
    let open_after = ctx.config.bool("export.open_after");
    let status = crate::l10n::tr("msg-converting");
    ctx.messages.push(status.clone());
    crate::jobs::spawn(status, move || {
        match crate::pandoc::export(&pandoc, &text, &path, format, &target) {
            Ok(()) => crate::jobs::Finished {
                message: crate::tr!("msg-exported", path = target.display().to_string()),
                error: false,
                open: open_after.then(|| crate::input::LinkAction::Url(file_url(&target))),
            },
            Err(e) => crate::jobs::Finished {
                message: crate::tr!("msg-pandoc-failed", error = e),
                error: true,
                open: None,
            },
        }
    });
    Ok(())
}

/// Converts a Word, OpenDocument, Markdown, HTML, EPUB or RTF file to Org
/// through pandoc, beside it, and opens the result.
fn import_file(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    let file = std::path::PathBuf::from(arg_str(args, "file")?.trim());
    let file = match (&ctx.document, file.is_relative()) {
        (Some(d), true) => d
            .meta
            .path
            .as_ref()
            .and_then(|p| p.parent())
            .map_or(file.clone(), |dir| dir.join(&file)),
        _ => file,
    };
    let search = std::env::var_os("PATH").unwrap_or_default();
    let pandoc = crate::pandoc::find_with(ctx.config.str("export.pandoc_path"), &search)
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-pandoc")))?;
    let target = file.with_extension("org");
    if target.exists() {
        return Err(CommandError::new(crate::tr!(
            "msg-import-exists",
            path = target.display().to_string()
        )));
    }
    let stem = file
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let media = std::path::PathBuf::from(format!("{stem}_assets"));
    let org = crate::pandoc::import(&pandoc, &file, Some(&media))
        .map_err(|e| CommandError::new(crate::tr!("msg-pandoc-failed", error = e)))?;
    std::fs::write(&target, org).map_err(|e| CommandError::new(e.to_string()))?;
    ctx.messages.push(crate::tr!(
        "msg-imported",
        path = target.display().to_string()
    ));
    ctx.requests.push(Request::Open {
        path: Some(target.display().to_string()),
    });
    Ok(())
}

/// The LaTeX back-end with `%% org:LINE` comments, for compiling.
const LATEX_LINES: org_export::Latex = org_export::Latex { source_lines: true };

/// The plain text back-end, as `ox-ascii` writes, in ASCII and in UTF-8.
const TEXT: org_export::Text = org_export::Text { utf8: false };
const TEXT_UTF8: org_export::Text = org_export::Text { utf8: true };

/// A `file:` URL for `path`.
fn file_url(path: &std::path::Path) -> String {
    let p = path.to_string_lossy().replace('\\', "/");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}")
    }
}

/// The export dialog's list: the formats, then the export settings, each
/// shown with its value; choosing a setting changes it and shows the list
/// again.
pub fn export_dialog_items(config: &crate::settings::Config) -> Vec<crate::palette::PaletteItem> {
    use crate::l10n::tr;
    let item = |id: &str, title: String, category: String| crate::palette::PaletteItem {
        id: id.to_string(),
        title,
        category,
        keys: String::new(),
        also: id.replace('.', " "),
    };
    let format = tr("category-export");
    let mut items: Vec<_> = [
        "export.html",
        "export.markdown",
        "export.gfm",
        "export.latex",
        "export.pdf",
        "export.docx",
        "export.odt",
        "export.epub",
        "export.rtf",
        "export.text",
        "export.htmlSubtree",
        "export.markdownSubtree",
        "export.latexSubtree",
    ]
    .into_iter()
    .map(|id| item(id, tr(&crate::l10n::command_key(id)), format.clone()))
    .collect();
    let on_off = |b: bool| tr(if b { "export-on" } else { "export-off" });
    let setting = tr("export-setting");
    items.push(item(
        "export.toggleBodyOnly",
        format!(
            "{}: {}",
            tr("export-body-only"),
            on_off(config.bool("export.body_only"))
        ),
        setting.clone(),
    ));
    items.push(item(
        "export.toggleOpenAfter",
        format!(
            "{}: {}",
            tr("export-open-after"),
            on_off(config.bool("export.open_after"))
        ),
        setting.clone(),
    ));
    items.push(item(
        "export.toggleMath",
        format!(
            "{}: {}",
            tr("export-math"),
            tr(if config.str("export.math") == "svg" {
                "export-math-svg"
            } else {
                "export-math-mathjax"
            })
        ),
        setting,
    ));
    items
}

/// Changes an export setting and shows the export dialog again.
fn export_setting(
    ctx: &mut EditorContext<'_>,
    key: &str,
    value: serde_json::Value,
) -> CommandResult {
    ctx.requests.push(Request::SetSetting {
        key: key.to_string(),
        value,
        quiet: false,
    });
    request(ctx, Request::ExportDialog)
}

/// Runs a line command on the text and selection of the document, as one
/// undo step; `None` from it changes nothing.
/// The `dir` argument of a pane command.
fn pane_dir(args: &Value) -> Result<crate::layout::Dir, CommandError> {
    let d = arg_str(args, "dir")?;
    crate::layout::Dir::parse(d).ok_or_else(|| CommandError::new(format!("dir: {d}")))
}

/// Inserts `text` over the selection, the cursor after it.
fn insert_plain(ctx: &mut EditorContext<'_>, text: &str) -> CommandResult {
    lines_command(ctx, |_, s| {
        let (a, z) = (s.anchor.min(s.head), s.anchor.max(s.head));
        let mut tx = org_edit::Transaction::new("Insert");
        let _ = tx.replace(a..z, text.to_string());
        Some(tx.select(org_edit::Selection::caret(a + text.len())))
    })
}

fn lines_command(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(&str, org_edit::Selection) -> Option<org_edit::Transaction>,
) -> CommandResult {
    let now = ctx.now;
    let d = ctx.doc()?;
    if let Some(tx) = f(d.text().as_str(), d.selection) {
        d.apply(&tx, org_edit::ChangeKind::Command, now);
    }
    Ok(())
}

/// Where the cursor goes in field `f`: its first character, inside the
/// quotes of a quoted one.
fn csv_caret(f: &crate::csv::Field) -> usize {
    f.range.start + usize::from(f.quoted)
}

/// Runs a CSV edit on the cell at the cursor: `f` gets the text, the
/// layout, the row, its record and the column, and gives the change and
/// the cell (row, column) the cursor goes to after it.
/// The CSV cell at the cursor, or the not-CSV error.
fn csv_cell(
    d: &crate::DocumentState,
) -> Result<
    (
        std::rc::Rc<crate::csv::Layout>,
        usize,
        crate::csv::Record,
        usize,
    ),
    CommandError,
> {
    crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))
}

/// In Edit mode, the cursor moved within the cell's value to the stop
/// `to` picks (given the value's stops and the cursor); whether it was.
fn csv_in_cell(
    d: &mut crate::DocumentState,
    to: impl FnOnce(&[usize], usize) -> Option<usize>,
) -> bool {
    if !d.csv_editing().is_some_and(|e| e.edit) {
        return false;
    }
    let Some((layout, _, rec, col)) = crate::csv::cell_at(d) else {
        return false;
    };
    let Some(f) = rec.fields.get(col) else {
        return true;
    };
    let stops = crate::csv::value_stops(d.text().as_str(), f, &layout.dialect);
    let at = d.selection.head;
    if let Some(p) = to(&stops, at) {
        d.selection = org_edit::Selection::caret(p);
    }
    true
}

/// An arrow in the grid: in Edit mode Left and Right within the cell;
/// else the cell `rows` down and `cols` right (the entry, in Enter mode,
/// ended), or with `extend` the selection's corner moved there.
fn csv_step(ctx: &mut EditorContext<'_>, rows: isize, cols: isize, extend: bool) -> CommandResult {
    let d = ctx.doc()?;
    if !extend
        && rows == 0
        && csv_in_cell(d, |stops, at| {
            let i = stops.iter().position(|s| *s >= at).unwrap_or(stops.len());
            if cols < 0 {
                i.checked_sub(1).and_then(|i| stops.get(i)).copied()
            } else {
                stops.iter().copied().find(|s| *s > at)
            }
        })
    {
        return Ok(());
    }
    let (layout, row, _, col) = csv_cell(d)?;
    let n = layout
        .index
        .borrow_mut()
        .count(d.text().as_str(), &layout.dialect);
    let to_row = (row as isize + rows).clamp(0, n.saturating_sub(1) as isize) as usize;
    let to_col = (col as isize + cols).max(0) as usize;
    d.csv_edit = None;
    if extend {
        // The selection's corner on the target cell (one the record has).
        let anchor = d.selection.anchor;
        let rec = layout
            .index
            .borrow_mut()
            .record(d.text().as_str(), to_row, &layout.dialect);
        if let Some(f) = rec
            .as_ref()
            .and_then(|r| r.fields.get(to_col).or(r.fields.last()))
        {
            let at = crate::csv::value_range(f).start;
            d.selection = org_edit::Selection { anchor, head: at };
        }
        return Ok(());
    }
    d.go_to_csv_cell(to_row, to_col);
    Ok(())
}

/// Ctrl with an arrow: to the edge of the data, as Excel goes: from a
/// filled cell with a filled one next, to the last filled one before a
/// blank; else to the next filled one; else to the sheet's edge.
fn csv_edge(ctx: &mut EditorContext<'_>, rows: isize, cols: isize) -> CommandResult {
    let d = ctx.doc()?;
    if csv_in_cell(d, |stops, _| {
        if cols < 0 || rows < 0 {
            stops.first().copied()
        } else {
            stops.last().copied()
        }
    }) {
        return Ok(());
    }
    let (layout, row, _, col) = csv_cell(d)?;
    let text = d.text().as_str();
    let dl = &layout.dialect;
    let n = layout.index.borrow_mut().count(text, dl);
    let last_col = layout.widths.len().saturating_sub(1).max(col);
    let filled = |r: usize, c: usize| -> bool {
        layout
            .index
            .borrow_mut()
            .record(text, r, dl)
            .and_then(|rec| {
                rec.fields
                    .get(c)
                    .map(|f| !crate::csv::value(text, f, dl).is_empty())
            })
            .unwrap_or(false)
    };
    let next = |(r, c): (usize, usize)| -> Option<(usize, usize)> {
        let r2 = r as isize + rows;
        let c2 = c as isize + cols;
        (r2 >= 0 && c2 >= 0 && (r2 as usize) < n && (c2 as usize) <= last_col)
            .then_some((r2 as usize, c2 as usize))
    };
    let mut at = (row, col);
    match next(at) {
        None => {}
        Some(first) if filled(at.0, at.1) && filled(first.0, first.1) => {
            at = first;
            while let Some(p) = next(at).filter(|p| filled(p.0, p.1)) {
                at = p;
            }
        }
        Some(first) => {
            at = first;
            while !filled(at.0, at.1) {
                match next(at) {
                    Some(p) => at = p,
                    None => break,
                }
            }
        }
    }
    d.csv_edit = None;
    d.go_to_csv_cell(at.0, at.1);
    Ok(())
}

/// Empties the selected cells, or the cursor's (Delete in Ready mode).
fn csv_clear_selection(d: &mut crate::DocumentState, now: std::time::Instant) -> CommandResult {
    let s = d.selection;
    if s.anchor != s.head {
        d.delete_in_grid(true, now);
        return Ok(());
    }
    let (layout, row, rec, col) = csv_cell(d)?;
    if rec.fields.get(col).is_some_and(|f| !f.range.is_empty()) {
        let tx = crate::csv::set_cell(d.text().as_str(), &rec, col, "", &layout.dialect);
        d.apply(&tx, org_edit::ChangeKind::Command, now);
    }
    d.go_to_csv_cell(row, col);
    Ok(())
}

fn csv_edit(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(
        &str,
        &crate::csv::Layout,
        usize,
        &crate::csv::Record,
        usize,
    ) -> Result<(Option<org_edit::Transaction>, Option<(usize, usize)>), CommandError>,
) -> CommandResult {
    let now = ctx.now;
    let d = ctx.doc()?;
    let (layout, row, rec, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let (tx, to) = f(d.text().as_str(), &layout, row, &rec, col)?;
    if let Some(tx) = tx {
        d.apply(&tx, org_edit::ChangeKind::Command, now);
    }
    if let Some((row, col)) = to {
        let layout = crate::csv::layout(d);
        let text = d.text().as_str();
        let r = layout.index.borrow_mut().record(text, row, &layout.dialect);
        if let Some(r) = r {
            // A column past the record's end (and on the grid): its
            // missing cell, selected without a change to the file.
            let columns = layout.widths.len();
            if col >= r.fields.len() && col < columns {
                d.selection = org_edit::Selection::caret(r.range.end);
                d.select_csv_virtual(col);
                d.settle_csv_edit();
                return Ok(());
            }
            let at = r
                .fields
                .get(col)
                .or(r.fields.last())
                .map_or(r.range.start, csv_caret);
            d.selection = org_edit::Selection::caret(at);
            d.settle_csv_edit();
        }
    }
    Ok(())
}

/// A preview of a CSV edit before it is made: the first rows as they
/// would be, each a choice that makes the edit (`command` again with
/// `confirmed`).
fn csv_preview(
    ctx: &mut EditorContext<'_>,
    command: &str,
    args: &Value,
    edit: impl FnOnce(&str, &crate::csv::Layout, usize) -> Result<org_edit::Transaction, CommandError>,
) -> CommandResult {
    let d = ctx.doc()?;
    let (layout, _, _, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let text = d.text().as_str();
    let tx = edit(text, &layout, col)?;
    let after = tx.apply(text);
    let rows = crate::csv::rows(&after, &layout.dialect);
    let mut confirmed = args.clone();
    confirmed["confirmed"] = Value::Bool(true);
    let id = crate::palette::invocation(command, &confirmed);
    let category = crate::tr!("category-preview");
    let items = rows
        .iter()
        .take(8)
        .map(|r| crate::palette::PaletteItem {
            id: id.clone(),
            title: r.join("  │  "),
            category: category.clone(),
            keys: String::new(),
            also: String::new(),
        })
        .collect();
    request(ctx, Request::Choose(items))
}

/// The optional `column` argument (counted from 0) of the commands on a
/// column's width.
fn arg_column(args: &Value) -> Option<usize> {
    args.get("column")
        .and_then(Value::as_u64)
        .and_then(|c| usize::try_from(c).ok())
}

/// Changes the columns the CSV grid shows and their widths, given the
/// layout and the column at the cursor.
fn csv_columns(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(&mut crate::csv::Columns, &crate::csv::Layout, usize) -> CommandResult,
) -> CommandResult {
    let d = ctx.doc()?;
    let (layout, _, _, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let mut cols = d.csv_columns.clone();
    f(&mut cols, &layout, col)?;
    d.csv_columns = cols;
    Ok(())
}

/// The cursor out of a hidden column: to the next one that shows, else
/// the one before.
fn csv_to_shown_column(ctx: &mut EditorContext<'_>) -> CommandResult {
    let d = ctx.doc()?;
    let Some((_, _, rec, col)) = crate::csv::cell_at(d) else {
        return Ok(());
    };
    let hidden = &d.csv_columns.hidden;
    let to = (col..rec.fields.len())
        .find(|j| !hidden.contains(j))
        .or_else(|| (0..col).rev().find(|j| !hidden.contains(j)));
    if let Some(f) = to.and_then(|j| rec.fields.get(j)) {
        d.selection = org_edit::Selection::caret(csv_caret(f));
    }
    Ok(())
}

/// Changes how the CSV grid shows the document.
fn csv_view(ctx: &mut EditorContext<'_>, f: impl FnOnce(&mut crate::csv::View)) -> CommandResult {
    let d = ctx.doc()?;
    if d.meta.mode != crate::DocumentMode::Csv {
        return Err(CommandError::new(crate::tr!("msg-not-csv")));
    }
    f(&mut d.csv_view);
    Ok(())
}

/// The rows and columns of the selection's rectangle of cells, or the
/// cursor's cell.
fn rectangle_or_cell(d: &crate::DocumentState) -> Result<crate::csv::Rectangle, CommandError> {
    if let Some(r) = crate::csv::cell_rectangle(d) {
        return Ok(r);
    }
    let (_, r1, _, c1) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let (_, r0, _, c0) = crate::csv::cell_at_offset(d, d.selection.anchor)
        .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    Ok(((r0.min(r1), r0.max(r1)), (c0.min(c1), c0.max(c1))))
}

/// The selection's cells as TSV, recorded on the clipboard, with a message
/// of how many.
fn csv_copy_cells(ctx: &mut EditorContext<'_>) -> Result<String, CommandError> {
    let d = ctx.doc()?;
    let ((r0, r1), (c0, c1)) = rectangle_or_cell(d)?;
    let layout = crate::csv::layout(d);
    let tsv =
        crate::csv_tools::rectangle_tsv(d.text().as_str(), &layout.dialect, (r0, r1), (c0, c1));
    ctx.messages.push(crate::tr!(
        "msg-csv-copied-cells",
        rows = r1 - r0 + 1,
        columns = c1 - c0 + 1
    ));
    ctx.clipboard.record(tsv.clone());
    Ok(tsv)
}

/// Fill Down and Fill Series in the column at the cursor: the rows the
/// selection covers from its first one, or without a selection the cell
/// from the one above (a series stepping as the two above do).
fn csv_fill(ctx: &mut EditorContext<'_>, series: bool) -> CommandResult {
    let now = ctx.now;
    let d = ctx.doc()?;
    let (layout, row, _, col) =
        crate::csv::cell_at(d).ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
    let tx = {
        let text = d.text().as_str();
        let dl = &layout.dialect;
        let top = usize::from(dl.header);
        let sel = d.selection;
        let cell = |r: usize| -> Option<String> {
            let rec = layout.index.borrow_mut().record(text, r, dl)?;
            rec.fields
                .get(col)
                .map(|f| crate::csv::value(text, f, dl).into_owned())
        };
        // The columns the selection covers, else the cursor's.
        let cols = if sel.anchor != sel.head {
            crate::csv::cell_at_offset(d, sel.anchor)
                .map_or((col, col), |(_, _, _, c)| (c.min(col), c.max(col)))
        } else {
            (col, col)
        };
        let (first, last) = if sel.anchor != sel.head {
            let mut idx = layout.index.borrow_mut();
            (
                idx.row_at(text, sel.anchor.min(sel.head), dl),
                idx.row_at(text, sel.anchor.max(sel.head), dl),
            )
        } else {
            (row.wrapping_sub(1), row)
        };
        if first >= last || first < top {
            return Err(CommandError::new(crate::tr!("msg-csv-no-row")));
        }
        let step = series.then(|| {
            if sel.anchor != sel.head {
                return 1.0;
            }
            let above2 = (first > top).then(|| cell(first - 1)).flatten();
            let above = cell(first).unwrap_or_default();
            crate::csv_tools::series_step(above2.as_deref(), &above, dl.delimiter == b';')
        });
        crate::csv_tools::fill(text, dl, cols, first, last, step)
            .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-series")))?
    };
    d.apply(&tx, org_edit::ChangeKind::Command, now);
    Ok(())
}

/// Builds the PDF of the LaTeX document in the background: its project's
/// root document, with the engine and output folder the document and the
/// settings ask for (T2.7h.22).
fn latex_build(ctx: &mut EditorContext<'_>) -> CommandResult {
    let save_options = ctx.config.save_options();
    let doc = ctx
        .document
        .as_deref_mut()
        .ok_or_else(|| CommandError::new(crate::tr!("msg-no-document")))?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    // LaTeX compiles the file: the text as it is in the editor, saved.
    if doc.is_modified() {
        doc.save(save_options, false)
            .map_err(|e| CommandError::new(crate::tr!("msg-pdf-failed", error = e.to_string())))?;
    }
    let path = std::path::absolute(&path).unwrap_or(path);
    let text = doc.text().as_str().to_string();
    let disk = latex_model::project::Disk;
    let root = crate::latex_view::find_root(&path, &text);
    let project = latex_model::project::ProjectCache::default().load(&root, &disk);
    let root_text = if root == path {
        text
    } else {
        std::fs::read_to_string(&root).unwrap_or_default()
    };
    let engine = crate::latex_build::engine(
        &root_text,
        &project.model,
        ctx.config.str("latex.engine").trim(),
    );
    let out = ctx.config.str("latex.output_directory").trim().to_string();
    let out_dir = (!out.is_empty()).then(|| std::path::PathBuf::from(&out));
    // One build at a time on the same `.aux` and `.pdf`: one asked for
    // while another runs is built when it ends (publish_todo 3.3).
    if crate::latex_build::running() {
        crate::latex_build::queue((root, engine, out_dir));
        ctx.messages.push(crate::l10n::tr("msg-build-queued"));
        return Ok(());
    }
    let open_after = ctx.config.bool("export.open_after");
    let status = crate::l10n::tr("msg-compiling-pdf");
    ctx.messages.push(status.clone());
    crate::jobs::spawn(status, move || {
        let (mut root, mut engine, mut out_dir) = (root, engine, out_dir);
        let mut result = crate::latex_build::build(&root, engine, out_dir.as_deref());
        while let Some((r, e, o)) = crate::latex_build::take_queued() {
            // The one before shows its problems in the text all the same.
            if let Ok(b) = &result {
                crate::latex_build::record(&root, &b.problems);
            }
            (root, engine, out_dir) = (r, e, o);
            result = crate::latex_build::build(&root, engine, out_dir.as_deref());
        }
        match result {
            Err(e) => crate::jobs::Finished {
                message: crate::tr!("msg-pdf-failed", error = e),
                error: true,
                open: None,
            },
            Ok(b) => {
                use crate::latex_build::Severity;
                // The editor shows them in the text.
                crate::latex_build::record(&root, &b.problems);
                let errors: Vec<_> = b
                    .problems
                    .iter()
                    .filter(|p| p.severity == Severity::Error)
                    .collect();
                let warnings = b
                    .problems
                    .iter()
                    .filter(|p| p.severity == Severity::Warning)
                    .count();
                if let Some(first) = errors.first() {
                    let file = first
                        .file
                        .clone()
                        .unwrap_or_else(|| root.display().to_string());
                    let place = match first.line {
                        Some(n) => format!("{file}:{n}"),
                        None => file,
                    };
                    crate::jobs::Finished {
                        message: crate::tr!(
                            "msg-pdf-error",
                            place = place,
                            error = first.message.clone(),
                            count = errors.len() - 1
                        ),
                        error: true,
                        open: None,
                    }
                } else {
                    // In a repository that does not leave the build
                    // outputs out: the command that does, offered.
                    let hint = crate::latex_build::ignore_missing(&root, out_dir.as_deref())
                        .map_or_else(String::new, |_| {
                            format!(" {}", crate::l10n::tr("msg-latex-ignore-hint"))
                        });
                    match b.pdf {
                        Some(pdf) => crate::jobs::Finished {
                            message: crate::tr!(
                                "msg-latex-built",
                                path = pdf.display().to_string(),
                                count = warnings
                            ) + &hint,
                            error: false,
                            open: open_after.then(|| crate::input::LinkAction::Url(file_url(&pdf))),
                        },
                        None => crate::jobs::Finished {
                            message: crate::tr!(
                                "msg-pdf-failed",
                                error = crate::l10n::tr("msg-build-no-pdf")
                            ),
                            error: true,
                            open: None,
                        },
                    }
                }
            }
        }
    });
    Ok(())
}

/// Runs a LaTeX editing function on the document; `None` from it runs
/// `fallback` instead, or reports that it does not apply.
fn latex_edit_with(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(
        &str,
        org_edit::Selection,
        &latex_syntax::SyntaxNode,
        Option<&str>,
    ) -> Option<org_edit::Transaction>,
    fallback: Option<fn(&mut EditorContext<'_>) -> CommandResult>,
) -> CommandResult {
    let now = ctx.now;
    let d = ctx.doc()?;
    let Some(l) = d.latex() else {
        return Err(CommandError::new(crate::tr!("msg-not-latex")));
    };
    let root = l.parse().syntax();
    let class = l.model().class.as_ref().map(|c| c.name.clone());
    match f(d.text().as_str(), d.selection, &root, class.as_deref()) {
        Some(tx) => {
            d.apply(&tx, org_edit::ChangeKind::Command, now);
            Ok(())
        }
        None => match fallback {
            Some(g) => g(ctx),
            None => Err(CommandError::new(crate::tr!("msg-latex-not-here"))),
        },
    }
}

/// Moves the cursor to the next (or previous) of a LaTeX document's
/// diagnostics, the build's problems among them, and says what it is.
fn goto_problem(ctx: &mut EditorContext<'_>, back: bool) -> CommandResult {
    let d = ctx.doc()?;
    if d.latex().is_none() {
        return Err(CommandError::new(crate::tr!("msg-not-latex")));
    }
    if d.latex_diagnostics().is_none() {
        d.update_latex_diagnostics();
    }
    let diags = d.latex_diagnostics().cloned().unwrap_or_default();
    let at = crate::latex_check::next(&diags, d.selection.head, back)
        .ok_or_else(|| CommandError::new(crate::tr!("msg-no-problems")))?;
    d.move_cursor(at, false);
    if let Some(m) = crate::latex_view::diagnostic_at(d, at) {
        ctx.messages.push(m);
    }
    Ok(())
}

/// The commands of LaTeX documents (T2.7h.15).
fn latex_commands() -> Vec<Command> {
    use crate::command::Scope;
    use crate::latex_edit as e;
    let c = |id: &str, title: &str, keys: &[&str], h: Handler| {
        scoped(
            cmd(id, title, "LaTeX", keys, None, h),
            Scope::only(&["latex"]),
        )
    };
    fn newline(ctx: &mut EditorContext<'_>) -> CommandResult {
        let now = ctx.now;
        let d = ctx.doc()?;
        let s = d.selection;
        let tx = crate::input::newline(
            d.text().as_str(),
            s.head,
            (s.anchor != s.head).then_some(s.anchor),
        );
        d.apply(&tx, org_edit::ChangeKind::Command, now);
        Ok(())
    }
    fn indent(ctx: &mut EditorContext<'_>) -> CommandResult {
        // In math, Tab goes to the next empty argument.
        latex_edit_with(
            ctx,
            |t, s, r, _| {
                crate::latex_edit::next_stop(t, s.head, r)
                    .or_else(|| crate::latex_table::next_cell(t, s.head, r, false))
            },
            Some(|ctx| {
                let now = ctx.now;
                ctx.doc()?.indent(false, now);
                Ok(())
            }),
        )
    }
    fn outdent(ctx: &mut EditorContext<'_>) -> CommandResult {
        // In a table, Shift+Tab goes to the previous cell.
        latex_edit_with(
            ctx,
            |t, s, r, _| crate::latex_table::next_cell(t, s.head, r, true),
            Some(|ctx| {
                let now = ctx.now;
                ctx.doc()?.indent(true, now);
                Ok(())
            }),
        )
    }
    vec![
        c("latex.enter", "New Line or Item", &["enter"], |ctx, _| {
            latex_edit_with(ctx, |t, s, r, _| e::enter(t, s, r), Some(newline))
        }),
        c("latex.link.open", "Open Link", &[], |ctx, _| {
            use crate::input::LinkAction;
            let doc = ctx.doc()?;
            match crate::latex_view::link_at(doc, doc.selection.head) {
                Some(LinkAction::Jump(p)) => {
                    doc.move_cursor(p, false);
                    Ok(())
                }
                Some(LinkAction::Missing(s)) => Err(CommandError::new(crate::tr!(
                    "latex-unknown-label",
                    key = s
                ))),
                Some(other) => request(ctx, Request::OpenLink(other)),
                None => Err(CommandError::new(crate::tr!("msg-no-link"))),
            }
        }),
        c("latex.list.indent", "Nest Item", &["tab"], |ctx, _| {
            // On a section's line: fold it, as Tab does on Org's headlines.
            {
                let d = ctx.doc()?;
                let text = d.text();
                let line = text.line_range(text.line_of(d.selection.head));
                let on_section = d.latex().is_some_and(|l| {
                    l.model().sections.iter().any(|s| {
                        s.file == 0
                            && line.start <= s.range.start
                            && s.range.start <= line.end
                            && text.as_str()[line.start..s.range.start].trim().is_empty()
                    })
                });
                if on_section {
                    return request(ctx, Request::Fold { global: false });
                }
            }
            latex_edit_with(
                ctx,
                |t, s, r, _| e::indent_item(t, s.head, r, true),
                Some(indent),
            )
        }),
        c(
            "latex.list.outdent",
            "Unnest Item",
            &["shift+tab"],
            |ctx, _| {
                latex_edit_with(
                    ctx,
                    |t, s, r, _| e::indent_item(t, s.head, r, false),
                    Some(outdent),
                )
            },
        ),
        c("latex.cancelBuild", "Cancel Build", &[], |ctx, _| {
            if crate::latex_build::cancel() {
                ctx.messages.push(crate::tr!("msg-build-cancelling"));
                Ok(())
            } else {
                Err(CommandError::new(crate::tr!("msg-no-build")))
            }
        }),
        c(
            "latex.nextProblem",
            "Next Problem",
            &["alt+f8"],
            |ctx, args| {
                // `at`: the problem at that offset (the problems list).
                match args.get("at").and_then(Value::as_u64) {
                    Some(at) => {
                        let d = ctx.doc()?;
                        let at = (at as usize).min(d.text().len());
                        d.move_cursor(at, false);
                        if let Some(m) = crate::latex_view::diagnostic_at(d, at) {
                            ctx.messages.push(m);
                        }
                        Ok(())
                    }
                    None => goto_problem(ctx, false),
                }
            },
        ),
        c("latex.problems", "Show Problems", &[], |ctx, _| {
            let d = ctx.doc()?;
            if d.latex().is_none() {
                return Err(CommandError::new(crate::tr!("msg-not-latex")));
            }
            if d.latex_diagnostics().is_none() {
                d.update_latex_diagnostics();
            }
            let diags = d.latex_diagnostics().cloned().unwrap_or_default();
            let text = d.text();
            let items: Vec<crate::palette::PaletteItem> = diags
                .iter()
                .map(|x| {
                    let line = text.line_of(x.range.start.min(text.len())) + 1;
                    let warning = x.severity == crate::latex_check::Severity::Warning;
                    crate::palette::PaletteItem {
                        id: crate::palette::invocation(
                            "latex.nextProblem",
                            &serde_json::json!({ "at": x.range.start }),
                        ),
                        title: format!("{line}: {} {}", if warning { "⚠" } else { "ⓘ" }, x.message),
                        category: x.code.to_string(),
                        keys: String::new(),
                        also: String::new(),
                    }
                })
                .collect();
            if items.is_empty() {
                return Err(CommandError::new(crate::tr!("msg-no-problems")));
            }
            request(ctx, Request::Choose(items))
        }),
        c(
            "latex.previousProblem",
            "Previous Problem",
            &["alt+shift+f8"],
            |ctx, _| goto_problem(ctx, true),
        ),
        c("latex.fix", "Quick Fix", &["ctrl+."], |ctx, _| {
            let d = ctx.doc()?;
            let Some(l) = d.latex() else {
                return Err(CommandError::new(crate::tr!("msg-not-latex")));
            };
            let tx =
                crate::latex_check::quick_fix(d.text().as_str(), d.selection, &l.parse().syntax())
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-no-fix")))?;
            let now = ctx.now;
            ctx.doc()?.apply(&tx, org_edit::ChangeKind::Command, now);
            Ok(())
        }),
        c("latex.format.bold", "Bold", &["ctrl+b"], |ctx, _| {
            latex_edit_with(ctx, |t, s, r, _| e::toggle(t, s, r, "textbf"), None)
        }),
        c("latex.format.italic", "Emphasis", &["ctrl+i"], |ctx, _| {
            latex_edit_with(ctx, |t, s, r, _| e::toggle(t, s, r, "emph"), None)
        }),
        c(
            "latex.format.code",
            "Typewriter",
            &["ctrl+shift+k"],
            |ctx, _| latex_edit_with(ctx, |t, s, r, _| e::toggle(t, s, r, "texttt"), None),
        ),
        c(
            "latex.format.underline",
            "Underline",
            &["ctrl+u"],
            |ctx, _| latex_edit_with(ctx, |t, s, r, _| e::toggle(t, s, r, "underline"), None),
        ),
        c(
            "latex.section.setLevel",
            "Heading Level",
            &[],
            |ctx, args| {
                let level = args.get("level").and_then(Value::as_u64).ok_or_else(|| {
                    CommandError::new(crate::tr!("msg-missing-argument", name = "level"))
                })? as usize;
                latex_edit_with(
                    ctx,
                    |t, s, r, class| e::set_level(t, s.head, r, class, level),
                    None,
                )
            },
        ),
        c(
            "latex.section.promote",
            "Promote Section",
            &["alt+shift+left"],
            |ctx, _| latex_edit_with(ctx, |t, s, r, _| e::promote(t, s.head, r, true), None),
        ),
        c(
            "latex.section.demote",
            "Demote Section",
            &["alt+shift+right"],
            |ctx, _| latex_edit_with(ctx, |t, s, r, _| e::promote(t, s.head, r, false), None),
        ),
        c(
            "latex.section.moveUp",
            "Move Section Up",
            &["alt+shift+up"],
            |ctx, _| latex_edit_with(ctx, |t, s, r, _| e::move_section(t, s.head, r, false), None),
        ),
        c(
            "latex.section.moveDown",
            "Move Section Down",
            &["alt+shift+down"],
            |ctx, _| latex_edit_with(ctx, |t, s, r, _| e::move_section(t, s.head, r, true), None),
        ),
        c("latex.math.toggleDisplay", "Display Math", &[], |ctx, _| {
            latex_edit_with(ctx, |t, s, r, _| e::toggle_display(t, s.head, r), None)
        }),
        c(
            "latex.math.toggleNumbering",
            "Number Equation",
            &[],
            |ctx, _| latex_edit_with(ctx, |_, s, r, _| e::toggle_numbering(s.head, r), None),
        ),
        c("latex.insert.figure", "Insert Figure", &[], |ctx, args| {
            // Without a picture: a dialog asking for it, its width and its
            // caption, one after the other.
            if args.get("path").is_none() {
                return request(
                    ctx,
                    Request::PickFile {
                        command: "latex.insert.figure".into(),
                        arg: "path".into(),
                        args: serde_json::json!({ "ask": true }),
                    },
                );
            }
            let asking = args.get("ask").and_then(Value::as_bool) == Some(true);
            for step in ["width", "caption"] {
                if asking && args.get(step).is_none() {
                    return request(
                        ctx,
                        Request::Ask {
                            command: "latex.insert.figure".into(),
                            args: args.clone(),
                            arg: step.into(),
                        },
                    );
                }
            }
            let caption = args
                .get("caption")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let path = args
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            // A share of the line (`0.5`), or a length as written.
            let width = match args.get("width") {
                Some(Value::Number(n)) => format!("{}\\linewidth", n),
                Some(Value::String(w)) if w.trim().parse::<f64>().is_ok() => {
                    format!("{}\\linewidth", w.trim())
                }
                Some(Value::String(w)) if !w.trim().is_empty() => w.trim().to_string(),
                _ => "0.8\\linewidth".to_string(),
            };
            latex_insert(ctx, |text, indent, _| {
                let stem = std::path::Path::new(&path)
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                // As the document writes its floats (T2.7h.15).
                let style = crate::latex_edit::Style::infer(text);
                let inner = format!("{indent}{}", style.step);
                let label = format!(
                    "\\label{{{}}}",
                    crate::latex_edit::unique_label(text, &format!("{}{stem}", style.prefixes[0]))
                );
                let (head, tail) = float_caption(&inner, &caption, &label, style.label);
                let head = format!(
                    "{indent}\\begin{{figure}}[htbp]\n{inner}\\centering\n{inner}\\includegraphics[width={width}]{{{path}}}\n{head}"
                );
                let tail = format!("{tail}{indent}\\end{{figure}}\n");
                // The cursor in the caption, or after the figure once it has one.
                let at = if caption.is_empty() {
                    head.len()
                } else {
                    head.len() + tail.len()
                };
                (format!("{head}{tail}"), at)
            })
        }),
        c("latex.insert.table", "Insert Table", &[], |ctx, args| {
            let columns = args
                .get("columns")
                .and_then(Value::as_u64)
                .unwrap_or(3)
                .clamp(1, 26) as usize;
            let rows = args
                .get("rows")
                .and_then(Value::as_u64)
                .unwrap_or(2)
                .clamp(1, 200) as usize;
            latex_insert(ctx, move |text, indent, model| {
                // booktabs' rules when the document loads it.
                let booktabs = model.packages.iter().any(|p| p.name == "booktabs");
                let (top, mid, bottom) = if booktabs {
                    ("\\toprule", "\\midrule", "\\bottomrule")
                } else {
                    ("\\hline", "\\hline", "\\hline")
                };
                // As the document writes its floats (T2.7h.15).
                let style = crate::latex_edit::Style::infer(text);
                let inner = format!("{indent}{}", style.step);
                let cells = format!("{inner}{}", style.step);
                let row = format!("{cells}{} \\\\\n", vec![""; columns].join(" & "));
                let label = format!(
                    "\\label{{{}}}",
                    crate::latex_edit::unique_label(text, &style.prefixes[1])
                );
                let (caption_head, caption_tail) = float_caption(&inner, "", &label, style.label);
                let head =
                    format!("{indent}\\begin{{table}}[htbp]\n{inner}\\centering\n{caption_head}");
                let mut t = head.clone();
                t.push_str(&caption_tail);
                t.push_str(&format!(
                    "{inner}\\begin{{tabular}}{{{}}}\n{cells}{top}\n",
                    "l".repeat(columns)
                ));
                t.push_str(&row);
                t.push_str(&format!("{cells}{mid}\n"));
                for _ in 1..rows {
                    t.push_str(&row);
                }
                t.push_str(&format!(
                    "{cells}{bottom}\n{inner}\\end{{tabular}}\n{indent}\\end{{table}}\n"
                ));
                (t, head.len())
            })
        }),
        c("latex.insert.equation", "Insert Equation", &[], |ctx, _| {
            latex_insert(ctx, |text, indent, _| {
                // As the document writes its equations (T2.7h.15).
                let style = crate::latex_edit::Style::infer(text);
                let inner = format!("{indent}{}", style.step);
                let label = format!(
                    "\\label{{{}}}",
                    crate::latex_edit::unique_label(text, &style.prefixes[2])
                );
                // No line of the body left empty: in a formula a blank
                // line is a paragraph's end, an error. The formula goes
                // before the label, or after it on the `\begin` line.
                if style.equation_label_inline {
                    let head = format!("{indent}\\begin{{equation}}{label} ");
                    (format!("{head}\n{indent}\\end{{equation}}\n"), head.len())
                } else {
                    let head = format!("{indent}\\begin{{equation}}\n{inner}");
                    (
                        format!("{head} {label}\n{indent}\\end{{equation}}\n"),
                        head.len(),
                    )
                }
            })
        }),
        c(
            "latex.insert.citation",
            "Insert Citation",
            &[],
            latex_insert_citation,
        ),
        c(
            "latex.export.html",
            "Export as HTML (pandoc)",
            &[],
            |ctx, _| latex_pandoc(ctx, "html5", "html"),
        ),
        c(
            "latex.export.markdown",
            "Export as Markdown (pandoc)",
            &[],
            |ctx, _| latex_pandoc(ctx, "markdown", "md"),
        ),
        c(
            "latex.export.docx",
            "Export as Word (pandoc)",
            &[],
            |ctx, _| latex_pandoc(ctx, "docx", "docx"),
        ),
        c(
            "latex.convertToOrg",
            "Convert to Org (pandoc)",
            &[],
            |ctx, _| {
                let d = ctx.doc()?;
                let path =
                    d.meta.path.clone().ok_or_else(|| {
                        CommandError::new(crate::l10n::tr("msg-export-needs-file"))
                    })?;
                import_file(
                    ctx,
                    &serde_json::json!({ "file": path.display().to_string() }),
                )?;
                // One way: the LaTeX file stays as it is.
                ctx.messages
                    .push(crate::l10n::tr("msg-latex-converted-one-way"));
                Ok(())
            },
        ),
    ]
}

/// Writes the LaTeX document (its project's root) as `to` through pandoc
/// beside it, in the background.
fn latex_pandoc(ctx: &mut EditorContext<'_>, to: &'static str, ext: &'static str) -> CommandResult {
    let d = ctx.doc()?;
    let path = d
        .meta
        .path
        .clone()
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-export-needs-file")))?;
    let path = std::path::absolute(&path).unwrap_or(path);
    let text = d.text().as_str().to_string();
    let root = crate::latex_view::find_root(&path, &text);
    let search = std::env::var_os("PATH").unwrap_or_default();
    let pandoc = crate::pandoc::find_with(ctx.config.str("export.pandoc_path"), &search)
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-pandoc")))?;
    let out = root.with_extension(ext);
    let open_after = ctx.config.bool("export.open_after");
    let status = crate::l10n::tr("msg-converting");
    ctx.messages.push(status.clone());
    crate::jobs::spawn(status, move || {
        match crate::pandoc::export_latex(&pandoc, &root, to, &out) {
            Ok(()) => crate::jobs::Finished {
                message: crate::tr!("msg-exported", path = out.display().to_string()),
                error: false,
                open: open_after.then(|| crate::input::LinkAction::Url(file_url(&out))),
            },
            Err(e) => crate::jobs::Finished {
                message: crate::tr!("msg-pandoc-failed", error = e),
                error: true,
                open: None,
            },
        }
    });
    Ok(())
}

/// Inserts a citation of `key` (in a `\cite{…}` at the cursor, one key
/// more); without a key, the entries of the bibliography to choose from.
fn latex_insert_citation(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    let key = args.get("key").and_then(Value::as_str).map(str::to_string);
    let now = ctx.now;
    let d = ctx.doc()?;
    let Some(l) = d.latex() else {
        return Err(CommandError::new(crate::tr!("msg-not-latex")));
    };
    let Some(key) = key else {
        let files = l.bibliography_files(d.meta.path.as_deref());
        let bib = crate::cite::load(&files);
        let items: Vec<crate::palette::PaletteItem> = bib
            .entries()
            .iter()
            .map(|e| crate::palette::PaletteItem {
                id: crate::palette::invocation(
                    "latex.insert.citation",
                    &serde_json::json!({ "key": e.key }),
                ),
                title: format!("{}  {}", e.key, crate::cite::describe(e)),
                category: e.kind.to_lowercase(),
                keys: String::new(),
                also: String::new(),
            })
            .collect();
        if items.is_empty() {
            return Err(CommandError::new(crate::tr!("msg-latex-no-bibliography")));
        }
        return request(ctx, Request::Choose(items));
    };
    let pos = d.selection.head;
    let text = d.text().as_str();
    let root = l.parse().syntax();
    let inside = (|| {
        let t = latex_syntax::token_before(&root, pos)?;
        let cmd = t.parent_ancestors().find(|a| {
            a.kind() == latex_syntax::SyntaxKind::COMMAND
                && latex_syntax::name(a)
                    .is_some_and(|n| latex_syntax::signatures::command(&n) == "*oom")
        })?;
        let g = cmd
            .children()
            .find(|c| c.kind() == latex_syntax::SyntaxKind::GROUP)?;
        let end = usize::from(g.text_range().end());
        text[..end].ends_with('}').then_some(end - 1)
    })();
    let (at, insert) = match inside {
        Some(end) => (end, format!(",{key}")),
        // Only in text: not in a name, a label or a formula.
        None if !crate::latex_edit::in_text(&root, pos) => {
            return Err(CommandError::new(crate::tr!("msg-latex-not-here")));
        }
        None => (pos, format!("\\cite{{{key}}}")),
    };
    let mut tx = org_edit::Transaction::new("Insert Citation");
    tx.replace(at..at, insert.clone())
        .map_err(|e| CommandError::new(e.to_string()))?;
    let tx = tx.select(org_edit::Selection::caret(
        at + insert.len() + usize::from(inside.is_some()),
    ));
    d.apply(&tx, org_edit::ChangeKind::Command, now);
    Ok(())
}

/// Inserts what `make` gives (text and the cursor's place in it) on the
/// cursor's line when it is empty, else on a line of its own after it,
/// with the line's indentation.
/// The bookmarks to choose from, each running `command` with its name.
fn bookmark_choice(ctx: &mut EditorContext<'_>, command: &str) -> CommandResult {
    let marks = crate::bookmarks::load();
    if marks.is_empty() {
        return Err(CommandError::new(crate::tr!("msg-no-bookmarks")));
    }
    let items = marks
        .iter()
        .map(|b| crate::palette::PaletteItem {
            id: crate::palette::invocation(command, &serde_json::json!({ "name": b.name })),
            title: b.name.clone(),
            category: crate::tr!("category-bookmarks"),
            keys: String::new(),
            also: format!("{} {}", b.path.display(), b.context.trim()),
        })
        .collect();
    request(ctx, Request::Choose(items))
}

/// A float's caption and label lines, as `place` puts the label: the
/// text up to the caption's text (where the cursor goes) and the rest.
fn float_caption(
    inner: &str,
    caption: &str,
    label: &str,
    place: crate::latex_edit::LabelPlace,
) -> (String, String) {
    use crate::latex_edit::LabelPlace as P;
    match place {
        P::InCaption => (
            format!("{inner}\\caption{{{caption}"),
            format!("{label}}}\n"),
        ),
        P::BeforeCaption => (
            format!("{inner}{label}\n{inner}\\caption{{{caption}"),
            "}\n".to_string(),
        ),
        P::AfterCaption => (
            format!("{inner}\\caption{{{caption}"),
            format!("}}\n{inner}{label}\n"),
        ),
    }
}

fn latex_insert(
    ctx: &mut EditorContext<'_>,
    make: impl FnOnce(&str, &str, &latex_model::Model) -> (String, usize),
) -> CommandResult {
    let now = ctx.now;
    let d = ctx.doc()?;
    let Some(l) = d.latex() else {
        return Err(CommandError::new(crate::tr!("msg-not-latex")));
    };
    let model = l.model();
    let text = d.text().as_str();
    // After a float, table, formula or command the cursor is in, and
    // after one the end of its line is in (a command over lines).
    let root = l.parse().syntax();
    let mut pos = crate::latex_edit::block_position(text, &root, d.selection.head);
    loop {
        let end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
        let after = crate::latex_edit::block_position(text, &root, end);
        if after <= end {
            break;
        }
        pos = after;
    }
    let line_start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    let line = &text[line_start..line_end];
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let (block, cursor) = make(text, &indent, &model);
    // On a list's `\begin` line: before the list (in it, before its first
    // `\item`, nothing can go).
    let opens_list = ["itemize", "enumerate", "description", "thebibliography"]
        .iter()
        .any(|l| line.trim_start().starts_with(&format!("\\begin{{{l}}}")));
    let (replace, lead) = if line.trim().is_empty() {
        (line_start..(line_end + 1).min(text.len()), String::new())
    } else if opens_list {
        (line_start..line_start, String::new())
    } else if line_end == text.len() {
        (line_end..line_end, "\n".to_string())
    } else {
        (line_end + 1..line_end + 1, String::new())
    };
    let insert = format!("{lead}{block}");
    let mut tx = org_edit::Transaction::new("Insert");
    tx.replace(replace.clone(), insert)
        .map_err(|e| CommandError::new(e.to_string()))?;
    let tx = tx.select(org_edit::Selection::caret(
        replace.start + lead.len() + cursor,
    ));
    d.apply(&tx, org_edit::ChangeKind::Command, now);
    Ok(())
}

/// The commands of CSV documents (§2.6.2).
/// The commands of the BibTeX grid (T2.7h.19).
fn bib_commands() -> Vec<Command> {
    use crate::command::Scope;
    let c = |id: &str, title: &str, keys: &[&str], h: Handler| {
        scoped(
            cmd(id, title, "BibTeX", keys, None, h),
            Scope::only(&["bib"]),
        )
    };
    fn bib(ctx: &mut EditorContext<'_>) -> Result<(), CommandError> {
        if crate::bibtex::is_bib(ctx.doc()?) {
            Ok(())
        } else {
            Err(CommandError::new(crate::tr!("msg-not-bib")))
        }
    }
    vec![
        c(
            "bib.sortView",
            "Sort Entries by Column",
            &[],
            |ctx, args| {
                // The rows in the order of the column named, or of the field at
                // the cursor (again: descending); the file keeps its order.
                bib(ctx)?;
                let reverse = arg_bool(args, "reverse");
                let d = ctx.doc()?;
                let col = crate::bibtex::column(d, args.get("column").and_then(Value::as_str));
                let reverse = reverse || d.bib_sort == Some((col, false));
                d.bib_sort = Some((col, reverse));
                Ok(())
            },
        ),
        c("bib.unsortView", "File Order", &[], |ctx, _| {
            bib(ctx)?;
            ctx.doc()?.bib_sort = None;
            Ok(())
        }),
        c("bib.setField", "Set Field", &[], |ctx, args| {
            bib(ctx)?;
            let field = args
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let value = args.get("value").and_then(Value::as_str).unwrap_or("");
            if field.is_empty()
                || !field
                    .chars()
                    .all(|c| c.is_alphanumeric() || "-_:.".contains(c))
            {
                return Err(CommandError::new(crate::tr!("msg-bib-field-name")));
            }
            let now = ctx.now;
            let d = ctx.doc()?;
            let e = crate::bibtex::entry_at_cursor(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-bib-no-entry")))?;
            let tx = crate::bibtex::set_field(d.text().as_str(), &e, &field, value);
            d.apply(&tx, org_edit::ChangeKind::Command, now);
            Ok(())
        }),
        c("bib.newEntry", "New Entry", &[], |ctx, args| {
            bib(ctx)?;
            let kind = args
                .get("type")
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .unwrap_or("article")
                .trim()
                .to_string();
            let key = args
                .get("key")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if key.is_empty() || key.contains([',', '{', '}', ' ']) {
                return Err(CommandError::new(crate::tr!("msg-bib-key")));
            }
            let now = ctx.now;
            let d = ctx.doc()?;
            let text = d.text().as_str();
            let g = crate::bibtex::grid(d);
            if g.entries.iter().any(|e| text[e.key.clone()] == key) {
                return Err(CommandError::new(crate::tr!(
                    "msg-bib-key-taken",
                    key = key
                )));
            }
            // After the entry at the cursor, else at the end.
            let pos = d.selection.head;
            let at = g
                .entries
                .iter()
                .find(|e| e.range.start <= pos && pos <= e.range.end)
                .map_or(text.len(), |e| e.range.end);
            let lead = if at == 0 || text[..at].ends_with("\n\n") {
                ""
            } else if text[..at].ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            let fields = ["author", "title", "year"];
            let body: String = fields.iter().map(|f| format!("  {f} = {{}},\n")).collect();
            let entry = format!("{lead}@{kind}{{{key},\n{body}}}\n");
            // The cursor in the first field's braces.
            let cursor = at + lead.len() + format!("@{kind}{{{key},\n  author = {{").len();
            let mut tx = org_edit::Transaction::new("New Entry");
            let _ = tx.insert(at, entry);
            tx.selection_after = Some(org_edit::Selection::caret(cursor));
            d.apply(&tx, org_edit::ChangeKind::Command, now);
            Ok(())
        }),
    ]
}

fn markdown_commands() -> Vec<Command> {
    use crate::command::Scope;
    vec![
        scoped(
            cmd(
                "markdown.toggleCheckbox",
                "Toggle Checkbox",
                "Markdown",
                &["ctrl+shift+c"],
                None,
                |ctx, _| {
                    // The task list item at the cursor: `[ ]` ⇄ `[x]`.
                    let d = ctx.doc()?;
                    let md = crate::markdown::parsed(d);
                    let at = d.selection.head;
                    lines_command(ctx, |t, _| crate::markdown::toggle_checkbox(&md, t, at))
                },
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.emphasis.bold",
                "Bold",
                "Markdown",
                &["ctrl+b"],
                Some("editorMode == markdown"),
                |ctx, _| lines_command(ctx, |t, s| Some(crate::markdown::wrap(t, s, "**", "**"))),
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.emphasis.italic",
                "Italic",
                "Markdown",
                &["ctrl+i"],
                Some("editorMode == markdown"),
                |ctx, _| lines_command(ctx, |t, s| Some(crate::markdown::wrap(t, s, "*", "*"))),
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.emphasis.code",
                "Code",
                "Markdown",
                &[],
                Some("editorMode == markdown"),
                |ctx, _| lines_command(ctx, |t, s| Some(crate::markdown::wrap(t, s, "`", "`"))),
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.emphasis.strikeThrough",
                "Strike-through",
                "Markdown",
                &[],
                Some("editorMode == markdown"),
                |ctx, _| lines_command(ctx, |t, s| Some(crate::markdown::wrap(t, s, "~~", "~~"))),
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.insert.link",
                "Insert Link",
                "Markdown",
                &["ctrl+k"],
                Some("editorMode == markdown"),
                |ctx, args| {
                    let bare = arg_bool(args, "bare");
                    lines_command(ctx, |t, s| Some(crate::markdown::insert_link(t, s, bare)))
                },
            ),
            Scope::only(&["markdown"]),
        ),
        md_table(
            "markdown.table.nextField",
            "Next Field",
            &["tab"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::next_field(md, t, at, true)
                })
            },
        ),
        md_table(
            "markdown.table.previousField",
            "Previous Field",
            &["shift+tab"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::next_field(md, t, at, false)
                })
            },
        ),
        md_table(
            "markdown.table.recalculate",
            "Recalculate Table",
            &["f9"],
            |ctx, _| {
                let now = ctx.now;
                let d = ctx.doc()?;
                let md = crate::markdown::parsed(d);
                let at = d.selection.head;
                match crate::markdown_table::recalculate_at(&md, d.text().as_str(), at) {
                    Ok(Some(tx)) => {
                        d.apply(&tx, org_edit::ChangeKind::Command, now);
                        Ok(())
                    }
                    Ok(None) => Err(CommandError::new(crate::tr!("msg-no-formulas"))),
                    Err(e) => Err(CommandError::new(e)),
                }
            },
        ),
        md_table("markdown.table.align", "Align Table", &[], |ctx, _| {
            md_table_run(ctx, crate::markdown_table::align_at)
        }),
        md_table(
            "markdown.table.moveRowUp",
            "Move Row Up",
            &["alt+up"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::MoveRow(true),
                    )
                })
            },
        ),
        md_table(
            "markdown.table.moveRowDown",
            "Move Row Down",
            &["alt+down"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::MoveRow(false),
                    )
                })
            },
        ),
        md_table(
            "markdown.table.moveColumnLeft",
            "Move Column Left",
            &["alt+left"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::MoveColumn(true),
                    )
                })
            },
        ),
        md_table(
            "markdown.table.moveColumnRight",
            "Move Column Right",
            &["alt+right"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::MoveColumn(false),
                    )
                })
            },
        ),
        md_table(
            "markdown.table.insertRow",
            "Insert Row",
            &["alt+shift+down"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::InsertRow,
                    )
                })
            },
        ),
        md_table(
            "markdown.table.deleteRow",
            "Delete Row",
            &["alt+shift+up"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::DeleteRow,
                    )
                })
            },
        ),
        md_table(
            "markdown.table.insertColumn",
            "Insert Column",
            &["alt+shift+right"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::InsertColumn,
                    )
                })
            },
        ),
        md_table(
            "markdown.table.deleteColumn",
            "Delete Column",
            &["alt+shift+left"],
            |ctx, _| {
                md_table_run(ctx, |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::DeleteColumn,
                    )
                })
            },
        ),
        md_table(
            "markdown.table.sort",
            "Sort Rows by Column",
            &[],
            |ctx, args| {
                let reverse = arg_bool(args, "reverse");
                md_table_run(ctx, move |md, t, at| {
                    crate::markdown_table::edit_at(
                        md,
                        t,
                        at,
                        crate::markdown_table::TableEdit::Sort(reverse),
                    )
                })
            },
        ),
        // Markdown to Org beside the file, without pandoc (T2.7c.7).
        scoped(
            cmd(
                "markdown.convertToOrg",
                "Convert to Org",
                "Markdown",
                &[],
                None,
                |ctx, _| {
                    let d = ctx.doc()?;
                    let org = crate::markdown_org::to_org(d.text().as_str());
                    let path = d
                        .meta
                        .path
                        .clone()
                        .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-save-first")))?;
                    let target = path.with_extension("org");
                    if target.exists() {
                        return Err(CommandError::new(crate::tr!(
                            "msg-import-exists",
                            path = target.display().to_string()
                        )));
                    }
                    std::fs::write(&target, org).map_err(|e| CommandError::new(e.to_string()))?;
                    ctx.messages.push(crate::tr!(
                        "msg-csv-converted",
                        path = target.display().to_string()
                    ));
                    request(
                        ctx,
                        Request::Open {
                            path: Some(target.display().to_string()),
                        },
                    )
                },
            ),
            Scope::only(&["markdown"]),
        ),
        // The link at the cursor: a web address, a file, a wiki page.
        scoped(
            cmd(
                "markdown.openLink",
                "Open Link",
                "Markdown",
                &[],
                None,
                |ctx, _| {
                    let d = ctx.doc()?;
                    let md = crate::markdown::parsed(d);
                    let action = crate::markdown::link_at(
                        &md,
                        d.text().as_str(),
                        d.selection.head,
                        d.meta.path.as_deref(),
                    )
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-no-link")))?;
                    request(ctx, Request::OpenLink(action))
                },
            ),
            Scope::only(&["markdown"]),
        ),
        // A picture: chosen, copied into `images/` when it is elsewhere,
        // and linked.
        scoped(
            cmd(
                "markdown.insert.image",
                "Insert Image",
                "Markdown",
                &[],
                Some("editorMode == markdown"),
                |ctx, args| {
                    let Some(path) = args.get("path").and_then(Value::as_str) else {
                        return request(
                            ctx,
                            Request::PickFile {
                                command: "markdown.insert.image".into(),
                                arg: "path".into(),
                                args: serde_json::json!({}),
                            },
                        );
                    };
                    let now = ctx.now;
                    let d = ctx.doc()?;
                    let doc =
                        d.meta.path.clone().ok_or_else(|| {
                            CommandError::new(crate::tr!("msg-picture-needs-file"))
                        })?;
                    let file = std::path::PathBuf::from(crate::settings::expand_home(path));
                    let file = if file.is_relative() {
                        doc.parent().unwrap_or(std::path::Path::new("")).join(file)
                    } else {
                        file
                    };
                    d.drop_pictures(&[file], now).map_err(CommandError::new)
                },
            ),
            Scope::only(&["markdown"]),
        ),
        // The front matter as a form (T2.7c.9).
        scoped(
            cmd(
                "markdown.frontMatter.edit",
                "Edit Properties",
                "Markdown",
                &[],
                Some("editorMode == markdown"),
                |ctx, _| {
                    let d = ctx.doc()?;
                    let fields = crate::front_matter::read(d.text().as_str())
                        .map(|f| f.fields)
                        .unwrap_or_default();
                    let category = crate::tr!("category-properties");
                    let mut items: Vec<crate::palette::PaletteItem> = fields
                        .iter()
                        .map(|f| crate::palette::PaletteItem {
                            id: crate::palette::invocation(
                                "markdown.frontMatter.set",
                                &serde_json::json!({ "key": f.key, "value_default": f.value }),
                            ),
                            title: format!("{}: {}", f.key, f.value),
                            category: category.clone(),
                            keys: String::new(),
                            also: String::new(),
                        })
                        .collect();
                    items.push(crate::palette::PaletteItem {
                        id: "markdown.frontMatter.set".into(),
                        title: crate::tr!("front-matter-add"),
                        category: category.clone(),
                        keys: String::new(),
                        also: String::new(),
                    });
                    items.extend(fields.iter().map(|f| crate::palette::PaletteItem {
                        id: crate::palette::invocation(
                            "markdown.frontMatter.delete",
                            &serde_json::json!({ "key": f.key }),
                        ),
                        title: crate::tr!("front-matter-delete", key = f.key.as_str()),
                        category: category.clone(),
                        keys: String::new(),
                        also: String::new(),
                    }));
                    request(ctx, Request::Choose(items))
                },
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.frontMatter.set",
                "Set Property",
                "Markdown",
                &[],
                Some("editorMode == markdown"),
                |ctx, args| {
                    let key = arg_str(args, "key")?.to_string();
                    let value = arg_str(args, "value")?.to_string();
                    lines_command(ctx, |t, _| crate::front_matter::set(t, &key, &value))
                },
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.frontMatter.delete",
                "Delete Property",
                "Markdown",
                &[],
                Some("editorMode == markdown"),
                |ctx, args| {
                    let key = arg_str(args, "key")?.to_string();
                    lines_command(ctx, |t, _| crate::front_matter::delete(t, &key))
                },
            ),
            Scope::only(&["markdown"]),
        ),
        // List items moved and lists renumbered (T2.7c.5).
        scoped(
            cmd(
                "markdown.list.moveUp",
                "Move Item Up",
                "Markdown",
                &["alt+up"],
                Some("editorMode == markdown && inMarkdownItem && !inMarkdownTable"),
                |ctx, _| md_table_run(ctx, |md, t, at| crate::markdown::move_item(md, t, at, true)),
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.list.moveDown",
                "Move Item Down",
                "Markdown",
                &["alt+down"],
                Some("editorMode == markdown && inMarkdownItem && !inMarkdownTable"),
                |ctx, _| {
                    md_table_run(ctx, |md, t, at| {
                        crate::markdown::move_item(md, t, at, false)
                    })
                },
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.list.renumber",
                "Renumber List",
                "Markdown",
                &[],
                Some("editorMode == markdown"),
                |ctx, _| md_table_run(ctx, |_, t, at| crate::markdown::renumber_list(t, at)),
            ),
            Scope::only(&["markdown"]),
        ),
        // Tab and Shift+Tab nest an item under the one before it and take
        // it out again, as in Org and LaTeX lists.
        scoped(
            cmd(
                "markdown.list.indent",
                "Nest Item",
                "Markdown",
                &["tab"],
                Some("editorMode == markdown && inMarkdownItem && !inMarkdownTable"),
                |ctx, _| {
                    md_table_run(ctx, |md, t, at| {
                        crate::markdown::indent_item(md, t, at, true)
                    })
                },
            ),
            Scope::only(&["markdown"]),
        ),
        scoped(
            cmd(
                "markdown.list.outdent",
                "Unnest Item",
                "Markdown",
                &["shift+tab"],
                Some("editorMode == markdown && inMarkdownItem && !inMarkdownTable"),
                |ctx, _| {
                    md_table_run(ctx, |md, t, at| {
                        crate::markdown::indent_item(md, t, at, false)
                    })
                },
            ),
            Scope::only(&["markdown"]),
        ),
        // Enter in a list item or a quote continues it (T2.7c.5).
        scoped(
            cmd(
                "markdown.newline",
                "New Item",
                "Markdown",
                &[],
                Some("editorMode == markdown && inMarkdownList"),
                |ctx, _| md_table_run(ctx, crate::markdown::newline),
            ),
            Scope::only(&["markdown"]),
        ),
    ]
}

/// A command on the Markdown table at the cursor, its keys bound there.
fn md_table(id: &str, title: &str, keys: &[&str], h: Handler) -> Command {
    scoped(
        cmd(
            id,
            title,
            "Markdown",
            keys,
            Some("editorMode == markdown && inMarkdownTable"),
            h,
        ),
        crate::command::Scope::only(&["markdown"]),
    )
}

fn md_table_run(
    ctx: &mut EditorContext<'_>,
    f: impl Fn(&crate::markdown::Md, &str, usize) -> Option<org_edit::Transaction>,
) -> CommandResult {
    let now = ctx.now;
    let d = ctx.doc()?;
    let md = crate::markdown::parsed(d);
    let at = d.selection.head;
    let Some(tx) = f(&md, d.text().as_str(), at) else {
        return Ok(());
    };
    if tx.edits.is_empty() {
        if let Some(s) = tx.selection_after {
            d.selection = s;
        }
    } else {
        d.apply(&tx, org_edit::ChangeKind::Command, now);
    }
    Ok(())
}

fn csv_commands() -> Vec<Command> {
    use crate::command::Scope;
    let csv = || Scope::only(&["csv"]);
    let c = |id: &str, title: &str, keys: &[&str], h: Handler| {
        scoped(cmd(id, title, "CSV", keys, None, h), csv())
    };
    vec![
        c("csv.nextField", "Next Field", &["tab"], |ctx, _| {
            csv_edit(ctx, |text, l, row, rec, col| {
                if col + 1 < rec.fields.len().max(l.widths.len()) {
                    return Ok((None, Some((row, col + 1))));
                }
                let n = l.index.borrow_mut().count(text, &l.dialect);
                if row + 1 < n {
                    return Ok((None, Some((row + 1, 0))));
                }
                // Past the last field: a new row, as Tab in an Org table.
                let columns = l.widths.len().max(rec.fields.len());
                Ok((
                    Some(crate::csv::insert_row(text, rec, columns, &l.dialect)),
                    Some((row + 1, 0)),
                ))
            })
        }),
        // Excel's keys in the grid (bound with `!sourceView` in the
        // keymap): in Ready mode the arrows go from cell to cell, Ctrl
        // with them to the data's edge and Shift extends the selection; in
        // Edit mode (F2) Left, Right, Home and End move in the cell; in
        // Enter mode (typing into a cell) the arrows end it and move.
        c("csv.cellLeft", "Cell Left", &[], |ctx, _| {
            csv_step(ctx, 0, -1, false)
        }),
        c("csv.cellRight", "Cell Right", &[], |ctx, _| {
            csv_step(ctx, 0, 1, false)
        }),
        c("csv.extendLeft", "Extend Selection Left", &[], |ctx, _| {
            csv_step(ctx, 0, -1, true)
        }),
        c(
            "csv.extendRight",
            "Extend Selection Right",
            &[],
            |ctx, _| csv_step(ctx, 0, 1, true),
        ),
        c("csv.extendUp", "Extend Selection Up", &[], |ctx, _| {
            csv_step(ctx, -1, 0, true)
        }),
        c("csv.extendDown", "Extend Selection Down", &[], |ctx, _| {
            csv_step(ctx, 1, 0, true)
        }),
        c("csv.edgeLeft", "Data Edge Left", &[], |ctx, _| {
            csv_edge(ctx, 0, -1)
        }),
        c("csv.edgeRight", "Data Edge Right", &[], |ctx, _| {
            csv_edge(ctx, 0, 1)
        }),
        c("csv.edgeUp", "Data Edge Up", &[], |ctx, _| {
            csv_edge(ctx, -1, 0)
        }),
        c("csv.edgeDown", "Data Edge Down", &[], |ctx, _| {
            csv_edge(ctx, 1, 0)
        }),
        c("csv.rowStart", "Row Start", &[], |ctx, _| {
            let d = ctx.doc()?;
            if csv_in_cell(d, |stops, _| stops.first().copied()) {
                return Ok(());
            }
            let (_, row, _, _) = csv_cell(d)?;
            d.csv_edit = None;
            d.go_to_csv_cell(row, 0);
            Ok(())
        }),
        c("csv.rowEnd", "Row End", &[], |ctx, _| {
            let d = ctx.doc()?;
            if csv_in_cell(d, |stops, _| stops.last().copied()) {
                return Ok(());
            }
            let (_, row, rec, _) = csv_cell(d)?;
            d.csv_edit = None;
            d.go_to_csv_cell(row, rec.fields.len().saturating_sub(1));
            Ok(())
        }),
        c("csv.firstCell", "First Cell", &[], |ctx, _| {
            let d = ctx.doc()?;
            d.csv_edit = None;
            d.go_to_csv_cell(0, 0);
            Ok(())
        }),
        c("csv.lastCell", "Last Cell", &[], |ctx, _| {
            // The last row's cell in the last column, as Excel's Ctrl+End.
            let d = ctx.doc()?;
            let (layout, _, _, _) = csv_cell(d)?;
            let text = d.text().as_str();
            let n = layout.index.borrow_mut().count(text, &layout.dialect);
            let col = layout.widths.len().saturating_sub(1);
            d.csv_edit = None;
            d.go_to_csv_cell(n.saturating_sub(1), col);
            Ok(())
        }),
        c("csv.editCell", "Edit Cell", &[], |ctx, args| {
            // F2: Edit mode, the cursor at the value's end (where it is,
            // from a double click, with `here`); in Enter mode, Edit mode.
            let d = ctx.doc()?;
            let here = args.get("here").and_then(Value::as_bool).unwrap_or(false);
            let (layout, _, rec, col) = csv_cell(d)?;
            if d.csv_editing().is_some() {
                if let Some(e) = d.csv_edit.as_mut() {
                    e.edit = true;
                }
                return Ok(());
            }
            if let Some(f) = rec.fields.get(col) {
                let stops = crate::csv::value_stops(d.text().as_str(), f, &layout.dialect);
                let at = d.selection.head;
                let to = if here && stops.contains(&at) {
                    at
                } else if here {
                    stops
                        .iter()
                        .copied()
                        .min_by_key(|s| s.abs_diff(at))
                        .unwrap_or(at)
                } else {
                    stops.last().copied().unwrap_or(at)
                };
                d.selection = org_edit::Selection::caret(to);
            }
            d.begin_csv_edit(true);
            Ok(())
        }),
        c("csv.cancelEdit", "Cancel Entry", &[], |ctx, _| {
            // Escape: the cell as it was; else the selection collapsed.
            let now = ctx.now;
            let d = ctx.doc()?;
            if !d.cancel_csv_edit(now) {
                let head = d.selection.head;
                d.selection = org_edit::Selection::caret(head);
                d.clear_extra();
            }
            Ok(())
        }),
        c("csv.clearCells", "Clear Contents", &[], |ctx, _| {
            // Delete: in Ready mode the selected cells emptied; typing
            // into a cell, the character after the cursor.
            let now = ctx.now;
            let d = ctx.doc()?;
            if d.csv_editing().is_some() {
                d.delete_in_grid(true, now);
                return Ok(());
            }
            csv_clear_selection(d, now)
        }),
        c("csv.backspaceCell", "Clear and Type", &[], |ctx, _| {
            // Backspace: in Ready mode the cell emptied and typed into, as
            // in Excel; typing into a cell, the character before.
            let now = ctx.now;
            let d = ctx.doc()?;
            if d.csv_editing().is_some() {
                d.delete_in_grid(false, now);
                return Ok(());
            }
            let (_, _, rec, _) = csv_cell(d)?;
            let record = d.text().as_str()[rec.range.clone()].to_string();
            csv_clear_selection(d, now)?;
            d.begin_csv_edit(false);
            if let Some(e) = d.csv_edit.as_mut() {
                e.record = record;
            }
            Ok(())
        }),
        // Enter and Shift+Enter: the cell below or above, as in a
        // spreadsheet (Enter broke the record in two). Past the last row
        // Enter adds one, as Tab does.
        c("csv.cellBelow", "Cell Below", &[], |ctx, _| {
            csv_edit(ctx, |text, l, row, rec, col| {
                let n = l.index.borrow_mut().count(text, &l.dialect);
                if row + 1 < n {
                    return Ok((None, Some((row + 1, col))));
                }
                let columns = l.widths.len().max(rec.fields.len());
                Ok((
                    Some(crate::csv::insert_row(text, rec, columns, &l.dialect)),
                    Some((row + 1, col)),
                ))
            })
        }),
        c("csv.cellAbove", "Cell Above", &[], |ctx, _| {
            csv_edit(ctx, |_, _, row, _, col| {
                Ok((None, row.checked_sub(1).map(|r| (r, col))))
            })
        }),
        c(
            "csv.previousField",
            "Previous Field",
            &["shift+tab"],
            |ctx, _| {
                csv_edit(ctx, |_, l, row, _, col| {
                    if col > 0 {
                        return Ok((None, Some((row, col - 1))));
                    }
                    if row == 0 {
                        return Ok((None, None));
                    }
                    let _ = l;
                    Ok((None, Some((row - 1, usize::MAX))))
                })
            },
        ),
        c(
            "csv.insertRow",
            "Insert Row",
            &["alt+shift+down"],
            |ctx, _| {
                csv_edit(ctx, |text, l, row, rec, col| {
                    let columns = l.widths.len().max(rec.fields.len());
                    Ok((
                        Some(crate::csv::insert_row(text, rec, columns, &l.dialect)),
                        Some((row + 1, col)),
                    ))
                })
            },
        ),
        c(
            "csv.deleteRow",
            "Delete Row",
            &["alt+shift+up"],
            |ctx, _| {
                // The rows the selection covers, else the cursor's.
                let ((r0, r1), _) = rectangle_or_cell(ctx.doc()?)?;
                csv_edit(ctx, |text, l, _, _, col| {
                    let mut idx = l.index.borrow_mut();
                    let (Some(a), Some(b)) = (
                        idx.record(text, r0, &l.dialect),
                        idx.record(text, r1, &l.dialect),
                    ) else {
                        return Err(CommandError::new(crate::tr!("msg-csv-no-row")));
                    };
                    Ok((Some(crate::csv::delete_rows(text, &a, &b)), Some((r0, col))))
                })
            },
        ),
        c("csv.moveRowUp", "Move Row Up", &["alt+up"], |ctx, _| {
            csv_edit(ctx, |text, l, row, rec, col| {
                let top = usize::from(l.dialect.header);
                if row <= top {
                    return Err(CommandError::new(crate::tr!("msg-csv-no-row")));
                }
                let prev = l.index.borrow_mut().record(text, row - 1, &l.dialect);
                let prev = prev.ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?;
                Ok((
                    Some(crate::csv::swap_rows(text, &prev, rec)),
                    Some((row - 1, col)),
                ))
            })
        }),
        c(
            "csv.moveRowDown",
            "Move Row Down",
            &["alt+down"],
            |ctx, _| {
                csv_edit(ctx, |text, l, row, rec, col| {
                    if l.dialect.header && row == 0 {
                        return Err(CommandError::new(crate::tr!("msg-csv-no-row")));
                    }
                    let next = l.index.borrow_mut().record(text, row + 1, &l.dialect);
                    let next =
                        next.ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?;
                    Ok((
                        Some(crate::csv::swap_rows(text, rec, &next)),
                        Some((row + 1, col)),
                    ))
                })
            },
        ),
        c(
            "csv.insertColumn",
            "Insert Column",
            &["alt+shift+right"],
            |ctx, _| {
                csv_edit(ctx, |text, l, row, _, col| {
                    Ok((
                        Some(crate::csv::insert_column(text, &l.dialect, col)),
                        Some((row, col)),
                    ))
                })
            },
        ),
        c(
            "csv.deleteColumn",
            "Delete Column",
            &["alt+shift+left"],
            |ctx, _| {
                // The columns the selection covers, else the cursor's.
                let (_, (c0, c1)) = rectangle_or_cell(ctx.doc()?)?;
                csv_edit(ctx, |text, l, row, _, _| {
                    let left = l.widths.len().saturating_sub(c1 - c0 + 1);
                    Ok((
                        Some(crate::csv::delete_columns(text, &l.dialect, c0, c1)),
                        Some((row, c0.min(left.saturating_sub(1)))),
                    ))
                })
            },
        ),
        c(
            "csv.moveColumnLeft",
            "Move Column Left",
            &["alt+left"],
            |ctx, _| {
                csv_edit(ctx, |text, l, row, _, col| {
                    if col == 0 {
                        return Err(CommandError::new(crate::tr!("msg-csv-no-column")));
                    }
                    Ok((
                        Some(crate::csv::swap_columns(text, &l.dialect, col - 1)),
                        Some((row, col - 1)),
                    ))
                })
            },
        ),
        c(
            "csv.moveColumnRight",
            "Move Column Right",
            &["alt+right"],
            |ctx, _| {
                csv_edit(ctx, |text, l, row, rec, col| {
                    if col + 1 >= rec.fields.len() {
                        return Err(CommandError::new(crate::tr!("msg-csv-no-column")));
                    }
                    Ok((
                        Some(crate::csv::swap_columns(text, &l.dialect, col)),
                        Some((row, col + 1)),
                    ))
                })
            },
        ),
        c("csv.sortFile", "Sort File by Column", &[], |ctx, args| {
            let reverse = arg_bool(args, "reverse");
            csv_edit(ctx, |text, l, _, _, col| {
                let top = usize::from(l.dialect.header);
                Ok((
                    Some(crate::csv::sort_file(text, &l.dialect, col, reverse)),
                    Some((top, col)),
                ))
            })
        }),
        // A record as a form, a long field in a line of its own, find and
        // replace in a column, the frequency table, fields killed and
        // yanked (T2.7d.9).
        c("csv.recordView", "Record View", &[], |ctx, _| {
            let d = ctx.doc()?;
            let (layout, row, rec, _) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let text = d.text().as_str();
            let dl = &layout.dialect;
            let header = if dl.header {
                layout.index.borrow_mut().record(text, 0, dl)
            } else {
                None
            };
            let n = rec
                .fields
                .len()
                .max(header.as_ref().map_or(0, |h| h.fields.len()));
            let category = crate::tr!("category-record", row = row + 1);
            let items = (0..n)
                .map(|col| {
                    let name = header
                        .as_ref()
                        .and_then(|h| h.fields.get(col))
                        .map(|f| crate::csv::value(text, f, dl).into_owned())
                        .unwrap_or_else(|| crate::tr!("csv-column", n = col + 1));
                    let v = rec
                        .fields
                        .get(col)
                        .map(|f| crate::csv::value(text, f, dl).into_owned())
                        .unwrap_or_default();
                    crate::palette::PaletteItem {
                        id: crate::palette::invocation(
                            "csv.setField",
                            &serde_json::json!({ "row": row, "column": col, "value_default": v }),
                        ),
                        title: format!("{name}: {v}"),
                        category: category.clone(),
                        keys: String::new(),
                        also: String::new(),
                    }
                })
                .collect();
            request(ctx, Request::Choose(items))
        }),
        c("csv.editField", "Edit Field", &[], |ctx, _| {
            let d = ctx.doc()?;
            let (layout, row, rec, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let v = rec
                .fields
                .get(col)
                .map(|f| crate::csv::value(d.text().as_str(), f, &layout.dialect).into_owned())
                .unwrap_or_default();
            request(
                ctx,
                Request::Ask {
                    command: "csv.setField".into(),
                    args: serde_json::json!({ "row": row, "column": col, "value_default": v }),
                    arg: "value".into(),
                },
            )
        }),
        c("csv.setField", "Set Field", &[], |ctx, args| {
            let row = args.get("row").and_then(Value::as_u64).map(|r| r as usize);
            let col = args
                .get("column")
                .and_then(Value::as_u64)
                .map(|c| c as usize);
            let value = arg_str(args, "value")?.to_string();
            csv_edit(ctx, |text, l, here, rec, here_col| {
                let row = row.unwrap_or(here);
                let col = col.unwrap_or(here_col);
                let rec = if row == here {
                    rec.clone()
                } else {
                    l.index
                        .borrow_mut()
                        .record(text, row, &l.dialect)
                        .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?
                };
                Ok((
                    Some(crate::csv::set_cell(text, &rec, col, &value, &l.dialect)),
                    Some((row, col)),
                ))
            })
        }),
        c(
            "csv.replaceInColumn",
            "Replace in Column",
            &[],
            |ctx, args| {
                let find = arg_str(args, "find")?.to_string();
                let with = args
                    .get("replace")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let mut count = 0;
                csv_edit(ctx, |text, l, row, _, col| {
                    let (tx, n) =
                        crate::csv::replace_in_column(text, &l.dialect, col, &find, &with);
                    count = n;
                    Ok(((n > 0).then_some(tx), Some((row, col))))
                })?;
                ctx.messages
                    .push(crate::tr!("msg-replaced-count", count = count));
                Ok(())
            },
        ),
        c("csv.frequencies", "Frequency Table", &[], |ctx, _| {
            let d = ctx.doc()?;
            let (layout, _, _, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let freq = crate::csv::frequencies(d.text().as_str(), &layout.dialect, col);
            let max = freq.first().map_or(0, |f| f.1);
            let category = crate::tr!("category-frequencies");
            let items = freq
                .into_iter()
                .take(500)
                .map(|(v, n)| crate::palette::PaletteItem {
                    id: crate::palette::invocation("csv.filter", &serde_json::json!({ "text": v })),
                    title: format!("{n:>6}  {}  {v}", crate::csv::bar(n, max, 20)),
                    category: category.clone(),
                    keys: String::new(),
                    also: String::new(),
                })
                .collect();
            request(ctx, Request::Choose(items))
        }),
        c("csv.histogram", "Histogram of Column", &[], |ctx, _| {
            // The column's numbers in ranges; choosing one goes to its
            // first row.
            let d = ctx.doc()?;
            let (layout, _, _, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let bins = crate::csv::histogram(d.text().as_str(), &layout.dialect, col);
            if bins.is_empty() {
                return Err(CommandError::new(crate::tr!("msg-csv-no-numbers")));
            }
            let max = bins.iter().map(|b| b.count).max().unwrap_or(0);
            let width = bins
                .iter()
                .map(|b| crate::csv::bin_label(b).chars().count())
                .max()
                .unwrap_or(0);
            let category = crate::tr!("category-histogram");
            let items = bins
                .iter()
                .map(|b| {
                    let label = crate::csv::bin_label(b);
                    let pad = width - label.chars().count();
                    crate::palette::PaletteItem {
                        id: if b.count == 0 {
                            crate::palette::invocation("csv.histogram", &serde_json::json!({}))
                        } else {
                            crate::palette::invocation(
                                "csv.goToCell",
                                &serde_json::json!({ "cell": format!("@{}${}", b.first + 1, col + 1) }),
                            )
                        },
                        title: format!(
                            "{label}{}  {:>6}  {}",
                            " ".repeat(pad),
                            b.count,
                            crate::csv::bar(b.count, max, 20)
                        ),
                        category: category.clone(),
                        keys: String::new(),
                        also: String::new(),
                    }
                })
                .collect();
            request(ctx, Request::Choose(items))
        }),
        c("csv.killField", "Kill Field", &[], |ctx, _| {
            let mut killed = String::new();
            csv_edit(ctx, |text, l, row, rec, col| {
                killed = rec
                    .fields
                    .get(col)
                    .map(|f| crate::csv::value(text, f, &l.dialect).into_owned())
                    .unwrap_or_default();
                Ok((
                    Some(crate::csv::set_cell(text, rec, col, "", &l.dialect)),
                    Some((row, col)),
                ))
            })?;
            ctx.clipboard.record(killed.clone());
            request(ctx, Request::CopyText(killed))
        }),
        c("csv.yankField", "Yank Field", &[], |ctx, _| {
            let v = ctx.clipboard.text.clone();
            csv_edit(ctx, |text, l, row, rec, col| {
                Ok((
                    Some(crate::csv::set_cell(text, rec, col, &v, &l.dialect)),
                    Some((row, col)),
                ))
            })
        }),
        c("csv.filter", "Filter Rows", &[], |ctx, args| {
            // Only the rows with a field holding the text show; the file
            // stays as it is. An empty text shows them all.
            let text = arg_str(args, "text")?.trim().to_string();
            let d = ctx.doc()?;
            if d.meta.mode != crate::DocumentMode::Csv {
                return Err(CommandError::new(crate::tr!("msg-not-csv")));
            }
            d.csv_filter = (!text.is_empty()).then_some(text);
            Ok(())
        }),
        c("csv.sortView", "Sort View by Column", &[], |ctx, args| {
            // The rows shown in the order of the column at the cursor (again:
            // descending); the file keeps its order.
            let reverse = arg_bool(args, "reverse");
            let d = ctx.doc()?;
            let (_, _, _, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let reverse = reverse || d.csv_sort == Some((col, false));
            d.csv_sort = Some((col, reverse));
            Ok(())
        }),
        scoped(
            cmd(
                "csv.openAsWorkbook",
                "Open as Workbook",
                "CSV",
                &[],
                None,
                open_as_workbook,
            ),
            Scope::only(&["csv", "text"]),
        ),
        // The dialect by hand: it is detected once and kept.
        c("csv.setDelimiter", "Set Delimiter", &[], |ctx, args| {
            let v = arg_str(args, "delimiter")?.to_string();
            let b = match v.trim() {
                "tab" | "\\t" => b'\t',
                s if s.len() == 1 => s.as_bytes()[0],
                _ => {
                    return Err(CommandError::new(crate::tr!(
                        "msg-csv-bad-delimiter",
                        value = v.as_str()
                    )));
                }
            };
            let d = ctx.doc()?;
            let mut dialect = crate::csv::layout(d).dialect;
            dialect.delimiter = b;
            d.csv_dialect.set(Some(dialect));
            Ok(())
        }),
        c("csv.setQuote", "Set Quote Character", &[], |ctx, args| {
            let v = arg_str(args, "quote")?.to_string();
            let Some(&q) = v.trim().as_bytes().first().filter(|_| v.trim().len() == 1) else {
                return Err(CommandError::new(crate::tr!(
                    "msg-csv-bad-delimiter",
                    value = v.as_str()
                )));
            };
            let d = ctx.doc()?;
            let mut dialect = crate::csv::layout(d).dialect;
            dialect.quote = q;
            d.csv_dialect.set(Some(dialect));
            Ok(())
        }),
        c(
            "csv.toggleHeader",
            "First Row Is a Header",
            &[],
            |ctx, _| {
                let d = ctx.doc()?;
                let mut dialect = crate::csv::layout(d).dialect;
                dialect.header = !dialect.header;
                d.csv_dialect.set(Some(dialect));
                Ok(())
            },
        ),
        c(
            "csv.detectDialect",
            "Detect Delimiter and Header Again",
            &[],
            |ctx, _| {
                let d = ctx.doc()?;
                d.csv_dialect
                    .set(Some(crate::csv::detect(d.text().as_str())));
                Ok(())
            },
        ),
        c("csv.unsortView", "File Order", &[], |ctx, _| {
            ctx.doc()?.csv_sort = None;
            Ok(())
        }),
        c("csv.clearFilter", "Show All Rows", &[], |ctx, _| {
            ctx.doc()?.csv_filter = None;
            Ok(())
        }),
        c(
            "csv.copyAsTsv",
            "Copy as Tab-Separated Values",
            &[],
            |ctx, _| {
                let d = ctx.doc()?;
                let layout = crate::csv::layout(d);
                let text = d.text().as_str();
                let sel = d.selection;
                // The records the selection covers (all of them without one).
                let part = if sel.anchor == sel.head {
                    text.to_string()
                } else {
                    let (a, b) = (sel.anchor.min(sel.head), sel.anchor.max(sel.head));
                    let mut idx = layout.index.borrow_mut();
                    let first = idx.row_at(text, a, &layout.dialect);
                    let last = idx.row_at(text, b, &layout.dialect);
                    let s = idx
                        .record(text, first, &layout.dialect)
                        .map_or(a, |r| r.range.start);
                    let e = idx
                        .record(text, last, &layout.dialect)
                        .map_or(b, |r| r.range.end);
                    text[s..e].to_string()
                };
                let tsv = crate::csv::to_tsv(&crate::csv::rows(&part, &layout.dialect));
                request(ctx, Request::CopyText(tsv))
            },
        ),
        // The grid's view (never written to the file).
        c(
            "csv.toggleAlignment",
            "Align Numbers Right",
            &[],
            |ctx, _| csv_view(ctx, |v| v.align_numbers = !v.align_numbers),
        ),
        c("csv.toggleRainbow", "Rainbow Columns", &[], |ctx, _| {
            csv_view(ctx, |v| v.rainbow = !v.rainbow)
        }),
        c("csv.toggleCoordinates", "Coordinate Grid", &[], |ctx, _| {
            csv_view(ctx, |v| v.coordinates = !v.coordinates)
        }),
        // Columns as the grid shows them (never written to the file).
        c("csv.hideColumn", "Hide Column", &[], |ctx, _| {
            csv_columns(ctx, |cols, l, col| {
                let shown = (0..l.widths.len())
                    .filter(|j| !cols.hidden.contains(j))
                    .count();
                if shown <= 1 {
                    return Err(CommandError::new(crate::tr!("msg-csv-last-column")));
                }
                cols.hidden.insert(col);
                Ok(())
            })?;
            // The cursor to a column that shows.
            csv_to_shown_column(ctx)
        }),
        c("csv.showColumns", "Show All Columns", &[], |ctx, _| {
            csv_columns(ctx, |cols, _, _| {
                cols.hidden.clear();
                Ok(())
            })
        }),
        c("csv.widenColumn", "Widen Column", &[], |ctx, _| {
            csv_columns(ctx, |cols, l, col| {
                let w = l.widths.get(col).copied().unwrap_or(1);
                cols.widths.insert(col, w + 2);
                Ok(())
            })
        }),
        c("csv.narrowColumn", "Narrow Column", &[], |ctx, _| {
            csv_columns(ctx, |cols, l, col| {
                let w = l.widths.get(col).copied().unwrap_or(1);
                cols.widths.insert(col, w.saturating_sub(2).max(2));
                Ok(())
            })
        }),
        c("csv.setColumnWidth", "Column Width", &[], |ctx, args| {
            let w: usize = arg_str(args, "width")?
                .trim()
                .parse()
                .ok()
                .filter(|w| (2..=500).contains(w))
                .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-bad-width")))?;
            // The cursor's column, or the one given (a column's edge
            // dragged in the letters bar).
            let given = arg_column(args);
            csv_columns(ctx, |cols, _, col| {
                cols.widths.insert(given.unwrap_or(col), w);
                Ok(())
            })
        }),
        c("csv.autosizeColumn", "Autosize Column", &[], |ctx, args| {
            let text = ctx.doc()?.text().as_str().to_string();
            let given = arg_column(args);
            csv_columns(ctx, |cols, l, col| {
                let col = given.unwrap_or(col);
                let w = crate::csv::natural_widths(&text, &l.dialect)
                    .get(col)
                    .copied()
                    .unwrap_or(1);
                cols.widths.insert(col, w.clamp(2, 500));
                Ok(())
            })
        }),
        c(
            "csv.autosizeColumns",
            "Autosize All Columns",
            &[],
            |ctx, _| {
                let text = ctx.doc()?.text().as_str().to_string();
                csv_columns(ctx, |cols, l, _| {
                    for (j, w) in crate::csv::natural_widths(&text, &l.dialect)
                        .into_iter()
                        .enumerate()
                    {
                        cols.widths.insert(j, w.clamp(2, 500));
                    }
                    Ok(())
                })
            },
        ),
        c("csv.resetWidths", "Reset Column Widths", &[], |ctx, _| {
            csv_columns(ctx, |cols, _, _| {
                cols.widths.clear();
                Ok(())
            })
        }),
        c("csv.toggleFrozen", "Freeze First Column", &[], |ctx, _| {
            csv_columns(ctx, |cols, _, _| {
                cols.frozen = !cols.frozen;
                Ok(())
            })
        }),
        c("csv.copyCells", "Copy Cells", &[], |ctx, _| {
            // The rectangle from the selection's anchor cell to the
            // cursor's, as TSV; Paste as Block writes it back as cells.
            let tsv = csv_copy_cells(ctx)?;
            request(ctx, Request::CopyText(tsv))
        }),
        c("csv.cutCells", "Cut Cells", &[], |ctx, _| {
            // Copied as Copy Cells does, then emptied: the rows and columns
            // stay.
            let tsv = csv_copy_cells(ctx)?;
            let now = ctx.now;
            let d = ctx.doc()?;
            let ((r0, r1), (c0, c1)) = rectangle_or_cell(d)?;
            let layout = crate::csv::layout(d);
            let blank = vec![vec![String::new(); c1 - c0 + 1]; r1 - r0 + 1];
            if let Some(tx) =
                crate::csv_tools::paste_block(d.text().as_str(), &layout.dialect, r0, c0, &blank)
            {
                d.apply(&tx, org_edit::ChangeKind::Command, now);
            }
            request(ctx, Request::CopyText(tsv))
        }),
        c("csv.pasteBlock", "Paste as Block", &[], |ctx, _| {
            let d = ctx.doc()?;
            if d.meta.mode != crate::DocumentMode::Csv {
                return Err(CommandError::new(crate::tr!("msg-not-csv")));
            }
            d.csv_paste_block = true;
            request(ctx, Request::Paste { plain: false })
        }),
        c("csv.fillDown", "Fill Down", &[], |ctx, _| {
            csv_fill(ctx, false)
        }),
        c("csv.fillSeries", "Fill Series", &[], |ctx, _| {
            csv_fill(ctx, true)
        }),
        c(
            "csv.removeDuplicates",
            "Remove Duplicate Rows",
            &[],
            |ctx, _| {
                let now = ctx.now;
                let d = ctx.doc()?;
                let layout = crate::csv::layout(d);
                let (tx, n) =
                    crate::csv_tools::remove_duplicates(d.text().as_str(), &layout.dialect);
                if n > 0 {
                    d.apply(&tx, org_edit::ChangeKind::Command, now);
                }
                ctx.messages
                    .push(crate::tr!("msg-csv-duplicates", count = n));
                Ok(())
            },
        ),
        c("csv.transpose", "Transpose", &[], |ctx, _| {
            let now = ctx.now;
            let d = ctx.doc()?;
            let layout = crate::csv::layout(d);
            let tx = crate::csv_tools::transpose(d.text().as_str(), &layout.dialect);
            d.apply(&tx, org_edit::ChangeKind::Command, now);
            Ok(())
        }),
        c("csv.splitColumn", "Split Column", &[], |ctx, args| {
            let sep = arg_str(args, "separator")?.to_string();
            let sep = if sep.is_empty() { " ".to_string() } else { sep };
            if !arg_bool(args, "confirmed") {
                return csv_preview(ctx, "csv.splitColumn", args, |text, l, col| {
                    crate::csv_tools::split_column(text, &l.dialect, col, &sep).ok_or_else(|| {
                        CommandError::new(crate::tr!(
                            "msg-csv-nothing-to-split",
                            separator = sep.as_str()
                        ))
                    })
                });
            }
            csv_edit(ctx, |text, l, row, _, col| {
                let tx = crate::csv_tools::split_column(text, &l.dialect, col, &sep).ok_or_else(
                    || {
                        CommandError::new(crate::tr!(
                            "msg-csv-nothing-to-split",
                            separator = sep.as_str()
                        ))
                    },
                )?;
                Ok((Some(tx), Some((row, col))))
            })
        }),
        c(
            "csv.joinColumns",
            "Join with Next Column",
            &[],
            |ctx, args| {
                let sep = arg_str(args, "separator")?.to_string();
                if !arg_bool(args, "confirmed") {
                    return csv_preview(ctx, "csv.joinColumns", args, |text, l, col| {
                        crate::csv_tools::join_columns(text, &l.dialect, col, &sep)
                            .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-column")))
                    });
                }
                csv_edit(ctx, |text, l, row, _, col| {
                    let tx = crate::csv_tools::join_columns(text, &l.dialect, col, &sep)
                        .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-column")))?;
                    Ok((Some(tx), Some((row, col))))
                })
            },
        ),
        c(
            "csv.sortFileBy",
            "Sort File by Columns",
            &[],
            |ctx, args| {
                // Columns by letter or number in order, `-` for descending:
                // `B, -A`.
                let spec = arg_str(args, "columns")?.to_string();
                let keys = crate::csv_tools::parse_sort_keys(&spec).ok_or_else(|| {
                    CommandError::new(crate::tr!("msg-csv-bad-columns", value = spec.as_str()))
                })?;
                csv_edit(ctx, |text, l, _, _, col| {
                    let top = usize::from(l.dialect.header);
                    Ok((
                        Some(crate::csv_tools::sort_by(text, &l.dialect, &keys)),
                        Some((top, col)),
                    ))
                })
            },
        ),
        c("csv.sumColumn", "Sum Column", &[], |ctx, args| {
            // As `C-c +` in an Org table: the sum shown and copied; with
            // `insert`, written into the cell at the cursor as a value.
            let insert = arg_bool(args, "insert");
            let mut sum = String::new();
            csv_edit(ctx, |text, l, row, rec, col| {
                let skip = insert.then_some(row);
                let total = crate::csv_tools::column_sum(text, &l.dialect, col, skip)
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-numbers")))?;
                sum = crate::formulas::number(total);
                Ok((
                    insert.then(|| crate::csv::set_cell(text, rec, col, &sum, &l.dialect)),
                    None,
                ))
            })?;
            ctx.messages
                .push(crate::tr!("msg-csv-sum", sum = sum.as_str()));
            if insert {
                Ok(())
            } else {
                request(ctx, Request::CopyText(sum))
            }
        }),
        c("csv.cellCoordinates", "Cell Coordinates", &[], |ctx, _| {
            let d = ctx.doc()?;
            let (layout, row, _, col) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let (cell, org) = crate::csv_tools::coordinates(row, col);
            let text = d.text().as_str();
            let header = layout
                .dialect
                .header
                .then(|| layout.index.borrow_mut().record(text, 0, &layout.dialect))
                .flatten()
                .and_then(|r| {
                    r.fields
                        .get(col)
                        .map(|f| crate::csv::value(text, f, &layout.dialect).into_owned())
                })
                .filter(|h| !h.is_empty());
            ctx.messages.push(match header {
                Some(h) => crate::tr!(
                    "msg-csv-cell-named",
                    cell = cell.as_str(),
                    org = org.as_str(),
                    column = h.as_str()
                ),
                None => crate::tr!("msg-csv-cell", cell = cell.as_str(), org = org.as_str()),
            });
            Ok(())
        }),
        // A click on a column's letter or a row's number, as in a
        // spreadsheet: the whole column or row selected, a rectangle of
        // cells that Copy, Cut and Delete take.
        c("csv.selectColumn", "Select Column", &[], |ctx, args| {
            let d = ctx.doc()?;
            let (layout, _, _, here) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let col = arg_column(args).unwrap_or(here);
            let text = d.text().as_str();
            let mut idx = layout.index.borrow_mut();
            let n = idx.count(text, &layout.dialect);
            // From the first record with the column to the last one.
            let ends: Vec<usize> = (0..n)
                .filter_map(|r| idx.record(text, r, &layout.dialect))
                .filter_map(|r| r.fields.get(col).map(|f| f.range.start))
                .collect();
            let (Some(&first), Some(&last)) = (ends.first(), ends.last()) else {
                return Err(CommandError::new(crate::tr!("msg-csv-no-column")));
            };
            drop(idx);
            d.selection = org_edit::Selection {
                anchor: first,
                head: last,
            };
            Ok(())
        }),
        c("csv.selectRow", "Select Row", &[], |ctx, args| {
            let d = ctx.doc()?;
            let (layout, here, _, _) = crate::csv::cell_at(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-not-csv")))?;
            let row = args
                .get("row")
                .and_then(Value::as_u64)
                .and_then(|r| usize::try_from(r).ok())
                .unwrap_or(here);
            let text = d.text().as_str();
            let rec = layout
                .index
                .borrow_mut()
                .record(text, row, &layout.dialect)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?;
            d.selection = org_edit::Selection {
                anchor: rec.range.start,
                head: rec.range.end,
            };
            Ok(())
        }),
        c("csv.goToCell", "Go to Cell", &[], |ctx, args| {
            // `B3`, `@3$2` or `3,2`.
            let name = arg_str(args, "cell")?.to_string();
            let (row, col) = crate::csv_tools::parse_cell(&name).ok_or_else(|| {
                CommandError::new(crate::tr!("msg-csv-bad-cell", value = name.as_str()))
            })?;
            csv_edit(ctx, |text, l, _, _, _| {
                let n = l.index.borrow_mut().count(text, &l.dialect);
                if row >= n {
                    return Err(CommandError::new(crate::tr!(
                        "msg-csv-bad-cell",
                        value = name.as_str()
                    )));
                }
                Ok((None, Some((row, col))))
            })
        }),
        c("csv.openAsText", "Open as Plain Text", &[], |ctx, _| {
            let base = ctx.config.parse_base();
            let doc = ctx.doc()?;
            doc.set_mode(crate::DocumentMode::Text { language: None }, &base);
            ctx.messages.push(crate::tr!(
                "msg-mode-set",
                mode = crate::DocumentMode::Text { language: None }.title()
            ));
            request(ctx, Request::ModeChanged)
        }),
        c("csv.convertToOrg", "Convert to Org Table", &[], |ctx, _| {
            let d = ctx.doc()?;
            let layout = crate::csv::layout(d);
            let table = crate::csv::to_org_table(d.text().as_str(), &layout.dialect);
            let path = d
                .meta
                .path
                .clone()
                .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-save-first")))?;
            let target = path.with_extension("org");
            if target.exists() {
                return Err(CommandError::new(crate::tr!(
                    "msg-import-exists",
                    path = target.display().to_string()
                )));
            }
            std::fs::write(&target, table).map_err(|e| CommandError::new(e.to_string()))?;
            ctx.messages.push(crate::tr!(
                "msg-csv-converted",
                path = target.display().to_string()
            ));
            request(
                ctx,
                Request::Open {
                    path: Some(target.display().to_string()),
                },
            )
        }),
    ]
}

thread_local! {
    /// The font size before Big Text, to go back to.
    static BIG_FROM: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
}

/// Saves setting `key` as `value` in the user's settings.
fn set_setting(ctx: &mut EditorContext<'_>, key: &str, value: Value) -> CommandResult {
    request(
        ctx,
        Request::SetSetting {
            key: key.into(),
            value,
            quiet: false,
        },
    )
}

/// The selection on one line, else the word at the cursor, else nothing.
fn word_or_selection(ctx: &mut EditorContext<'_>) -> Result<String, CommandError> {
    let doc = ctx.doc()?;
    if let Some(t) = doc.selected_text().filter(|t| !t.contains('\n')) {
        return Ok(t.to_string());
    }
    let text = doc.text();
    Ok(crate::lines::word_at(text.as_str(), doc.selection.head)
        .map(|r| text.as_str()[r].to_string())
        .unwrap_or_default())
}

/// `s` for a URL's query: unreserved characters kept, the rest as
/// `%XX`, spaces as `+`.
fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// A character as `SPC h '` describes it: itself, its code point and
/// its UTF-8 bytes.
fn describe_char(c: char) -> String {
    let mut buf = [0u8; 4];
    let bytes: Vec<String> = c
        .encode_utf8(&mut buf)
        .bytes()
        .map(|b| format!("{b:02X}"))
        .collect();
    let shown = match c {
        '\n' => "\\n".to_string(),
        '\t' => "\\t".to_string(),
        ' ' => "space".to_string(),
        c if c.is_control() => "control".to_string(),
        c => c.to_string(),
    };
    format!("{shown}  U+{:04X}  UTF-8 {}", c as u32, bytes.join(" "))
}

/// The counterpart of `path` (Doom's `SPC p o`): the next existing file
/// beside it with the same name and another of these extensions, after
/// its own, so repeated use goes round them.
fn other_file(path: &std::path::Path) -> Option<std::path::PathBuf> {
    const ORDER: [&str; 8] = ["org", "md", "tex", "html", "pdf", "docx", "odt", "txt"];
    let ext = path.extension()?.to_str()?.to_lowercase();
    let at = ORDER
        .iter()
        .position(|e| *e == ext)
        .unwrap_or(ORDER.len() - 1);
    (1..ORDER.len())
        .map(|i| path.with_extension(ORDER[(at + i) % ORDER.len()]))
        .find(|p| p.is_file())
}

/// The active document's file, made absolute.
fn this_file(ctx: &mut EditorContext<'_>) -> Result<std::path::PathBuf, CommandError> {
    let path = ctx
        .doc()?
        .meta
        .path
        .clone()
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-file")))?;
    Ok(std::path::absolute(&path).unwrap_or(path))
}

/// Moves or copies this file to argument `target` (asked for when
/// missing); the document follows a move ([`crate::dired::follow`]).
fn this_file_to(
    ctx: &mut EditorContext<'_>,
    args: &Value,
    kind: kalem_fs::OpKind,
) -> CommandResult {
    let path = this_file(ctx)?;
    let Some(target) = args.get("target").and_then(Value::as_str) else {
        let command = if kind == kalem_fs::OpKind::Move {
            "file.rename"
        } else {
            "file.copy"
        };
        return request(
            ctx,
            Request::Ask {
                command: command.into(),
                args: serde_json::json!({}),
                arg: "target".into(),
            },
        );
    };
    let target = std::path::PathBuf::from(crate::settings::expand_home(target));
    let target = match (target.is_absolute(), path.parent()) {
        (false, Some(dir)) => dir.join(target),
        _ => target,
    };
    request(
        ctx,
        Request::FileOp(crate::command::FileOp {
            kind,
            sources: vec![path],
            target: Some(target),
        }),
    )
}

/// The commands of language servers (D57, T3.8.2), for files a language
/// plugin serves; the answers arrive later, through `lsp::take_outcomes`.
fn code_commands() -> Vec<Command> {
    use crate::lsp::Kind;
    fn ask(ctx: &mut EditorContext<'_>, kind: Kind) -> CommandResult {
        crate::lsp::request(ctx.doc()?, kind).map_err(CommandError::new)
    }
    fn places(
        ctx: &mut EditorContext<'_>,
        title: &str,
        places: Vec<crate::lsp::Place>,
    ) -> CommandResult {
        if places.is_empty() {
            ctx.messages.push(crate::tr!("lsp-no-problems"));
            return Ok(());
        }
        let _ = title;
        request(ctx, Request::Choose(crate::lsp::place_items(&places)))
    }
    let server = Some("hasLanguageServer");
    vec![
        cmd(
            "code.documentation",
            "Show Documentation",
            "Code",
            &[],
            None,
            |ctx, _| ask(ctx, Kind::Hover),
        ),
        cmd(
            "code.definition",
            "Go to Definition",
            "Code",
            &["f12"],
            None,
            |ctx, _| ask(ctx, Kind::Definition),
        ),
        cmd(
            "code.declaration",
            "Go to Declaration",
            "Code",
            &[],
            server,
            |ctx, _| ask(ctx, Kind::Declaration),
        ),
        cmd(
            "code.typeDefinition",
            "Go to Type Definition",
            "Code",
            &[],
            None,
            |ctx, _| ask(ctx, Kind::TypeDefinition),
        ),
        cmd(
            "code.implementation",
            "Go to Implementations",
            "Code",
            &[],
            None,
            |ctx, _| ask(ctx, Kind::Implementation),
        ),
        cmd(
            "code.references",
            "Find References",
            "Code",
            &["shift+f12"],
            None,
            |ctx, _| ask(ctx, Kind::References),
        ),
        cmd(
            "code.symbols",
            "Go to Symbol in Document",
            "Code",
            &[],
            server,
            |ctx, _| ask(ctx, Kind::Symbols),
        ),
        cmd(
            "code.problems",
            "List Problems",
            "Code",
            &[],
            None,
            |ctx, _| {
                let path = ctx.doc()?.meta.path.clone().unwrap_or_default();
                places(ctx, "Problems", crate::lsp::problems_of(Some(&path)))
            },
        ),
        cmd(
            "code.allProblems",
            "List Problems of Open Files",
            "Code",
            &[],
            None,
            |ctx, _| places(ctx, "Problems", crate::lsp::all_problems()),
        ),
        cmd(
            "code.restartServer",
            "Restart Language Server",
            "Code",
            &[],
            server,
            |ctx, _| {
                let m = crate::lsp::restart(ctx.doc()?).map_err(CommandError::new)?;
                ctx.messages.push(m);
                Ok(())
            },
        ),
        cmd(
            "code.serverStatus",
            "Language Server Status",
            "Code",
            &[],
            None,
            |ctx, _| {
                let mut lines = crate::lsp::report();
                if let Ok(d) = ctx.doc()
                    && let Some(s) = crate::lsp::describe(d)
                {
                    lines.insert(0, s);
                }
                for p in crate::languages::problems() {
                    lines.push(p);
                }
                ctx.messages.push(if lines.is_empty() {
                    crate::tr!("lsp-none-running")
                } else {
                    lines.join(" · ")
                });
                Ok(())
            },
        ),
        cmd(
            "code.rename",
            "Rename Symbol",
            "Code",
            &[],
            None,
            |ctx, args| match args["name"]
                .as_str()
                .map(str::trim)
                .filter(|n| !n.is_empty())
            {
                Some(name) => crate::lsp::rename(ctx.doc()?, name).map_err(CommandError::new),
                None => request(
                    ctx,
                    Request::Ask {
                        command: "code.rename".into(),
                        args: serde_json::json!({}),
                        arg: "name".into(),
                    },
                ),
            },
        ),
        cmd(
            "code.actions",
            "Code Actions",
            "Code",
            &[],
            None,
            |ctx, _| crate::lsp::code_actions(ctx.doc()?).map_err(CommandError::new),
        ),
        cmd(
            "code.applyEdit",
            "Apply Language Server Edit",
            "Code",
            &[],
            None,
            |ctx, args| {
                let m = crate::lsp::apply_plan(args["id"].as_u64().unwrap_or(0))
                    .map_err(CommandError::new)?;
                ctx.messages.push(m);
                Ok(())
            },
        ),
        cmd(
            "code.dropEdit",
            "Drop Language Server Edit",
            "Code",
            &[],
            None,
            |_ctx, args| {
                crate::lsp::drop_plan(args["id"].as_u64().unwrap_or(0));
                Ok(())
            },
        ),
        cmd(
            "code.runAction",
            "Run Code Action",
            "Code",
            &[],
            None,
            |ctx, args| {
                let m = crate::lsp::run_offer(args["id"].as_u64().unwrap_or(0))
                    .map_err(CommandError::new)?;
                if !m.is_empty() {
                    ctx.messages.push(m);
                }
                Ok(())
            },
        ),
        cmd(
            "code.goto",
            "Go to Place",
            "Code",
            &[],
            None,
            |ctx, args| {
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| CommandError::new("No place"))?;
                request(
                    ctx,
                    Request::OpenAt {
                        path: path.to_string(),
                        line: args["line"].as_u64().unwrap_or(1),
                        column: args["column"].as_u64().unwrap_or(0) as usize,
                    },
                )
            },
        ),
    ]
}

/// Plugins installed, updated and removed from inside Kalem
/// (`plugin_store`, T3.3.3). Downloads run as background jobs; what was
/// found is offered as a list to confirm, with its permissions.
fn plugin_commands() -> Vec<Command> {
    use crate::palette::{PaletteItem, invocation};
    use serde_json::json;
    fn item(id: String, title: String, category: String) -> PaletteItem {
        PaletteItem {
            also: title.clone(),
            id,
            title,
            category,
            keys: String::new(),
        }
    }
    fn index_urls(ctx: &EditorContext<'_>) -> Vec<String> {
        crate::plugin_store::index_urls(ctx.config)
    }
    /// The user's plugin indexes (`plugins.sources`).
    fn sources(ctx: &EditorContext<'_>) -> Vec<String> {
        ctx.config
            .strings("plugins.sources")
            .into_iter()
            .map(str::to_string)
            .collect()
    }
    /// Downloads `source` in the background, then offers to install it.
    fn start_install(ctx: &mut EditorContext<'_>, source: String) -> CommandResult {
        let index = index_urls(ctx);
        crate::jobs::spawn(
            crate::tr!("plugin-fetching", source = source.as_str()),
            move || match crate::plugin_store::prepare(&source, &index) {
                Ok(p) => {
                    let lines = crate::plugin_store::summary(&p);
                    let staging = p.staging.to_string_lossy().into_owned();
                    let verb_key = if p.replaces.is_some() {
                        "plugin-verb-update"
                    } else {
                        "plugin-verb-install"
                    };
                    let mut items = vec![item(
                        invocation(
                            "plugin.confirmInstall",
                            &json!({ "staging": staging, "source": p.source }),
                        ),
                        crate::tr!(verb_key, what = lines[0].as_str()),
                        lines[1..].join(" · "),
                    )];
                    items.push(item(
                        invocation("plugin.cancelInstall", &json!({ "staging": staging })),
                        crate::tr!("plugin-cancel"),
                        String::new(),
                    ));
                    crate::jobs::offer(items);
                    crate::jobs::Finished {
                        message: crate::tr!(
                            "plugin-ready",
                            name = p.name.as_str(),
                            version = p.version.as_str()
                        ),
                        error: false,
                        open: None,
                    }
                }
                Err(e) => crate::jobs::Finished {
                    message: e,
                    error: true,
                    open: None,
                },
            },
        );
        Ok(())
    }
    vec![
        // Where plugins are listed: the official index and the user's own
        // (a fork of it on GitHub, or an index of their own plugins).
        cmd(
            "plugin.sources",
            "Plugin Sources…",
            "Plugins",
            &[],
            None,
            |ctx, _| {
                let mut items = vec![item(
                    invocation("plugin.addSource", &json!({})),
                    crate::tr!("plugin-source-add"),
                    String::new(),
                )];
                for url in sources(ctx) {
                    items.push(item(
                        invocation("plugin.removeSource", &json!({ "url": url })),
                        crate::tr!("plugin-source-remove", url = url.as_str()),
                        crate::tr!("plugin-source-yours"),
                    ));
                }
                let main = match ctx.config.str("plugins.index") {
                    "" => crate::plugin_store::DEFAULT_INDEX.to_string(),
                    s => s.to_string(),
                };
                items.push(item(
                    invocation("plugin.setIndex", &json!({})),
                    crate::tr!("plugin-source-main", url = main.as_str()),
                    crate::tr!("plugin-source-official"),
                ));
                request(ctx, Request::Choose(items))
            },
        ),
        cmd(
            "plugin.addSource",
            "Add Plugin Source…",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let Some(url) = args["url"]
                    .as_str()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                else {
                    return request(
                        ctx,
                        Request::Ask {
                            command: "plugin.addSource".into(),
                            args: json!({}),
                            arg: "url".into(),
                        },
                    );
                };
                let mut list = sources(ctx);
                if !list.iter().any(|u| u == url) {
                    list.insert(0, url.to_string());
                }
                ctx.messages
                    .push(crate::tr!("plugin-source-added", url = url));
                set_setting(ctx, "plugins.sources", json!(list))
            },
        ),
        cmd(
            "plugin.removeSource",
            "Remove Plugin Source",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let url = args["url"].as_str().unwrap_or_default().to_string();
                let list: Vec<String> = sources(ctx).into_iter().filter(|u| *u != url).collect();
                ctx.messages
                    .push(crate::tr!("plugin-source-removed", url = url.as_str()));
                set_setting(ctx, "plugins.sources", json!(list))
            },
        ),
        cmd(
            "plugin.setIndex",
            "Set Official Plugin Index…",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let Some(url) = args["url"].as_str().map(str::trim) else {
                    return request(
                        ctx,
                        Request::Ask {
                            command: "plugin.setIndex".into(),
                            args: json!({}),
                            arg: "url".into(),
                        },
                    );
                };
                // Empty: the default again.
                let url = if url.is_empty() {
                    crate::plugin_store::DEFAULT_INDEX
                } else {
                    url
                };
                set_setting(ctx, "plugins.index", json!(url))
            },
        ),
        cmd(
            "plugin.browse",
            "Browse Plugins",
            "Plugins",
            &[],
            None,
            |ctx, _| {
                let index = index_urls(ctx);
                crate::jobs::spawn(crate::tr!("plugin-reading-index"), move || {
                    match crate::plugin_store::fetch_indexes(&index) {
                        Ok(entries) => {
                            // Remembers the versions for Installed Plugins.
                            let _ = crate::plugin_store::updates(&entries);
                            let installed = crate::plugin_store::installed();
                            let items: Vec<PaletteItem> = entries
                                .iter()
                                .map(|e| {
                                    let state = match installed.iter().find(|i| i.id == e.id) {
                                        Some(i) if i.version == e.version => {
                                            crate::tr!(
                                                "plugin-installed-version",
                                                version = i.version.as_str()
                                            )
                                        }
                                        Some(i) => crate::tr!(
                                            "plugin-installed-available",
                                            version = i.version.as_str(),
                                            available = e.version.as_str()
                                        ),
                                        // A component not released: built from
                                        // its source only.
                                        None if !e.declarative && e.download.is_none() => {
                                            crate::tr!("plugin-no-release")
                                        }
                                        None => e.version.clone(),
                                    };
                                    item(
                                        invocation("plugin.install", &json!({ "source": e.id })),
                                        format!("{} — {}", e.name, e.description),
                                        state,
                                    )
                                })
                                .collect();
                            let n = items.len();
                            // A plugin of one's own, from its repository.
                            let mut items = items;
                            items.insert(
                                0,
                                item(
                                    invocation("plugin.installGitHub", &json!({})),
                                    crate::tr!("plugin-github-item"),
                                    crate::tr!("plugin-github-item-detail"),
                                ),
                            );
                            crate::jobs::offer(items);
                            crate::jobs::Finished {
                                message: crate::tr!("plugin-index-count", count = n),
                                error: false,
                                open: None,
                            }
                        }
                        Err(e) => crate::jobs::Finished {
                            message: e,
                            error: true,
                            open: None,
                        },
                    }
                });
                Ok(())
            },
        ),
        cmd(
            "plugin.install",
            "Install Plugin…",
            "Plugins",
            &[],
            None,
            |ctx, args| match args["source"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(s) => start_install(ctx, s.to_string()),
                None => request(
                    ctx,
                    Request::Ask {
                        command: "plugin.install".into(),
                        args: json!({}),
                        arg: "source".into(),
                    },
                ),
            },
        ),
        // A file protected by a password (a PDF): asked for when it would
        // not open, kept for the session, the file opened again.
        cmd(
            "file.openWithPassword",
            "Open with Password…",
            "File",
            &[],
            None,
            |ctx, args| {
                let Some(path) = args["path"].as_str().map(str::to_owned) else {
                    return Err(CommandError::new(crate::tr!("msg-no-document")));
                };
                let Some(password) = args["password"].as_str() else {
                    return request(
                        ctx,
                        Request::Ask {
                            command: "file.openWithPassword".into(),
                            args: json!({ "path": path }),
                            arg: "password".into(),
                        },
                    );
                };
                let path = std::path::PathBuf::from(&path);
                let path = dunce::canonicalize(&path).unwrap_or(path);
                crate::viewer::remember_password(&path, password);
                request(
                    ctx,
                    Request::Open {
                        path: Some(path.display().to_string()),
                    },
                )
            },
        ),
        // A plugin of one's own (or anyone's) from its GitHub repository:
        // a declarative one as its folder is, a component one with its
        // build from the repository's releases.
        cmd(
            "plugin.installGitHub",
            "Install Plugin from GitHub…",
            "Plugins",
            &[],
            None,
            |ctx, args| match args["link"]
                .as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(link) => match crate::plugin_store::github_link(link) {
                    Some(source) => start_install(ctx, source),
                    None => Err(CommandError::new(crate::tr!(
                        "plugin-not-github",
                        link = link
                    ))),
                },
                None => request(
                    ctx,
                    Request::Ask {
                        command: "plugin.installGitHub".into(),
                        args: json!({}),
                        arg: "link".into(),
                    },
                ),
            },
        ),
        cmd(
            "plugin.confirmInstall",
            "Confirm Plugin Installation",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let staging =
                    std::path::PathBuf::from(args["staging"].as_str().unwrap_or_default());
                let source = args["source"].as_str().unwrap_or_default();
                // Read again from the staging folder: what is installed is
                // what the user was shown.
                let p = crate::plugin_store::prepared_at(&staging, source)
                    .map_err(CommandError::new)?;
                let dir = crate::plugin_store::install(&p).map_err(CommandError::new)?;
                ctx.messages.push(crate::tr!(
                    "plugin-installed",
                    name = p.name.as_str(),
                    version = p.version.as_str(),
                    dir = dir.display().to_string()
                ));
                Ok(())
            },
        ),
        cmd(
            "plugin.cancelInstall",
            "Cancel Plugin Installation",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                crate::plugin_store::discard(std::path::Path::new(
                    args["staging"].as_str().unwrap_or_default(),
                ));
                ctx.messages.push(crate::tr!("plugin-not-installed-cancel"));
                Ok(())
            },
        ),
        cmd(
            "plugin.list",
            "Installed Plugins",
            "Plugins",
            &[],
            None,
            |ctx, _| {
                let items: Vec<PaletteItem> = crate::plugin_store::installed()
                    .into_iter()
                    .map(|p| {
                        let from = p
                            .source
                            .clone()
                            .unwrap_or_else(|| p.dir.display().to_string());
                        let category = match crate::plugin_store::available(&p.id, &p.version) {
                            Some(v) => {
                                crate::tr!("plugin-available-from", version = v, from = from)
                            }
                            None => from,
                        };
                        item(
                            invocation("plugin.manage", &json!({ "id": p.id })),
                            format!("{} {}", p.name, p.version),
                            category,
                        )
                    })
                    .collect();
                if items.is_empty() {
                    ctx.messages.push(crate::tr!("plugin-none-installed"));
                    return Ok(());
                }
                request(ctx, Request::Choose(items))
            },
        ),
        cmd(
            "plugin.manage",
            "Manage Plugin",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let id = args["id"].as_str().unwrap_or_default();
                let p = crate::plugin_store::installed()
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| {
                        CommandError::new(crate::tr!("plugin-not-installed", id = id))
                    })?;
                let mut items = Vec::new();
                if let Some(src) = &p.source {
                    items.push(item(
                        invocation("plugin.install", &json!({ "source": src })),
                        crate::tr!("plugin-update", name = p.name.as_str()),
                        crate::tr!("plugin-from-short", source = src.as_str()),
                    ));
                }
                items.push(item(
                    invocation("plugin.remove", &json!({ "id": p.id })),
                    crate::tr!("plugin-remove", name = p.name.as_str()),
                    crate::tr!("plugin-asks-first"),
                ));
                items.push(item(
                    invocation("file.open", &json!({ "path": p.dir })),
                    crate::tr!("plugin-show-folder"),
                    p.dir.display().to_string(),
                ));
                request(ctx, Request::Choose(items))
            },
        ),
        cmd(
            "plugin.remove",
            "Remove Plugin",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let id = args["id"].as_str().unwrap_or_default();
                let p = crate::plugin_store::installed()
                    .into_iter()
                    .find(|p| p.id == id)
                    .ok_or_else(|| {
                        CommandError::new(crate::tr!("plugin-not-installed", id = id))
                    })?;
                request(
                    ctx,
                    Request::Choose(vec![
                        item(
                            invocation("plugin.removeConfirmed", &json!({ "id": p.id })),
                            crate::tr!(
                                "plugin-remove-version",
                                name = p.name.as_str(),
                                version = p.version.as_str()
                            ),
                            crate::tr!("plugin-deletes", dir = p.dir.display().to_string()),
                        ),
                        item(
                            "plugin.list".into(),
                            crate::tr!("plugin-cancel"),
                            String::new(),
                        ),
                    ]),
                )
            },
        ),
        cmd(
            "plugin.removeConfirmed",
            "Remove Plugin Now",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let name = crate::plugin_store::remove(args["id"].as_str().unwrap_or_default())
                    .map_err(CommandError::new)?;
                ctx.messages.push(crate::tr!("plugin-removed", name = name));
                Ok(())
            },
        ),
    ]
}

/// Where a new workbook goes: the path asked (the folder of the file
/// open, `Book1.xlsx` or the next free name offered), its extension added,
/// and a file there replaced only when chosen. `Ok(None)` when it asked.
fn new_file_target(
    ctx: &mut EditorContext<'_>,
    id: &str,
    args: &Value,
    stem: &str,
    ext: &str,
) -> Result<Option<std::path::PathBuf>, CommandError> {
    let dir = ctx
        .document
        .as_deref()
        .and_then(|d| d.meta.path.as_ref())
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .or_else(|| std::env::var_os("HOME").map(std::path::PathBuf::from))
        .unwrap_or_default();
    let Some(path) = args.get("path").and_then(Value::as_str) else {
        let free = (1..)
            .map(|n| dir.join(format!("{stem}{n}.{ext}")))
            .find(|p| !p.exists())
            .unwrap_or_else(|| dir.join(format!("{stem}.{ext}")));
        let mut a = args.clone();
        a["path_default"] = Value::String(free.display().to_string());
        request(
            ctx,
            Request::Ask {
                command: id.into(),
                args: a,
                arg: "path".into(),
            },
        )?;
        return Ok(None);
    };
    let mut target = std::path::PathBuf::from(crate::settings::expand_home(path.trim()));
    if target.is_relative() {
        target = dir.join(target);
    }
    if target.extension().is_none() {
        target.set_extension(ext);
    }
    if target.exists() && !arg_bool(args, "replace") {
        let name = target
            .file_name()
            .map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let mut a = args.clone();
        a["path"] = Value::String(target.display().to_string());
        a["replace"] = Value::Bool(true);
        let item = crate::palette::PaletteItem {
            id: crate::palette::invocation(id, &a),
            title: format!("Replace {name}"),
            category: format!("{name} exists"),
            keys: String::new(),
            also: String::new(),
        };
        request(ctx, Request::Choose(vec![item]))?;
        return Ok(None);
    }
    Ok(Some(target))
}

/// New Workbook: a blank workbook of one sheet, written where asked and
/// opened.
fn new_workbook(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    let Some(target) = new_file_target(ctx, "app.newWorkbook", args, "Book", "xlsx")? else {
        return Ok(());
    };
    let bytes = crate::workbook_io::blank_xlsx(&["Sheet1".to_string()]);
    std::fs::write(&target, bytes).map_err(|e| CommandError::new(e.to_string()))?;
    request(
        ctx,
        Request::Open {
            path: Some(target.display().to_string()),
        },
    )
}

/// New from Template: a template (`.xltx`, `.xltm`) chosen, a workbook made
/// of it written where asked and opened.
fn new_from_template(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    const ID: &str = "app.newFromTemplate";
    let Some(template) = args
        .get("template")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return request(
            ctx,
            Request::PickFile {
                command: ID.into(),
                arg: "template".into(),
                args: args.clone(),
            },
        );
    };
    let template = std::path::PathBuf::from(crate::settings::expand_home(&template));
    let macros = template
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xltm"));
    let stem = template
        .file_stem()
        .map_or("Book".into(), |s| s.to_string_lossy().into_owned());
    let ext = if macros { "xlsm" } else { "xlsx" };
    let Some(target) = new_file_target(ctx, ID, args, &stem, ext)? else {
        return Ok(());
    };
    let bytes = std::fs::read(&template).map_err(|e| CommandError::new(e.to_string()))?;
    let bytes = crate::workbook_io::template_to_workbook(&bytes).map_err(CommandError::new)?;
    std::fs::write(&target, bytes).map_err(|e| CommandError::new(e.to_string()))?;
    request(
        ctx,
        Request::Open {
            path: Some(target.display().to_string()),
        },
    )
}

/// Open as Workbook (Excel's Text Import Wizard): the file's records
/// read with a delimiter and an encoding chosen, each column as General,
/// Text, a date in an order, or left out; written as a workbook beside it
/// (or where asked) and opened.
fn open_as_workbook(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    const ID: &str = "csv.openAsWorkbook";
    let item = |a: Value, title: &str, category: &str| crate::palette::PaletteItem {
        id: crate::palette::invocation(ID, &a),
        title: title.into(),
        category: category.into(),
        keys: String::new(),
        also: title.into(),
    };
    let with = |key: &str, v: &str| {
        let mut a = args.clone();
        a[key] = Value::String(v.into());
        a
    };
    let d = ctx.doc()?;
    let detected = crate::csv::detect(d.text().as_str()).delimiter;
    let path = d.meta.path.clone();
    let Some(delimiter) = args
        .get("delimiter")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        let mut choices = vec![
            (",", "Comma"),
            (";", "Semicolon"),
            ("tab", "Tab"),
            ("|", "Vertical Bar"),
            (" ", "Space"),
        ];
        // The delimiter the file seems to have, first.
        let found = if detected == b'\t' {
            "tab".to_string()
        } else {
            (detected as char).to_string()
        };
        if let Some(k) = choices.iter().position(|c| c.0 == found) {
            let c = choices.remove(k);
            choices.insert(0, c);
        }
        let items = choices
            .iter()
            .enumerate()
            .map(|(k, (v, t))| {
                let t = if k == 0 && *v == found {
                    format!("{t} (found)")
                } else {
                    (*t).to_string()
                };
                item(with("delimiter", v), &t, "Open as Workbook: delimiter")
            })
            .collect();
        return request(ctx, Request::Choose(items));
    };
    let Some(encoding) = args
        .get("encoding")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        let items = [
            ("utf-8", "UTF-8"),
            ("windows-1254", "Turkish (Windows-1254)"),
            ("iso-8859-9", "Turkish (ISO-8859-9)"),
            ("windows-1252", "Western (Windows-1252)"),
            ("utf-16le", "UTF-16 LE"),
            ("utf-16be", "UTF-16 BE"),
        ]
        .iter()
        .map(|(v, t)| item(with("encoding", v), t, "Open as Workbook: encoding"))
        .collect();
        return request(ctx, Request::Choose(items));
    };
    let enc = encoding_rs::Encoding::for_label(encoding.as_bytes())
        .ok_or_else(|| CommandError::new(format!("{encoding} is not an encoding")))?;
    let delimiter_byte = if delimiter == "tab" {
        b'\t'
    } else {
        delimiter.as_bytes().first().copied().unwrap_or(b',')
    };
    let dialect = crate::csv::Dialect {
        delimiter: delimiter_byte,
        quote: b'"',
        header: false,
        crlf: false,
    };
    // The text as the editor holds it when it has unsaved edits; else the
    // file's bytes read again in the encoding chosen (a new file's text
    // as it is).
    let text = {
        let d = ctx.doc()?;
        match path
            .as_ref()
            .filter(|_| !d.is_modified())
            .map(std::fs::read)
        {
            Some(Ok(bytes)) => enc.decode(&bytes).0.into_owned(),
            _ => d.text().as_str().to_owned(),
        }
    };
    let rows = crate::csv::rows(&text, &dialect);
    // How numbers are written: `1,234.56` or `1.234,56` (publish_todo
    // 3.7: the second was read as text).
    let Some(decimal) = args
        .get("decimal")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        let comma = crate::workbook_io::guess_decimal_comma(&rows, delimiter_byte);
        let mut choices = [(".", "Point: 1,234.56"), (",", "Comma: 1.234,56")];
        if comma {
            choices.swap(0, 1);
        }
        let items = choices
            .iter()
            .enumerate()
            .map(|(k, (v, t))| {
                let t = if k == 0 {
                    format!("{t} (found)")
                } else {
                    (*t).to_string()
                };
                item(with("decimal", v), &t, "Open as Workbook: decimals")
            })
            .collect();
        return request(ctx, Request::Choose(items));
    };
    let Some(types) = args
        .get("column types")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return request(
            ctx,
            Request::Ask {
                command: ID.into(),
                args: args.clone(),
                arg: "column types".into(),
            },
        );
    };
    let types = crate::workbook_io::parse_types(&types).map_err(CommandError::new)?;
    let Some(target) = args.get("path").and_then(Value::as_str).map(str::to_owned) else {
        let default = path.as_ref().map_or("Book.xlsx".into(), |p| {
            p.with_extension("xlsx").display().to_string()
        });
        let mut a = args.clone();
        a["path_default"] = Value::String(default);
        return request(
            ctx,
            Request::Ask {
                command: ID.into(),
                args: a,
                arg: "path".into(),
            },
        );
    };
    let mut target = std::path::PathBuf::from(crate::settings::expand_home(target.trim()));
    if target.is_relative()
        && let Some(dir) = path.as_ref().and_then(|p| p.parent())
    {
        target = dir.join(target);
    }
    // A workbook: `out.csv` would get a workbook's bytes otherwise.
    if !target
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
    {
        target.set_extension("xlsx");
    }
    if target.exists() && !arg_bool(args, "replace") {
        let name = target
            .file_name()
            .map_or(String::new(), |f| f.to_string_lossy().into_owned());
        let mut a = args.clone();
        a["path"] = Value::String(target.display().to_string());
        a["replace"] = Value::Bool(true);
        return request(
            ctx,
            Request::Choose(vec![item(
                a,
                &format!("Replace {name}"),
                &format!("{name} exists"),
            )]),
        );
    }
    let rows = crate::workbook_io::import_rows(&rows, &types, decimal == ",");
    let sheet: String = target
        .file_stem()
        .map_or("Sheet1".into(), |s| s.to_string_lossy().into_owned())
        .chars()
        .filter(|c| !"[]:*?/\\".contains(*c))
        .take(31)
        .collect();
    let bytes =
        crate::workbook_io::rows_to_xlsx(if sheet.is_empty() { "Sheet1" } else { &sheet }, &rows);
    std::fs::write(&target, bytes).map_err(|e| CommandError::new(e.to_string()))?;
    request(
        ctx,
        Request::Open {
            path: Some(target.display().to_string()),
        },
    )
}

fn request(ctx: &mut EditorContext<'_>, r: Request) -> CommandResult {
    ctx.requests.push(r);
    Ok(())
}

fn plain_commands() -> Vec<Command> {
    use org_edit::emphasis::Emphasis;
    use org_edit::headline as h;
    use org_edit::list as l;
    use org_edit::table as t;
    use org_edit::todo::{PriorityAction, TodoArg};
    vec![
        // The application and the frontend.
        cmd("app.save", "Save", "File", &["ctrl+s"], None, |ctx, _| {
            request(ctx, Request::Save)
        }),
        cmd(
            "export.html",
            "Export as HTML",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_doc(ctx, &org_export::Html, ".html", false),
        ),
        cmd(
            "export.htmlSubtree",
            "Export Subtree as HTML",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_doc(ctx, &org_export::Html, ".html", true),
        ),
        cmd(
            "export.htmlBrowser",
            "Export as HTML and Open",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| {
                let target = export_doc_to(ctx, &org_export::Html, ".html", false)?;
                if !ctx.config.bool("export.open_after") {
                    let url = file_url(&target);
                    ctx.requests
                        .push(Request::OpenLink(crate::input::LinkAction::Url(url)));
                }
                Ok(())
            },
        ),
        cmd(
            "export.markdown",
            "Export as Markdown",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_doc(ctx, &org_export::Markdown, ".md", false),
        ),
        cmd(
            "export.dialog",
            "Export…",
            "Export",
            &["ctrl+alt+e"],
            Some("editorMode == org"),
            |ctx, _| request(ctx, Request::ExportDialog),
        ),
        cmd(
            "export.toggleBodyOnly",
            "Toggle Export of the Body Only",
            "Export",
            &[],
            None,
            |ctx, _| {
                let v = !ctx.config.bool("export.body_only");
                export_setting(ctx, "export.body_only", v.into())
            },
        ),
        cmd(
            "export.toggleOpenAfter",
            "Toggle Opening Exported Files",
            "Export",
            &[],
            None,
            |ctx, _| {
                let v = !ctx.config.bool("export.open_after");
                export_setting(ctx, "export.open_after", v.into())
            },
        ),
        cmd(
            "export.toggleMath",
            "Toggle Formulas as MathJax or SVG",
            "Export",
            &[],
            None,
            |ctx, _| {
                let v = if ctx.config.str("export.math") == "svg" {
                    "mathjax"
                } else {
                    "svg"
                };
                export_setting(ctx, "export.math", v.into())
            },
        ),
        cmd(
            "export.gfm",
            "Export as GitHub Markdown",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_doc(ctx, &org_export::Gfm, ".md", false),
        ),
        cmd(
            "export.latex",
            "Export as LaTeX",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_doc(ctx, &LATEX, ".tex", false),
        ),
        cmd(
            "export.text",
            "Export as Plain Text",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| {
                let utf8 = ctx.config.str("export.text_charset") == "utf-8";
                let backend: &dyn org_export::Backend = if utf8 { &TEXT_UTF8 } else { &TEXT };
                export_doc(ctx, backend, ".txt", false)
            },
        ),
        cmd(
            "export.pdf",
            "Export as PDF (LaTeX)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_pdf(ctx, false),
        ),
        cmd(
            "file.print",
            "Print",
            "File",
            &[],
            Some("editorMode == org"),
            |ctx, _| pdf_then(ctx, false, true),
        ),
        cmd(
            "export.pdfSubtree",
            "Export Subtree as PDF (LaTeX)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_pdf(ctx, true),
        ),
        cmd(
            "export.docx",
            "Export as Word (pandoc)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_pandoc(ctx, crate::pandoc::Format::Docx),
        ),
        cmd(
            "export.odt",
            "Export as OpenDocument (pandoc)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_pandoc(ctx, crate::pandoc::Format::Odt),
        ),
        cmd(
            "export.epub",
            "Export as EPUB (pandoc)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_pandoc(ctx, crate::pandoc::Format::Epub),
        ),
        cmd(
            "export.rtf",
            "Export as RTF (pandoc)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_pandoc(ctx, crate::pandoc::Format::Rtf),
        ),
        cmd(
            "file.import",
            "Import as Org (pandoc)…",
            "File",
            &[],
            None,
            import_file,
        ),
        cmd(
            "export.latexSubtree",
            "Export Subtree as LaTeX",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_doc(ctx, &LATEX, ".tex", true),
        ),
        cmd(
            "export.markdownSubtree",
            "Export Subtree as Markdown",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, _| export_doc(ctx, &org_export::Markdown, ".md", true),
        ),
        cmd(
            "app.saveAs",
            "Save As",
            "File",
            &["ctrl+shift+s"],
            None,
            |ctx, _| request(ctx, Request::SaveAs),
        ),
        cmd(
            "app.newWorkbook",
            "New Workbook",
            "File",
            &[],
            None,
            new_workbook,
        ),
        cmd(
            "app.newFromTemplate",
            "New from Template",
            "File",
            &[],
            None,
            new_from_template,
        ),
        cmd("app.quit", "Quit", "File", &["ctrl+q"], None, |ctx, _| {
            request(ctx, Request::Quit)
        }),
        cmd(
            "app.settings",
            "Settings",
            "File",
            &["ctrl+,"],
            None,
            |ctx, _| request(ctx, Request::Settings),
        ),
        cmd(
            "app.revert",
            "Revert to Saved",
            "File",
            &[],
            None,
            |ctx, _| {
                let now = ctx.now;
                let doc = ctx
                    .document
                    .as_deref_mut()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-no-document")))?;
                doc.reload(now).map_err(|e| {
                    CommandError::new(crate::tr!("msg-cannot-reload", error = e.to_string()))
                })
            },
        ),
        cmd("edit.copy", "Copy", "Edit", &["ctrl+c"], None, |ctx, _| {
            // A viewer's document has no text: its picture is copied.
            if ctx.document.as_deref().is_some_and(|d| d.viewer.is_some()) {
                return crate::viewer::copy(ctx);
            }
            request(ctx, Request::Copy)
        }),
        cmd("edit.cut", "Cut", "Edit", &["ctrl+x"], None, |ctx, _| {
            // A spreadsheet's cells are cut to be moved where they are pasted.
            if ctx
                .document
                .as_deref()
                .and_then(|d| d.viewer.as_deref())
                .is_some_and(|v| v.is_grid())
            {
                return crate::viewer::cut(ctx);
            }
            request(ctx, Request::Cut)
        }),
        cmd(
            "file.reopenWithEncoding",
            "Reopen with Encoding",
            "File",
            &[],
            None,
            |ctx, args| {
                let name = arg_str(args, "encoding")?.to_string();
                let enc = crate::files::encoding_for(&name).ok_or_else(|| {
                    CommandError::new(crate::tr!("msg-unknown-encoding", name = name.clone()))
                })?;
                let now = ctx.now;
                let doc = ctx.doc()?;
                if doc.meta.path.is_none() {
                    return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
                }
                if doc.is_modified() {
                    return Err(CommandError::new(crate::l10n::tr("msg-reopen-modified")));
                }
                doc.reopen_with(enc, now)
                    .map_err(|e| CommandError::new(e.to_string()))?;
                let lossy = doc.meta.lossy;
                ctx.messages.push(if lossy {
                    crate::tr!("msg-opened-lossy", encoding = enc.name())
                } else {
                    crate::tr!("msg-reopened", encoding = enc.name())
                });
                Ok(())
            },
        ),
        cmd(
            "file.saveWithEncoding",
            "Save with Encoding",
            "File",
            &[],
            None,
            |ctx, args| {
                let name = arg_str(args, "encoding")?.to_string();
                let enc = crate::files::encoding_for(&name).ok_or_else(|| {
                    CommandError::new(crate::tr!("msg-unknown-encoding", name = name.clone()))
                })?;
                let doc = ctx.doc()?;
                if let Some(ch) = crate::files::unencodable(doc.text().as_str(), enc) {
                    return Err(CommandError::new(crate::tr!(
                        "msg-unencodable",
                        ch = ch.to_string(),
                        encoding = enc.name()
                    )));
                }
                let utf16 = enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE;
                doc.meta.bom = utf16 || (enc == encoding_rs::UTF_8 && doc.meta.bom);
                doc.meta.encoding = enc;
                doc.meta.lossy = false;
                // Saved as the Save command saves (a file name asked for if
                // there is none).
                request(ctx, Request::Save)
            },
        ),
        scoped(
            cmd(
                "lines.duplicate",
                "Duplicate Lines",
                "Edit",
                &["ctrl+shift+d"],
                None,
                |ctx, _| lines_command(ctx, |t, s| Some(crate::lines::duplicate(t, s))),
            ),
            crate::command::Scope::except(&["org"]),
        ),
        scoped(
            cmd(
                "lines.moveUp",
                "Move Lines Up",
                "Edit",
                &["alt+up"],
                None,
                |ctx, _| lines_command(ctx, |t, s| crate::lines::move_lines(t, s, true)),
            ),
            // A CSV grid moves its rows on these keys, records that span
            // lines whole.
            crate::command::Scope::except(&["org", "csv"]),
        ),
        scoped(
            cmd(
                "lines.moveDown",
                "Move Lines Down",
                "Edit",
                &["alt+down"],
                None,
                |ctx, _| lines_command(ctx, |t, s| crate::lines::move_lines(t, s, false)),
            ),
            // A CSV grid moves its rows on these keys, records that span
            // lines whole.
            crate::command::Scope::except(&["org", "csv"]),
        ),
        cmd("lines.join", "Join Lines", "Edit", &[], None, |ctx, _| {
            lines_command(ctx, crate::lines::join)
        }),
        cmd(
            "lines.sort",
            "Sort Lines",
            "Edit",
            &[],
            None,
            |ctx, args| {
                let reverse = arg_bool(args, "reverse");
                lines_command(ctx, |t, s| crate::lines::sort(t, s, reverse))
            },
        ),
        cmd(
            "edit.trimTrailingWhitespace",
            "Trim Trailing Whitespace",
            "Edit",
            &[],
            None,
            |ctx, _| lines_command(ctx, |t, _| crate::lines::trim_trailing(t)),
        ),
        cmd(
            "edit.trimTrailingBlankLines",
            "Delete Trailing Blank Lines",
            "Edit",
            &[],
            None,
            |ctx, _| lines_command(ctx, |t, _| crate::lines::trim_trailing_blank_lines(t)),
        ),
        cmd(
            "edit.formatDocument",
            "Format Document",
            "Edit",
            &[],
            Some("editorMode == org || editorMode == latex || hasFormatter"),
            |ctx, _| {
                // The language server's formatting (D57); its edits come
                // back through `lsp::take_outcomes`.
                if crate::lsp::can(ctx.doc()?, crate::lsp::Kind::Format) {
                    crate::lsp::request(ctx.doc()?, crate::lsp::Kind::Format)
                        .map_err(CommandError::new)?;
                    return Ok(());
                }
                // Without a server that formats: the language plugin's
                // formatter command, in the background.
                if crate::lsp::has_format_command(ctx.doc()?) {
                    crate::lsp::format_with_command(ctx.doc()?).map_err(CommandError::new)?;
                    return Ok(());
                }
                // A language pack's formatter (T2.7a.7); a syntax error
                // refuses, at its place.
                if let Some(f) = crate::packs::format(ctx.doc()?) {
                    let new = match f {
                        crate::packs::Formatted::Text(t) => t,
                        crate::packs::Formatted::Refused(d) => {
                            let at = d.range.start;
                            ctx.doc()?.selection = org_edit::Selection::caret(at);
                            return Err(CommandError::new(d.message));
                        }
                    };
                    return lines_command(ctx, |t, _| {
                        crate::lines::replace_differing(t, &new, "Format Document")
                    });
                }
                let latex = ctx.doc()?.meta.mode == crate::DocumentMode::Latex;
                lines_command(ctx, |t, _| {
                    // As `kalem fmt` does.
                    let new = if latex {
                        crate::latex_fmt::format(t, false)
                    } else {
                        org_edit::format::format(&org_model::Document::new(org_syntax::parse(t)))
                    };
                    crate::lines::replace_differing(t, &new, "Format Document")
                })
            },
        ),
        cmd(
            "edit.selectWord",
            "Select Word",
            "Edit",
            &[],
            None,
            |ctx, _| {
                let d = ctx.doc()?;
                if let Some(w) = crate::lines::word_at(d.text().as_str(), d.selection.head) {
                    d.selection = org_edit::Selection {
                        anchor: w.start,
                        head: w.end,
                    };
                }
                Ok(())
            },
        ),
        cmd(
            "edit.expandSelection",
            "Expand Selection",
            "Edit",
            &["ctrl+alt+right"],
            None,
            |ctx, _| {
                ctx.doc()?.expand_selection();
                Ok(())
            },
        ),
        cmd(
            "edit.shrinkSelection",
            "Shrink Selection",
            "Edit",
            &["ctrl+alt+left"],
            None,
            |ctx, _| {
                ctx.doc()?.shrink_selection();
                Ok(())
            },
        ),
        cmd(
            "edit.complete",
            "Complete",
            "Edit",
            &["ctrl+space", "alt+/"],
            None,
            |ctx, _| request(ctx, Request::Complete),
        ),
        // Where a language has comments: not CSV, plain text or listings.
        scoped(
            cmd(
                "edit.toggleComment",
                "Toggle Comment",
                "Edit",
                &["ctrl+alt+c"],
                Some("hasComments"),
                |ctx, _| {
                    let now = ctx.now;
                    let d = ctx.doc()?;
                    let style = crate::code::language_at(d)
                        .and_then(|l| crate::code::comment_style(&l))
                        .ok_or_else(|| {
                            CommandError::new(crate::l10n::tr("msg-no-comment-style"))
                        })?;
                    let tx = crate::code::toggle_comment(d.text().as_str(), d.selection, style);
                    d.apply(&tx, org_edit::ChangeKind::Command, now);
                    Ok(())
                },
            ),
            crate::command::Scope::except(&["csv", "text", "directory", "binary"]),
        ),
        cmd(
            "edit.gotoBracket",
            "Go to Matching Bracket",
            "Edit",
            &["ctrl+alt+b"],
            None,
            |ctx, _| {
                let d = ctx.doc()?;
                let head = d.selection.head;
                let Some((open, close)) = crate::code::matching(d.text().as_str(), head) else {
                    ctx.messages.push(crate::l10n::tr("msg-no-bracket"));
                    return Ok(());
                };
                // To the other one: after a closing bracket, before an
                // opening one.
                let target = if head == open || head == open + 1 {
                    close + 1
                } else {
                    open
                };
                d.move_cursor(target, false);
                Ok(())
            },
        ),
        cmd(
            "cursor.addBelow",
            "Add Cursor Below",
            "Edit",
            &["ctrl+alt+down"],
            None,
            |ctx, _| {
                ctx.doc()?.add_cursor_vertical(false);
                Ok(())
            },
        ),
        cmd(
            "cursor.addAbove",
            "Add Cursor Above",
            "Edit",
            &["ctrl+alt+up"],
            None,
            |ctx, _| {
                ctx.doc()?.add_cursor_vertical(true);
                Ok(())
            },
        ),
        cmd(
            "selection.addNextOccurrence",
            "Add Next Occurrence",
            "Edit",
            &["ctrl+d"],
            None,
            |ctx, _| {
                if !ctx.doc()?.add_next_occurrence() {
                    ctx.messages
                        .push(crate::l10n::tr("msg-no-more-occurrences"));
                }
                Ok(())
            },
        ),
        cmd(
            "selection.allOccurrences",
            "Select All Occurrences",
            "Edit",
            &[],
            None,
            |ctx, _| {
                let n = ctx.doc()?.select_all_occurrences();
                ctx.messages
                    .push(crate::tr!("msg-occurrences-selected", count = n));
                Ok(())
            },
        ),
        cmd(
            "selection.columnDown",
            "Column Selection Down",
            "Edit",
            &["ctrl+alt+shift+down"],
            None,
            |ctx, _| {
                ctx.doc()?.extend_column(false);
                Ok(())
            },
        ),
        cmd(
            "selection.columnUp",
            "Column Selection Up",
            "Edit",
            &["ctrl+alt+shift+up"],
            None,
            |ctx, _| {
                ctx.doc()?.extend_column(true);
                Ok(())
            },
        ),
        cmd(
            "cursor.clearExtra",
            "Single Cursor",
            "Edit",
            &[],
            None,
            |ctx, _| {
                ctx.doc()?.clear_extra();
                Ok(())
            },
        ),
        cmd(
            "edit.gotoLine",
            "Go to Line",
            "Edit",
            &[],
            None,
            |ctx, args| {
                let line = args
                    .get("line")
                    .and_then(|v| v.as_u64().or_else(|| v.as_str()?.trim().parse().ok()))
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-bad-line")))?;
                let doc = ctx.doc()?;
                let n = doc.text().line_count();
                let line = (line as usize).clamp(1, n.max(1)) - 1;
                let pos = doc.text().line_start(line);
                doc.move_cursor(pos, false);
                Ok(())
            },
        ),
        cmd(
            "stats.chapters",
            "Word Count by Chapter",
            "View",
            &[],
            Some(ORG),
            |ctx, _| {
                let doc = ctx.doc()?;
                let Some((parse, _)) = doc.parse() else {
                    return Err(CommandError::new(crate::l10n::tr("msg-not-org")));
                };
                let root = parse.syntax();
                let chapters = crate::stats::chapters(&root);
                if chapters.is_empty() {
                    ctx.messages.push(crate::l10n::tr("msg-no-headings"));
                    return Ok(());
                }
                let text = doc.text();
                let items = chapters
                    .iter()
                    .map(|c| crate::palette::PaletteItem {
                        id: crate::palette::invocation(
                            "edit.gotoLine",
                            &serde_json::json!({ "line": text.line_of(c.start) + 1 }),
                        ),
                        title: format!("{}{}", "   ".repeat(c.level - 1), c.title),
                        category: match c.target {
                            Some(t) => crate::stats::progress(c.words, t),
                            None => crate::stats::thousands(c.words),
                        },
                        keys: String::new(),
                        also: String::new(),
                    })
                    .collect();
                ctx.requests.push(Request::Choose(items));
                Ok(())
            },
        ),
        cmd(
            "stats.setDocumentTarget",
            "Set Document Word Target",
            "View",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "words")?.to_string();
                let target = match v.trim() {
                    "" | "0" => None,
                    w => Some(crate::stats::parse_target(w).ok_or_else(|| {
                        CommandError::new(crate::l10n::tr("msg-bad-word-target"))
                    })?),
                };
                ctx.org(|d, _, _| {
                    let root = d.parse().syntax();
                    let text = root.to_string();
                    Ok(crate::stats::set_document_target(&root, &text, target))
                })
            },
        ),
        cmd(
            "stats.setSectionTarget",
            "Set Section Word Target",
            "View",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "words")?.trim().to_string();
                if v.is_empty() || v == "0" {
                    return ctx.org(|d, p, _| {
                        org_edit::property::delete_property(d, p, "WORD_TARGET").ok_or_else(|| {
                            org_edit::EditError {
                                message: crate::l10n::tr("msg-no-word-target"),
                                point: None,
                            }
                        })
                    });
                }
                let n = crate::stats::parse_target(&v)
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-bad-word-target")))?;
                ctx.org(|d, p, _| {
                    org_edit::property::set_property(d, p, "WORD_TARGET", &n.to_string(), false)
                })
            },
        ),
        cmd(
            "edit.copyRichText",
            "Copy as Rich Text",
            "Edit",
            &[],
            Some("editorMode == org || editorMode == markdown"),
            |ctx, _| {
                let doc = ctx.doc()?;
                let text = crate::rich_copy::selection_text(doc);
                let path = doc.meta.path.clone();
                let html = if doc.meta.mode == crate::DocumentMode::Markdown {
                    crate::markdown::to_html(&text)
                } else {
                    crate::rich_copy::html(&text, path.as_deref()).map_err(CommandError::new)?
                };
                ctx.requests.push(Request::CopyRich { html, text });
                Ok(())
            },
        ),
        cmd(
            "edit.copyHtml",
            "Copy as HTML",
            "Edit",
            &[],
            Some("editorMode == org || editorMode == markdown"),
            |ctx, _| {
                let doc = ctx.doc()?;
                let text = crate::rich_copy::selection_text(doc);
                let path = doc.meta.path.clone();
                let html = if doc.meta.mode == crate::DocumentMode::Markdown {
                    crate::markdown::to_html(&text)
                } else {
                    crate::rich_copy::html(&text, path.as_deref()).map_err(CommandError::new)?
                };
                ctx.requests.push(Request::CopyText(html));
                Ok(())
            },
        ),
        // Pictures left in the pictures folder after their links were
        // deleted, moved to the Trash (never deleted for good: undo in the
        // document can still want them, and the Trash gives them back).
        cmd(
            "file.removeUnusedImages",
            "Remove Unused Images",
            "File",
            &[],
            Some("editorMode == org || editorMode == markdown"),
            |ctx, _| {
                let doc = ctx.doc()?;
                let Some(style) = crate::images::LinkStyle::of(&doc.meta.mode) else {
                    return Ok(());
                };
                let path = doc
                    .meta
                    .path
                    .clone()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-picture-needs-file")))?;
                let unused = crate::images::unused(&path, doc.text().as_str(), style);
                if unused.is_empty() {
                    ctx.messages.push(crate::tr!("msg-no-unused-images"));
                    return Ok(());
                }
                kalem_fs::trash_paths(&unused).map_err(CommandError::new)?;
                let names: Vec<String> = unused
                    .iter()
                    .filter_map(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .collect();
                ctx.messages.push(crate::tr!(
                    "msg-unused-images-trashed",
                    count = unused.len() as i64,
                    names = names.join(", ")
                ));
                Ok(())
            },
        ),
        cmd(
            "edit.paste",
            "Paste",
            "Edit",
            &["ctrl+v"],
            None,
            |ctx, _| request(ctx, Request::Paste { plain: false }),
        ),
        cmd(
            "edit.pastePlain",
            "Paste as Plain Text",
            "Edit",
            &["ctrl+shift+v"],
            None,
            |ctx, _| request(ctx, Request::Paste { plain: true }),
        ),
        cmd(
            "edit.selectAll",
            "Select All",
            "Edit",
            &["ctrl+a"],
            None,
            |ctx, _| {
                let d = ctx.doc()?;
                d.selection = org_edit::Selection {
                    anchor: 0,
                    head: d.text().len(),
                };
                Ok(())
            },
        ),
        cmd(
            "org.link.open",
            "Open Link",
            "Links",
            &[],
            Some(ORG),
            |ctx, _| {
                use crate::input::LinkAction;
                let doc = ctx.doc()?;
                let model = doc
                    .model()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
                match crate::input::link_at(&model, doc.selection.head) {
                    Some(LinkAction::Jump(p)) => {
                        doc.move_cursor(p, false);
                        Ok(())
                    }
                    Some(LinkAction::Missing(s)) => Err(CommandError::new(crate::tr!(
                        "msg-no-match-for",
                        target = s
                    ))),
                    Some(other) => request(ctx, Request::OpenLink(other)),
                    None => Err(CommandError::new(crate::tr!("msg-no-link"))),
                }
            },
        ),
        cmd(
            "edit.enter",
            "New Line or Item",
            "Edit",
            &["enter"],
            Some(ORG),
            |ctx, _| ctx.org(crate::input::enter),
        ),
        cmd(
            "edit.newline",
            "Line Break",
            "Edit",
            &["shift+enter"],
            None,
            |ctx, _| {
                let now = ctx.now;
                let d = ctx.doc()?;
                if !d.extra.is_empty() {
                    d.insert_text("\n", now);
                    return Ok(());
                }
                let s = d.selection;
                let tx = match &d.meta.mode {
                    // Code: a level deeper after an opening bracket.
                    crate::DocumentMode::Text { language: Some(l) } => crate::code::newline(
                        d.text().as_str(),
                        s,
                        &crate::code::indent_text(d),
                        Some(l.as_str()),
                    ),
                    // CSV: leading tabs are empty fields and leading blanks
                    // part of a value, not indentation to copy.
                    crate::DocumentMode::Csv => {
                        d.insert_text("\n", now);
                        return Ok(());
                    }
                    _ => {
                        let mark = (s.anchor != s.head).then_some(s.anchor);
                        crate::input::newline(d.text().as_str(), s.head, mark)
                    }
                };
                d.apply(&tx, org_edit::ChangeKind::Command, now);
                Ok(())
            },
        ),
        // Open documents and projects (§2.8, D12).
        cmd(
            "file.open",
            "Open File",
            "File",
            &["ctrl+o"],
            None,
            |ctx, args| {
                let path = args.get("path").and_then(Value::as_str).map(str::to_string);
                // A typed path, from the document's folder, instead of the
                // system's dialog (Doom's `SPC .`): a new name makes a file.
                if path.is_none() && args.get("prompt").and_then(Value::as_bool) == Some(true) {
                    return request(
                        ctx,
                        Request::Ask {
                            command: "file.open".into(),
                            args: serde_json::json!({}),
                            arg: "path".into(),
                        },
                    );
                }
                request(ctx, Request::Open { path })
            },
        ),
        cmd(
            "file.new",
            "New Document",
            "File",
            &["ctrl+n"],
            None,
            |ctx, _| request(ctx, Request::New),
        ),
        cmd(
            "file.newFromTemplate",
            "New from Template…",
            "File",
            &[],
            None,
            |ctx, args| {
                let Some(name) = args.get("template").and_then(Value::as_str) else {
                    let items = crate::latex_templates::TEMPLATES
                        .iter()
                        .map(|t| crate::palette::PaletteItem {
                            id: crate::palette::invocation(
                                "file.newFromTemplate",
                                &serde_json::json!({ "template": t.name }),
                            ),
                            title: t.title.to_string(),
                            category: "LaTeX".into(),
                            keys: String::new(),
                            also: String::new(),
                        })
                        .collect();
                    return request(ctx, Request::Choose(items));
                };
                let t = crate::latex_templates::find(name).ok_or_else(|| {
                    CommandError::new(crate::tr!("msg-unknown-template", name = name))
                })?;
                // A copy beside the document (or in the working folder).
                let dir = match args.get("path").and_then(Value::as_str) {
                    Some(p) => std::path::PathBuf::from(p),
                    None => ctx
                        .document
                        .as_deref()
                        .and_then(|d| d.meta.path.as_deref())
                        .and_then(std::path::Path::parent)
                        .map(std::path::Path::to_path_buf)
                        .or_else(|| std::env::current_dir().ok())
                        .unwrap_or_default(),
                };
                let path = if dir.extension().is_some_and(|e| e == "tex") {
                    dir
                } else {
                    crate::latex_templates::free_path(&dir, t.name)
                };
                if path.exists() {
                    return Err(CommandError::new(crate::tr!(
                        "msg-import-exists",
                        path = path.display().to_string()
                    )));
                }
                std::fs::write(&path, t.text).map_err(|e| CommandError::new(e.to_string()))?;
                request(
                    ctx,
                    Request::Open {
                        path: Some(path.display().to_string()),
                    },
                )
            },
        ),
        cmd(
            "file.close",
            "Close Document",
            "File",
            &["ctrl+w"],
            None,
            |ctx, _| request(ctx, Request::Close),
        ),
        cmd("file.saveAll", "Save All", "File", &[], None, |ctx, _| {
            request(ctx, Request::Documents(DocumentsRequest::SaveAll))
        }),
        cmd(
            "file.closeOthers",
            "Close Other Documents",
            "File",
            &[],
            None,
            |ctx, _| request(ctx, Request::Documents(DocumentsRequest::CloseOthers)),
        ),
        cmd(
            "file.closeAll",
            "Close All Documents",
            "File",
            &[],
            None,
            |ctx, _| request(ctx, Request::Documents(DocumentsRequest::CloseAll)),
        ),
        cmd("file.last", "Last Document", "File", &[], None, |ctx, _| {
            request(ctx, Request::Documents(DocumentsRequest::Last))
        }),
        cmd(
            "file.bury",
            "Move Document to the End",
            "File",
            &[],
            None,
            |ctx, _| request(ctx, Request::Documents(DocumentsRequest::Bury)),
        ),
        cmd(
            "file.scratch",
            "Scratch Document",
            "File",
            &[],
            None,
            |ctx, args| {
                let project = args.get("project").and_then(Value::as_bool) == Some(true);
                request(
                    ctx,
                    Request::Documents(DocumentsRequest::Scratch { project }),
                )
            },
        ),
        cmd(
            "file.copyText",
            "Copy the Whole Document",
            "File",
            &[],
            None,
            |ctx, _| {
                let text = ctx.doc()?.text().as_str().to_string();
                request(ctx, Request::CopyText(text))
            },
        ),
        // This file (Doom's `SPC f`, T2.7i.3).
        cmd(
            "file.delete",
            "Delete This File",
            "File",
            &[],
            Some("hasFile"),
            |ctx, _| {
                let path = this_file(ctx)?;
                request(
                    ctx,
                    Request::FileOp(crate::command::FileOp {
                        kind: kalem_fs::OpKind::Trash,
                        sources: vec![path],
                        target: None,
                    }),
                )
            },
        ),
        cmd(
            "file.rename",
            "Rename or Move This File",
            "File",
            &[],
            Some("hasFile"),
            |ctx, args| this_file_to(ctx, args, kalem_fs::OpKind::Move),
        ),
        cmd(
            "file.copy",
            "Copy This File To",
            "File",
            &[],
            Some("hasFile"),
            |ctx, args| this_file_to(ctx, args, kalem_fs::OpKind::Copy),
        ),
        cmd(
            "file.copyPath",
            "Copy This File's Path",
            "File",
            &[],
            Some("hasFile"),
            |ctx, _| {
                let text = this_file(ctx)?.display().to_string();
                ctx.messages.push(text.clone());
                request(ctx, Request::CopyText(text))
            },
        ),
        cmd(
            "file.copyRelativePath",
            "Copy This File's Path from the Project",
            "File",
            &[],
            Some("hasFile"),
            |ctx, _| {
                let path = this_file(ctx)?;
                let dir = path
                    .parent()
                    .map(std::path::Path::to_path_buf)
                    .unwrap_or_default();
                let base = kalem_project::list::detect_root(&dir).unwrap_or(dir);
                let text = crate::kinds::relative(&base, &path)
                    .unwrap_or_else(|| path.display().to_string());
                ctx.messages.push(text.clone());
                request(ctx, Request::CopyText(text))
            },
        ),
        cmd(
            "file.openSettingsFolder",
            "Open the Settings Folder",
            "File",
            &[],
            None,
            |ctx, _| {
                let dir = crate::settings::config_dir()
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-config-dir")))?;
                let _ = std::fs::create_dir_all(&dir);
                let path = Some(dir.display().to_string());
                request(ctx, Request::Open { path })
            },
        ),
        cmd(
            "file.openKeymap",
            "Open Your Keymap",
            "File",
            &[],
            None,
            |ctx, _| {
                let dir = crate::settings::config_dir()
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-config-dir")))?;
                let _ = std::fs::create_dir_all(&dir);
                let path = Some(dir.join("keymap.json").display().to_string());
                request(ctx, Request::Open { path })
            },
        ),
        cmd(
            "file.openWorkspaceSettings",
            "Open the Workspace Settings",
            "File",
            &[],
            None,
            |ctx, _| {
                let doc = ctx.doc()?;
                let dir = crate::command::folder_of(doc)
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-file")))?;
                // The nearest one, else a new one at the project's root.
                let path = crate::settings::find_workspace_settings(&dir).unwrap_or_else(|| {
                    let root = kalem_project::list::detect_root(&dir).unwrap_or(dir);
                    let folder = root.join(".kalem");
                    let _ = std::fs::create_dir_all(&folder);
                    folder.join("settings.toml")
                });
                request(
                    ctx,
                    Request::Open {
                        path: Some(path.display().to_string()),
                    },
                )
            },
        ),
        cmd(
            "file.next",
            "Next Document",
            "File",
            &["ctrl+pagedown"],
            None,
            |ctx, _| request(ctx, Request::Cycle { back: false }),
        ),
        cmd(
            "file.previous",
            "Previous Document",
            "File",
            &["ctrl+pageup"],
            None,
            |ctx, _| request(ctx, Request::Cycle { back: true }),
        ),
        cmd(
            "file.switch",
            "Switch Document",
            "File",
            &["ctrl+alt+o"],
            None,
            |ctx, _| request(ctx, Request::Pick(PickKind::Documents)),
        ),
        cmd(
            "file.recent",
            "Open Recent File",
            "File",
            &["ctrl+alt+r"],
            None,
            |ctx, _| request(ctx, Request::Pick(PickKind::RecentFiles)),
        ),
        cmd(
            "view.openFiles",
            "Open Files",
            "View",
            &["ctrl+shift+e"],
            None,
            |ctx, _| request(ctx, Request::OpenFiles),
        ),
        cmd(
            "view.toggleFolderTree",
            "Toggle Folder Tree",
            "View",
            &[],
            None,
            |ctx, _| {
                let v = !ctx.config.bool("ui.folder_tree");
                request(
                    ctx,
                    Request::SetSetting {
                        key: "ui.folder_tree".into(),
                        value: v.into(),
                        quiet: false,
                    },
                )
            },
        ),
        cmd(
            "view.revealInTree",
            "Reveal in Folder Tree",
            "View",
            &[],
            Some("inProject"),
            |ctx, _| request(ctx, Request::Project(ProjectRequest::RevealInTree)),
        ),
        cmd(
            "project.switch",
            "Switch Project",
            "Project",
            &["ctrl+alt+p"],
            None,
            |ctx, _| request(ctx, Request::Pick(PickKind::Projects)),
        ),
        cmd(
            "project.findFile",
            "Find File in Project",
            "Project",
            &["ctrl+p"],
            None,
            |ctx, _| request(ctx, Request::Pick(PickKind::ProjectFiles)),
        ),
        cmd(
            "project.search",
            "Search in Project",
            "Project",
            &["ctrl+shift+f"],
            None,
            |ctx, _| request(ctx, Request::SearchProject),
        ),
        // Doom's `SPC s` (T2.7i.4).
        cmd(
            "search.lines",
            "Search Lines",
            "Search",
            &[],
            None,
            |ctx, args| {
                let flag = |k: &str| args.get(k).and_then(Value::as_bool) == Some(true);
                let text = if flag("word") {
                    word_or_selection(ctx)?
                } else {
                    String::new()
                };
                request(
                    ctx,
                    Request::SearchLines {
                        all: flag("all"),
                        headings: flag("headings"),
                        text,
                    },
                )
            },
        ),
        cmd(
            "search.folder",
            "Search in Folder",
            "Search",
            &[],
            None,
            |ctx, args| {
                let dir = match args.get("path").and_then(Value::as_str) {
                    Some(p) => std::path::PathBuf::from(crate::settings::expand_home(p)),
                    None if args.get("ask").and_then(Value::as_bool) == Some(true) => {
                        return request(
                            ctx,
                            Request::Ask {
                                command: "search.folder".into(),
                                args: serde_json::json!({}),
                                arg: "path".into(),
                            },
                        );
                    }
                    None => crate::command::folder_of(ctx.doc()?)
                        .or_else(|| std::env::current_dir().ok())
                        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-file")))?,
                };
                if !dir.is_dir() {
                    return Err(CommandError::new(crate::tr!(
                        "msg-not-a-folder",
                        path = dir.display().to_string()
                    )));
                }
                request(ctx, Request::SearchIn(dir))
            },
        ),
        // Doom's `SPC p` (T2.7i.8).
        cmd(
            "project.browseOther",
            "Browse Another Project",
            "Project",
            &[],
            None,
            |ctx, _| request(ctx, Request::PickProject(crate::projects::After::Browse)),
        ),
        cmd(
            "project.findFileOther",
            "Find File in Another Project",
            "Project",
            &[],
            None,
            |ctx, _| {
                request(
                    ctx,
                    Request::PickProject(crate::projects::After::Pick(PickKind::ProjectFiles)),
                )
            },
        ),
        cmd(
            "project.shellCommand",
            "Shell Command at the Project",
            "Project",
            &[],
            None,
            |ctx, args| {
                let command = args
                    .get("command")
                    .and_then(Value::as_str)
                    .ok_or_else(|| CommandError::new("command"))?
                    .to_string();
                let dir = crate::command::folder_of(ctx.doc()?)
                    .or_else(|| std::env::current_dir().ok())
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-file")))?;
                let dir = kalem_project::list::detect_root(&dir).unwrap_or(dir);
                request(
                    ctx,
                    Request::Shell(crate::command::ShellOp {
                        command,
                        files: Vec::new(),
                        dir,
                    }),
                )
            },
        ),
        cmd(
            "project.searchWord",
            "Search Project for Word",
            "Project",
            &[],
            None,
            |ctx, _| {
                // Doom's `SPC *`: the word at the cursor (the selection).
                let d = ctx.doc()?;
                let text = d.text().as_str();
                let (a, b) = (
                    d.selection.anchor.min(d.selection.head),
                    d.selection.anchor.max(d.selection.head),
                );
                let word = if a < b {
                    text[a..b].to_string()
                } else {
                    crate::lines::word_at(text, a)
                        .map(|r| text[r].to_string())
                        .unwrap_or_default()
                };
                if word.trim().is_empty() {
                    return Err(CommandError::new(crate::tr!("msg-no-word")));
                }
                request(ctx, Request::SearchProjectFor(word))
            },
        ),
        cmd(
            "bookmark.set",
            "Set Bookmark",
            "Bookmarks",
            &[],
            None,
            |ctx, args| {
                let d = ctx.doc()?;
                let path = d
                    .meta
                    .path
                    .clone()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-csv-save-first")))?;
                let path = std::path::absolute(&path).unwrap_or(path);
                let text = d.text();
                let at = d.selection.head.min(text.len());
                let line = text.line_of(at);
                let range = text.line_range(line);
                let context = text.as_str()[range.clone()].to_string();
                let stem = path
                    .file_name()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let name = args
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|n| !n.is_empty())
                    .map_or_else(|| format!("{stem}:{}", line + 1), str::to_string);
                crate::bookmarks::set(crate::bookmarks::Bookmark {
                    name: name.clone(),
                    path,
                    line: line as u64 + 1,
                    column: at - range.start,
                    context,
                })
                .map_err(CommandError::new)?;
                ctx.messages
                    .push(crate::tr!("msg-bookmark-set", name = name.as_str()));
                Ok(())
            },
        ),
        cmd(
            "bookmark.jump",
            "Jump to Bookmark",
            "Bookmarks",
            &[],
            None,
            |ctx, _| bookmark_choice(ctx, "bookmark.goto"),
        ),
        cmd(
            "bookmark.delete",
            "Delete Bookmark",
            "Bookmarks",
            &[],
            None,
            |ctx, args| match args.get("name").and_then(Value::as_str) {
                Some(name) => {
                    if crate::bookmarks::delete(name).map_err(CommandError::new)? {
                        ctx.messages
                            .push(crate::tr!("msg-bookmark-deleted", name = name));
                    }
                    Ok(())
                }
                None => bookmark_choice(ctx, "bookmark.delete"),
            },
        ),
        cmd(
            "bookmark.goto",
            "Go to Bookmark",
            "Bookmarks",
            &[],
            None,
            |ctx, args| {
                let name = arg_str(args, "name")?.to_string();
                let b = crate::bookmarks::load()
                    .into_iter()
                    .find(|b| b.name == name)
                    .ok_or_else(|| {
                        CommandError::new(crate::tr!("msg-no-match-for", target = name.as_str()))
                    })?;
                let line = std::fs::read_to_string(&b.path)
                    .map_or(b.line, |t| crate::bookmarks::line_now(&b, &t));
                request(
                    ctx,
                    Request::OpenLink(crate::input::LinkAction::File {
                        path: b.path.display().to_string(),
                        search: Some(line.to_string()),
                    }),
                )
            },
        ),
        cmd(
            "project.todos",
            "Project TODOs",
            "Project",
            &[],
            None,
            |ctx, _| request(ctx, Request::SearchProjectFor("TODO".into())),
        ),
        cmd(
            "file.other",
            "Other File",
            "File",
            &[],
            Some("hasFile"),
            |ctx, _| {
                let path = this_file(ctx)?;
                let other = other_file(&path)
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-other-file")))?;
                let text = matches!(
                    other.extension().and_then(|e| e.to_str()),
                    Some("org" | "md" | "tex" | "txt")
                );
                if text {
                    request(
                        ctx,
                        Request::Open {
                            path: Some(other.display().to_string()),
                        },
                    )
                } else {
                    request(
                        ctx,
                        Request::OpenLink(crate::input::LinkAction::System(other)),
                    )
                }
            },
        ),
        cmd(
            "project.searchOther",
            "Search in Another Project",
            "Project",
            &[],
            None,
            |ctx, _| request(ctx, Request::SearchOtherProject),
        ),
        cmd(
            "search.online",
            "Search Online",
            "Search",
            &[],
            None,
            |ctx, _| {
                let words = word_or_selection(ctx)?;
                if words.trim().is_empty() {
                    return Err(CommandError::new(crate::l10n::tr("msg-nothing-selected")));
                }
                let url = ctx
                    .config
                    .str("search.online_url")
                    .replace("%s", &url_encode(words.trim()));
                request(ctx, Request::OpenLink(crate::input::LinkAction::Url(url)))
            },
        ),
        cmd(
            "project.recentFiles",
            "Recent Files in Project",
            "Project",
            &[],
            None,
            |ctx, _| request(ctx, Request::Pick(PickKind::ProjectRecentFiles)),
        ),
        cmd(
            "project.switchDocument",
            "Switch Document in Project",
            "Project",
            &[],
            Some("inProject"),
            |ctx, _| request(ctx, Request::Pick(PickKind::ProjectDocuments)),
        ),
        cmd(
            "project.add",
            "Add Project",
            "Project",
            &[],
            None,
            |ctx, args| {
                let path = args.get("path").and_then(Value::as_str).map(str::to_string);
                request(ctx, Request::Project(ProjectRequest::Add(path)))
            },
        ),
        cmd(
            "project.remove",
            "Remove Project",
            "Project",
            &[],
            None,
            |ctx, args| match args.get("path").and_then(Value::as_str) {
                Some(p) => request(
                    ctx,
                    Request::Project(ProjectRequest::Remove(std::path::PathBuf::from(p))),
                ),
                None => request(ctx, Request::Pick(PickKind::RemoveProject)),
            },
        ),
        cmd(
            "project.addFolder",
            "Add Project Folder…",
            "Project",
            &[],
            None,
            |ctx, _| request(ctx, Request::Project(ProjectRequest::AddChosen)),
        ),
        cmd(
            "project.rename",
            "Rename Project",
            "Project",
            &[],
            Some("inProject"),
            |ctx, args| {
                let name = arg_str(args, "name")?.to_string();
                request(ctx, Request::Project(ProjectRequest::Rename(name)))
            },
        ),
        cmd(
            "project.refresh",
            "Refresh Project Files",
            "Project",
            &[],
            Some("inProject"),
            |ctx, _| request(ctx, Request::Project(ProjectRequest::Refresh)),
        ),
        cmd(
            "project.saveDocuments",
            "Save Project Documents",
            "Project",
            &[],
            Some("inProject"),
            |ctx, _| request(ctx, Request::Project(ProjectRequest::SaveAll)),
        ),
        cmd(
            "project.closeDocuments",
            "Close Project Documents",
            "Project",
            &[],
            Some("inProject"),
            |ctx, _| request(ctx, Request::Project(ProjectRequest::CloseAll)),
        ),
        cmd(
            "view.palette",
            "Command Palette",
            "View",
            &["ctrl+shift+p", "f1"],
            None,
            |ctx, _| request(ctx, Request::Palette),
        ),
        cmd("view.menus", "Menus", "View", &["f10"], None, |ctx, _| {
            request(ctx, Request::Menus)
        }),
        cmd("find.open", "Find", "Find", &["ctrl+f"], None, |ctx, _| {
            request(ctx, Request::Find { replace: false })
        }),
        cmd(
            "find.replace",
            "Find and Replace",
            "Find",
            &["ctrl+h"],
            None,
            |ctx, _| request(ctx, Request::Find { replace: true }),
        ),
        // Workspaces (Doom's `SPC TAB`, T2.7i.15).
        cmd(
            "workspace.list",
            "Switch Workspace",
            "Workspace",
            &[],
            None,
            |ctx, _| request(ctx, Request::Workspace(WorkspaceOp::List)),
        ),
        cmd(
            "workspace.new",
            "New Workspace",
            "Workspace",
            &[],
            None,
            |ctx, _| request(ctx, Request::Workspace(WorkspaceOp::New(None))),
        ),
        cmd(
            "workspace.newNamed",
            "New Named Workspace",
            "Workspace",
            &[],
            None,
            |ctx, args| {
                let name = arg_str(args, "name")?.to_string();
                request(ctx, Request::Workspace(WorkspaceOp::New(Some(name))))
            },
        ),
        cmd(
            "workspace.delete",
            "Delete Workspace",
            "Workspace",
            &[],
            None,
            |ctx, _| request(ctx, Request::Workspace(WorkspaceOp::Delete)),
        ),
        cmd(
            "workspace.rename",
            "Rename Workspace",
            "Workspace",
            &[],
            None,
            |ctx, args| {
                let name = arg_str(args, "name")?.to_string();
                request(ctx, Request::Workspace(WorkspaceOp::Rename(name)))
            },
        ),
        cmd(
            "workspace.next",
            "Next Workspace",
            "Workspace",
            &[],
            None,
            |ctx, _| request(ctx, Request::Workspace(WorkspaceOp::Cycle(false))),
        ),
        cmd(
            "workspace.previous",
            "Previous Workspace",
            "Workspace",
            &[],
            None,
            |ctx, _| request(ctx, Request::Workspace(WorkspaceOp::Cycle(true))),
        ),
        cmd(
            "workspace.switch",
            "Go to Workspace",
            "Workspace",
            &[],
            None,
            |ctx, args| {
                let i = args.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                request(ctx, Request::Workspace(WorkspaceOp::Switch(i)))
            },
        ),
        cmd(
            "workspace.last",
            "Last Workspace",
            "Workspace",
            &[],
            None,
            |ctx, _| request(ctx, Request::Workspace(WorkspaceOp::Last)),
        ),
        cmd(
            "workspace.save",
            "Save Workspace",
            "Workspace",
            &[],
            None,
            |ctx, _| request(ctx, Request::Workspace(WorkspaceOp::Save)),
        ),
        cmd(
            "workspace.load",
            "Load Workspace",
            "Workspace",
            &[],
            None,
            |ctx, args| {
                let name = args.get("name").and_then(Value::as_str).map(str::to_string);
                request(ctx, Request::Workspace(WorkspaceOp::Load(name)))
            },
        ),
        cmd(
            "workspace.deleteSaved",
            "Delete Saved Workspace",
            "Workspace",
            &[],
            None,
            |ctx, args| {
                let name = args.get("name").and_then(Value::as_str).map(str::to_string);
                request(ctx, Request::Workspace(WorkspaceOp::DeleteSaved(name)))
            },
        ),
        // Panes (Doom's `SPC w`, T2.7i.5).
        cmd(
            "pane.splitRight",
            "Split Right",
            "Window",
            &[],
            None,
            |ctx, _| request(ctx, Request::Pane(PaneOp::Split(Axis::Row))),
        ),
        cmd(
            "pane.splitBelow",
            "Split Below",
            "Window",
            &[],
            None,
            |ctx, _| request(ctx, Request::Pane(PaneOp::Split(Axis::Column))),
        ),
        cmd(
            "pane.focus",
            "Focus Pane",
            "Window",
            &[],
            None,
            |ctx, args| {
                let d = pane_dir(args)?;
                request(ctx, Request::Pane(PaneOp::Focus(d)))
            },
        ),
        cmd(
            "pane.move",
            "Move Pane",
            "Window",
            &[],
            None,
            |ctx, args| {
                let d = pane_dir(args)?;
                request(ctx, Request::Pane(PaneOp::Move(d)))
            },
        ),
        cmd("pane.close", "Close Pane", "Window", &[], None, |ctx, _| {
            request(ctx, Request::Pane(PaneOp::Close(false)))
        }),
        cmd(
            "pane.closeOrQuit",
            "Close Pane or Quit",
            "Window",
            &[],
            None,
            |ctx, _| request(ctx, Request::Pane(PaneOp::CloseOrQuit)),
        ),
        cmd(
            "pane.closeWithDocument",
            "Close Pane and Document",
            "Window",
            &[],
            None,
            |ctx, _| request(ctx, Request::Pane(PaneOp::Close(true))),
        ),
        cmd(
            "pane.only",
            "Only This Pane",
            "Window",
            &[],
            None,
            |ctx, _| request(ctx, Request::Pane(PaneOp::Only)),
        ),
        cmd("pane.next", "Next Pane", "Window", &[], None, |ctx, _| {
            request(ctx, Request::Pane(PaneOp::Cycle(false)))
        }),
        cmd(
            "pane.previous",
            "Previous Pane",
            "Window",
            &[],
            None,
            |ctx, _| request(ctx, Request::Pane(PaneOp::Previous)),
        ),
        cmd(
            "pane.balance",
            "Balance Panes",
            "Window",
            &[],
            None,
            |ctx, _| request(ctx, Request::Pane(PaneOp::Balance)),
        ),
        cmd(
            "pane.resize",
            "Resize Pane",
            "Window",
            &[],
            None,
            |ctx, args| {
                let axis = match args.get("axis").and_then(Value::as_str) {
                    Some("column") => Axis::Column,
                    _ => Axis::Row,
                };
                let by = args.get("by").and_then(Value::as_i64).unwrap_or(5) as i32;
                request(ctx, Request::Pane(PaneOp::Resize(axis, by)))
            },
        ),
        cmd("pane.swap", "Swap Panes", "Window", &[], None, |ctx, _| {
            request(ctx, Request::Pane(PaneOp::Swap))
        }),
        cmd(
            "pane.rotate",
            "Rotate Panes",
            "Window",
            &[],
            None,
            |ctx, args| request(ctx, Request::Pane(PaneOp::Rotate(arg_bool(args, "back")))),
        ),
        cmd("pane.undo", "Undo Layout", "Window", &[], None, |ctx, _| {
            request(ctx, Request::Pane(PaneOp::Undo))
        }),
        cmd("pane.redo", "Redo Layout", "Window", &[], None, |ctx, _| {
            request(ctx, Request::Pane(PaneOp::Redo))
        }),
        cmd("pane.new", "New Pane", "Window", &[], None, |ctx, _| {
            request(ctx, Request::Pane(PaneOp::New))
        }),
        // Doom's `SPC n` (T2.7i.12).
        cmd(
            "notes.search",
            "Search Notes",
            "Search",
            &[],
            None,
            |ctx, _| {
                let dir = crate::settings::expand_home(ctx.config.str("notes.directory"));
                let dir = std::path::PathBuf::from(dir);
                if !dir.is_dir() {
                    return Err(CommandError::new(crate::tr!(
                        "msg-no-notes-folder",
                        path = dir.display().to_string()
                    )));
                }
                request(ctx, Request::SearchIn(dir))
            },
        ),
        // Doom's `SPC i` (T2.7i.13).
        cmd(
            crate::insert::TEXT,
            "Insert Text",
            "Insert",
            &[],
            None,
            |ctx, args| {
                let text = arg_str(args, "text")?.to_string();
                insert_plain(ctx, &text)
            },
        ),
        cmd(
            "insert.unicode",
            "Insert Unicode Character",
            "Insert",
            &[],
            None,
            |ctx, _| request(ctx, Request::Choose(crate::insert::unicode_items())),
        ),
        cmd(
            "insert.emoji",
            "Insert Emoji",
            "Insert",
            &[],
            None,
            |ctx, _| request(ctx, Request::Choose(crate::insert::emoji_items())),
        ),
        cmd(
            "insert.fileName",
            "Insert File Name",
            "Insert",
            &[],
            Some("hasFile"),
            |ctx, _| {
                let path = this_file(ctx)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                insert_plain(ctx, &name)
            },
        ),
        cmd(
            "insert.filePath",
            "Insert File Path",
            "Insert",
            &[],
            Some("hasFile"),
            |ctx, _| {
                let path = this_file(ctx)?.display().to_string();
                insert_plain(ctx, &path)
            },
        ),
        cmd(
            "insert.fromHistory",
            "Insert from Clipboard History",
            "Insert",
            &[],
            None,
            |ctx, _| {
                let texts: Vec<(String, String)> = crate::command::clipboard_history()
                    .into_iter()
                    .map(|t| (String::new(), t))
                    .collect();
                if texts.is_empty() {
                    return Err(CommandError::new(crate::tr!("msg-no-history")));
                }
                let category = crate::tr!("category-clipboard");
                request(
                    ctx,
                    Request::Choose(crate::insert::text_items(&texts, &category)),
                )
            },
        ),
        cmd(
            "insert.fromRegister",
            "Insert from Register",
            "Insert",
            &[],
            None,
            |ctx, _| {
                let texts: Vec<(String, String)> = ctx
                    .clipboard
                    .registers
                    .iter()
                    .map(|(c, t)| (format!("\"{c}  "), t.clone()))
                    .collect();
                if texts.is_empty() {
                    return Err(CommandError::new(crate::tr!("msg-no-registers")));
                }
                let category = crate::tr!("category-registers");
                request(
                    ctx,
                    Request::Choose(crate::insert::text_items(&texts, &category)),
                )
            },
        ),
        // Sessions (Doom's `SPC q`, T2.7i.14).
        cmd(
            "session.save",
            "Save Session",
            "Session",
            &[],
            None,
            |ctx, args| {
                let name = args.get("name").and_then(Value::as_str);
                let name = name.unwrap_or(crate::sessions::LAST).to_string();
                request(ctx, Request::SaveSession(name))
            },
        ),
        cmd(
            "session.saveAs",
            "Save Session As",
            "Session",
            &[],
            None,
            |ctx, args| {
                let name = arg_str(args, "name")?.to_string();
                request(ctx, Request::SaveSession(name))
            },
        ),
        cmd(
            "session.restore",
            "Restore Last Session",
            "Session",
            &[],
            None,
            |ctx, args| {
                let name = args.get("name").and_then(Value::as_str);
                let name = name.unwrap_or(crate::sessions::LAST).to_string();
                request(ctx, Request::RestoreSession(name))
            },
        ),
        cmd(
            "session.restoreNamed",
            "Restore Session",
            "Session",
            &[],
            None,
            |ctx, args| {
                if let Some(name) = args.get("name").and_then(Value::as_str) {
                    return request(ctx, Request::RestoreSession(name.to_string()));
                }
                let names = crate::sessions::names();
                if names.is_empty() {
                    return Err(CommandError::new(crate::tr!("msg-no-sessions")));
                }
                let items = names
                    .into_iter()
                    .map(|n| crate::palette::PaletteItem {
                        id: crate::palette::invocation(
                            "session.restore",
                            &serde_json::json!({ "name": n }),
                        ),
                        title: n,
                        category: crate::tr!("category-session"),
                        keys: String::new(),
                        also: String::new(),
                    })
                    .collect();
                request(ctx, Request::Choose(items))
            },
        ),
        cmd(
            "app.quitWithoutSaving",
            "Quit Without Saving",
            "File",
            &[],
            None,
            |ctx, _| request(ctx, Request::QuitWithoutSaving),
        ),
        cmd(
            "window.close",
            "Close Window",
            "Window",
            &[],
            None,
            |ctx, _| request(ctx, Request::CloseWindow),
        ),
        cmd("app.restart", "Restart", "File", &[], None, |ctx, _| {
            request(ctx, Request::Restart { restore: false })
        }),
        cmd(
            "app.restartAndRestore",
            "Restart and Restore",
            "File",
            &[],
            None,
            |ctx, _| request(ctx, Request::Restart { restore: true }),
        ),
        cmd(
            "session.saveAndQuit",
            "Save Session and Quit",
            "Session",
            &[],
            None,
            |ctx, _| {
                request(ctx, Request::SaveSession(crate::sessions::LAST.to_string()))?;
                request(ctx, Request::Quit)
            },
        ),
        cmd(
            crate::prefix_arg::COMMAND,
            "Universal Argument",
            "Edit",
            &[],
            None,
            |ctx, _| request(ctx, Request::UniversalArgument),
        ),
        cmd(
            "picker.resume",
            "Resume Last Picker",
            "Search",
            &[],
            None,
            |ctx, _| request(ctx, Request::ResumePicker),
        ),
        cmd(
            "view.toggleLastPanel",
            "Toggle Last Panel",
            "View",
            &[],
            None,
            |ctx, _| request(ctx, Request::ToggleLastPanel),
        ),
        cmd(
            "view.outline",
            "Outline",
            "View",
            &["ctrl+shift+o"],
            None,
            |ctx, _| request(ctx, Request::Outline),
        ),
        cmd(
            "view.toggleSource",
            "Toggle Source View",
            "View",
            &["ctrl+/"],
            None,
            |ctx, _| request(ctx, Request::ToggleSource),
        ),
        cmd(
            "view.setMode",
            "Set Document Mode",
            "View",
            &[],
            None,
            |ctx, args| {
                let name = arg_str(args, "mode")?.to_string();
                let mode = crate::DocumentMode::from_name(&name).ok_or_else(|| {
                    CommandError::new(crate::tr!("msg-unknown-mode", mode = &name))
                })?;
                let base = ctx.config.parse_base();
                let doc = ctx
                    .document
                    .as_deref_mut()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-no-document")))?;
                doc.set_mode(mode.clone(), &base);
                // Remembered for the file in its workspace (§2.6).
                if let Some(p) = doc.meta.path.clone()
                    && let Err(e) = crate::settings::remember_mode(&p, &mode.setting_name())
                {
                    ctx.messages.push(e);
                }
                ctx.messages
                    .push(crate::tr!("msg-mode-set", mode = mode.title()));
                ctx.requests.push(Request::ModeChanged);
                Ok(())
            },
        ),
        // Doom's `SPC t` (T2.7i.6): each a setting, so the key and the
        // settings file agree.
        cmd(
            "view.toggleLineNumbers",
            "Toggle Line Numbers",
            "View",
            &[],
            None,
            |ctx, _| {
                let on = ctx.config.bool("editor.line_numbers");
                set_setting(ctx, "editor.line_numbers", Value::Bool(!on))
            },
        ),
        cmd(
            "view.toggleSourceMarkers",
            "Toggle Markup Characters",
            "View",
            &[],
            None,
            |ctx, _| {
                let always = ctx.config.str("editor.show_source_markers") == "always";
                let to = if always { "cursor" } else { "always" };
                set_setting(ctx, "editor.show_source_markers", Value::from(to))
            },
        ),
        cmd(
            "view.bigText",
            "Toggle Big Text",
            "View",
            &[],
            None,
            |ctx, _| {
                let size = ctx.config.int("editor.font_size");
                let to = BIG_FROM.with(|b| match b.take() {
                    Some(before) => before,
                    None => {
                        b.set(Some(size));
                        (size * 3 / 2).min(72)
                    }
                });
                set_setting(ctx, "editor.font_size", Value::from(to))
            },
        ),
        cmd(
            "view.toggleReadOnly",
            "Toggle Read-Only",
            "View",
            &[],
            None,
            |ctx, _| {
                let doc = ctx.doc()?;
                doc.read_only = !doc.read_only;
                let msg = if doc.read_only {
                    "msg-read-only-on"
                } else {
                    "msg-read-only-off"
                };
                ctx.messages.push(crate::l10n::tr(msg));
                Ok(())
            },
        ),
        // Doom's `SPC h` (T2.7i.9).
        cmd(
            "settings.set",
            "Set a Setting",
            "View",
            &[],
            None,
            |ctx, args| {
                let key = arg_str(args, "key")?.to_string();
                let value = args.get("value").cloned().unwrap_or(Value::Null);
                set_setting(ctx, &key, value)
            },
        ),
        cmd(
            "help.theme",
            "Choose the Theme",
            "Help",
            &[],
            None,
            |ctx, _| {
                let current = ctx.config.str("editor.theme").to_string();
                let items = ["system", "light", "dark"]
                    .into_iter()
                    .map(|t| crate::palette::PaletteItem {
                        id: crate::palette::invocation(
                            "settings.set",
                            &serde_json::json!({ "key": "editor.theme", "value": t }),
                        ),
                        title: crate::l10n::tr(&format!("theme-{t}")),
                        category: String::new(),
                        keys: if t == current {
                            "•".into()
                        } else {
                            String::new()
                        },
                        also: t.into(),
                    })
                    .collect();
                request(ctx, Request::Choose(items))
            },
        ),
        cmd(
            "help.mode",
            "Describe This Document",
            "Help",
            &[],
            None,
            |ctx, _| {
                let doc = ctx.doc()?;
                let file = doc
                    .meta
                    .path
                    .as_deref()
                    .and_then(|p| p.file_name())
                    .map_or_else(
                        || crate::l10n::tr("help-no-file"),
                        |n| n.to_string_lossy().into_owned(),
                    );
                let kind = crate::kinds::file_kind(doc).unwrap_or("none");
                let msg = crate::tr!(
                    "help-mode",
                    file = file,
                    mode = doc.meta.mode.name(),
                    text = doc.text_type(),
                    kind = kind.to_string()
                );
                ctx.messages.push(msg);
                Ok(())
            },
        ),
        cmd(
            "help.char",
            "Describe the Character",
            "Help",
            &[],
            None,
            |ctx, _| {
                let doc = ctx.doc()?;
                let text = doc.text();
                let at = doc.selection.head;
                let c = text.as_str()[at..]
                    .chars()
                    .next()
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("help-no-char")))?;
                ctx.messages.push(describe_char(c));
                Ok(())
            },
        ),
        cmd(
            "help.bindings",
            "All Key Bindings",
            "Help",
            &[],
            None,
            |ctx, _| request(ctx, Request::HelpBindings),
        ),
        cmd(
            "help.describeKey",
            "Describe Key",
            "Help",
            &[],
            None,
            |ctx, _| {
                ctx.messages.push(crate::l10n::tr("help-press-key"));
                request(ctx, Request::DescribeKey)
            },
        ),
        cmd(
            "help.reload",
            "Reload Settings and Keys",
            "Help",
            &[],
            None,
            |ctx, _| request(ctx, Request::ReloadSettings),
        ),
        cmd(
            "app.terminal",
            "Open a Terminal Here",
            "View",
            &[],
            None,
            |ctx, args| {
                let doc = ctx.doc()?;
                let dir = crate::command::folder_of(doc)
                    .or_else(|| std::env::current_dir().ok())
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-file")))?;
                let dir = if args.get("project").and_then(Value::as_bool) == Some(true) {
                    kalem_project::list::detect_root(&dir).unwrap_or(dir)
                } else {
                    dir
                };
                request(ctx, Request::Terminal(dir))
            },
        ),
        cmd(
            "app.newWindow",
            "New Window",
            "View",
            &[],
            None,
            |ctx, _| request(ctx, Request::NewWindow),
        ),
        cmd(
            "view.fullScreen",
            "Toggle Full Screen",
            "View",
            &[],
            None,
            |ctx, _| request(ctx, Request::FullScreen),
        ),
        cmd("view.zen", "Zen Mode", "View", &[], None, |ctx, _| {
            ctx.requests.push(Request::Focus);
            request(ctx, Request::FullScreen)
        }),
        cmd(
            "view.toggleWrap",
            "Toggle Soft Wrap",
            "View",
            &["alt+z"],
            None,
            |ctx, _| request(ctx, Request::ToggleWrap),
        ),
        cmd(
            "view.toggleMath",
            "Toggle Formula Preview",
            "View",
            &[],
            Some("editorMode == org || editorMode == latex || editorMode == markdown"),
            |ctx, _| request(ctx, Request::ToggleMath),
        ),
        cmd(
            "view.focus",
            "Focus Mode",
            "View",
            &["f8"],
            None,
            |ctx, _| request(ctx, Request::Focus),
        ),
        cmd(
            "view.split",
            "Split View",
            "View",
            &["ctrl+\\"],
            None,
            |ctx, _| request(ctx, Request::Split),
        ),
        cmd(
            "view.fold",
            "Fold or Unfold",
            "View",
            &[],
            Some("editorMode == org && onHeadline"),
            |ctx, _| request(ctx, Request::Fold { global: false }),
        ),
        cmd(
            "view.foldAll",
            "Fold or Unfold All",
            "View",
            &[],
            Some(ORG),
            |ctx, _| request(ctx, Request::Fold { global: true }),
        ),
        cmd("edit.undo", "Undo", "Edit", &["ctrl+z"], None, |ctx, _| {
            ctx.doc()?
                .undo()
                .map(|_| ())
                .ok_or_else(|| CommandError::new(crate::tr!("msg-nothing-to-undo")))
        }),
        cmd(
            "edit.redo",
            "Redo",
            "Edit",
            &["ctrl+shift+z", "ctrl+y"],
            None,
            |ctx, _| {
                ctx.doc()?
                    .redo()
                    .map(|_| ())
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-nothing-to-redo")))
            },
        ),
        // Headlines.
        cmd(
            "org.headline.promote",
            "Promote Headline",
            "Headlines",
            &["alt+left"],
            Some("editorMode == org && onHeadline"),
            |ctx, _| ctx.org(|d, p, _| h::promote(&text_of(d), p, d.parse().context())),
        ),
        cmd(
            "org.headline.demote",
            "Demote Headline",
            "Headlines",
            &["alt+right"],
            Some("editorMode == org && onHeadline"),
            |ctx, _| ctx.org(|d, p, _| h::demote(&text_of(d), p, d.parse().context())),
        ),
        cmd(
            "org.headline.promoteSubtree",
            "Promote Subtree",
            "Headlines",
            &["alt+shift+left"],
            Some(ORG),
            |ctx, _| ctx.org(|d, p, _| h::promote_subtree(&text_of(d), p, d.parse().context())),
        ),
        cmd(
            "org.headline.demoteSubtree",
            "Demote Subtree",
            "Headlines",
            &["alt+shift+right"],
            Some(ORG),
            |ctx, _| ctx.org(|d, p, _| h::demote_subtree(&text_of(d), p, d.parse().context())),
        ),
        cmd(
            "org.headline.moveSubtreeUp",
            "Move Subtree Up",
            "Headlines",
            &["alt+up"],
            Some("editorMode == org && onHeadline"),
            |ctx, _| ctx.org(|d, p, _| h::move_subtree(&text_of(d), p, false, d.parse().context())),
        ),
        cmd(
            "org.headline.moveSubtreeDown",
            "Move Subtree Down",
            "Headlines",
            &["alt+down"],
            Some("editorMode == org && onHeadline"),
            |ctx, _| ctx.org(|d, p, _| h::move_subtree(&text_of(d), p, true, d.parse().context())),
        ),
        cmd(
            "org.headline.setLevel",
            "Heading Level",
            "Headlines",
            &[],
            Some(ORG),
            |ctx, args| {
                let level = args.get("level").and_then(Value::as_u64).ok_or_else(|| {
                    CommandError::new(crate::tr!("msg-missing-argument", name = "level"))
                })?;
                ctx.org(|d, p, _| h::set_level(&text_of(d), p, level as usize, d.parse().context()))
            },
        ),
        cmd(
            "org.headline.cutSubtree",
            "Cut Subtree",
            "Headlines",
            &[],
            Some(ORG),
            |ctx, _| {
                let mut clip = String::new();
                let r = ctx.org(|d, p, _| {
                    h::cut_subtree(&text_of(d), p, d.parse().context()).map(|(t, c)| {
                        clip = c;
                        t
                    })
                });
                if r.is_ok() {
                    ctx.clipboard.record(clip);
                }
                r
            },
        ),
        cmd(
            "org.headline.copySubtree",
            "Copy Subtree",
            "Headlines",
            &[],
            Some(ORG),
            |ctx, _| {
                let doc = ctx.doc()?;
                let model = doc
                    .model()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
                let text = text_of(&model);
                let clip = h::copy_subtree(&text, doc.selection.head, model.parse().context())?;
                ctx.clipboard.record(clip);
                Ok(())
            },
        ),
        cmd(
            "org.headline.pasteSubtree",
            "Paste Subtree",
            "Headlines",
            &[],
            Some(ORG),
            |ctx, _| {
                let clip = ctx.clipboard.text.clone();
                ctx.org(|d, p, _| h::paste_subtree(&text_of(d), p, &clip, d.parse().context()))
            },
        ),
        cmd(
            "org.headline.sort",
            "Sort Entries",
            "Headlines",
            &[],
            Some(ORG),
            |ctx, args| {
                let by = args.get("by").and_then(Value::as_str).unwrap_or("a");
                let c = by.chars().next().unwrap_or('a');
                let prop = args.get("property").and_then(Value::as_str);
                let mut opts = org_edit::sort::SortOptions::from_char(c, prop)
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-invalid-sort")))?;
                opts.with_case = arg_bool(args, "withCase");
                let clock = ctx.clock;
                ctx.org(|d, p, m| org_edit::sort::sort_entries(d, p, m, &opts, clock))
            },
        ),
        // TODO and priority.
        cmd(
            "org.todo.cycle",
            "Cycle TODO State",
            "TODO",
            &["ctrl+enter"],
            Some(ORG),
            |ctx, _| todo(ctx, TodoArg::Cycle),
        ),
        cmd(
            "org.todo.next",
            "Next TODO Keyword",
            "TODO",
            &["shift+right"],
            Some("editorMode == org && onHeadline"),
            |ctx, _| todo(ctx, TodoArg::Next),
        ),
        cmd(
            "org.todo.previous",
            "Previous TODO Keyword",
            "TODO",
            &["shift+left"],
            Some("editorMode == org && onHeadline"),
            |ctx, _| todo(ctx, TodoArg::Previous),
        ),
        cmd(
            "org.todo.set",
            "Set TODO State",
            "TODO",
            &[],
            Some(ORG),
            |ctx, args| {
                let s = arg_str(args, "state")?.to_string();
                todo(ctx, TodoArg::State(s))
            },
        ),
        cmd(
            "org.todo.done",
            "Mark Done",
            "TODO",
            &[],
            Some(ORG),
            |ctx, _| todo(ctx, TodoArg::Done),
        ),
        cmd(
            "org.todo.nextSet",
            "Next Keyword Set",
            "TODO",
            &[],
            Some(ORG),
            |ctx, _| todo(ctx, TodoArg::NextSet),
        ),
        cmd(
            "org.todo.previousSet",
            "Previous Keyword Set",
            "TODO",
            &[],
            Some(ORG),
            |ctx, _| todo(ctx, TodoArg::PreviousSet),
        ),
        cmd(
            "org.priority.up",
            "Raise Priority",
            "TODO",
            &["shift+up"],
            Some("editorMode == org && onHeadline"),
            |ctx, _| priority(ctx, PriorityAction::Up),
        ),
        cmd(
            "org.priority.down",
            "Lower Priority",
            "TODO",
            &["shift+down"],
            Some("editorMode == org && onHeadline"),
            |ctx, _| priority(ctx, PriorityAction::Down),
        ),
        cmd(
            "org.priority.set",
            "Set Priority",
            "TODO",
            &[],
            Some(ORG),
            |ctx, args| {
                let c = arg_str(args, "priority")?
                    .chars()
                    .next()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-empty-priority")))?;
                priority(ctx, PriorityAction::Set(c))
            },
        ),
        cmd(
            "org.priority.remove",
            "Remove Priority",
            "TODO",
            &[],
            Some(ORG),
            |ctx, _| priority(ctx, PriorityAction::Remove),
        ),
        // Properties and tags.
        cmd(
            "org.property.set",
            "Set Property",
            "Properties",
            &[],
            Some(ORG),
            |ctx, args| {
                let (k, v) = (
                    arg_str(args, "key")?.to_string(),
                    arg_str(args, "value")?.to_string(),
                );
                ctx.org(|d, p, _| org_edit::property::set_property(d, p, &k, &v, false))
            },
        ),
        cmd(
            "org.insert.drawer",
            "Insert Drawer",
            "Insert",
            &[],
            Some(ORG),
            |ctx, args| {
                let name = arg_str(args, "name")?.trim().to_string();
                ctx.org(|d, p, m| org_edit::insert::insert_drawer(&text_of(d), p, m, &name))
            },
        ),
        cmd(
            "org.caption.set",
            "Set Caption",
            "Insert",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "caption")?.to_string();
                ctx.org(|d, p, _| crate::affiliated::set(d, p, "CAPTION", &v))
            },
        ),
        cmd(
            "org.name.set",
            "Set Name",
            "Insert",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "name")?.to_string();
                ctx.org(|d, p, _| crate::affiliated::set(d, p, "NAME", &v))
            },
        ),
        cmd(
            crate::affiliated::REFERENCE,
            "Insert Cross Reference",
            "Insert",
            &[],
            Some(ORG),
            |ctx, args| {
                if let Some(target) = args.get("target").and_then(Value::as_str) {
                    let target = target.to_string();
                    return ctx
                        .org(|d, p, m| org_edit::insert::insert_link(d, p, m, &target, None));
                }
                let doc = ctx.doc()?;
                let model = doc
                    .model()
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-org")))?;
                let items = crate::affiliated::picker_items(&model);
                if items.is_empty() {
                    ctx.messages.push(crate::l10n::tr("msg-no-references"));
                    return Ok(());
                }
                ctx.requests.push(Request::Choose(items));
                Ok(())
            },
        ),
        cmd(
            "org.property.edit",
            "Edit Properties",
            "Properties",
            &[],
            Some(ORG),
            |ctx, _| {
                let doc = ctx.doc()?;
                let pos = doc.selection.head;
                let model = doc
                    .model()
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-org")))?;
                ctx.requests
                    .push(Request::Choose(crate::properties::items(&model, pos)));
                Ok(())
            },
        ),
        cmd(
            "org.schedule",
            "Schedule",
            "Tasks",
            &["ctrl+alt+s"],
            Some(ORG),
            |ctx, args| planning(ctx, org_edit::todo::Planning::Scheduled, args, false),
        ),
        cmd(
            "org.deadline",
            "Set Deadline",
            "Tasks",
            &[],
            Some(ORG),
            |ctx, args| planning(ctx, org_edit::todo::Planning::Deadline, args, false),
        ),
        cmd(
            "org.schedule.remove",
            "Remove Schedule",
            "Tasks",
            &[],
            Some(ORG),
            |ctx, args| planning(ctx, org_edit::todo::Planning::Scheduled, args, true),
        ),
        cmd(
            "org.deadline.remove",
            "Remove Deadline",
            "Tasks",
            &[],
            Some(ORG),
            |ctx, args| planning(ctx, org_edit::todo::Planning::Deadline, args, true),
        ),
        cmd(
            "org.archive.toggleTag",
            "Toggle Archive Tag",
            "Tasks",
            &[],
            Some(ORG),
            |ctx, _| {
                let mut set = false;
                ctx.org(|d, p, _| {
                    let (tx, s) = org_edit::archive::toggle_archive_tag(d, p)?;
                    set = s;
                    Ok(tx)
                })?;
                ctx.messages.push(crate::l10n::tr(if set {
                    "msg-archived"
                } else {
                    "msg-unarchived"
                }));
                Ok(())
            },
        ),
        cmd(
            "org.archive.sibling",
            "Archive to Sibling",
            "Tasks",
            &[],
            Some(ORG),
            |ctx, _| {
                let now = jiff::Zoned::now().strftime("%Y-%m-%d %a %H:%M").to_string();
                ctx.org(|d, p, _| org_edit::archive::archive_to_sibling(d, p, &now))
            },
        ),
        cmd(
            crate::refile::REFILE,
            "Refile",
            "Tasks",
            &["ctrl+alt+w"],
            Some(ORG),
            |ctx, args| {
                if let Some(target) = args.get("target").and_then(Value::as_u64) {
                    let (clock, base) = (ctx.clock, ctx.config.todo_settings());
                    let mut pending = None;
                    ctx.org(|d, p, _| {
                        let settings = base.for_document(d);
                        let (t, n) = org_edit::archive::refile_logged(
                            d,
                            p,
                            target as usize,
                            &settings,
                            clock,
                        )?;
                        pending = n;
                        Ok(t)
                    })?;
                    return match pending {
                        Some(n) => ask_note(ctx, &n),
                        None => Ok(()),
                    };
                }
                let doc = ctx.doc()?;
                let pos = doc.selection.head;
                let model = doc
                    .model()
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-org")))?;
                if model
                    .outline()
                    .entries
                    .iter()
                    .all(|e| usize::from(e.range.start()) > pos)
                {
                    return Err(CommandError::new(crate::l10n::tr("msg-not-in-subtree")));
                }
                ctx.requests
                    .push(Request::Choose(crate::refile::picker_items(&model, pos)));
                Ok(())
            },
        ),
        cmd(
            "org.footnote.new",
            "New Footnote",
            "Footnotes",
            &["ctrl+alt+f"],
            Some(ORG),
            |ctx, args| {
                let base = footnote_settings(ctx.config);
                let text = text_of(
                    ctx.doc()?
                        .model()
                        .as_deref()
                        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-org")))?,
                );
                let s = base.for_text(&text);
                let label = args
                    .get("label")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                // `fnprompt` and `fnconfirm`: the label asked for, the
                // next free number offered with `fnconfirm`.
                if s.asks_label() && label.is_none() {
                    let mut a = serde_json::json!({});
                    if s.auto_label == org_edit::footnote::AutoLabel::Confirm {
                        a["label_default"] = org_edit::footnote::proposed_label(&text).into();
                    }
                    return request(
                        ctx,
                        Request::Ask {
                            command: "org.footnote.new".into(),
                            args: a,
                            arg: "label".into(),
                        },
                    );
                }
                ctx.org(|d, p, _| {
                    org_edit::footnote::new_labeled(&text_of(d), p, &s, label.as_deref())
                })
            },
        ),
        cmd(
            "org.note.add",
            "Add Note",
            "Tasks",
            &[],
            Some(ORG),
            add_note,
        ),
        cmd(
            "org.footnote.action",
            "Go to Footnote Definition or Reference",
            "Footnotes",
            &[],
            Some(ORG),
            |ctx, _| {
                let s = footnote_settings(ctx.config);
                ctx.org(|d, p, _| org_edit::footnote::action(&text_of(d), p, &s))
            },
        ),
        cmd(
            "org.footnote.renumber",
            "Renumber Footnotes",
            "Footnotes",
            &[],
            Some(ORG),
            |ctx, _| ctx.org(|d, p, _| org_edit::footnote::renumber(&text_of(d), p)),
        ),
        cmd(
            "org.footnote.sort",
            "Sort Footnote Definitions",
            "Footnotes",
            &[],
            Some(ORG),
            |ctx, _| {
                let s = footnote_settings(ctx.config);
                ctx.org(|d, p, _| org_edit::footnote::sort(&text_of(d), p, &s))
            },
        ),
        cmd(
            "org.footnote.normalize",
            "Normalize Footnotes",
            "Footnotes",
            &[],
            Some(ORG),
            |ctx, _| {
                let s = footnote_settings(ctx.config);
                ctx.org(|d, p, _| org_edit::footnote::normalize(&text_of(d), p, &s))
            },
        ),
        cmd(
            "org.footnote.delete",
            "Delete Footnote",
            "Footnotes",
            &[],
            Some(ORG),
            |ctx, _| {
                let base = footnote_settings(ctx.config);
                ctx.org(|d, p, _| {
                    let text = text_of(d);
                    org_edit::footnote::delete_adjusted(&text, p, &base.for_text(&text))
                })
            },
        ),
        cmd(
            "org.property.delete",
            "Delete Property",
            "Properties",
            &[],
            Some(ORG),
            |ctx, args| {
                let k = arg_str(args, "key")?.to_string();
                ctx.org(|d, p, _| {
                    org_edit::property::delete_property(d, p, &k).ok_or_else(|| {
                        org_edit::EditError {
                            message: crate::tr!("msg-no-property", key = k.as_str()),
                            point: None,
                        }
                    })
                })
            },
        ),
        cmd(
            "org.todo.toggleOrdered",
            "Toggle Ordered Subtasks",
            "TODO",
            &[],
            Some(ORG),
            |ctx, _| {
                let ordered = std::cell::Cell::new(false);
                ctx.org(|d, p, _| {
                    let (t, o) = org_edit::property::toggle_ordered(d, p)?;
                    ordered.set(o);
                    Ok(t)
                })?;
                ctx.messages.push(crate::l10n::tr(if ordered.get() {
                    "msg-ordered-on"
                } else {
                    "msg-ordered-off"
                }));
                Ok(())
            },
        ),
        cmd(
            "org.tags.set",
            "Set Tags",
            "Tags",
            &[],
            Some(ORG),
            |ctx, args| {
                let tags: Vec<String> = args
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(|t| t.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                ctx.org(|d, p, _| org_edit::tags::set_tags(d, p, &tags))
            },
        ),
        cmd(
            "org.tags.toggle",
            "Toggle Tag",
            "Tags",
            &[],
            Some(ORG),
            |ctx, args| {
                let tag = arg_str(args, "tag")?.to_string();
                ctx.org(|d, p, _| org_edit::tags::toggle_tag(d, p, &tag, None).map(|(t, _)| t))
            },
        ),
        cmd(
            "org.tags.alignAll",
            "Align All Tags",
            "Tags",
            &[],
            Some(ORG),
            |ctx, _| ctx.org(|d, p, _| Ok(org_edit::tags::align_all_tags(d, p))),
        ),
        // Lists.
        cmd(
            "list.indent",
            "Indent Item",
            "Lists",
            &["tab"],
            Some(LIST),
            |ctx, _| ctx.org(|d, p, m| l::indent_item(d, p, m, true, true)),
        ),
        cmd(
            "list.outdent",
            "Outdent Item",
            "Lists",
            &["shift+tab"],
            Some(LIST),
            |ctx, _| ctx.org(|d, p, m| l::indent_item(d, p, m, false, true)),
        ),
        cmd(
            "list.indentTree",
            "Indent Item and Children",
            "Lists",
            &["alt+shift+right"],
            Some(LIST),
            |ctx, _| ctx.org(|d, p, m| l::indent_item(d, p, m, true, false)),
        ),
        cmd(
            "list.outdentTree",
            "Outdent Item and Children",
            "Lists",
            &["alt+shift+left"],
            Some(LIST),
            |ctx, _| ctx.org(|d, p, m| l::indent_item(d, p, m, false, false)),
        ),
        cmd(
            "list.cycleBullet",
            "Cycle Bullet",
            "Lists",
            &["ctrl+shift+l"],
            Some(LIST),
            |ctx, args| {
                let which = match args.get("bullet").and_then(Value::as_str) {
                    Some(b) => l::BulletChoice::Bullet(b.to_string()),
                    None if arg_bool(args, "previous") => l::BulletChoice::Previous,
                    None => l::BulletChoice::Next,
                };
                ctx.org(|d, p, _| l::cycle_bullet(d, p, which))
            },
        ),
        cmd(
            "list.toggleCheckbox",
            "Toggle Checkbox",
            "Lists",
            &["ctrl+shift+c"],
            Some(ORG),
            |ctx, args| {
                let action = if arg_bool(args, "presence") {
                    l::CheckboxAction::Presence
                } else {
                    l::CheckboxAction::Toggle
                };
                ctx.org(|d, p, m| l::toggle_checkbox(d, p, m, action))
            },
        ),
        cmd(
            "list.moveUp",
            "Move Item Up",
            "Lists",
            &["alt+up"],
            Some(LIST),
            |ctx, _| ctx.org(|d, p, _| l::move_item(d, p, false)),
        ),
        cmd(
            "list.moveDown",
            "Move Item Down",
            "Lists",
            &["alt+down"],
            Some(LIST),
            |ctx, _| ctx.org(|d, p, _| l::move_item(d, p, true)),
        ),
        cmd(
            "list.insertItem",
            "Insert Item",
            "Lists",
            &["alt+enter"],
            Some(LIST),
            |ctx, args| {
                let checkbox = arg_bool(args, "checkbox");
                ctx.org(|d, p, _| {
                    l::insert_item(d, p, checkbox).ok_or_else(|| org_edit::EditError {
                        message: "Not in a list".into(),
                        point: None,
                    })
                })
            },
        ),
        cmd(
            "list.repair",
            "Repair List",
            "Lists",
            &[],
            Some(LIST),
            |ctx, _| ctx.org(|d, p, _| l::repair(d, p)),
        ),
        // Emphasis.
        cmd(
            "org.emphasis.bold",
            "Bold",
            "Format",
            &["ctrl+b"],
            Some(ORG),
            |ctx, _| emphasis(ctx, Emphasis::Bold),
        ),
        cmd(
            "org.emphasis.italic",
            "Italic",
            "Format",
            &["ctrl+i"],
            Some(ORG),
            |ctx, _| emphasis(ctx, Emphasis::Italic),
        ),
        cmd(
            "org.emphasis.underline",
            "Underline",
            "Format",
            &["ctrl+u"],
            Some(ORG),
            |ctx, _| emphasis(ctx, Emphasis::Underline),
        ),
        cmd(
            "org.emphasis.strikeThrough",
            "Strike Through",
            "Format",
            &["ctrl+shift+x"],
            Some(ORG),
            |ctx, _| emphasis(ctx, Emphasis::StrikeThrough),
        ),
        cmd(
            "org.emphasis.code",
            "Code",
            "Format",
            &["ctrl+shift+k"],
            Some(ORG),
            |ctx, _| emphasis(ctx, Emphasis::Code),
        ),
        cmd(
            "org.emphasis.verbatim",
            "Verbatim",
            "Format",
            &[],
            Some(ORG),
            |ctx, _| emphasis(ctx, Emphasis::Verbatim),
        ),
        // Inserting.
        cmd(
            "org.insert.link",
            "Insert Link",
            "Insert",
            &["ctrl+k"],
            Some(ORG),
            |ctx, args| {
                let link = arg_str(args, "link")?.to_string();
                let desc = args
                    .get("description")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                ctx.org(|d, p, m| org_edit::insert::insert_link(d, p, m, &link, desc.as_deref()))
            },
        ),
        cmd("link.store", "Store Link", "Insert", &[], None, |ctx, _| {
            let links = crate::links::links_of(ctx.doc()?);
            if links.is_empty() {
                return Err(CommandError::new(crate::tr!("msg-link-nothing-to-store")));
            }
            let names: Vec<&str> = links.iter().map(|l| l.description.as_str()).collect();
            let msg = crate::tr!("msg-link-stored", names = names.join(", "));
            crate::links::store(links);
            ctx.messages.push(msg);
            Ok(())
        }),
        cmd(
            "link.insertFiles",
            "Insert Links to Files",
            "Insert",
            &[],
            None,
            |ctx, args| {
                // Files dropped on a document: a link to each, in the
                // document's syntax, at the cursor.
                let paths: Vec<std::path::PathBuf> = args
                    .get("paths")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).map(Into::into).collect())
                    .unwrap_or_default();
                if paths.is_empty() {
                    return Err(CommandError::new(crate::tr!("msg-no-stored-link")));
                }
                let now = ctx.now;
                let d = ctx.doc()?;
                let text = crate::links::text_for(&d.meta.mode, &paths, d.meta.path.as_deref());
                let s = d.selection;
                let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
                let mut tx = org_edit::Transaction::new("Insert Links to Files");
                tx.replace(a..b, text.as_str())
                    .map_err(|e| CommandError::new(e.to_string()))?;
                let tx = tx.select(org_edit::Selection::caret(a + text.len()));
                d.apply(&tx, org_edit::ChangeKind::Command, now);
                Ok(())
            },
        ),
        cmd(
            "org.link.insertStored",
            "Insert Stored Link",
            "Insert",
            &[],
            Some(ORG),
            |ctx, _| {
                let links = crate::links::latest();
                if links.is_empty() {
                    return Err(CommandError::new(crate::tr!("msg-no-stored-link")));
                }
                let now = ctx.now;
                let d = ctx.doc()?;
                let text = crate::links::org_text(&links, d.meta.path.as_deref());
                let s = d.selection;
                let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
                let mut tx = org_edit::Transaction::new("Insert Stored Link");
                tx.replace(a..b, text.as_str())
                    .map_err(|e| CommandError::new(e.to_string()))?;
                let tx = tx.select(org_edit::Selection::caret(a + text.len()));
                d.apply(&tx, org_edit::ChangeKind::Command, now);
                Ok(())
            },
        ),
        cmd(
            crate::cite::INSERT,
            "Insert Citation",
            "Insert",
            &[],
            Some(ORG),
            |ctx, args| {
                if let Some(key) = args.get("key").and_then(Value::as_str) {
                    let key = key.trim_start_matches('@').to_string();
                    return ctx.org(|d, p, _| crate::cite::insert(d, p, &key));
                }
                let doc = ctx.doc()?;
                let path = doc.meta.path.clone();
                let model = doc
                    .model()
                    .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-org")))?;
                let bib = crate::cite::bibliography(&model, path.as_deref());
                if bib.entries().is_empty() {
                    ctx.messages.push(crate::l10n::tr("msg-no-bibliography"));
                    return Ok(());
                }
                ctx.requests
                    .push(Request::Choose(crate::cite::picker_items(&bib)));
                Ok(())
            },
        ),
        cmd(
            "org.insert.block",
            "Insert Block",
            "Insert",
            &[],
            Some(ORG),
            |ctx, args| {
                let ty = args
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("src")
                    .to_string();
                ctx.org(|d, p, m| org_edit::insert::insert_structure_template(d, p, m, &ty))
            },
        ),
        cmd(
            "org.insert.timestamp",
            "Insert Timestamp",
            "Insert",
            &["ctrl+shift+d"],
            Some(ORG),
            |ctx, args| {
                let (with_time, inactive) =
                    (arg_bool(args, "withTime"), arg_bool(args, "inactive"));
                let clock = ctx.clock;
                ctx.org(|d, p, _| {
                    Ok(org_edit::insert::insert_timestamp(
                        d, p, clock, with_time, inactive,
                    ))
                })
            },
        ),
        cmd(
            "org.insert.date",
            "Insert Date",
            "Insert",
            &["alt+shift+d"],
            Some(ORG),
            |ctx, args| {
                let input = arg_str(args, "date")?.to_string();
                let inactive = arg_bool(args, "inactive");
                let (date, with_time) =
                    crate::dates::parse(&input, ctx.clock).ok_or_else(|| {
                        CommandError::new(crate::tr!("msg-not-a-date", input = &input))
                    })?;
                ctx.org(|d, p, _| {
                    Ok(org_edit::insert::insert_timestamp(
                        d, p, date, with_time, inactive,
                    ))
                })
            },
        ),
        cmd(
            "org.insert.horizontalRule",
            "Insert Horizontal Rule",
            "Insert",
            &[],
            Some(ORG),
            |ctx, _| ctx.org(|d, p, _| Ok(org_edit::insert::insert_horizontal_rule(d, p))),
        ),
        // Tables.
        cmd(
            "table.create",
            "Insert Table",
            "Table",
            &["ctrl+shift+t"],
            Some(ORG),
            |ctx, args| {
                let n = |k: &str, d: u64| args.get(k).and_then(Value::as_u64).unwrap_or(d) as usize;
                let (columns, rows) = (n("columns", 5), n("rows", 2));
                // With a selection, its lines become the table
                // (`org-table-create-or-convert-from-region`).
                ctx.org(|d, p, m| match m.filter(|&m| m != p) {
                    Some(m) => {
                        org_edit::recalc::convert_region(d, m, p, org_table::csv::Separator::Auto)
                    }
                    None => t::create_table(d, p, columns, rows),
                })
            },
        ),
        cmd(
            "table.align",
            "Align Table",
            "Table",
            &[],
            Some(TABLE),
            |ctx, _| ctx.org(|d, p, _| t::align_table(d, p)),
        ),
        cmd(
            "table.recalculate",
            "Recalculate Table",
            "Table",
            &["f9"],
            Some(ORG),
            |ctx, args| {
                let iterate = arg_bool(args, "iterate");
                let mut lisp = Vec::new();
                let mut error = None;
                let r = ctx.org(|d, p, _| {
                    let r = org_edit::recalc::recalculate(d, p, iterate)?;
                    lisp = r.lisp;
                    error = r.error;
                    Ok(r.transaction)
                });
                if !lisp.is_empty() {
                    ctx.messages
                        .push(crate::tr!("msg-lisp-formulas", lhs = lisp.join(", ")));
                }
                // Emacs's error after the change, the change kept.
                match error {
                    Some(e) if r.is_ok() => Err(CommandError::new(e)),
                    _ => r,
                }
            },
        ),
        cmd(
            "table.import",
            "Import Table…",
            "Table",
            &[],
            Some(ORG),
            |ctx, args| {
                let file = args
                    .get("file")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let path = resolve_path(ctx.doc()?, &file);
                let cannot = |e: String| {
                    CommandError::new(crate::tr!(
                        "msg-cannot-read",
                        error = format!("{}: {e}", path.display())
                    ))
                };
                // Decoded as the editors open files (a byte order mark, a
                // legacy encoding).
                let bytes = std::fs::read(&path).map_err(|e| cannot(e.to_string()))?;
                let (content, meta) =
                    crate::files::decode(Some(&path), bytes).map_err(|e| cannot(e.to_string()))?;
                let content = content.replace("\r\n", "\n");
                // CSV that Emacs's reader would misread (another delimiter
                // than comma or tab, a line break in a quoted field): read
                // as CSV mode reads it, a rule under a detected header.
                if meta.mode == crate::DocumentMode::Csv {
                    let dialect = crate::csv::detect(&content);
                    let rows = crate::csv::rows(&content, &dialect);
                    let breaks = rows.iter().flatten().any(|v| v.contains('\n'));
                    if breaks || !matches!(dialect.delimiter, b',' | b'\t') {
                        let now = ctx.now;
                        let d = ctx.doc()?;
                        let point = d.selection.head;
                        let text = d.text().as_str();
                        let mut table = String::new();
                        if point > 0 && text.as_bytes()[point - 1] != b'\n' {
                            table.push('\n');
                        }
                        let caret = point + table.len();
                        table.push_str(&crate::csv::to_org_table(&content, &dialect));
                        let mut tx = org_edit::Transaction::new("Import table");
                        tx.edit(point..point, table);
                        let tx = tx.select(org_edit::Selection::caret(caret));
                        d.apply(&tx, org_edit::ChangeKind::Command, now);
                        return Ok(());
                    }
                }
                ctx.org(|d, p, _| {
                    org_edit::recalc::import(d, p, &content, org_table::csv::Separator::Auto)
                })
            },
        ),
        cmd(
            "table.export",
            "Export Table…",
            "Table",
            &[],
            Some(TABLE),
            |ctx, args| {
                let file = args
                    .get("file")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let path = resolve_path(ctx.doc()?, &file);
                let csv = path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("csv"));
                let format = if csv {
                    org_table::csv::Format::Csv
                } else {
                    org_table::csv::Format::Tsv
                };
                let doc = ctx.doc()?;
                let head = doc.selection.head;
                let model = doc
                    .model()
                    .ok_or_else(|| CommandError::new("Not an Org document"))?;
                let text = org_edit::recalc::export(&model, head, format)
                    .map_err(|e| CommandError::new(e.message))?;
                std::fs::write(&path, text).map_err(|e| {
                    CommandError::new(crate::tr!(
                        "msg-cannot-write",
                        path = path.display().to_string(),
                        reason = e.to_string()
                    ))
                })?;
                ctx.messages.push(crate::tr!(
                    "msg-exported",
                    path = path.display().to_string()
                ));
                Ok(())
            },
        ),
        cmd(
            "table.sortRows",
            "Sort Rows",
            "Table",
            &[],
            Some(TABLE),
            |ctx, args| {
                let by = args.get("by").and_then(Value::as_str).unwrap_or("a");
                let c = by.chars().next().unwrap_or('a');
                let with_case = arg_bool(args, "withCase");
                ctx.org(|d, p, _| t::sort_rows(d, p, c, with_case))
            },
        ),
        cmd(
            "table.setFormula",
            "Edit Formula",
            "Table",
            &["f2"],
            Some(ORG),
            |ctx, args| {
                let formula = args
                    .get("formula")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let field = arg_bool(args, "field") || formula.trim_start().starts_with(":=");
                let mut lisp = Vec::new();
                let r = ctx.org(|d, p, _| {
                    let r = org_edit::recalc::set_formula(d, p, &formula, field)?;
                    lisp = r.lisp;
                    Ok(r.transaction)
                });
                if !lisp.is_empty() {
                    ctx.messages
                        .push(crate::tr!("msg-lisp-formulas", lhs = lisp.join(", ")));
                }
                r
            },
        ),
        cmd(
            "table.insertRow",
            "Insert Row",
            "Table",
            &["alt+shift+down"],
            Some(TABLE),
            |ctx, args| {
                let below = arg_bool(args, "below");
                ctx.org(|d, p, _| t::insert_row(d, p, below))
            },
        ),
        cmd(
            "table.killRow",
            "Delete Row",
            "Table",
            &["alt+shift+up"],
            Some(TABLE),
            |ctx, _| ctx.org(|d, p, _| t::kill_row(d, p)),
        ),
        cmd(
            "table.moveRowUp",
            "Move Row Up",
            "Table",
            &["alt+up"],
            Some(TABLE),
            |ctx, _| ctx.org(|d, p, _| t::move_row(d, p, true)),
        ),
        cmd(
            "table.moveRowDown",
            "Move Row Down",
            "Table",
            &["alt+down"],
            Some(TABLE),
            |ctx, _| ctx.org(|d, p, _| t::move_row(d, p, false)),
        ),
        cmd(
            "table.insertHline",
            "Insert Horizontal Rule",
            "Table",
            &[],
            Some(TABLE),
            |ctx, args| {
                let above = arg_bool(args, "above");
                ctx.org(|d, p, _| t::insert_hline(d, p, above))
            },
        ),
        cmd(
            "table.insertColumn",
            "Insert Column",
            "Table",
            &["alt+shift+right"],
            Some(TABLE),
            |ctx, _| ctx.org(|d, p, _| t::insert_column(d, p)),
        ),
        cmd(
            "table.deleteColumn",
            "Delete Column",
            "Table",
            &["alt+shift+left"],
            Some(TABLE),
            |ctx, _| ctx.org(|d, p, _| t::delete_column(d, p)),
        ),
        cmd(
            "table.moveColumnLeft",
            "Move Column Left",
            "Table",
            &["alt+left"],
            Some(TABLE),
            |ctx, _| ctx.org(|d, p, _| t::move_column(d, p, true)),
        ),
        cmd(
            "table.moveColumnRight",
            "Move Column Right",
            "Table",
            &["alt+right"],
            Some(TABLE),
            |ctx, _| ctx.org(|d, p, _| t::move_column(d, p, false)),
        ),
        cmd(
            "table.nextField",
            "Next Field",
            "Table",
            &["tab"],
            Some(TABLE),
            |ctx, _| {
                ctx.org(|d, p, _| t::next_field(d, p))?;
                auto_recalc(ctx);
                Ok(())
            },
        ),
        cmd(
            "table.previousField",
            "Previous Field",
            "Table",
            &["shift+tab"],
            Some(TABLE),
            |ctx, _| {
                ctx.org(|d, p, _| t::previous_field(d, p))?;
                auto_recalc(ctx);
                Ok(())
            },
        ),
        cmd(
            "table.nextRow",
            "Next Row",
            "Table",
            &["enter"],
            Some(TABLE),
            |ctx, _| {
                ctx.org(|d, p, _| t::next_row(d, p))?;
                auto_recalc(ctx);
                Ok(())
            },
        ),
        // Narrowing.
        cmd(
            "view.narrowToSubtree",
            "Narrow to Subtree",
            "View",
            &[],
            Some(ORG),
            |ctx, _| narrow(ctx, org_edit::narrow::subtree),
        ),
        cmd(
            "view.narrowToElement",
            "Narrow to Element",
            "View",
            &[],
            Some(ORG),
            |ctx, _| narrow(ctx, org_edit::narrow::element),
        ),
        cmd(
            "view.narrowToBlock",
            "Narrow to Block",
            "View",
            &[],
            Some(ORG),
            |ctx, _| narrow(ctx, org_edit::narrow::block),
        ),
        cmd("view.widen", "Widen", "View", &[], None, |ctx, _| {
            ctx.doc()?.narrowing = None;
            Ok(())
        }),
    ]
}

fn narrow(
    ctx: &mut EditorContext<'_>,
    range: fn(&org_model::Document, usize) -> Result<std::ops::Range<usize>, org_edit::EditError>,
) -> CommandResult {
    let doc = ctx.doc()?;
    let model = doc
        .model()
        .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
    let r = range(&model, doc.selection.head)?;
    doc.narrowing = Some(r);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default keys and when-clauses of every built-in command parse
    /// (debug builds stop on one that does not; R2.2).
    #[test]
    fn every_builtin_parses() {
        let reg = CommandRegistry::with_builtins();
        for c in reg.commands() {
            if c.source == CommandSource::Builtin {
                assert!(
                    !matches!(c.when, Some(WhenClause::Const(false))),
                    "{} has a when-clause that does not parse",
                    c.id
                );
            }
        }
        assert!(reg.commands().count() > 300);
    }

    #[test]
    fn planning_inputs() {
        assert_eq!(planning_input("2026-10-05"), ("2026-10-05", None));
        assert_eq!(
            planning_input("2026-10-05 10:00 +1w"),
            ("2026-10-05 10:00", Some("+1w".into()))
        );
        assert_eq!(
            planning_input("friday .+2d -3d"),
            ("friday", Some(".+2d -3d".into()))
        );
        assert_eq!(planning_input("+3d"), ("+3d", None));
    }
    use std::sync::Arc;
    use std::time::Instant;

    use serde_json::json;

    use crate::command::{Clipboard, CommandRegistry, EditorContext};
    use crate::document::{DocumentState, LineEnding, Metadata};
    use crate::mode::DocumentMode;

    fn doc(text: &str, point: usize) -> DocumentState {
        let meta = Metadata {
            path: None,
            mode: DocumentMode::Org,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let mut d = DocumentState::new(text, meta, Arc::new(org_model::Settings::default()));
        d.selection = org_edit::Selection::caret(point);
        d
    }

    #[test]
    fn org_commands_need_org_documents() {
        use crate::when::{Context, Value as V};
        let reg = CommandRegistry::with_builtins();
        let mut text = Context::default();
        text.set("editorMode", V::Str("text".into()));
        for flag in ["onHeadline", "inTable", "inList", "inBlock", "inSrcBlock"] {
            text.flag(flag, true);
        }
        for c in reg.commands() {
            let org_only = ["org.", "table.", "list."]
                .iter()
                .any(|p| c.id.starts_with(p))
                || matches!(
                    c.id.as_str(),
                    "view.fold"
                        | "view.foldAll"
                        | "view.narrowToSubtree"
                        | "view.narrowToElement"
                        | "view.narrowToBlock"
                );
            let applies = c.when.as_ref().is_none_or(|w| w.eval(&text));
            if org_only {
                assert!(!applies, "{} applies in a text file", c.id);
            }
        }
    }

    #[test]
    fn export_beside_the_file() {
        let dir = std::env::temp_dir().join(format!("kalem-export-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("n.org");
        std::fs::write(&path, "* Saved\n").unwrap();
        let mut d = DocumentState::open(
            &path,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        // Unsaved changes are exported too.
        d.insert_text("New ", Instant::now());
        let reg = CommandRegistry::with_builtins();
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("export.markdown", &mut ctx, &json!({}))
            .unwrap();
        assert!(ctx.messages[0].contains("n.md"));
        let md = std::fs::read_to_string(dir.join("n.md")).unwrap();
        assert!(md.contains("New * Saved") || md.contains("New"), "{md}");
        reg.execute("export.html", &mut ctx, &json!({})).unwrap();
        assert!(dir.join("n.html").is_file());
    }

    #[test]
    fn log_notes_and_footnote_startup() {
        let reg = CommandRegistry::with_builtins();
        let config = crate::settings::Config::default();
        let exec = |d: &mut DocumentState, id: &str, args: Value| {
            let mut clip = Clipboard::default();
            let mut ctx = EditorContext {
                document: Some(d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            reg.execute(id, &mut ctx, &args).unwrap();
            ctx.requests
        };
        // A note asked for, then stored with the answer.
        let mut d = doc("#+STARTUP: lognotedone\n* TODO A\n", 24);
        let r = exec(&mut d, "org.todo.done", json!({}));
        let Some(Request::Ask {
            command,
            mut args,
            arg,
        }) = r.into_iter().next()
        else {
            panic!("no note asked for");
        };
        assert_eq!((command.as_str(), arg.as_str()), ("org.note.add", "note"));
        args["note"] = "finished".into();
        exec(&mut d, &command, args);
        assert!(
            d.text()
                .as_str()
                .contains("- CLOSING NOTE [2026-09-28 Mon 10:00] \\\\\n  finished"),
            "{}",
            d.text().as_str()
        );
        // Rescheduling logged at once.
        let mut d = doc(
            "#+STARTUP: logreschedule\n* TODO A\nSCHEDULED: <2026-10-01 Thu>\n",
            26,
        );
        assert!(exec(&mut d, "org.schedule", json!({"date": "2026-10-05"})).is_empty());
        assert!(
            d.text()
                .as_str()
                .contains("- Rescheduled from \"[2026-10-01 Thu]\" on [2026-09-28 Mon 10:00]")
        );
        // Add Note on its own.
        exec(&mut d, "org.note.add", json!({"note": "hi"}));
        assert!(
            d.text()
                .as_str()
                .contains("- Note taken on [2026-09-28 Mon 10:00] \\\\\n  hi")
        );
        // `fnconfirm` offers the next number; `fninline` defines in place.
        let mut d = doc("#+STARTUP: fnconfirm fninline\nText.\n", 34);
        let r = exec(&mut d, "org.footnote.new", json!({}));
        let Some(Request::Ask { args, .. }) = r.into_iter().next() else {
            panic!("no label asked for");
        };
        assert_eq!(args["label_default"], "1");
        exec(&mut d, "org.footnote.new", json!({"label": "x"}));
        assert_eq!(
            d.text().as_str(),
            "#+STARTUP: fnconfirm fninline\nText[fn:x:].\n"
        );
    }

    #[test]
    fn todo_dependencies_and_tag_triggers() {
        let reg = CommandRegistry::with_builtins();
        let config = crate::settings::Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "[org]\nenforce_todo_dependencies = true\nenforce_todo_checkbox_dependencies = true\ntodo_state_tags_triggers = [\"done: -next +closed\", \"todo: -closed\"]\n",
        )]);
        let exec = |d: &mut DocumentState, id: &str| {
            let mut clip = Clipboard::default();
            let mut ctx = EditorContext {
                document: Some(d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            let r = reg.execute(id, &mut ctx, &json!({}));
            (r, ctx.messages)
        };
        // An open task below blocks, and so does an unchecked box.
        let mut d = doc("* TODO A :next:\n** TODO b\n* TODO C\n- [ ] box\n", 0);
        let (r, _) = exec(&mut d, "org.todo.done");
        assert!(r.unwrap_err().message.contains("blocked (by \"TODO b\")"));
        d.selection = org_edit::Selection::caret(d.text().as_str().find("* TODO C").unwrap());
        let (r, _) = exec(&mut d, "org.todo.done");
        assert!(r.unwrap_err().message.contains("contained checkboxes"));
        // Done below: the triggers set and remove tags.
        d.selection = org_edit::Selection::caret(d.text().as_str().find("** TODO b").unwrap());
        exec(&mut d, "org.todo.done").0.unwrap();
        d.selection = org_edit::Selection::caret(0);
        exec(&mut d, "org.todo.done").0.unwrap();
        let first = d.text().as_str().lines().next().unwrap().to_string();
        assert!(
            first.starts_with("* DONE A") && first.ends_with(":closed:"),
            "{first}"
        );
        // ORDERED, toggled: the second task waits for the first.
        let mut d = doc("* P\n** TODO one\n** TODO two\n", 0);
        let (r, m) = exec(&mut d, "org.todo.toggleOrdered");
        r.unwrap();
        assert_eq!(m.len(), 1);
        assert!(d.text().as_str().contains(":ORDERED:  t"));
        d.selection = org_edit::Selection::caret(d.text().as_str().find("** TODO two").unwrap());
        assert!(exec(&mut d, "org.todo.done").0.is_err());
        d.selection = org_edit::Selection::caret(0);
        exec(&mut d, "org.todo.toggleOrdered").0.unwrap();
        assert_eq!(d.text().as_str(), "* P\n** TODO one\n** TODO two\n");
    }

    #[test]
    fn pdf_messages() {
        use crate::pdf::{Compiled, Problem};
        crate::l10n::set_language("en");
        let org = std::path::Path::new("/d/notes.org");
        let warn = Problem {
            tex_line: Some(3),
            org_line: Some(2),
            message: "Reference undefined".into(),
            error: false,
        };
        let ok = super::pdf_result(
            org,
            Ok(Compiled {
                pdf: Some("/d/notes.pdf".into()),
                problems: vec![warn.clone(), warn.clone()],
            }),
            true,
        );
        assert_eq!(ok.message, "Exported /d/notes.pdf (2 LaTeX warnings)");
        assert!(!ok.error && ok.open.is_some());
        let clean = super::pdf_result(
            org,
            Ok(Compiled {
                pdf: Some("/d/notes.pdf".into()),
                problems: vec![],
            }),
            false,
        );
        assert_eq!(clean.message, "Exported /d/notes.pdf");
        let err = Problem {
            error: true,
            message: "Undefined control sequence.".into(),
            ..warn
        };
        let bad = super::pdf_result(
            org,
            Ok(Compiled {
                pdf: None,
                problems: vec![err.clone(), err],
            }),
            false,
        );
        assert_eq!(
            bad.message,
            "notes.org:2: Undefined control sequence. (and 1 more error)"
        );
        assert!(bad.error);
    }

    #[test]
    fn tables_recalculate_when_a_field_is_left() {
        let run = |text: &str, at: usize, config: &str, keys: &[&str]| -> String {
            let mut d = doc(text, at);
            let reg = CommandRegistry::with_builtins();
            let mut clip = Clipboard::default();
            let config = crate::settings::Config::from_layers(&[(
                crate::settings::Layer::User,
                None,
                config,
            )]);
            let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
            let mut ctx = EditorContext {
                document: Some(&mut d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock,
                messages: Vec::new(),
                requests: Vec::new(),
            };
            for k in keys {
                reg.execute(k, &mut ctx, &json!({})).unwrap();
            }
            d.text().as_str().to_string()
        };
        let t = "| 2 | 3 | |\n#+TBLFM: $3=$1*$2\n";
        // Off by default: F9 computes.
        assert!(!run(t, 2, "", &["table.nextField"]).contains('6'));
        let on = "[org]\ntable_auto_recalc = true\n";
        let out = run(t, 2, on, &["table.nextField"]);
        assert!(out.starts_with("| 2 | 3 | 6 |"), "{out}");
        let out = run(t, 6, on, &["table.previousField"]);
        assert!(out.starts_with("| 2 | 3 | 6 |"), "{out}");
        // A table without formulas is left alone.
        let plain = "| a | b |\n";
        assert!(run(plain, 2, on, &["table.nextField"]).starts_with("| a | b |"));
    }

    #[test]
    fn no_formatting_beyond_org() {
        // What Org cannot express is not offered in it (T2.13.13).
        let reg = CommandRegistry::with_builtins();
        for id in [
            "format.font",
            "format.color",
            "format.align",
            "format.lineSpacing",
            "format.allowMarkup",
            "file.saveAsOrg",
            "file.makeKalemDocument",
        ] {
            assert!(reg.get(id).is_none(), "{id}");
        }
    }

    #[test]
    fn export_settings_and_dialog() {
        use crate::command::Request;
        let dir = std::env::temp_dir().join(format!("kalem-export-set-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.org");
        std::fs::write(&path, "* A\nThe $x^2$ formula.\n").unwrap();
        let mut d = DocumentState::open(
            &path,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        let reg = CommandRegistry::with_builtins();
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "[export]\nbody_only = true\nopen_after = true\nmath = \"svg\"\n",
        )]);
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("export.html", &mut ctx, &json!({})).unwrap();
        let html = std::fs::read_to_string(dir.join("m.html")).unwrap();
        assert!(!html.contains("<html"), "body only: {html}");
        assert!(html.contains("data:image/svg+xml"), "SVG formulas: {html}");
        assert!(
            matches!(&ctx.requests[..], [Request::OpenLink(crate::input::LinkAction::Url(u))] if u.starts_with("file://") && u.ends_with("m.html")),
            "{:?}",
            ctx.requests
        );
        // The dialog: formats, then the settings with their values.
        ctx.requests.clear();
        reg.execute("export.dialog", &mut ctx, &json!({})).unwrap();
        assert_eq!(ctx.requests, vec![Request::ExportDialog]);
        let items = crate::export_dialog_items(&config);
        assert_eq!(items[0].id, "export.html");
        assert!(
            items
                .iter()
                .any(|i| i.id == "export.toggleMath" && i.title.contains("SVG"))
        );
        ctx.requests.clear();
        reg.execute("export.toggleBodyOnly", &mut ctx, &json!({}))
            .unwrap();
        assert_eq!(
            ctx.requests,
            vec![
                Request::SetSetting {
                    key: "export.body_only".into(),
                    value: json!(false),
                    quiet: false,
                },
                Request::ExportDialog
            ]
        );
    }

    #[test]
    fn builtins_run_and_undo() {
        let reg = CommandRegistry::with_builtins();
        assert!(reg.commands().count() > 50);
        let mut d = doc("* A\n* B\n", 0);
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("org.headline.moveSubtreeDown", &mut ctx, &json!({}))
            .unwrap();
        reg.execute("org.todo.cycle", &mut ctx, &json!({})).unwrap();
        reg.execute("org.tags.toggle", &mut ctx, &json!({"tag": "x"}))
            .unwrap();
        let err = reg
            .execute("org.todo.set", &mut ctx, &json!({"state": "NOPE"}))
            .unwrap_err();
        assert!(err.message.contains("not valid"));
        assert!(reg.execute("org.todo.set", &mut ctx, &json!({})).is_err());
        drop(ctx);
        assert!(d.text().as_str().starts_with("* B\n* TODO A"));
        d.undo();
        d.undo();
        d.undo();
        assert_eq!(d.text().as_str(), "* A\n* B\n");
    }

    #[test]
    fn markdown_commands() {
        let reg = CommandRegistry::with_builtins();
        let mut d = doc("| a | b |\n|---|---|\n| 1 | 2 |\n\n- [ ] task\n", 2);
        d.set_mode(
            DocumentMode::Markdown,
            &crate::settings::Config::default().parse_base(),
        );
        assert_eq!(
            d.when_context().get("inMarkdownTable"),
            Some(&crate::when::Value::Bool(true))
        );
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 1).at(9, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("markdown.table.nextField", &mut ctx, &json!({}))
            .unwrap();
        drop(ctx);
        assert_eq!(
            d.text().as_str(),
            "| a   | b   |\n| --- | --- |\n| 1   | 2   |\n\n- [ ] task\n"
        );
        assert_eq!(
            &d.text().as_str()[d.selection.head..d.selection.head + 1],
            "b"
        );
        d.selection = org_edit::Selection::caret(d.text().len() - 3);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 1).at(9, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("markdown.toggleCheckbox", &mut ctx, &json!({}))
            .unwrap();
        drop(ctx);
        assert!(d.text().as_str().ends_with("- [x] task\n"));
    }

    #[test]
    fn markdown_links_and_wiki_completion() {
        let dir = std::env::temp_dir().join(format!("kalem-md-links-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Garden Notes.md"), "x").unwrap();
        let path = dir.join("index.md");
        std::fs::write(&path, "See [[Garden").unwrap();
        let base = crate::settings::Config::default().parse_base();
        let mut d =
            DocumentState::open(&path, Arc::new(org_model::Settings::default()), &base).unwrap();
        d.selection = org_edit::Selection::caret(d.text().len());
        let items = crate::completers::Registry::with_builtins().complete(
            &mut d,
            false,
            std::time::Duration::from_millis(200),
        );
        let wiki: Vec<_> = items.iter().filter(|i| i.source == "wiki").collect();
        assert_eq!(wiki.len(), 1, "{items:?}");
        assert_eq!(wiki[0].insert, "Garden Notes]]");
        // Open Link on a wiki link asks the frontend to open the page.
        let reg = CommandRegistry::with_builtins();
        let config = crate::settings::Config::default();
        let mut clip = Clipboard::default();
        let mut tx = org_edit::Transaction::new("t");
        tx.replace(d.text().len()..d.text().len(), " Notes]]")
            .unwrap();
        d.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
        d.selection = org_edit::Selection::caret(8);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 1).at(9, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("markdown.openLink", &mut ctx, &json!({}))
            .unwrap();
        assert!(matches!(
            ctx.requests.last(),
            Some(Request::OpenLink(crate::input::LinkAction::File { path, .. })) if path == "Garden Notes.md"
        ));
        reg.execute("edit.copyHtml", &mut ctx, &json!({})).unwrap();
        assert!(matches!(ctx.requests.last(), Some(Request::CopyText(h)) if h.contains("<p>See")));
        drop(ctx);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn language_packs_in_the_editor() {
        // A pack's four hooks reach the editor (T2.7a.7).
        crate::packs::register(Arc::new(crate::packs::tests::IniPack));
        let reg = CommandRegistry::with_builtins();
        let mut d = doc("[a]\nx=1\n[b]\ny  =2\n", 0);
        d.set_mode(
            DocumentMode::Text {
                language: Some("kalem-test-ini".into()),
            },
            &crate::settings::Config::default().parse_base(),
        );
        let items = crate::packs::outline_items(&d).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(
            d.when_context().get("hasFormatter"),
            Some(&crate::when::Value::Bool(true))
        );
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 1).at(9, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("edit.formatDocument", &mut ctx, &json!({}))
            .unwrap();
        drop(ctx);
        assert_eq!(d.text().as_str(), "[a]\nx = 1\n[b]\ny = 2\n");
        assert_eq!(crate::formulas::selection_stats(&d), None);
        // A syntax error: in the status bar, and Format Document refuses
        // with the cursor at it.
        d.selection = org_edit::Selection::caret(d.text().len());
        let mut tx = org_edit::Transaction::new("t");
        tx.replace(d.text().len()..d.text().len(), "oops\n")
            .unwrap();
        d.apply(&tx, org_edit::ChangeKind::Command, Instant::now());
        d.selection = org_edit::Selection::caret(0);
        assert_eq!(
            crate::formulas::selection_stats(&d).as_deref(),
            Some("One problem")
        );
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 1).at(9, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        let err = reg
            .execute("edit.formatDocument", &mut ctx, &json!({}))
            .unwrap_err();
        assert!(err.message.contains("oops"));
        drop(ctx);
        assert_eq!(d.selection.head, 20);
        assert_eq!(
            crate::formulas::selection_stats(&d).as_deref(),
            Some("No value: oops")
        );
    }

    #[test]
    fn show_in_pdf_goes_to_the_lines_page() {
        // T2.7h.24: the built PDF, at the page SyncTeX gives the cursor's
        // line.
        let dir = std::env::temp_dir().join(format!("kalem-show-pdf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tex = dir.join("main.tex");
        let text =
            "\\documentclass{article}\n\\begin{document}\nOne.\n\\newpage\nTwo.\n\\end{document}\n";
        std::fs::write(&tex, text).unwrap();
        std::fs::write(dir.join("main.pdf"), "%PDF-1.4\n").unwrap();
        std::fs::write(
            dir.join("main.synctex"),
            format!(
                "SyncTeX Version:1\nInput:1:{}\nUnit:1\nContent:\n{{1\n(1,3:100,100:1000,10,0\n)\n}}1\n{{2\n(1,5:100,6578176:1000,0,0\n)\n}}2\n",
                dir.join("./main.tex").display()
            ),
        )
        .unwrap();
        let reg = CommandRegistry::with_builtins();
        let mut d = DocumentState::open(
            &tex,
            std::sync::Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        d.move_cursor(text.find("Two").unwrap(), false);
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 3).at(10, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("latex.showInPdf", &mut ctx, &json!({}))
            .unwrap();
        assert_eq!(
            ctx.requests,
            vec![Request::OpenAt {
                path: dir.join("main.pdf").display().to_string(),
                line: 2,
                column: 100,
            }]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn csv_spreadsheet_commands() {
        let reg = CommandRegistry::with_builtins();
        let mut d = doc("name,n\nAda,1\nBob,\nCy,\nAda,1\n", 0);
        d.set_mode(
            DocumentMode::Csv,
            &crate::settings::Config::default().parse_base(),
        );
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 10, 1).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        let mut run = |id: &str, args: serde_json::Value| {
            let r = reg.execute(id, &mut ctx, &args);
            let d = ctx.document.as_deref().unwrap();
            r.map(|_| (d.text().as_str().to_string(), d.selection.head))
        };
        // Go to a cell by its spreadsheet or Org name.
        assert_eq!(run("csv.goToCell", json!({"cell": "B3"})).unwrap().1, 17);
        assert_eq!(run("csv.goToCell", json!({"cell": "@3$2"})).unwrap().1, 17);
        assert!(run("csv.goToCell", json!({"cell": "B9"})).is_err());
        // Fill the cell from the one above, then a series.
        let (t, _) = run("csv.fillDown", json!({})).unwrap();
        assert_eq!(t, "name,n\nAda,1\nBob,1\nCy,\nAda,1\n");
        run("csv.goToCell", json!({"cell": "B4"})).unwrap();
        let (t, _) = run("csv.fillSeries", json!({})).unwrap();
        assert_eq!(t, "name,n\nAda,1\nBob,1\nCy,1\nAda,1\n");
        let (t, _) = run("csv.sumColumn", json!({})).unwrap();
        assert_eq!(t, "name,n\nAda,1\nBob,1\nCy,1\nAda,1\n");
        let (t, _) = run("csv.removeDuplicates", json!({})).unwrap();
        assert_eq!(t, "name,n\nAda,1\nBob,1\nCy,1\n");
        // A histogram of the numbers; none in the names.
        run("csv.goToCell", json!({"cell": "B2"})).unwrap();
        assert!(run("csv.histogram", json!({})).is_ok());
        run("csv.goToCell", json!({"cell": "A2"})).unwrap();
        assert!(run("csv.histogram", json!({})).is_err());
        run("csv.goToCell", json!({"cell": "A1"})).unwrap();
        let (t, _) = run("csv.sortFileBy", json!({"columns": "B, -A"})).unwrap();
        assert_eq!(t, "name,n\nCy,1\nBob,1\nAda,1\n");
        assert!(run("csv.sortFileBy", json!({"columns": "?"})).is_err());
        // Join and split show the first rows as they would be first.
        let (t, _) = run("csv.joinColumns", json!({"separator": "-"})).unwrap();
        assert_eq!(t, "name,n\nCy,1\nBob,1\nAda,1\n");
        let (t, _) = run(
            "csv.joinColumns",
            json!({"separator": "-", "confirmed": true}),
        )
        .unwrap();
        assert_eq!(t, "name-n\nCy-1\nBob-1\nAda-1\n");
        let (t, _) = run("csv.splitColumn", json!({"separator": "-"})).unwrap();
        assert_eq!(t, "name-n\nCy-1\nBob-1\nAda-1\n");
        let (t, _) = run(
            "csv.splitColumn",
            json!({"separator": "-", "confirmed": true}),
        )
        .unwrap();
        assert_eq!(t, "name,n\nCy,1\nBob,1\nAda,1\n");
        assert!(run("csv.splitColumn", json!({"separator": "#"})).is_err());
        run("csv.cellCoordinates", json!({})).unwrap();
        // The view changes; the file does not.
        let before = run("csv.toggleRainbow", json!({})).unwrap().0;
        run("csv.toggleCoordinates", json!({})).unwrap();
        run("csv.toggleAlignment", json!({})).unwrap();
        let after = run("csv.toggleRainbow", json!({})).unwrap().0;
        assert_eq!(before, after);
        let (t, _) = run("csv.transpose", json!({})).unwrap();
        assert_eq!(t, "name,Cy,Bob,Ada\nn,1,1,1\n");
        let v = ctx.document.as_deref().unwrap().csv_view;
        assert!(v.coordinates && !v.rainbow && !v.align_numbers);
        let msgs = ctx.messages.clone();
        assert!(msgs.iter().any(|m| m.contains("Sum: 4")), "{msgs:?}");
        assert!(
            msgs.iter()
                .any(|m| m.contains("A2 (@2$1)") && m.contains("name")),
            "{msgs:?}"
        );
        assert!(msgs.iter().any(|m| m.contains("duplicate")), "{msgs:?}");
        assert!(
            ctx.requests
                .iter()
                .any(|r| matches!(r, Request::CopyText(t) if t == "4"))
        );
        let previews: Vec<_> = ctx
            .requests
            .iter()
            .filter_map(|r| match r {
                Request::Choose(items) => Some(items),
                _ => None,
            })
            .collect();
        let (histograms, previews): (Vec<_>, Vec<_>) = previews
            .into_iter()
            .partition(|items| items[0].title.contains('–'));
        assert_eq!(histograms.len(), 1);
        assert!(
            histograms[0][0].title.starts_with("1 – 2"),
            "{:?}",
            histograms[0][0].title
        );
        assert!(histograms[0][0].title.contains("     3"));
        assert_eq!(previews.len(), 2);
        assert_eq!(previews[0][0].title, "name-n");
        assert_eq!(previews[0][1].title, "Cy-1");
        assert_eq!(previews[1][0].title, "name  │  n");
        assert!(previews[1][0].id.contains("confirmed"));
    }

    #[test]
    fn csv_columns_view() {
        let reg = CommandRegistry::with_builtins();
        let text = "name,note,n\nAda,a long note here,1\nBob,x,2\n";
        let mut d = doc(text, 9);
        d.set_mode(
            DocumentMode::Csv,
            &crate::settings::Config::default().parse_base(),
        );
        d.csv_view = crate::csv::View {
            align_numbers: false,
            rainbow: false,
            coordinates: false,
            sheet: false,
        };
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 10, 3).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        let mut run = |id: &str, args: serde_json::Value| reg.execute(id, &mut ctx, &args);
        // The note column: hidden, then shown, then narrowed and autosized.
        run("csv.goToCell", json!({"cell": "B2"})).unwrap();
        run("csv.hideColumn", json!({})).unwrap();
        run("csv.goToCell", json!({"cell": "A2"})).unwrap();
        run("csv.hideColumn", json!({})).unwrap();
        // The last column that shows stays.
        assert!(run("csv.hideColumn", json!({})).is_err());
        let d = ctx.document.as_deref_mut().unwrap();
        assert_eq!(
            d.csv_columns.hidden.iter().copied().collect::<Vec<_>>(),
            vec![0, 1]
        );
        let line = |d: &crate::DocumentState, n: usize| {
            let t = d.text();
            let r = t.line_range(n);
            crate::csv::line_view(&crate::csv::layout(d), t.as_str(), r, None).display()
        };
        assert_eq!(line(d, 1), "1");
        d.csv_columns.hidden.clear();
        d.csv_columns.widths.insert(1, 6);
        assert_eq!(line(d, 1), "Ada  │ a lon… │ 1");
        assert_eq!(line(d, 2), "Bob  │ x      │ 2");
        d.csv_columns.hidden.insert(2);
        assert_eq!(line(d, 2), "Bob  │ x");
        d.csv_columns.frozen = true;
        assert_eq!(crate::csv::frozen_width(&crate::csv::layout(d)), Some(6));
        // Autosize: the widest value, uncut.
        d.csv_columns = crate::csv::Columns::default();
        assert_eq!(d.text().as_str(), text);
        let mut clip2 = Clipboard::default();
        let mut ctx = EditorContext {
            document: Some(d),
            clipboard: &mut clip2,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("csv.goToCell", &mut ctx, &json!({"cell": "B2"}))
            .unwrap();
        reg.execute("csv.autosizeColumn", &mut ctx, &json!({}))
            .unwrap();
        reg.execute("csv.narrowColumn", &mut ctx, &json!({}))
            .unwrap();
        let d = ctx.document.as_deref().unwrap();
        assert_eq!(d.csv_columns.widths.get(&1), Some(&14));
        // A column given, as dragging its edge gives it.
        reg.execute(
            "csv.setColumnWidth",
            &mut ctx,
            &json!({"width": "7", "column": 2}),
        )
        .unwrap();
        let d = ctx.document.as_deref().unwrap();
        assert_eq!(d.csv_columns.widths.get(&2), Some(&7));
        assert_eq!(d.csv_columns.widths.get(&1), Some(&14));
        assert_eq!(d.text().as_str(), text);
        // Paste as Block: the next paste writes over the cells.
        reg.execute("csv.pasteBlock", &mut ctx, &json!({})).unwrap();
        assert!(matches!(
            ctx.requests.last(),
            Some(Request::Paste { plain: false })
        ));
        let d = ctx.document.as_deref_mut().unwrap();
        d.paste("p\tq\nr\ts\n", None, false, Instant::now());
        assert_eq!(d.text().as_str(), "name,note,n\nAda,p,q\nBob,r,s\n");
        assert!(!d.csv_paste_block);
        // Copy Cells: the rectangle from the anchor's cell to the cursor's.
        d.selection = org_edit::Selection {
            anchor: d.text().as_str().find("p,").unwrap(),
            head: d.text().as_str().find('s').unwrap(),
        };
        reg.execute("csv.copyCells", &mut ctx, &json!({})).unwrap();
        assert!(matches!(
            ctx.requests.last(),
            Some(Request::CopyText(t)) if t == "p\tq\nr\ts\n"
        ));
        let d = ctx.document.as_deref().unwrap();
        let t = d.text().as_str();
        let ranges: Vec<&str> = crate::csv::rectangle_ranges(d)
            .unwrap()
            .into_iter()
            .map(|r| &t[r])
            .collect();
        assert_eq!(ranges, ["p,q", "r,s"]);
        // Cut Cells: copied, then emptied.
        reg.execute("csv.cutCells", &mut ctx, &json!({})).unwrap();
        let d = ctx.document.as_deref_mut().unwrap();
        assert_eq!(d.text().as_str(), "name,note,n\nAda,,\nBob,,\n");
        d.selection = org_edit::Selection::caret(d.text().len());
        // Once: the next paste inserts as before.
        d.paste("z", None, false, Instant::now());
        assert!(d.text().as_str().contains('z'));
    }

    #[test]
    fn csv_commands() {
        let reg = CommandRegistry::with_builtins();
        let mut d = doc("name,age\nAda,36\nBob,7\n", 9);
        d.set_mode(
            DocumentMode::Csv,
            &crate::settings::Config::default().parse_base(),
        );
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        let mut run = |id: &str, args: serde_json::Value| {
            let r = reg.execute(id, &mut ctx, &args);
            let d = ctx.document.as_deref().unwrap();
            r.map(|_| (d.text().as_str().to_string(), d.selection.head))
        };
        assert_eq!(run("csv.nextField", json!({})).unwrap().1, 13);
        assert_eq!(run("csv.nextField", json!({})).unwrap().1, 16);
        assert_eq!(run("csv.previousField", json!({})).unwrap().1, 13);
        let (t, at) = run("csv.moveRowDown", json!({})).unwrap();
        assert_eq!(t, "name,age\nBob,7\nAda,36\n");
        assert_eq!(at, 19);
        let (t, at) = run("csv.moveRowUp", json!({})).unwrap();
        assert_eq!(t, "name,age\nAda,36\nBob,7\n");
        assert_eq!(at, 13);
        // Not above the header.
        assert!(run("csv.moveRowUp", json!({})).is_err());
        let (t, _) = run("csv.moveRowDown", json!({})).unwrap();
        assert_eq!(t, "name,age\nBob,7\nAda,36\n");
        // By the column at the cursor, as numbers.
        let (t, _) = run("csv.sortFile", json!({"reverse": true})).unwrap();
        assert_eq!(t, "name,age\nAda,36\nBob,7\n");
        let (t, _) = run("csv.sortFile", json!({})).unwrap();
        assert_eq!(t, "name,age\nBob,7\nAda,36\n");
        run("csv.previousField", json!({})).unwrap();
        let (t, _) = run("csv.moveColumnRight", json!({})).unwrap();
        assert_eq!(t, "age,name\n7,Bob\n36,Ada\n");
        let (t, _) = run("csv.moveColumnLeft", json!({})).unwrap();
        assert_eq!(t, "name,age\nBob,7\nAda,36\n");
        let (t, _) = run("csv.insertColumn", json!({})).unwrap();
        assert_eq!(t, ",name,age\n,Bob,7\n,Ada,36\n");
        let (t, _) = run("csv.deleteColumn", json!({})).unwrap();
        assert_eq!(t, "name,age\nBob,7\nAda,36\n");
        let (t, _) = run("csv.insertRow", json!({})).unwrap();
        assert_eq!(t, "name,age\nBob,7\n,\nAda,36\n");
        let (t, _) = run("csv.deleteRow", json!({})).unwrap();
        assert_eq!(t, "name,age\nBob,7\nAda,36\n");
        run("csv.copyAsTsv", json!({})).unwrap();
        assert!(
            matches!(ctx.requests.last(), Some(Request::CopyText(t)) if t == "name\tage\nBob\t7\nAda\t36")
        );
        drop(ctx);
        // Tab past the last field: a new row.
        d.selection = org_edit::Selection::caret(d.text().len() - 1);
        let mut clip = Clipboard::default();
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("csv.nextField", &mut ctx, &json!({})).unwrap();
        drop(ctx);
        assert_eq!(d.text().as_str(), "name,age\nBob,7\nAda,36\n,\n");
        // The column's numbers in the status bar.
        d.selection = org_edit::Selection::caret(13);
        let s = crate::formulas::selection_stats(&d).unwrap();
        assert!(s.contains("43"), "{s}");
        // Spreadsheet rows pasted with the file's delimiter.
        d.paste("x\t1\ny\t2", None, false, Instant::now());
        assert!(d.text().as_str().contains("x,1\ny,2"));
    }

    #[test]
    fn narrowed_commands() {
        let reg = CommandRegistry::with_builtins();
        let mut d = doc("* P\n** b\n** a\n* Q\n** z\n** y\n", 0);
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("view.narrowToSubtree", &mut ctx, &json!({}))
            .unwrap();
        reg.execute("org.headline.sort", &mut ctx, &json!({"by": "a"}))
            .unwrap();
        drop(ctx);
        // Emacs ends the narrowed subtree with a line feed before sorting.
        assert_eq!(d.text().as_str(), "* P\n** a\n** b\n\n* Q\n** z\n** y\n");
        assert_eq!(
            d.narrowing.clone().map(|r| &d.text().as_str()[r]),
            Some("* P\n** a\n** b\n")
        );
    }

    #[test]
    fn heading_levels_and_tables() {
        let reg = CommandRegistry::with_builtins();
        let mut d = doc("Title\n", 0);
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("org.headline.setLevel", &mut ctx, &json!({"level": 2}))
            .unwrap();
        reg.execute("org.headline.setLevel", &mut ctx, &json!({"level": 1}))
            .unwrap();
        drop(ctx);
        assert_eq!(d.text().as_str(), "* Title\n");
        d.selection = org_edit::Selection::caret(d.text().len());
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("table.create", &mut ctx, &json!({"columns": 2, "rows": 1}))
            .unwrap();
        drop(ctx);
        assert_eq!(d.text().as_str(), "* Title\n|   |   |\n");
    }

    #[test]
    fn settings_reach_commands() {
        let reg = CommandRegistry::with_builtins();
        let toml =
            "[org]\ntodo_keywords = [\"TODO\", \"NEXT\", \"|\", \"DONE\"]\nlog_done = \"time\"\n";
        let config =
            crate::settings::Config::from_layers(&[(crate::settings::Layer::User, None, toml)]);
        let meta = Metadata {
            path: None,
            mode: DocumentMode::Org,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let mut d = DocumentState::with_base(
            "* A\n",
            meta,
            Arc::new(org_model::Settings::default()),
            &config.parse_base(),
        );
        let mut clip = Clipboard::default();
        let clock = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        for _ in 0..3 {
            reg.execute("org.todo.cycle", &mut ctx, &json!({})).unwrap();
        }
        drop(ctx);
        assert_eq!(
            d.text().as_str(),
            "* DONE A\nCLOSED: [2026-09-28 Mon 10:00]\n"
        );
    }

    #[test]
    fn every_command_has_a_scope() {
        let reg = CommandRegistry::with_builtins();
        assert!(reg.commands().all(|c| c.scope.is_some()));
        let mut c = reg.get("edit.undo").unwrap().clone();
        c.id = "myPlugin.noScope".into();
        c.scope = None;
        assert!(CommandRegistry::new().register(c).is_err());
        // The scopes at work: Org's in Org text only, the line commands in
        // code, source blocks of Org documents included.
        use crate::command::Scope;
        let get = |id: &str| reg.get(id).unwrap().scope.clone().unwrap();
        assert_eq!(get("org.emphasis.bold"), Scope::only(&["org"]));
        assert_eq!(get("lines.moveUp"), Scope::except(&["org", "csv"]));
        assert_eq!(get("edit.undo"), Scope::all());
        let mut d = doc("* A\n#+begin_src python\nx = 1\n#+end_src\n", 26);
        assert_eq!(d.text_type(), "python");
        let ctx = d.when_context();
        let when = |id: &str| {
            reg.get(id)
                .unwrap()
                .when
                .as_ref()
                .is_none_or(|w| w.eval(&ctx))
        };
        assert!(!when("org.emphasis.bold"));
        assert!(when("lines.moveUp"));
        // A document without a file is an Org document.
        d.selection = org_edit::Selection::caret(1);
        assert_eq!(d.text_type(), "org");
    }

    #[test]
    fn registry_rules() {
        let mut reg = CommandRegistry::with_builtins();
        let mut c = reg.get("edit.undo").unwrap().clone();
        assert!(reg.register(c.clone()).is_err());
        c.id = "Bad.id".into();
        assert!(reg.register(c.clone()).is_err());
        c.id = "myPlugin.doThing".into();
        assert!(reg.register(c).is_ok());
    }

    #[test]
    fn plugin_sources_from_the_menu() {
        // Plugin Sources lists the user's indexes and the official one; a
        // source is asked for, added first, and removed (asked by the
        // owner, 2026-10-05: forks and own plugins from an address the user
        // sets).
        let reg = CommandRegistry::with_builtins();
        let mut clip = crate::command::Clipboard::default();
        let config = crate::Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "plugins.sources = [\"https://example.com/old.json\"]\n",
        )]);
        let mut run = |id: &str, args: serde_json::Value| {
            let mut ctx = EditorContext {
                document: None,
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 10, 5).at(9, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            reg.execute(id, &mut ctx, &args).unwrap();
            ctx.requests
        };
        let req = run("plugin.sources", json!({}));
        let [Request::Choose(items)] = &req[..] else {
            panic!("{req:?}")
        };
        assert_eq!(items.len(), 3);
        assert!(items[1].id.contains("plugin.removeSource"));
        assert!(items[2].title.contains(crate::plugin_store::DEFAULT_INDEX));
        let req = run("plugin.addSource", json!({}));
        assert!(matches!(&req[..], [Request::Ask { arg, .. }] if arg == "url"));
        let req = run(
            "plugin.addSource",
            json!({ "url": "https://github.com/ada/p/blob/main/index.json" }),
        );
        assert!(matches!(&req[..], [Request::SetSetting { key, value, .. }]
            if key == "plugins.sources"
                && *value == json!(["https://github.com/ada/p/blob/main/index.json", "https://example.com/old.json"])));
        let req = run(
            "plugin.removeSource",
            json!({ "url": "https://example.com/old.json" }),
        );
        assert!(matches!(&req[..], [Request::SetSetting { value, .. }] if *value == json!([])));
        let req = run("plugin.setIndex", json!({ "url": "" }));
        assert!(matches!(&req[..], [Request::SetSetting { key, value, .. }]
            if key == "plugins.index" && *value == json!(crate::plugin_store::DEFAULT_INDEX)));
    }

    #[test]
    fn online_search_addresses() {
        assert_eq!(super::url_encode("org mode ç&x"), "org+mode+%C3%A7%26x");
    }

    #[test]
    fn read_only_documents() {
        let mut d = doc("one two\n", 0);
        let (reg, mut clip, config) = (
            CommandRegistry::with_builtins(),
            crate::command::Clipboard::default(),
            crate::settings::Config::default(),
        );
        let mut run = |d: &mut DocumentState, id: &str| {
            let mut ctx = EditorContext {
                document: Some(d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 9, 30).at(9, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            reg.execute(id, &mut ctx, &serde_json::Value::Null).unwrap();
            ctx.requests
        };
        run(&mut d, "view.toggleReadOnly");
        assert!(d.read_only);
        d.insert_text("x", Instant::now());
        assert_eq!(d.text().as_str(), "one two\n");
        d.move_cursor(4, false);
        assert_eq!(d.selection.head, 4);
        assert!(d.undo().is_none());
        run(&mut d, "view.toggleReadOnly");
        d.insert_text("x", Instant::now());
        assert_eq!(d.text().as_str(), "one xtwo\n");
        // The toggles are settings.
        let req = run(&mut d, "view.toggleLineNumbers");
        assert!(matches!(&req[..], [Request::SetSetting { key, value, .. }]
            if key == "editor.line_numbers" && *value == serde_json::Value::Bool(false)));
        let req = run(&mut d, "view.bigText");
        assert!(matches!(&req[..], [Request::SetSetting { value, .. }] if *value == 24));
        let req = run(&mut d, "view.bigText");
        assert!(matches!(&req[..], [Request::SetSetting { value, .. }] if *value == 16));
    }

    #[test]
    fn open_keys_in_the_system() {
        let dir = std::env::temp_dir().join(format!("kalem-open-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::create_dir_all(dir.join("notes")).unwrap();
        let file = dir.join("notes/a.org");
        std::fs::write(&file, "* A\nText.\n").unwrap();
        let mut d = DocumentState::open(
            &file,
            Arc::new(org_model::Settings::default()),
            &Default::default(),
        )
        .unwrap();
        let (reg, mut clip, config) = (
            CommandRegistry::with_builtins(),
            Clipboard::default(),
            crate::settings::Config::default(),
        );
        let mut run = |d: &mut DocumentState, id: &str, args: serde_json::Value| {
            let mut ctx = EditorContext {
                document: Some(d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 9, 30).at(9, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            reg.execute(id, &mut ctx, &args).unwrap();
            ctx.requests
        };
        // `SPC o b`: the HTML written beside it and opened.
        let req = run(&mut d, "export.htmlBrowser", json!({}));
        assert!(dir.join("notes/a.html").is_file());
        assert!(
            matches!(&req[..], [Request::OpenLink(crate::input::LinkAction::Url(u))]
            if u.ends_with("a.html"))
        );
        // `SPC o t` here, `SPC o T` at the project's root.
        let req = run(&mut d, "app.terminal", json!({}));
        assert!(matches!(&req[..], [Request::Terminal(p)] if p.ends_with("notes")));
        let req = run(&mut d, "app.terminal", json!({ "project": true }));
        assert!(matches!(&req[..], [Request::Terminal(p)] if !p.ends_with("notes")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_files() {
        let dir = std::env::temp_dir().join(format!("kalem-other-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in ["a.org", "a.html", "a.pdf", "b.org"] {
            std::fs::write(dir.join(f), "").unwrap();
        }
        assert_eq!(
            super::other_file(&dir.join("a.org")),
            Some(dir.join("a.html"))
        );
        assert_eq!(
            super::other_file(&dir.join("a.html")),
            Some(dir.join("a.pdf"))
        );
        assert_eq!(
            super::other_file(&dir.join("a.pdf")),
            Some(dir.join("a.org"))
        );
        assert_eq!(super::other_file(&dir.join("b.org")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn characters_described() {
        assert_eq!(super::describe_char('ç'), "ç  U+00E7  UTF-8 C3 A7");
        assert_eq!(super::describe_char('\n'), "\\n  U+000A  UTF-8 0A");
    }

    #[test]
    fn format_document_as_kalem_fmt() {
        let mut d = doc("* A\n| a |b|\n| ccc | d |\n\n\n", 0);
        let (reg, mut clip, config) = (
            CommandRegistry::with_builtins(),
            Clipboard::default(),
            crate::settings::Config::default(),
        );
        for id in ["edit.formatDocument", "edit.trimTrailingBlankLines"] {
            let mut ctx = EditorContext {
                document: Some(&mut d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 9, 30).at(9, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            reg.execute(id, &mut ctx, &serde_json::Value::Null).unwrap();
        }
        assert_eq!(d.text().as_str(), "* A\n| a   | b |\n| ccc | d |\n");
    }
}
