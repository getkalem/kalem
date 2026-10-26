//! The built-in commands: every command of `org-edit`, and undo and redo.
//! Their default keys are those of the Word-like profile (§7.3); other
//! profiles come from keymaps.

use serde_json::Value;

use crate::command::{
    Command, CommandError, CommandResult, DocumentsRequest, EditorContext, PickKind,
    ProjectRequest, Request, arg_bool, arg_str, cmd, request, scoped,
};
use crate::layout::{Axis, PaneOp};
use crate::workspaces::WorkspaceOp;

/// The when-clause of a command on the text itself (its lines, its
/// brackets, its encoding, its source): never a viewer's document, which
/// has none.
const TEXT_ONLY: &str = "editorMode != viewer";

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
        (
            "org.headline.insertTodo",
            object(&[("afterSubtree", "boolean", false)]),
        ),
        // Without an argument they offer the document's keywords and
        // priorities to choose from.
        ("org.todo.set", object(&[("state", "string", false)])),
        ("org.priority.set", object(&[("priority", "string", false)])),
        (
            "org.property.set",
            object(&[("key", "string", true), ("value", "string", true)]),
        ),
        ("org.tags.set", object(&[("tags", "array", true)])),
        ("flow.comment.new", object(&[("text", "string", true)])),
        ("flow.comment.reply", object(&[("text", "string", true)])),
        ("flow.comment.edit", object(&[("text", "string", true)])),
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
            object(&[
                ("path", "string", false),
                ("prompt", "boolean", false),
                ("createFolder", "boolean", false),
            ]),
        ),
        ("file.scratch", object(&[("project", "boolean", false)])),
        (
            "plugin.install",
            object(&[("source", "string", false), ("open", "string", false)]),
        ),
        (
            "plugin.forFile",
            object(&[("path", "string", true), ("how", "string", false)]),
        ),
        (
            "file.openWithPassword",
            object(&[("path", "string", true), ("password", "string", false)]),
        ),
        ("plugin.installGitHub", object(&[("link", "string", false)])),
        ("code.rename", object(&[("name", "string", false)])),
        ("project.run", object(&[("command", "string", false)])),
        ("project.test", object(&[("command", "string", false)])),
        (
            "code.structuralReplace",
            object(&[("search", "string", false), ("replace", "string", false)]),
        ),
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
        (
            "file.reveal",
            object(&[("project", "boolean", false), ("path", "string", false)]),
        ),
        ("settings.set", object(&[("key", "string", true)])),
        (
            "settings.item",
            object(&[("key", "string", true), ("at", "integer", false)]),
        ),
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
            object(&[
                ("path", "string", false),
                ("ask", "boolean", false),
                ("ignore", "array", false),
            ]),
        ),
        ("file.copy", object(&[("target", "string", false)])),
        ("org.property.delete", object(&[("key", "string", true)])),
        ("org.cite.insert", object(&[("key", "string", false)])),
        ("org.insert.drawer", object(&[("name", "string", true)])),
        ("org.caption.set", object(&[("caption", "string", true)])),
        ("edit.gotoLine", object(&[("line", "integer", true)])),
        ("view.editRow", object(&[("row", "integer", false)])),
        (
            "view.narrowToLines",
            object(&[
                ("start", "integer", false),
                ("end", "integer", false),
                ("label", "string", false),
            ]),
        ),
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
        ("org.babel.execute", object(&[("answer", "string", false)])),
        (
            "org.babel.insertResult",
            object(&[("run", "integer", false)]),
        ),
        (
            "org.babel.exportStep",
            object(&[
                ("job", "integer", false),
                ("answer", "string", false),
                ("ran", "boolean", false),
            ]),
        ),
        ("org.footnote.new", object(&[("label", "string", false)])),
        ("project.remove", object(&[("path", "string", false)])),
        ("project.add", object(&[("path", "string", false)])),
        ("pane.closeOrQuit", object(&[("force", "boolean", false)])),
        (
            "app.quitWithoutSaving",
            object(&[("force", "boolean", false)]),
        ),
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
        (
            "csv.filter",
            object(&[("text", "string", true), ("column", "integer", false)]),
        ),
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
        ("pane.next", object(&[("back", "boolean", false)])),
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

