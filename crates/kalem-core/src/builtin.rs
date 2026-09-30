//! The built-in commands: every command of `org-edit`, and undo and redo.
//! Their default keys are those of the Word-like profile (§7.3); other
//! profiles come from keymaps.

use serde_json::Value;

use crate::command::{
    Command, CommandError, CommandHandler, CommandResult, CommandSource, EditorContext, PickKind,
    ProjectRequest, Request,
};
use crate::keys::KeySequence;
use crate::when::WhenClause;

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
        default_keys: keys
            .iter()
            .map(|k| KeySequence::parse(k).expect("valid default key"))
            .collect(),
        when: when.map(|w| WhenClause::parse(w).expect("valid when-clause")),
        handler: CommandHandler::Native(handler),
        args_schema: None,
        source: CommandSource::Builtin,
        scope: None,
    }
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
        ("csv.filter", object(&[("text", "string", true)])),
        ("csv.sortView", object(&[("reverse", "boolean", false)])),
        ("csv.setDelimiter", object(&[("delimiter", "string", true)])),
        ("csv.setQuote", object(&[("quote", "string", true)])),
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
    all.extend(csv_commands());
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
    all.extend(latex_commands());
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
    Ok(())
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
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-latex")))?;
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
            let at = r
                .fields
                .get(col)
                .or(r.fields.last())
                .map_or(r.range.start, csv_caret);
            d.selection = org_edit::Selection::caret(at);
        }
    }
    Ok(())
}

/// Builds the PDF of the LaTeX document in the background: its project's
/// root document, with the engine and output folder the document and the
/// settings ask for (T2.7h.22).
fn latex_build(ctx: &mut EditorContext<'_>) -> CommandResult {
    let doc = ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new(crate::tr!("msg-no-document")))?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let path = std::path::absolute(&path).unwrap_or(path);
    let text = doc.text().as_str().to_string();
    let disk = latex_model::project::Disk;
    let root = latex_model::project::find_root(&path, &text, &disk, None, None);
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
    let open_after = ctx.config.bool("export.open_after");
    let status = crate::l10n::tr("msg-compiling-pdf");
    ctx.messages.push(status.clone());
    crate::jobs::spawn(status, move || {
        let out_dir = (!out.is_empty()).then(|| std::path::PathBuf::from(&out));
        match crate::latex_build::build(&root, engine, out_dir.as_deref()) {
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
                    match b.pdf {
                        Some(pdf) => crate::jobs::Finished {
                            message: crate::tr!(
                                "msg-latex-built",
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
            latex_insert(ctx, |_, indent, _| {
                let stem = std::path::Path::new(&path)
                    .file_stem()
                    .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
                let head = format!(
                    "{indent}\\begin{{figure}}[htbp]\n{indent}  \\centering\n{indent}  \\includegraphics[width={width}]{{{path}}}\n{indent}  \\caption{{{caption}"
                );
                let tail =
                    format!("}}\n{indent}  \\label{{fig:{stem}}}\n{indent}\\end{{figure}}\n");
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
            latex_insert(ctx, move |_, indent, model| {
                // booktabs' rules when the document loads it.
                let booktabs = model.packages.iter().any(|p| p.name == "booktabs");
                let (top, mid, bottom) = if booktabs {
                    ("\\toprule", "\\midrule", "\\bottomrule")
                } else {
                    ("\\hline", "\\hline", "\\hline")
                };
                let row = format!("{indent}    {} \\\\\n", vec![""; columns].join(" & "));
                let head = format!(
                    "{indent}\\begin{{table}}[htbp]\n{indent}  \\centering\n{indent}  \\caption{{"
                );
                let mut t = head.clone();
                t.push_str(&format!(
                    "}}\n{indent}  \\label{{tab:}}\n{indent}  \\begin{{tabular}}{{{}}}\n{indent}    {top}\n",
                    "l".repeat(columns)
                ));
                t.push_str(&row);
                t.push_str(&format!("{indent}    {mid}\n"));
                for _ in 1..rows {
                    t.push_str(&row);
                }
                t.push_str(&format!(
                    "{indent}    {bottom}\n{indent}  \\end{{tabular}}\n{indent}\\end{{table}}\n"
                ));
                (t, head.len())
            })
        }),
        c("latex.insert.equation", "Insert Equation", &[], |ctx, _| {
            latex_insert(ctx, |_, indent, _| {
                let head = format!("{indent}\\begin{{equation}}\n{indent}  ");
                (
                    format!("{head}\n{indent}  \\label{{eq:}}\n{indent}\\end{{equation}}\n"),
                    head.len(),
                )
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
    let root =
        latex_model::project::find_root(&path, &text, &latex_model::project::Disk, None, None);
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
    let pos = d.selection.head;
    let line_start = text[..pos].rfind('\n').map_or(0, |i| i + 1);
    let line_end = text[pos..].find('\n').map_or(text.len(), |i| pos + i);
    let line = &text[line_start..line_end];
    let indent: String = line
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect();
    let (block, cursor) = make(text, &indent, &model);
    let (replace, lead) = if line.trim().is_empty() {
        (line_start..(line_end + 1).min(text.len()), String::new())
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

fn csv_commands() -> Vec<Command> {
    use crate::command::Scope;
    let csv = || Scope::only(&["csv"]);
    let c = |id: &str, title: &str, keys: &[&str], h: Handler| {
        scoped(cmd(id, title, "CSV", keys, None, h), csv())
    };
    vec![
        c("csv.nextField", "Next Field", &["tab"], |ctx, _| {
            csv_edit(ctx, |text, l, row, rec, col| {
                if col + 1 < rec.fields.len() {
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
        c("csv.insertRow", "Insert Row", &[], |ctx, _| {
            csv_edit(ctx, |text, l, row, rec, col| {
                let columns = l.widths.len().max(rec.fields.len());
                Ok((
                    Some(crate::csv::insert_row(text, rec, columns, &l.dialect)),
                    Some((row + 1, col)),
                ))
            })
        }),
        c("csv.deleteRow", "Delete Row", &[], |ctx, _| {
            csv_edit(ctx, |text, _, row, rec, col| {
                Ok((Some(crate::csv::delete_row(text, rec)), Some((row, col))))
            })
        }),
        c("csv.moveRowUp", "Move Row Up", &[], |ctx, _| {
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
        c("csv.moveRowDown", "Move Row Down", &[], |ctx, _| {
            csv_edit(ctx, |text, l, row, rec, col| {
                if l.dialect.header && row == 0 {
                    return Err(CommandError::new(crate::tr!("msg-csv-no-row")));
                }
                let next = l.index.borrow_mut().record(text, row + 1, &l.dialect);
                let next = next.ok_or_else(|| CommandError::new(crate::tr!("msg-csv-no-row")))?;
                Ok((
                    Some(crate::csv::swap_rows(text, rec, &next)),
                    Some((row + 1, col)),
                ))
            })
        }),
        c("csv.insertColumn", "Insert Column", &[], |ctx, _| {
            csv_edit(ctx, |text, l, row, _, col| {
                Ok((
                    Some(crate::csv::insert_column(text, &l.dialect, col)),
                    Some((row, col)),
                ))
            })
        }),
        c("csv.deleteColumn", "Delete Column", &[], |ctx, _| {
            csv_edit(ctx, |text, l, row, _, col| {
                Ok((
                    Some(crate::csv::delete_column(text, &l.dialect, col)),
                    Some((
                        row,
                        col.saturating_sub(usize::from(col + 1 >= l.widths.len())),
                    )),
                ))
            })
        }),
        c("csv.moveColumnLeft", "Move Column Left", &[], |ctx, _| {
            csv_edit(ctx, |text, l, row, _, col| {
                if col == 0 {
                    return Err(CommandError::new(crate::tr!("msg-csv-no-column")));
                }
                Ok((
                    Some(crate::csv::swap_columns(text, &l.dialect, col - 1)),
                    Some((row, col - 1)),
                ))
            })
        }),
        c("csv.moveColumnRight", "Move Column Right", &[], |ctx, _| {
            csv_edit(ctx, |text, l, row, rec, col| {
                if col + 1 >= rec.fields.len() {
                    return Err(CommandError::new(crate::tr!("msg-csv-no-column")));
                }
                Ok((
                    Some(crate::csv::swap_columns(text, &l.dialect, col)),
                    Some((row, col + 1)),
                ))
            })
        }),
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
            request(ctx, Request::Copy)
        }),
        cmd("edit.cut", "Cut", "Edit", &["ctrl+x"], None, |ctx, _| {
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
            crate::command::Scope::except(&["org"]),
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
            crate::command::Scope::except(&["org"]),
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
                None,
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
            Some(ORG),
            |ctx, _| {
                let doc = ctx.doc()?;
                let text = crate::rich_copy::selection_text(doc);
                let path = doc.meta.path.clone();
                let html =
                    crate::rich_copy::html(&text, path.as_deref()).map_err(CommandError::new)?;
                ctx.requests.push(Request::CopyRich { html, text });
                Ok(())
            },
        ),
        cmd(
            "edit.copyHtml",
            "Copy as HTML",
            "Edit",
            &[],
            Some(ORG),
            |ctx, _| {
                let doc = ctx.doc()?;
                let text = crate::rich_copy::selection_text(doc);
                let path = doc.meta.path.clone();
                let html =
                    crate::rich_copy::html(&text, path.as_deref()).map_err(CommandError::new)?;
                ctx.requests.push(Request::CopyText(html));
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
            |ctx, _| request(ctx, Request::Pick(PickKind::RemoveProject)),
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
            Some(ORG),
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
                    ctx.clipboard.text = clip;
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
                ctx.clipboard.text = clip;
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
                        tx.replace(point..point, table).expect("one edit");
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
        assert_eq!(get("lines.moveUp"), Scope::except(&["org"]));
        assert_eq!(get("edit.undo"), Scope::all());
        assert!(get("org.emphasis.bold").serves("klm"));
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
}