/// The folder of setting `notes.directory`, when it is there.
fn notes_folder(ctx: &EditorContext<'_>) -> Result<std::path::PathBuf, CommandError> {
    let dir = crate::settings::expand_home(ctx.config.str("notes.directory"));
    let dir = std::path::PathBuf::from(dir);
    if !dir.is_dir() {
        return Err(CommandError::new(crate::tr!(
            "msg-no-notes-folder",
            path = dir.display().to_string()
        )));
    }
    Ok(dir)
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
const ON_TIMESTAMP: &str = "editorMode == org && onTimestamp && !hasSelection";

/// What Enter does in Vim's Normal mode in an Org document, as Doom
/// Emacs's `+org/dwim-at-point`: on a link it opens it, on a footnote it
/// goes to the other end, on a heading with a TODO keyword it marks it
/// done (or not done again), on an item with a checkbox it toggles it,
/// in a table it recalculates it; elsewhere nothing.
fn org_dwim(ctx: &mut EditorContext<'_>) -> CommandResult {
    let doc = ctx.doc()?;
    let model = doc
        .model()
        .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
    let pos = doc.selection.head;
    let when = doc.when_context();
    let on = |k: &str| matches!(when.get(k), Some(crate::when::Value::Bool(true)));
    let text = model.parse().text();
    let run = |id: &str, args: Value| Request::Run {
        command: id.to_string(),
        args,
    };
    let chosen = if crate::input::link_at(&model, pos).is_some() {
        Some(run("org.link.open", serde_json::json!({})))
    } else if org_edit::footnote::at_footnote(&text, pos) {
        Some(run("org.footnote.action", serde_json::json!({})))
    } else if on("onHeadline") {
        let bol = text[..pos].rfind('\n').map_or(0, |i| i + 1);
        let line = text[bol..].split('\n').next().unwrap_or("");
        let ctx_ = model.parse().context();
        org_model::complex_heading(line, ctx_)
            .and_then(|c| c.todo)
            .map(|r| {
                let kw = &line[r];
                if ctx_.done_keywords.iter().any(|d| d == kw) {
                    let first = ctx_.todo_keywords.first().cloned().unwrap_or_default();
                    run("org.todo.set", serde_json::json!({ "state": first }))
                } else {
                    run("org.todo.done", serde_json::json!({}))
                }
            })
    } else if on("inSrcBlock") {
        Some(run("org.babel.execute", serde_json::json!({})))
    } else if on("inTable") {
        Some(run("table.recalculate", serde_json::json!({})))
    } else if on("inList") && checkbox_on_line(&text, pos) {
        Some(run("list.toggleCheckbox", serde_json::json!({})))
    } else {
        None
    };
    match chosen {
        Some(r) => request(ctx, r),
        None => Ok(()),
    }
}

/// Doom Emacs's `+org/insert-item-below` (C-RET) and `-above` (C-S-RET),
/// Insert Item Below and Above:
/// next to the list item, table row or heading at the cursor, a new one
/// of its kind (an item with a checkbox when it has one, a heading with
/// its TODO keyword), then Vim's Insert mode there.
fn org_insert_item(ctx: &mut EditorContext<'_>, above: bool) -> CommandResult {
    let now = ctx.now;
    let doc = ctx.doc()?;
    let model = doc
        .model()
        .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
    let pos = doc.selection.head;
    let when = doc.when_context();
    let on = |k: &str| matches!(when.get(k), Some(crate::when::Value::Bool(true)));
    let run = |id: &str, args: Value| Request::Run {
        command: id.to_string(),
        args,
    };
    if on("inTable") {
        let args = if above {
            serde_json::json!({})
        } else {
            serde_json::json!({ "below": true })
        };
        ctx.requests.push(run("table.insertRow", args));
    } else if let Some((start, checkbox)) = org_edit::list::item_at(&model, pos) {
        let to = if above {
            start
        } else {
            let text = doc.text();
            text.line_range(text.line_of(pos)).end
        };
        doc.move_cursor(to, false);
        ctx.requests.push(run(
            "list.insertItem",
            serde_json::json!({ "checkbox": checkbox }),
        ));
    } else {
        let ctx_ = model.parse().context().clone();
        let text = text_of(&model);
        let t = org_edit::headline::insert_heading_doom(&text, pos, above, &ctx_)
            .map_err(CommandError::from)?;
        doc.apply(&t, org_edit::ChangeKind::Command, now);
    }
    ctx.requests.push(Request::VimInsert);
    Ok(())
}

/// The document's TODO keywords to choose from, and none, as
/// `org-todo`'s fast selection offers them (Doom's `SPC m t`).
fn choose_todo_state(ctx: &mut EditorContext<'_>) -> CommandResult {
    let model = ctx
        .doc()?
        .model()
        .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
    let c = model.parse().context();
    let item = |state: &str, title: String| crate::palette::PaletteItem {
        id: crate::palette::invocation("org.todo.set", &serde_json::json!({ "state": state })),
        title,
        category: String::new(),
        detail: String::new(),
        keys: String::new(),
        also: String::new(),
    };
    let mut items: Vec<_> = c
        .todo_keywords
        .iter()
        .chain(&c.done_keywords)
        .map(|k| item(k, k.clone()))
        .collect();
    items.push(item("", crate::tr!("choice-no-keyword")));
    request(ctx, Request::Choose(items))
}

/// The document's priorities to choose from, and none.
fn choose_priority(ctx: &mut EditorContext<'_>) -> CommandResult {
    let model = ctx
        .doc()?
        .model()
        .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
    let pr = &model.info().priorities;
    let (highest, lowest) = (pr.highest, pr.lowest);
    let numeric = lowest < 65;
    let item = |id: String, title: String| crate::palette::PaletteItem {
        id,
        title,
        category: String::new(),
        detail: String::new(),
        keys: String::new(),
        also: String::new(),
    };
    let mut items: Vec<_> = (highest.min(lowest)..=lowest.max(highest))
        .map(|p| {
            let name = if numeric {
                p.to_string()
            } else {
                char::from_u32(p).map_or_else(String::new, String::from)
            };
            item(
                crate::palette::invocation(
                    "org.priority.set",
                    &serde_json::json!({ "priority": name }),
                ),
                format!("[#{name}]"),
            )
        })
        .collect();
    items.push(item(
        "org.priority.remove".to_string(),
        crate::tr!("choice-no-priority"),
    ));
    request(ctx, Request::Choose(items))
}

/// Whether the line at `pos` is an item with a checkbox.
fn checkbox_on_line(text: &str, pos: usize) -> bool {
    let bol = text[..pos.min(text.len())].rfind('\n').map_or(0, |i| i + 1);
    let line = &text[bol..];
    let line = line.split('\n').next().unwrap_or("");
    let rest = line.trim_start();
    let after = rest
        .strip_prefix(['-', '+', '*'])
        .or_else(|| {
            let n = rest.bytes().take_while(u8::is_ascii_alphanumeric).count();
            (n > 0)
                .then(|| rest[n..].strip_prefix(['.', ')']))
                .flatten()
        })
        .unwrap_or("");
    let after = after.trim_start_matches([' ', '\t']);
    ["[ ]", "[X]", "[x]", "[-]"]
        .iter()
        .any(|c| after.starts_with(c))
}

/// `org-timestamp-change` at the cursor by `n` of `what`, or of the part
/// of the timestamp the cursor is in.
fn timestamp_change(
    ctx: &mut EditorContext<'_>,
    n: i64,
    what: Option<org_edit::timestamp::TsField>,
) -> CommandResult {
    ctx.org(|d, p, _| org_edit::timestamp::timestamp_change(&text_of(d), p, n, what, true))
}

/// The text and parse context of a model, for the commands that take text.
fn text_of(d: &org_model::Document) -> String {
    d.parse().text()
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
    // In bytes: a word may end inside a character (`/😀`).
    let is_part = |w: &str| {
        let body = w.trim_start_matches(['.', '+', '-', '/']).as_bytes();
        w.len() > body.len()
            && body.len() > 1
            && body[..body.len() - 1].iter().all(u8::is_ascii_digit)
            && matches!(body[body.len() - 1], b'h' | b'd' | b'w' | b'm' | b'y')
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
            org_edit::emphasis::toggle_at_caret(d, p, kind)
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
    all.extend(crate::csv::commands::csv_commands());
    all.extend(crate::markdown::commands::markdown_commands());
    all.extend(crate::flow::commands::flow_commands());
    all.extend(crate::bibtex::commands::bib_commands());
    all.extend(crate::latex_commands::commands());
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
/// property names the file). Its source blocks whose results it exports
/// run first ([`crate::babel::export`]): command `command` runs again with
/// `args` once they have.
fn export_doc(
    ctx: &mut EditorContext<'_>,
    command: &str,
    args: &Value,
    backend: &dyn org_export::Backend,
    extension: &str,
    subtree: bool,
) -> CommandResult {
    export_doc_to(ctx, command, args, backend, extension, subtree).map(|_| ())
}

/// [`export_doc`], returning the file written; none while its source
/// blocks run first.
fn export_doc_to(
    ctx: &mut EditorContext<'_>,
    command: &str,
    args: &Value,
    backend: &dyn org_export::Backend,
    extension: &str,
    subtree: bool,
) -> Result<Option<std::path::PathBuf>, CommandError> {
    if ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?
        .meta
        .path
        .is_none()
    {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    }
    let Some(text) = crate::babel::export::source(ctx, command, args, subtree)? else {
        return Ok(None);
    };
    let doc = ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let path = std::path::absolute(&path).unwrap_or(path);
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
    Ok(Some(target))
}

/// The LaTeX back-end, as `ox-latex` writes.
const LATEX: org_export::Latex = org_export::Latex {
    source_lines: false,
};

/// Exports the active document as LaTeX with `%% org:LINE` comments
/// beside its file, then compiles it to PDF in the background
/// (`crate::pdf`); the result, or LaTeX's first error at its Org line,
/// shows when it ends.
fn export_pdf(
    ctx: &mut EditorContext<'_>,
    command: &str,
    args: &Value,
    subtree: bool,
) -> CommandResult {
    pdf_then(ctx, command, args, subtree, false)
}

/// Compiles the document (or the subtree at the cursor) to a PDF in the
/// background; then opens it (`export.open_after`) or, with `print`,
/// hands it to the system's print dialog. Its source blocks whose results
/// it exports run first, `command` then running again with `args`.
fn pdf_then(
    ctx: &mut EditorContext<'_>,
    command: &str,
    args: &Value,
    subtree: bool,
    print: bool,
) -> CommandResult {
    if ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?
        .meta
        .path
        .is_none()
    {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    }
    let Some(text) = crate::babel::export::source(ctx, command, args, subtree)? else {
        return Ok(());
    };
    let doc = ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let path = std::path::absolute(&path).unwrap_or(path);
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
fn export_pandoc(
    ctx: &mut EditorContext<'_>,
    command: &str,
    args: &Value,
    format: crate::pandoc::Format,
) -> CommandResult {
    if ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?
        .meta
        .path
        .is_none()
    {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    }
    let Some(text) = crate::babel::export::source(ctx, command, args, false)? else {
        return Ok(());
    };
    let doc = ctx
        .document
        .as_deref()
        .ok_or_else(|| CommandError::new("No document"))?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let path = std::path::absolute(&path).unwrap_or(path);
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
pub(crate) fn import_file(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
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
pub(crate) fn file_url(path: &std::path::Path) -> String {
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
        detail: String::new(),
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

pub(crate) fn lines_command(
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
            detail: String::new(),
            keys: String::new(),
            also: format!("{} {}", b.path.display(), b.context.trim()),
        })
        .collect();
    request(ctx, Request::Choose(items))
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

thread_local! {
    /// The font size before Big Text, to go back to.
    static BIG_FROM: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) };
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
            "code.toggleInlayHints",
            "Toggle Inlay Hints",
            "Code",
            &[],
            None,
            |ctx, _| {
                ctx.messages.push(if crate::lsp::toggle_inlay_hints() {
                    crate::tr!("lsp-inlay-hints-shown")
                } else {
                    crate::tr!("lsp-inlay-hints-hidden")
                });
                Ok(())
            },
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
        // A server's own requests (the plugin's `requests`): one command
        // each, shown where the document's server has the request.
        cmd(
            "code.expandMacro",
            "Expand Macro",
            "Code",
            &[],
            Some("server:code.expandMacro"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.expandMacro")
                    .map_err(CommandError::new)
            },
        ),
        cmd(
            "code.openDocs",
            "Open Documentation in the Browser",
            "Code",
            &[],
            Some("server:code.openDocs"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.openDocs").map_err(CommandError::new)
            },
        ),
        cmd(
            "code.parentModule",
            "Go to Parent Module",
            "Code",
            &[],
            Some("server:code.parentModule"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.parentModule")
                    .map_err(CommandError::new)
            },
        ),
        cmd(
            "code.openManifest",
            "Go to Project File",
            "Code",
            &[],
            Some("server:code.openManifest"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.openManifest")
                    .map_err(CommandError::new)
            },
        ),
        cmd(
            "code.reloadProject",
            "Reload Project",
            "Code",
            &[],
            Some("server:code.reloadProject"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.reloadProject")
                    .map_err(CommandError::new)
            },
        ),
        cmd(
            "code.joinLines",
            "Join Lines (Language Server)",
            "Code",
            &[],
            Some("server:code.joinLines"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.joinLines").map_err(CommandError::new)
            },
        ),
        cmd(
            "code.moveItemUp",
            "Move Item Up",
            "Code",
            &[],
            Some("server:code.moveItemUp"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.moveItemUp").map_err(CommandError::new)
            },
        ),
        cmd(
            "code.moveItemDown",
            "Move Item Down",
            "Code",
            &[],
            Some("server:code.moveItemDown"),
            |ctx, _| {
                crate::lsp::server_request(ctx.doc()?, "code.moveItemDown")
                    .map_err(CommandError::new)
            },
        ),
        // A query and its replacement, asked one after the other where the
        // server's request has them (rust-analyzer's `foo($a) ==>>
        // bar($a)`), the edits offered before anything changes.
        cmd(
            "code.structuralReplace",
            "Structural Search and Replace",
            "Code",
            &[],
            Some("server:code.structuralReplace"),
            |ctx, args| {
                let d = ctx.doc()?;
                let asked = crate::lsp::request_inputs(d, "code.structuralReplace")
                    .into_iter()
                    .find(|n| args.get(*n).and_then(serde_json::Value::as_str).is_none());
                match asked {
                    Some(n) => request(
                        ctx,
                        Request::Ask {
                            command: "code.structuralReplace".into(),
                            args: args.clone(),
                            arg: n.into(),
                        },
                    ),
                    None => crate::lsp::server_request_with(d, "code.structuralReplace", args)
                        .map_err(CommandError::new),
                }
            },
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
    /// An item of the plugins' lists: what it does, and what is said of
    /// it after (a version, what an install brings).
    fn item(id: String, title: String, detail: String) -> PaletteItem {
        PaletteItem {
            also: title.clone(),
            id,
            title,
            category: String::new(),
            detail,
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
    /// Fetches a plugin and offers to install it; `open`, a file to open
    /// once it is installed (the file it was installed for, T3.7.9).
    fn start_install(
        ctx: &mut EditorContext<'_>,
        source: String,
        open: Option<String>,
    ) -> CommandResult {
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
                    let mut confirm = json!({ "staging": staging, "source": p.source });
                    if let Some(open) = &open {
                        confirm["open"] = json!(open);
                    }
                    let mut items = vec![item(
                        invocation("plugin.confirmInstall", &confirm),
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
                Some(s) => {
                    start_install(ctx, s.to_string(), args["open"].as_str().map(str::to_owned))
                }
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
        // The plugin the status bar suggested when a file opened (a
        // language's, a Logseq graph's), installed after the usual
        // confirmation.
        cmd(
            "plugin.installSuggested",
            "Install Suggested Plugin",
            "Plugins",
            &[],
            None,
            |ctx, _| match crate::plugin_store::suggested() {
                Some(id) => start_install(ctx, id, None),
                None => Err(CommandError::new(crate::tr!("plugin-none-suggested"))),
            },
        ),
        // A file Kalem cannot open yet (not text, no viewer installed): the
        // released plugins of the indexes that open it offered, installed
        // and the file opened at a choice; else the system's application
        // (T3.7.9). No plugin is built into Kalem: each is installed when
        // it is wanted.
        cmd(
            "plugin.forFile",
            "Find a Plugin for the File",
            "Plugins",
            &[],
            None,
            |ctx, args| {
                let Some(path) = args["path"].as_str().map(str::to_owned) else {
                    return Err(CommandError::new(crate::tr!("msg-no-document")));
                };
                if args["how"].as_str() == Some("system") {
                    return request(
                        ctx,
                        Request::OpenLink(crate::input::LinkAction::System(
                            std::path::PathBuf::from(&path),
                        )),
                    );
                }
                offer_plugins(ctx, Some(path.clone()), &path);
                Ok(())
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
                    Some(source) => start_install(ctx, source, None),
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
                // Installed for a file: opened once the plugin opens it. The
                // editor's watch of the plugins chooses the viewers again
                // within a second or two of an install.
                if let Some(open) = args["open"].as_str().map(std::path::PathBuf::from) {
                    let name = p.name.clone();
                    crate::jobs::spawn(
                        crate::tr!("plugin-starting", name = name.as_str()),
                        move || {
                            let deadline =
                                std::time::Instant::now() + std::time::Duration::from_secs(15);
                            while crate::viewer::for_file(&open).is_none()
                                && std::time::Instant::now() < deadline
                            {
                                std::thread::sleep(std::time::Duration::from_millis(200));
                            }
                            if crate::viewer::for_file(&open).is_some() {
                                crate::jobs::Finished {
                                    message: String::new(),
                                    error: false,
                                    open: Some(crate::input::LinkAction::File {
                                        path: open.display().to_string(),
                                        search: None,
                                    }),
                                }
                            } else {
                                crate::jobs::Finished {
                                    message: crate::tr!(
                                        "plugin-did-not-start",
                                        name = name.as_str()
                                    ),
                                    error: true,
                                    open: None,
                                }
                            }
                        },
                    );
                }
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
                        let detail = match crate::plugin_store::available(&p.id, &p.version) {
                            Some(v) => {
                                crate::tr!("plugin-available-from", version = v, from = from)
                            }
                            None => from,
                        };
                        item(
                            invocation("plugin.manage", &json!({ "id": p.id })),
                            format!("{} {}", p.name, p.version),
                            detail,
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
            detail: String::new(),
            keys: String::new(),
            also: String::new(),
        };
        request(ctx, Request::Choose(vec![item]))?;
        return Ok(None);
    }
    Ok(Some(target))
}

/// The released plugins of the indexes that open a file named `name` (by
/// what each declares: its extension, its first bytes) offered to install, read in the background, with the
/// system's application when `open` is a file: for a file Kalem cannot
/// open yet (`plugin.forFile`), the file then opened; or for a command
/// that needs such a plugin (New Workbook), `open` none. No plugin is
/// built into Kalem: each is installed when it is wanted (T3.7.9).
pub(crate) fn offer_plugins(ctx: &mut EditorContext<'_>, open: Option<String>, name: &str) {
    let item = |id: String, title: String, detail: String| crate::palette::PaletteItem {
        also: title.clone(),
        id,
        title,
        category: String::new(),
        detail,
        keys: String::new(),
    };
    let file = std::path::Path::new(name)
        .file_name()
        .map_or(name.to_string(), |n| n.to_string_lossy().into_owned());
    let kind = std::path::Path::new(name)
        .extension()
        .map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
    let name = name.to_string();
    let index = crate::plugin_store::index_urls(ctx.config);
    crate::jobs::spawn(
        crate::tr!("plugin-finding", file = file.as_str()),
        move || {
            let system = open.as_ref().map(|path| {
                item(
                    crate::palette::invocation(
                        "plugin.forFile",
                        &serde_json::json!({ "path": path, "how": "system" }),
                    ),
                    crate::tr!("plugin-open-system"),
                    String::new(),
                )
            });
            // The index as last read (at a start, or by Browse Plugins)
            // when it cannot be read now.
            let read = match crate::plugin_store::fetch_indexes(&index) {
                Err(e) => {
                    let known = crate::plugin_store::known_index();
                    if known.is_empty() { Err(e) } else { Ok(known) }
                }
                ok => ok,
            };
            match read {
                Ok(entries) => {
                    // Its first bytes, for the plugins that know a file
                    // by them; a kind of file named alone has none.
                    let head = open
                        .as_ref()
                        .and_then(|p| crate::applies::head_of(std::path::Path::new(p)));
                    let found = crate::plugin_store::opening(
                        &entries,
                        std::path::Path::new(&name),
                        head.as_deref(),
                    );
                    let mut items: Vec<crate::palette::PaletteItem> = found
                        .iter()
                        .map(|e| {
                            let (args, title) = match &open {
                                Some(path) => (
                                    serde_json::json!({ "source": e.id, "open": path }),
                                    crate::tr!(
                                        "plugin-install-to-open",
                                        name = e.name.as_str(),
                                        version = e.version.as_str(),
                                        file = file.as_str()
                                    ),
                                ),
                                None => (
                                    serde_json::json!({ "source": e.id }),
                                    crate::tr!(
                                        "plugin-install-for-kind",
                                        name = e.name.as_str(),
                                        version = e.version.as_str(),
                                        kind = kind.as_str()
                                    ),
                                ),
                            };
                            item(
                                crate::palette::invocation("plugin.install", &args),
                                title,
                                e.description.clone(),
                            )
                        })
                        .collect();
                    let message = match (items.is_empty(), &open) {
                        (true, Some(_)) => crate::tr!("plugin-none-opens", file = file.as_str()),
                        (true, None) => crate::tr!("plugin-none-for-kind", kind = kind.as_str()),
                        (false, Some(_)) => {
                            crate::tr!("plugin-not-installed-for", file = file.as_str())
                        }
                        (false, None) => {
                            crate::tr!("plugin-needed-for-kind", kind = kind.as_str())
                        }
                    };
                    items.extend(system);
                    if !items.is_empty() {
                        crate::jobs::offer(items);
                    }
                    crate::jobs::Finished {
                        message,
                        error: false,
                        open: None,
                    }
                }
                Err(e) => {
                    if let Some(system) = system {
                        crate::jobs::offer(vec![system]);
                    }
                    crate::jobs::Finished {
                        message: crate::tr!(
                            "plugin-index-unread-for",
                            file = file.as_str(),
                            error = e
                        ),
                        error: true,
                        open: None,
                    }
                }
            }
        },
    );
}

/// The workbook plugin, which makes workbooks; when it is not installed,
/// the plugins that do offered to install, and none.
fn workbook_or_offer(
    ctx: &mut EditorContext<'_>,
) -> Option<std::sync::Arc<dyn kalem_viewer::Viewer>> {
    match crate::workbook_io::workbook_viewer() {
        Ok(v) => Some(v),
        Err(_) => {
            offer_plugins(ctx, None, "Book.xlsx");
            None
        }
    }
}

/// New Workbook: a blank workbook of one sheet, written where asked and
/// opened.
fn new_workbook(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    let Some(viewer) = workbook_or_offer(ctx) else {
        return Ok(());
    };
    let Some(target) = new_file_target(ctx, "app.newWorkbook", args, "Book", "xlsx")? else {
        return Ok(());
    };
    let sheet = kalem_viewer::NewSheet {
        name: "Sheet1".into(),
        rows: Vec::new(),
    };
    let bytes = viewer
        .new_file("xlsx", &[sheet])
        .map_err(|e| CommandError::new(e.to_string()))?;
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
    let Some(viewer) = workbook_or_offer(ctx) else {
        return Ok(());
    };
    let Some(target) = new_file_target(ctx, ID, args, &stem, ext)? else {
        return Ok(());
    };
    let bytes = std::fs::read(&template).map_err(|e| CommandError::new(e.to_string()))?;
    // The template opened by the workbook plugin and written as a workbook.
    let bytes = crate::workbook_io::open_bytes(viewer.as_ref(), &template, bytes)
        .and_then(|mut d| d.save_as(ext).map(|o| o.bytes).map_err(|e| e.to_string()))
        .map_err(CommandError::new)?;
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
pub(crate) fn open_as_workbook(ctx: &mut EditorContext<'_>, args: &Value) -> CommandResult {
    const ID: &str = "csv.openAsWorkbook";
    // The workbook plugin makes the workbook: offered first when missing.
    if workbook_or_offer(ctx).is_none() {
        return Ok(());
    }
    let item = |a: Value, title: &str, category: &str| crate::palette::PaletteItem {
        id: crate::palette::invocation(ID, &a),
        title: title.into(),
        category: category.into(),
        detail: String::new(),
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
    let sheet = kalem_viewer::NewSheet {
        name: if sheet.is_empty() {
            "Sheet1".into()
        } else {
            sheet
        },
        rows,
    };
    let bytes = crate::workbook_io::workbook_viewer()
        .and_then(|v| v.new_file("xlsx", &[sheet]).map_err(|e| e.to_string()))
        .map_err(CommandError::new)?;
    std::fs::write(&target, bytes).map_err(|e| CommandError::new(e.to_string()))?;
    request(
        ctx,
        Request::Open {
            path: Some(target.display().to_string()),
        },
    )
}

/// The cursor to the first line of the next change marked beside the
/// lines (`forward`), or of the previous one.
fn goto_change(ctx: &mut EditorContext<'_>, forward: bool) -> CommandResult {
    let doc = ctx.doc()?;
    let starts = doc.changes_starts();
    if starts.is_empty() {
        let m = crate::l10n::tr("msg-no-changes");
        ctx.messages.push(m);
        return Ok(());
    }
    let line = doc.text().line_of(doc.selection.head);
    let to = if forward {
        starts.iter().find(|l| **l > line)
    } else {
        starts.iter().rev().find(|l| **l < line)
    };
    match to {
        Some(l) => {
            let doc = ctx.doc()?;
            let pos = doc.text().line_start(*l);
            doc.move_cursor(pos, false);
        }
        None => ctx.messages.push(crate::l10n::tr("msg-no-more-changes")),
    }
    Ok(())
}

/// Runs the active document's language plugin's command `name` (`run`,
/// `test`, `testAtPoint`) in its project's root, its output in a document
/// ([`crate::runs`]). Without one, the command given as `command` is run by
/// the shell in the project's root, asked for when not given; a test at
/// the cursor is only the plugin's.
fn run_project(ctx: &mut EditorContext<'_>, args: &Value, id: &str, name: &str) -> CommandResult {
    let d = ctx.doc()?;
    if let Some(command) = args
        .get("command")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|c| !c.is_empty())
    {
        let dir = crate::command::folder_of(d)
            .or_else(|| std::env::current_dir().ok())
            .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-file")))?;
        let dir = kalem_project::list::detect_root(&dir).unwrap_or(dir);
        return crate::runs::start(crate::runs::shell(command), dir)
            .map(|_| ())
            .map_err(CommandError::new);
    }
    match crate::lsp::plugin_command(d, name) {
        Some((argv, dir)) => crate::runs::start(argv, dir)
            .map(|_| ())
            .map_err(CommandError::new),
        None if name == "testAtPoint" => {
            Err(CommandError::new(crate::l10n::tr("run-no-test-at-cursor")))
        }
        None => request(
            ctx,
            Request::Ask {
                command: id.into(),
                args: serde_json::json!({}),
                arg: "command".into(),
            },
        ),
    }
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
            |ctx, a| export_doc(ctx, "export.html", a, &org_export::Html, ".html", false),
        ),
        cmd(
            "export.htmlSubtree",
            "Export Subtree as HTML",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| {
                export_doc(
                    ctx,
                    "export.htmlSubtree",
                    a,
                    &org_export::Html,
                    ".html",
                    true,
                )
            },
        ),
        cmd(
            "export.htmlBrowser",
            "Export as HTML and Open",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| {
                let Some(target) = export_doc_to(
                    ctx,
                    "export.htmlBrowser",
                    a,
                    &org_export::Html,
                    ".html",
                    false,
                )?
                else {
                    return Ok(());
                };
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
            |ctx, a| {
                export_doc(
                    ctx,
                    "export.markdown",
                    a,
                    &org_export::Markdown,
                    ".md",
                    false,
                )
            },
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
            |ctx, a| export_doc(ctx, "export.gfm", a, &org_export::Gfm, ".md", false),
        ),
        cmd(
            "export.latex",
            "Export as LaTeX",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| export_doc(ctx, "export.latex", a, &LATEX, ".tex", false),
        ),
        cmd(
            "export.text",
            "Export as Plain Text",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| {
                let utf8 = ctx.config.str("export.text_charset") == "utf-8";
                let backend: &dyn org_export::Backend = if utf8 { &TEXT_UTF8 } else { &TEXT };
                export_doc(ctx, "export.text", a, backend, ".txt", false)
            },
        ),
        cmd(
            "export.pdf",
            "Export as PDF (LaTeX)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| export_pdf(ctx, "export.pdf", a, false),
        ),
        cmd(
            "file.print",
            "Print",
            "File",
            &[],
            Some("editorMode == org"),
            |ctx, a| pdf_then(ctx, "file.print", a, false, true),
        ),
        cmd(
            "export.pdfSubtree",
            "Export Subtree as PDF (LaTeX)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| export_pdf(ctx, "export.pdfSubtree", a, true),
        ),
        cmd(
            "export.docx",
            "Export as Word (pandoc)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| export_pandoc(ctx, "export.docx", a, crate::pandoc::Format::Docx),
        ),
        cmd(
            "export.odt",
            "Export as OpenDocument (pandoc)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| export_pandoc(ctx, "export.odt", a, crate::pandoc::Format::Odt),
        ),
        cmd(
            "export.epub",
            "Export as EPUB (pandoc)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| export_pandoc(ctx, "export.epub", a, crate::pandoc::Format::Epub),
        ),
        cmd(
            "export.rtf",
            "Export as RTF (pandoc)",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| export_pandoc(ctx, "export.rtf", a, crate::pandoc::Format::Rtf),
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
            |ctx, a| export_doc(ctx, "export.latexSubtree", a, &LATEX, ".tex", true),
        ),
        cmd(
            "export.markdownSubtree",
            "Export Subtree as Markdown",
            "Export",
            &[],
            Some("editorMode == org"),
            |ctx, a| {
                export_doc(
                    ctx,
                    "export.markdownSubtree",
                    a,
                    &org_export::Markdown,
                    ".md",
                    true,
                )
            },
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
            if ctx
                .document
                .as_deref()
                .is_some_and(|d| d.viewer().is_some())
            {
                return crate::viewer::copy(ctx);
            }
            request(ctx, Request::Copy)
        }),
        cmd("edit.cut", "Cut", "Edit", &["ctrl+x"], None, |ctx, _| {
            // A spreadsheet's cells are cut to be moved where they are pasted.
            if ctx
                .document
                .as_deref()
                .and_then(|d| d.viewer())
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
            Some(TEXT_ONLY),
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
            Some(TEXT_ONLY),
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
                Some(TEXT_ONLY),
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
                Some(TEXT_ONLY),
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
                Some(TEXT_ONLY),
                |ctx, _| lines_command(ctx, |t, s| crate::lines::move_lines(t, s, false)),
            ),
            // A CSV grid moves its rows on these keys, records that span
            // lines whole.
            crate::command::Scope::except(&["org", "csv"]),
        ),
        cmd(
            "lines.join",
            "Join Lines",
            "Edit",
            &[],
            Some(TEXT_ONLY),
            |ctx, _| lines_command(ctx, crate::lines::join),
        ),
        cmd(
            "lines.sort",
            "Sort Lines",
            "Edit",
            &[],
            Some(TEXT_ONLY),
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
            Some(TEXT_ONLY),
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
            &["shift+alt+f"],
            // Everywhere, as Doom's `SPC c f`: a type without a formatter
            // is told so (`hasFormatter` says where one is known).
            None,
            |ctx, _| {
                // The formatter the user set for the document's type
                // (`formatters.TYPE`) comes before every other.
                if let Some(f) = crate::formatters::user_formatter(ctx.doc()?) {
                    return crate::lsp::format_with(ctx.doc()?, f).map_err(CommandError::new);
                }
                // Or turned off (`formatters.TYPE = "off"`): no formatter
                // runs, the server's neither.
                if crate::formatters::turned_off(ctx.doc()?) {
                    let doc = ctx.doc()?;
                    let key = crate::formatters::names(doc)
                        .first()
                        .cloned()
                        .unwrap_or_default();
                    return Err(CommandError::new(crate::tr!(
                        "fmt-off",
                        kind = doc.mode_title(),
                        key = key.as_str()
                    )));
                }
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
                // A plugin's formatter (API 0.2.16); a refusal at its
                // place, when it says one.
                if let Some(spec) = crate::formatters::plugins::for_doc(ctx.doc()?) {
                    use crate::formatters::plugins::Answer;
                    let new = match crate::formatters::plugins::format(&spec, ctx.doc()?) {
                        Answer::Text(t) => t,
                        Answer::Refused { message, at } => {
                            if let Some(at) = at {
                                let doc = ctx.doc()?;
                                let mut at = at.min(doc.text().len());
                                while !doc.text().as_str().is_char_boundary(at) {
                                    at -= 1;
                                }
                                doc.selection = org_edit::Selection::caret(at);
                            }
                            return Err(CommandError::new(message));
                        }
                        Answer::Busy => {
                            return Err(CommandError::new(crate::tr!(
                                "fmt-plugin-busy",
                                plugin = spec.plugin.as_str()
                            )));
                        }
                        Answer::Failed(why) => return Err(CommandError::new(why)),
                    };
                    return lines_command(ctx, |t, _| {
                        crate::lines::replace_differing(t, &new, "Format Document")
                    });
                }
                // The language's usual formatter (`formatters::DEFAULTS`),
                // when the user did not turn it off.
                if let Some(f) = crate::formatters::default_formatter(ctx.doc()?) {
                    return crate::lsp::format_with(ctx.doc()?, f).map_err(CommandError::new);
                }
                let mode = ctx.doc()?.meta.mode.clone();
                let latex = mode == crate::DocumentMode::Latex;
                if !latex && mode != crate::DocumentMode::Org {
                    // A type without a formatter (a workbook, a picture, a
                    // language nothing is known for), or a document without
                    // a file for its formatter to read.
                    let doc = ctx.doc()?;
                    return Err(CommandError::new(
                        if doc.meta.path.is_none() && crate::formatters::known(doc) {
                            crate::l10n::tr("msg-no-file")
                        } else {
                            let kind = doc.mode_title();
                            match crate::formatters::names(doc).first() {
                                Some(key) => {
                                    crate::tr!("fmt-none", kind = kind, key = key.as_str())
                                }
                                None => crate::tr!("fmt-none-kind", kind = kind),
                            }
                        },
                    ));
                }
                let path = ctx.doc()?.meta.path.clone();
                lines_command(ctx, |t, _| {
                    // As `kalem fmt` does.
                    let new = if latex {
                        match &path {
                            Some(p) => crate::latex_fmt::format_file(p, t, false),
                            None => crate::latex_fmt::format(t, false),
                        }
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
            Some(TEXT_ONLY),
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
            Some(TEXT_ONLY),
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
        // The changes plugins mark beside the lines (the git plugin's).
        cmd(
            "edit.nextChange",
            "Next Change",
            "Edit",
            &["alt+f5"],
            None,
            |ctx, _| goto_change(ctx, true),
        ),
        cmd(
            "edit.previousChange",
            "Previous Change",
            "Edit",
            &["shift+alt+f5"],
            None,
            |ctx, _| goto_change(ctx, false),
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
                        category: String::new(),
                        detail: match c.target {
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
        // A grid selects its cells (`viewer.grid.selectAll`).
        cmd(
            "edit.selectAll",
            "Select All",
            "Edit",
            &["ctrl+a"],
            Some("!viewerGrid"),
            |ctx, _| {
                let d = ctx.doc()?;
                d.selection = org_edit::Selection {
                    anchor: 0,
                    head: d.text().len(),
                };
                Ok(())
            },
        ),
        // Doom Emacs's `+org/insert-item-below` and `-above` (C-RET and
        // C-S-RET with Vim keys).
        cmd(
            "org.insertItemBelow",
            "Insert Item Below",
            "Lists",
            &[],
            Some(ORG),
            |ctx, _| org_insert_item(ctx, false),
        ),
        cmd(
            "org.insertItemAbove",
            "Insert Item Above",
            "Lists",
            &[],
            Some(ORG),
            |ctx, _| org_insert_item(ctx, true),
        ),
        // Doom Emacs's `+org/remove-link` (SPC m l d).
        cmd(
            "org.link.remove",
            "Remove Link",
            "Links",
            &[],
            Some(ORG),
            |ctx, _| ctx.org(|d, p, _| org_edit::insert::remove_link(&text_of(d), p)),
        ),
        // Doom Emacs's `+org/dwim-at-point`, Enter in Vim's Normal mode;
        // in a source block too, which it runs.
        scoped(
            cmd(
                "org.dwim",
                "Act at Cursor",
                "Edit",
                &[],
                Some(ORG),
                |ctx, _| org_dwim(ctx),
            ),
            crate::command::Scope::all(),
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
        // Doom's `SPC m l y` (`+org/yank-link`): the link's target, not
        // its description, on the clipboard.
        cmd(
            "org.link.copy",
            "Copy Link",
            "Links",
            &[],
            Some(ORG),
            |ctx, _| {
                let doc = ctx.doc()?;
                let model = doc
                    .model()
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
                let link = crate::input::raw_link_at(&model, doc.selection.head)
                    .ok_or_else(|| CommandError::new(crate::tr!("msg-no-link")))?;
                ctx.messages
                    .push(crate::tr!("msg-link-copied", link = link.as_str()));
                request(ctx, Request::CopyText(link))
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
                    // Code: a level deeper after an opening bracket; the
                    // language server's new line, when it has one, takes
                    // its place as it answers.
                    crate::DocumentMode::Text { language: Some(l) } => {
                        crate::lsp::entering(d);
                        crate::code::newline(
                            d.text().as_str(),
                            s,
                            &crate::code::indent_text(d),
                            Some(l.as_str()),
                        )
                    }
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
                // A new file in a folder that does not exist: the folder is
                // made once asked (Doom's `doom-create-missing-directories-h`);
                // saving it failed.
                if let Some(p) = &path {
                    let full = std::path::PathBuf::from(crate::settings::expand_home(p));
                    let base = ctx.document.as_deref().and_then(crate::command::folder_of);
                    let full = match base {
                        Some(d) if !full.is_absolute() => d.join(&full),
                        _ => full,
                    };
                    if let Some(dir) = full.parent().filter(|d| {
                        !d.as_os_str().is_empty() && std::fs::symlink_metadata(d).is_err()
                    }) {
                        if !arg_bool(args, "createFolder") {
                            let name = full
                                .file_name()
                                .map_or(String::new(), |f| f.to_string_lossy().into_owned());
                            let dir = crate::projects::tilde(dir);
                            let mut a = args.clone();
                            a["path"] = Value::String(full.display().to_string());
                            a["createFolder"] = Value::Bool(true);
                            let item = crate::palette::PaletteItem {
                                id: crate::palette::invocation("file.open", &a),
                                title: crate::tr!("open-create-folder", name = name),
                                category: crate::tr!("open-no-folder", dir = dir.as_str()),
                                detail: String::new(),
                                keys: String::new(),
                                also: String::new(),
                            };
                            return request(ctx, Request::Choose(vec![item]));
                        }
                        std::fs::create_dir_all(dir)
                            .map_err(|e| CommandError::new(e.to_string()))?;
                    }
                }
                // At a line (from 1) and a byte of it, as a plugin opens a
                // diff's line.
                if let (Some(p), Some(line)) = (&path, args.get("line").and_then(Value::as_u64)) {
                    let column = args.get("column").and_then(Value::as_u64).unwrap_or(0);
                    return request(
                        ctx,
                        Request::OpenAt {
                            path: p.clone(),
                            line: line.max(1),
                            column: column as usize,
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
                            detail: String::new(),
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
        // How the file is written: its line endings and byte order mark,
        // from the next save.
        cmd(
            "file.lineEndingsLf",
            "Line Endings: LF (macOS, Linux)",
            "File",
            &[],
            None,
            |ctx, _| {
                let now = ctx.now;
                ctx.doc()?
                    .set_line_ending(crate::document::LineEnding::Lf, now);
                ctx.messages
                    .push(crate::tr!("msg-line-endings", ending = "LF"));
                Ok(())
            },
        ),
        cmd(
            "file.lineEndingsCrlf",
            "Line Endings: CRLF (Windows)",
            "File",
            &[],
            None,
            |ctx, _| {
                let now = ctx.now;
                ctx.doc()?
                    .set_line_ending(crate::document::LineEnding::CrLf, now);
                ctx.messages
                    .push(crate::tr!("msg-line-endings", ending = "CRLF"));
                Ok(())
            },
        ),
        cmd(
            "file.toggleBom",
            "Byte Order Mark: Add or Remove",
            "File",
            &[],
            None,
            |ctx, _| {
                let d = ctx.doc()?;
                if d.meta.encoding != encoding_rs::UTF_8 {
                    return Err(CommandError::new(crate::tr!("msg-bom-utf8-only")));
                }
                let bom = !d.meta.bom;
                d.set_bom(bom);
                ctx.messages.push(crate::l10n::tr(if bom {
                    "msg-bom-added"
                } else {
                    "msg-bom-removed"
                }));
                Ok(())
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
            |ctx, _| {
                // The tree hidden (as it is by default), revealing shows it.
                if !ctx.config.bool("ui.folder_tree") {
                    ctx.requests.push(Request::SetSetting {
                        key: "ui.folder_tree".into(),
                        value: true.into(),
                        quiet: true,
                    });
                }
                request(ctx, Request::Project(ProjectRequest::RevealInTree))
            },
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
                // Patterns a caller (a plugin's graph, say) leaves out besides
                // the folder's own: its application's folders, backups.
                let ignore = args
                    .get("ignore")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default();
                request(ctx, Request::SearchIn { dir, ignore })
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
        // The project's run and test commands (Projectile's `p R` and
        // `p T`): the language plugin's, else one asked for.
        cmd(
            "project.run",
            "Run Project",
            "Project",
            &[],
            None,
            |ctx, args| run_project(ctx, args, "project.run", "run"),
        ),
        cmd(
            "project.test",
            "Test Project",
            "Project",
            &[],
            None,
            |ctx, args| run_project(ctx, args, "project.test", "test"),
        ),
        cmd(
            "project.testAtCursor",
            "Test at Cursor",
            "Project",
            &[],
            None,
            |ctx, args| run_project(ctx, args, "project.testAtCursor", "testAtPoint"),
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
            |ctx, args| match args.get("path").and_then(Value::as_str) {
                Some(path) => request(ctx, Request::Project(ProjectRequest::Add(path.to_string()))),
                // The folder is asked for, the document's own offered
                // (Doom's `SPC p a`); adding it unasked did nothing to see
                // when it already was a project.
                None => request(
                    ctx,
                    Request::Ask {
                        command: "project.add".into(),
                        args: serde_json::json!({}),
                        arg: "path".into(),
                    },
                ),
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
        // Whether a folder under version control becomes a project when
        // one of its files opens (`projects.auto_add`). Off by default:
        // projects are added by hand (owner, 2026-10-06).
        cmd(
            "project.toggleAutoAdd",
            "Toggle Adding Projects Automatically",
            "Project",
            &[],
            None,
            |ctx, _| {
                let on = !ctx.config.bool("projects.auto_add");
                ctx.messages.push(crate::l10n::tr(if on {
                    "msg-project-auto-add-on"
                } else {
                    "msg-project-auto-add-off"
                }));
                request(
                    ctx,
                    Request::SetSetting {
                        key: "projects.auto_add".into(),
                        value: Value::Bool(on),
                        quiet: true,
                    },
                )
            },
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
        // A grid finds and replaces in its cells (`viewer.grid.find`).
        cmd(
            "find.open",
            "Find",
            "Find",
            &["ctrl+f"],
            Some("!viewerGrid"),
            |ctx, _| request(ctx, Request::Find { replace: false }),
        ),
        cmd(
            "find.replace",
            "Find and Replace",
            "Find",
            &["ctrl+h"],
            Some("!viewerGrid"),
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
                // A negative index is the last workspace (Doom's `SPC TAB 0`).
                let i = args.get("index").and_then(Value::as_i64).unwrap_or(0);
                let op = match usize::try_from(i) {
                    Ok(i) => WorkspaceOp::Switch(i),
                    Err(_) => WorkspaceOp::Final,
                };
                request(ctx, Request::Workspace(op))
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
            |ctx, args| {
                let force = args.get("force").and_then(Value::as_bool) == Some(true);
                request(ctx, Request::Pane(PaneOp::CloseOrQuit { force }))
            },
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
        cmd(
            "pane.next",
            "Next Pane",
            "Window",
            &[],
            None,
            |ctx, args| {
                // `back`: the one before in the cycle (Vim's `C-w W`).
                request(ctx, Request::Pane(PaneOp::Cycle(arg_bool(args, "back"))))
            },
        ),
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
                let dir = notes_folder(ctx)?;
                request(
                    ctx,
                    Request::SearchIn {
                        dir,
                        ignore: Vec::new(),
                    },
                )
            },
        ),
        // Doom's `SPC n F`: the notes folder in the file manager.
        cmd(
            "notes.browse",
            "Browse Notes",
            "Search",
            &[],
            None,
            |ctx, _| {
                let dir = notes_folder(ctx)?;
                request(
                    ctx,
                    Request::Open {
                        path: Some(dir.display().to_string()),
                    },
                )
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
                        detail: String::new(),
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
            |ctx, args| {
                let force = args.get("force").and_then(Value::as_bool) == Some(true);
                request(ctx, Request::QuitWithoutSaving { force })
            },
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
            Some(TEXT_ONLY),
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
        // A list's or a table's item typed in the settings panel: in place
        // of item `at`, else added (a table's as `path = mode`).
        cmd(
            "settings.item",
            "Set a Setting's Item",
            "View",
            &[],
            None,
            |ctx, args| {
                let key = arg_str(args, "key")?.to_string();
                let spec = crate::settings::spec(&key)
                    .ok_or_else(|| CommandError::new(format!("Unknown setting `{key}`")))?;
                let at = args
                    .get("at")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok());
                let text = arg_str(args, "value")?;
                let field = crate::settings_list::Field::of(spec);
                let value = crate::settings_list::put(ctx.config, &field, at, text)
                    .map_err(CommandError::new)?;
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
                        detail: String::new(),
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
        // Doom's `SPC o o` and `SPC o O` (`:os macos`): the file, else the
        // listing's folder, or with `project` the project's folder, in
        // Finder or the system's file manager.
        cmd(
            "file.reveal",
            "Show in System File Manager",
            "File",
            &[],
            None,
            |ctx, args| {
                // A path named (by a plugin, for a file Kalem does not
                // open): shown in its folder.
                if let Some(p) = args.get("path").and_then(Value::as_str) {
                    let path = std::path::PathBuf::from(crate::settings::expand_home(p));
                    if !path.exists() {
                        return Err(CommandError::new(crate::tr!(
                            "msg-md-missing-file",
                            path = path.display().to_string()
                        )));
                    }
                    let path = std::path::absolute(&path).unwrap_or(path);
                    return request(
                        ctx,
                        Request::OpenLink(crate::input::LinkAction::Reveal(path)),
                    );
                }
                let doc = ctx.doc()?;
                let file = doc
                    .meta
                    .path
                    .as_deref()
                    .filter(|_| doc.dired().is_none())
                    .and_then(|p| std::path::absolute(p).ok());
                let dir = crate::command::folder_of(doc);
                let path = if args.get("project").and_then(Value::as_bool) == Some(true) {
                    dir.map(|d| kalem_project::list::detect_root(&d).unwrap_or(d))
                } else {
                    file.or(dir)
                }
                .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-no-file")))?;
                request(
                    ctx,
                    Request::OpenLink(crate::input::LinkAction::Reveal(path)),
                )
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
        // Vim's `z` keys, as Doom's Org module has them.
        cmd("view.foldOpen", "Open Fold", "View", &[], None, |ctx, _| {
            request(ctx, Request::FoldOp(crate::view::FoldOp::Open))
        }),
        cmd(
            "view.foldOpenSubtree",
            "Open Fold and the Folds Under It",
            "View",
            &[],
            None,
            |ctx, _| request(ctx, Request::FoldOp(crate::view::FoldOp::OpenSubtree)),
        ),
        cmd(
            "view.foldClose",
            "Close Fold",
            "View",
            &[],
            None,
            |ctx, _| request(ctx, Request::FoldOp(crate::view::FoldOp::Close)),
        ),
        cmd(
            "view.foldToggle",
            "Toggle Fold",
            "View",
            &[],
            None,
            |ctx, _| request(ctx, Request::FoldOp(crate::view::FoldOp::Toggle)),
        ),
        cmd(
            "view.foldCloseAll",
            "Close All Folds",
            "View",
            &[],
            None,
            |ctx, _| request(ctx, Request::FoldOp(crate::view::FoldOp::CloseAll)),
        ),
        cmd(
            "view.foldOpenAll",
            "Open All Folds",
            "View",
            &[],
            None,
            |ctx, _| request(ctx, Request::FoldOp(crate::view::FoldOp::OpenAll)),
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
        // `org-toggle-heading` (C-c *, Doom's SPC m h).
        cmd(
            "org.headline.toggle",
            "Toggle Heading",
            "Headlines",
            &[],
            Some(ORG),
            |ctx, _| {
                ctx.org(|d, p, m| {
                    let t =
                        org_edit::toggle::toggle_heading(&text_of(d), p, m, d.parse().context())?;
                    if t.is_empty() {
                        return Err(org_edit::EditError {
                            message: crate::tr!("msg-cannot-toggle-heading"),
                            point: None,
                        });
                    }
                    Ok(t)
                })
            },
        ),
        // `org-toggle-item` (C-c -, Doom's SPC m i).
        cmd(
            "list.toggleItem",
            "Toggle Item",
            "Lists",
            &[],
            Some(ORG),
            |ctx, _| {
                ctx.org(|d, p, m| {
                    org_edit::toggle::toggle_item(&text_of(d), p, m, d.parse().context())
                })
            },
        ),
        // `org-insert-heading` (M-RET): in a list Alt+Enter is Insert Item.
        cmd(
            "org.headline.insert",
            "Insert Heading",
            "Headlines",
            &["alt+enter"],
            Some("editorMode == org && !inList && !inTable && !inBlock"),
            |ctx, _| {
                ctx.org(|d, p, _| {
                    h::insert_heading(&text_of(d), p, h::HeadingPlace::Here, d.parse().context())
                })
            },
        ),
        cmd(
            "org.headline.insertAfterSubtree",
            "Insert Heading After Subtree",
            "Headlines",
            &[],
            Some(ORG),
            |ctx, _| {
                ctx.org(|d, p, _| {
                    let place = h::HeadingPlace::AfterSubtree;
                    h::insert_heading(&text_of(d), p, place, d.parse().context())
                })
            },
        ),
        cmd(
            "org.headline.insertSubheading",
            "Insert Subheading",
            "Headlines",
            &[],
            Some(ORG),
            |ctx, _| ctx.org(|d, p, _| h::insert_subheading(&text_of(d), p, d.parse().context())),
        ),
        // `org-insert-todo-heading` (M-S-RET): in a list, an item with a
        // checkbox.
        cmd(
            "org.headline.insertTodo",
            "Insert TODO Heading",
            "Headlines",
            &[],
            Some("editorMode == org && !inTable && !inBlock"),
            |ctx, args| {
                let place = if arg_bool(args, "afterSubtree") {
                    h::HeadingPlace::AfterSubtree
                } else {
                    h::HeadingPlace::Here
                };
                ctx.org(|d, p, _| match l::insert_item(d, p, true) {
                    Some(t) if place == h::HeadingPlace::Here => Ok(t),
                    _ => h::insert_todo_heading(&text_of(d), p, place, false, d.parse().context()),
                })
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
                if args.get("state").is_none() {
                    return choose_todo_state(ctx);
                }
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
        // Shift with the arrows on a timestamp, before a heading's
        // priority and TODO state as in Org (`org-shiftup` and the
        // others); with a selection they extend it.
        cmd(
            "org.timestamp.up",
            "Increase Date at Cursor",
            "Tasks",
            &["shift+up"],
            Some(ON_TIMESTAMP),
            |ctx, _| timestamp_change(ctx, 1, None),
        ),
        cmd(
            "org.timestamp.down",
            "Decrease Date at Cursor",
            "Tasks",
            &["shift+down"],
            Some(ON_TIMESTAMP),
            |ctx, _| timestamp_change(ctx, -1, None),
        ),
        cmd(
            "org.timestamp.dayLater",
            "One Day Later",
            "Tasks",
            &["shift+right"],
            Some(ON_TIMESTAMP),
            |ctx, _| timestamp_change(ctx, 1, Some(org_edit::timestamp::TsField::Day)),
        ),
        cmd(
            "org.timestamp.dayEarlier",
            "One Day Earlier",
            "Tasks",
            &["shift+left"],
            Some(ON_TIMESTAMP),
            |ctx, _| timestamp_change(ctx, -1, Some(org_edit::timestamp::TsField::Day)),
        ),
        cmd(
            "org.priority.set",
            "Set Priority",
            "TODO",
            &[],
            Some(ORG),
            |ctx, args| {
                if args.get("priority").is_none() {
                    return choose_priority(ctx);
                }
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
        // Babel's commands serve the source blocks of Org documents (their
        // text type is the block's language), and the results come back
        // wherever the cursor has gone.
        scoped(
            cmd(
                "org.babel.execute",
                "Execute Source Block",
                "Babel",
                &[],
                Some("editorMode == org && inSrcBlock"),
                crate::babel::execute,
            ),
            crate::command::Scope::all(),
        ),
        scoped(
            cmd(
                "org.babel.insertResult",
                "Write a Source Block's Results",
                "Babel",
                &[],
                Some(ORG),
                crate::babel::insert_result,
            ),
            crate::command::Scope::all(),
        ),
        scoped(
            cmd(
                "org.babel.exportStep",
                "Run an Export's Source Blocks",
                "Babel",
                &[],
                Some(ORG),
                crate::babel::export::step,
            ),
            crate::command::Scope::all(),
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
                let path = ctx.doc()?.meta.path.clone();
                let link = crate::links::same_file_link(&link, path.as_deref());
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
        // In any mode: the lines `start` to `end` (from 1) a plugin names
        // (an outliner's block, a function), else the selection's lines;
        // `label` what the part is called, shown above it.
        cmd(
            "view.narrowToLines",
            "Narrow to Lines",
            "View",
            &[],
            Some(TEXT_ONLY),
            |ctx, args| {
                let line = |k: &str| {
                    args.get(k)
                        .and_then(|v| v.as_u64().or_else(|| v.as_str()?.trim().parse().ok()))
                        .map(|n| (n as usize).max(1) - 1)
                };
                let bad = || CommandError::new(crate::l10n::tr("msg-bad-line"));
                let doc = ctx.doc()?;
                let (first, last) = match (line("start"), line("end")) {
                    (Some(s), e) => (s, e.unwrap_or(s)),
                    (None, Some(_)) => return Err(bad()),
                    (None, None) => {
                        let s = doc.selection;
                        let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
                        let t = doc.text();
                        // A selection that ends at a line's start leaves
                        // that line out.
                        let mut last = t.line_of(b);
                        if b > a && t.line_start(last) == b {
                            last -= 1;
                        }
                        (t.line_of(a), last)
                    }
                };
                if last < first {
                    return Err(bad());
                }
                doc.narrow_to_lines(first, last);
                doc.narrowing_label = args
                    .get("label")
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(str::to_string);
                Ok(())
            },
        ),
        // A layer's line under the cursor's line that shows a file's text
        // (API 0.2.15), typed into in place: the first, or line `row`.
        cmd(
            "view.editRow",
            "Edit Line Under Line",
            "View",
            &[],
            Some(TEXT_ONLY),
            |ctx, args| {
                let doc = ctx.doc()?;
                let line = doc
                    .text()
                    .line_start(doc.text().line_of(doc.selection.head));
                let rows = doc.editable_rows(line);
                let row = match args.get("row").and_then(serde_json::Value::as_u64) {
                    Some(r) => rows.into_iter().find(|x| *x == r as usize),
                    None => rows.first().copied(),
                };
                match row {
                    Some(r) if doc.start_row_edit(line, r) => Ok(()),
                    _ => Err(CommandError::new(crate::l10n::tr("msg-no-row"))),
                }
            },
        ),
        cmd("view.widen", "Widen", "View", &[], None, |ctx, _| {
            let doc = ctx.doc()?;
            doc.narrowing = None;
            doc.narrowing_label = None;
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
    doc.narrowing_label = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::CommandSource;
    use crate::when::WhenClause;

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
        assert_eq!(planning_input("friday /😀"), ("friday /😀", None));
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
    fn copy_link_at_the_cursor() {
        let reg = CommandRegistry::with_builtins();
        let config = crate::settings::Config::default();
        let mut clip = Clipboard::default();
        let text = "See [[https://kalem.org/x][the site]] and [[file:a.org::*B][a]].\n";
        let mut copied = Vec::new();
        for at in [12, 45, 0] {
            let mut d = doc(text, at);
            let mut ctx = EditorContext {
                document: Some(&mut d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 10, 1).at(9, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            let r = reg.execute("org.link.copy", &mut ctx, &json!({}));
            copied.push(match (r, ctx.requests.last()) {
                (Ok(()), Some(Request::CopyText(t))) => t.clone(),
                (r, _) => format!("{r:?}"),
            });
        }
        assert_eq!(copied[0], "https://kalem.org/x");
        assert_eq!(copied[1], "file:a.org::*B");
        assert!(copied[2].starts_with("Err"), "{}", copied[2]);
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
    fn a_file_opens_at_a_line() {
        let reg = CommandRegistry::with_builtins();
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let mut ctx = EditorContext {
            document: None,
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 8).at(10, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        let path = std::env::temp_dir().join("a.rs").display().to_string();
        reg.execute(
            "file.open",
            &mut ctx,
            &json!({ "path": path, "line": 12, "column": 4 }),
        )
        .unwrap();
        reg.execute("file.open", &mut ctx, &json!({ "path": path, "line": 0 }))
            .unwrap();
        assert_eq!(
            ctx.requests,
            [
                Request::OpenAt {
                    path: path.clone(),
                    line: 12,
                    column: 4
                },
                Request::OpenAt {
                    path,
                    line: 1,
                    column: 0
                }
            ]
        );
    }

    #[test]
    fn opening_in_a_missing_folder_asks_first() {
        crate::l10n::set_language("en");
        let dir = std::env::temp_dir().join(format!("kalem-open-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("new/a.txt");
        let reg = CommandRegistry::with_builtins();
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let mut ctx = EditorContext {
            document: None,
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 7).at(10, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute("file.open", &mut ctx, &json!({ "path": path }))
            .unwrap();
        let Some(Request::Choose(items)) = ctx.requests.pop() else {
            panic!("asked first");
        };
        assert_eq!(items[0].title, "Create it and open a.txt");
        // The folder as the system writes paths: `\new` on Windows.
        let missing = format!("{}new does not exist", std::path::MAIN_SEPARATOR);
        assert!(
            items[0].category.ends_with(&missing),
            "{}",
            items[0].category
        );
        assert!(!dir.exists());
        let (id, args) = crate::palette::split_invocation(&items[0].id);
        reg.execute(id, &mut ctx, &args).unwrap();
        assert!(dir.join("new").is_dir());
        assert!(matches!(
            ctx.requests.pop(),
            Some(Request::Open { path: Some(p) }) if std::path::Path::new(&p) == path
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn show_in_pdf_says_why_it_cannot() {
        // A line the PDF has: its page. A file the build left out (not in
        // the SyncTeX file): said so, not "build it again". No SyncTeX
        // file: build again.
        crate::l10n::set_language("en");
        let dir = std::env::temp_dir().join(format!("kalem-show-in-pdf-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let main = dir.join("main.tex");
        let lines: String = (1..=10).map(|i| format!("% line {i}\n")).collect();
        std::fs::write(&main, format!("\\documentclass{{article}}\n{lines}")).unwrap();
        let left_out = dir.join("left-out.tex");
        std::fs::write(&left_out, "% !TEX root = main.tex\nText.\n").unwrap();
        std::fs::write(dir.join("main.pdf"), b"%PDF-1.5\n").unwrap();
        let synctex =
            crate::synctex::tests::SAMPLE.replace("/d/./", &format!("{}/./", dir.display()));
        std::fs::write(dir.join("main.synctex"), synctex).unwrap();
        let reg = CommandRegistry::with_builtins();
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let mut show = |file: &std::path::Path, line: usize| {
            let mut d = DocumentState::open(
                file,
                Arc::new(org_model::Settings::default()),
                &Default::default(),
            )
            .unwrap();
            let at = d.text().line_start(line - 1);
            d.move_cursor(at, false);
            let mut ctx = EditorContext {
                document: Some(&mut d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 10, 6).at(10, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            reg.execute("latex.showInPdf", &mut ctx, &json!({}))
                .unwrap();
            (ctx.messages, ctx.requests)
        };
        let (messages, requests) = show(&main, 7);
        assert!(messages.is_empty(), "{messages:?}");
        assert!(
            matches!(&requests[..], [Request::OpenAt { line: 1, .. }]),
            "{requests:?}"
        );
        let (messages, _) = show(&left_out, 2);
        assert_eq!(messages, [crate::l10n::tr("msg-synctex-not-built")]);
        std::fs::remove_file(dir.join("main.synctex")).unwrap();
        let (messages, _) = show(&main, 7);
        assert_eq!(messages, [crate::l10n::tr("msg-no-synctex")]);
        let _ = std::fs::remove_dir_all(&dir);
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
        // Without a state, the keywords to choose from.
        reg.execute("org.todo.set", &mut ctx, &json!({})).unwrap();
        assert!(matches!(ctx.requests.last(), Some(Request::Choose(_))));
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
        // The wiki pages are walked on a thread of their own: a wait that
        // a runner with every core busy does not run out of (the walk
        // missed its 100 ms on Windows); it ends with the last answer.
        let items = crate::completers::Registry::with_builtins().complete(
            &mut d,
            false,
            std::time::Duration::from_secs(10),
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
        let v = ctx.document.as_deref().unwrap().csv().unwrap().view;
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
        d.csv_mut().unwrap().view = crate::csv::View {
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
            d.csv()
                .unwrap()
                .columns
                .hidden
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        let line = |d: &crate::DocumentState, n: usize| {
            let t = d.text();
            let r = t.line_range(n);
            crate::csv::line_view(&crate::csv::layout(d), t.as_str(), r, None).display()
        };
        assert_eq!(line(d, 1), "1");
        d.csv_mut().unwrap().columns.hidden.clear();
        d.csv_mut().unwrap().columns.widths.insert(1, 6);
        assert_eq!(line(d, 1), "Ada  │ a lon… │ 1");
        assert_eq!(line(d, 2), "Bob  │ x      │ 2");
        d.csv_mut().unwrap().columns.hidden.insert(2);
        assert_eq!(line(d, 2), "Bob  │ x");
        d.csv_mut().unwrap().columns.frozen = true;
        assert_eq!(crate::csv::frozen_width(&crate::csv::layout(d)), Some(6));
        // Autosize: the widest value, uncut.
        d.csv_mut().unwrap().columns = crate::csv::Columns::default();
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
        assert_eq!(d.csv().unwrap().columns.widths.get(&1), Some(&14));
        // A column given, as dragging its edge gives it.
        reg.execute(
            "csv.setColumnWidth",
            &mut ctx,
            &json!({"width": "7", "column": 2}),
        )
        .unwrap();
        let d = ctx.document.as_deref().unwrap();
        assert_eq!(d.csv().unwrap().columns.widths.get(&2), Some(&7));
        assert_eq!(d.csv().unwrap().columns.widths.get(&1), Some(&14));
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
        assert!(!d.csv().unwrap().paste_block);
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
    fn csv_steps_through_the_rows_the_view_shows() {
        // With a filter on, Enter, Tab and the arrows go to the next row
        // shown, not onto a hidden one; at the last row shown, with
        // hidden rows after it, Enter stays.
        let reg = CommandRegistry::with_builtins();
        let mut d = doc(
            "name,city\nAda,Izmir\nBob,Ankara\nCem,Izmir\nDen,Bursa\n",
            10,
        );
        d.set_mode(
            DocumentMode::Csv,
            &crate::settings::Config::default().parse_base(),
        );
        d.csv_mut().unwrap().filter = Some("Izmir".into());
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 6).at(10, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        // The cell after a command, the text and the selection's head.
        let mut run = |id: &str| {
            reg.execute(id, &mut ctx, &json!({})).unwrap();
            let d = ctx.document.as_deref().unwrap();
            (
                crate::csv::cell_at(d).map(|(_, row, _, col)| (row, col)),
                d.text().as_str().to_string(),
                d.selection.head,
            )
        };
        assert_eq!(run("csv.cellBelow").0, Some((3, 0)), "Cem, past Bob");
        assert_eq!(run("csv.cellAbove").0, Some((1, 0)), "Ada, past Bob");
        assert_eq!(run("csv.nextField").0, Some((1, 1)));
        assert_eq!(run("csv.nextField").0, Some((3, 0)), "Tab to Cem");
        assert_eq!(run("csv.previousField").0, Some((1, 1)), "back to Ada");
        // Shift with the arrow: the selection's corner on Cem's row.
        let (_, text, head) = run("csv.extendDown");
        assert!(text[head..].starts_with("Izmir\nDen"), "{head}");
        // Enter within those two cells would go between them: collapsed
        // on Cem's first.
        run("csv.cancelEdit");
        let (_, before, _) = run("csv.cellBelow");
        let (cell, after, _) = run("csv.cellBelow");
        assert_eq!(cell, Some((3, 1)), "Den is hidden: it stays");
        assert_eq!(after, before);
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
    fn narrowed_to_lines_in_any_mode() {
        let reg = CommandRegistry::with_builtins();
        let mut d = doc("# A\n- one\n  - two\n- three\n", 0);
        d.set_mode(DocumentMode::Markdown, &org_syntax::ParseContext::default());
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        let clock = jiff::civil::date(2026, 10, 14).at(10, 0, 0, 0);
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock,
            messages: Vec::new(),
            requests: Vec::new(),
        };
        // Lines a plugin names, from 1: the cursor taken into them.
        reg.execute(
            "view.narrowToLines",
            &mut ctx,
            &json!({"start": 2, "end": 3}),
        )
        .unwrap();
        let shown = |d: &DocumentState| {
            d.narrowing
                .clone()
                .map(|r| d.text().as_str()[r].to_string())
        };
        let d = ctx.document.as_deref_mut().unwrap();
        assert_eq!(shown(d).as_deref(), Some("- one\n  - two\n"));
        assert_eq!(d.selection, org_edit::Selection::caret(4));
        // It stays when the mode changes; the cursor stays in it, at the
        // end of its last line, not past its line feed.
        d.set_mode(
            DocumentMode::Text { language: None },
            &org_syntax::ParseContext::default(),
        );
        assert!(d.narrowing.is_some());
        d.move_cursor(d.text().len(), false);
        assert!(d.confine());
        assert_eq!(d.selection.head, "# A\n- one\n  - two".len());
        assert!(!d.confine());
        // Without lines: the selection's, the line it ends at the start
        // of left out.
        reg.execute("view.widen", &mut ctx, &json!({})).unwrap();
        let d = ctx.document.as_deref_mut().unwrap();
        d.selection = org_edit::Selection {
            anchor: 0,
            head: "# A\n- one\n".len(),
        };
        reg.execute("view.narrowToLines", &mut ctx, &json!({}))
            .unwrap();
        let d = ctx.document.as_deref_mut().unwrap();
        assert_eq!(shown(d).as_deref(), Some("# A\n- one\n"));
        // The last line without a line feed, and lines past the text.
        reg.execute(
            "view.narrowToLines",
            &mut ctx,
            &json!({"start": 4, "end": 9}),
        )
        .unwrap();
        let d = ctx.document.as_deref_mut().unwrap();
        assert_eq!(shown(d).as_deref(), Some("- three\n"));
        assert!(
            reg.execute(
                "view.narrowToLines",
                &mut ctx,
                &json!({"start": 3, "end": 2})
            )
            .is_err()
        );
        // A part named: its name while it is narrowed, none once widened or
        // narrowed again unnamed.
        reg.execute(
            "view.narrowToLines",
            &mut ctx,
            &json!({"start": 2, "end": 3, "label": "Notes › A"}),
        )
        .unwrap();
        let d = ctx.document.as_deref_mut().unwrap();
        assert_eq!(d.narrowed_label(), Some("Notes › A"));
        reg.execute("view.widen", &mut ctx, &json!({})).unwrap();
        let d = ctx.document.as_deref_mut().unwrap();
        assert_eq!(
            (d.narrowed_label(), d.narrowing_label.as_deref()),
            (None, None)
        );
        reg.execute(
            "view.narrowToLines",
            &mut ctx,
            &json!({"start": 2, "end": 3, "label": "Notes › A"}),
        )
        .unwrap();
        reg.execute("view.narrowToLines", &mut ctx, &json!({"start": 2}))
            .unwrap();
        assert_eq!(ctx.document.as_deref().unwrap().narrowed_label(), None);
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
        // Babel runs the block from inside it, by Enter (org.dwim) too.
        assert!(when("org.babel.execute"));
        assert!(when("org.dwim"));
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
        let notes = format!(
            "notes.directory = {:?}\n",
            dir.join("notes").display().to_string()
        );
        let (reg, mut clip, config) = (
            CommandRegistry::with_builtins(),
            Clipboard::default(),
            crate::settings::Config::from_layers(&[(crate::settings::Layer::User, None, &notes)]),
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
        // `SPC o o` shows the file in Finder, `SPC o O` the project.
        use crate::input::LinkAction::Reveal;
        let req = run(&mut d, "file.reveal", json!({}));
        assert!(matches!(&req[..], [Request::OpenLink(Reveal(p))] if p.ends_with("notes/a.org")));
        let req = run(&mut d, "file.reveal", json!({ "project": true }));
        assert!(
            matches!(&req[..], [Request::OpenLink(Reveal(p))] if !p.ends_with("notes") && dir.ends_with(p.file_name().unwrap())),
            "{req:?}"
        );
        // A path named, as a plugin names a file Kalem does not open; a
        // path that is not there refused.
        let board = dir.join("notes/Board.canvas");
        std::fs::write(&board, "{}").unwrap();
        let req = run(
            &mut d,
            "file.reveal",
            json!({ "path": board.display().to_string() }),
        );
        assert!(
            matches!(&req[..], [Request::OpenLink(Reveal(p))] if *p == board),
            "{req:?}"
        );
        let mut ctx = EditorContext {
            document: Some(&mut d),
            clipboard: &mut Clipboard::default(),
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 9, 30).at(9, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        let gone = dir.join("notes/gone.canvas").display().to_string();
        assert!(
            reg.execute("file.reveal", &mut ctx, &json!({ "path": gone }))
                .is_err()
        );
        // A folder searched, with patterns left out besides its own.
        let req = run(
            &mut d,
            "search.folder",
            json!({ "path": dir.display().to_string(), "ignore": ["/logseq/", 3] }),
        );
        assert!(
            matches!(&req[..], [Request::SearchIn { dir: p, ignore }] if *p == dir && *ignore == ["/logseq/"]),
            "{req:?}"
        );
        let req = run(&mut d, "notes.search", json!({}));
        assert!(matches!(&req[..], [Request::SearchIn { ignore, .. }] if ignore.is_empty()));
        // `SPC n F`: the notes folder in the file manager.
        let req = run(&mut d, "notes.browse", json!({}));
        assert!(matches!(&req[..], [Request::Open { path: Some(p) }] if p.ends_with("notes")));
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
    fn last_workspace_and_panes_back() {
        let (reg, mut clip, config) = (
            CommandRegistry::with_builtins(),
            Clipboard::default(),
            crate::settings::Config::default(),
        );
        let mut run = |id: &str, args: serde_json::Value| {
            let mut ctx = EditorContext {
                document: None,
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 10, 7).at(9, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            reg.execute(id, &mut ctx, &args).unwrap();
            ctx.requests
        };
        // `SPC TAB 0` the last workspace, `SPC TAB 3` the third.
        let req = run("workspace.switch", json!({ "index": -1 }));
        assert_eq!(req, [Request::Workspace(WorkspaceOp::Final)]);
        let req = run("workspace.switch", json!({ "index": 2 }));
        assert_eq!(req, [Request::Workspace(WorkspaceOp::Switch(2))]);
        // `SPC w w` and `SPC w W`.
        let req = run("pane.next", json!({}));
        assert_eq!(req, [Request::Pane(PaneOp::Cycle(false))]);
        let req = run("pane.next", json!({ "back": true }));
        assert_eq!(req, [Request::Pane(PaneOp::Cycle(true))]);
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
