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
        ("file.open", object(&[("path", "string", false)])),
        ("org.property.delete", object(&[("key", "string", true)])),
        ("org.cite.insert", object(&[("key", "string", false)])),
        ("org.insert.drawer", object(&[("name", "string", true)])),
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
        ("format.font", object(&[("family", "string", true)])),
        ("format.size", object(&[("size", "string", true)])),
        ("format.color", object(&[("color", "string", true)])),
        ("format.highlight", object(&[("color", "string", true)])),
        ("format.align", object(&[("align", "string", true)])),
        ("format.documentFont", object(&[("family", "string", true)])),
        ("format.documentSize", object(&[("size", "string", true)])),
        ("format.lineSpacing", object(&[("spacing", "string", true)])),
        ("format.spaceBefore", object(&[("points", "string", true)])),
        ("format.spaceAfter", object(&[("points", "string", true)])),
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
        ("view.setMode", {
            let mut s = object(&[("mode", "string", true)]);
            s["properties"]["mode"]["enum"] = serde_json::json!(["org", "markdown", "csv", "text"]);
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
        todo(d, p, &opts).map(|o| o.transaction)
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
    let base = ctx.config.todo_settings();
    let mut message = String::new();
    ctx.org(|d, p, _| {
        let (t, m) = schedule(d, p, kind, &change, &base.for_document(d))?;
        message = m;
        Ok(t)
    })?;
    ctx.messages.push(message);
    Ok(())
}

fn priority(ctx: &mut EditorContext<'_>, a: org_edit::todo::PriorityAction) -> CommandResult {
    let base = ctx.config.todo_settings();
    ctx.org(|d, p, _| org_edit::todo::priority(d, p, a, false, &base.for_document(d)))
}

/// Kalem's character formatting on the selection (or the word at the
/// cursor), as a word processor formats (`crate::rich`).
/// In a strict `.org` file (§3.7), Kalem's formatting is not written:
/// the frontend offers to make the document a Kalem document or to allow
/// the formatting in this file. Whether that happened.
fn strict_org(ctx: &mut EditorContext<'_>) -> bool {
    let config = ctx.config;
    let Some(doc) = ctx.document.as_deref() else {
        return false;
    };
    if crate::kinds::file_kind(doc) != Some("org") || crate::kinds::markup_allowed(doc, config) {
        return false;
    }
    ctx.messages.push(crate::l10n::tr("kind-offer"));
    ctx.requests
        .push(Request::Choose(crate::kinds::offer_items()));
    true
}

fn rich_format(ctx: &mut EditorContext<'_>, change: crate::rich::Change) -> CommandResult {
    // Clearing takes Kalem's formatting away, which strict Org allows.
    if change != crate::rich::Change::Clear && strict_org(ctx) {
        return Ok(());
    }
    ctx.org(|d, p, m| {
        let root = d.parse().syntax();
        let text = root.text().to_string();
        let (mut s, mut e) = m.map_or((p, p), |m| (m.min(p), m.max(p)));
        if s == e {
            // The word at the cursor, over formatting snippets.
            let mut p = p;
            while let Some(m) = crate::rich::marker_at(&root, p) {
                p = m.end;
            }
            let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
            s = text[..p]
                .char_indices()
                .rev()
                .take_while(|(_, c)| is_word(*c))
                .last()
                .map_or(p, |(i, _)| i);
            e = p + text[p..]
                .char_indices()
                .take_while(|(_, c)| is_word(*c))
                .last()
                .map_or(0, |(i, c)| i + c.len_utf8());
            if s == e {
                return Err(org_edit::EditError {
                    message: crate::l10n::tr("msg-select-text"),
                    point: None,
                });
            }
        }
        let (tx, r) =
            crate::rich::apply(&root, &text, s..e, &change).ok_or_else(|| org_edit::EditError {
                message: crate::l10n::tr("msg-cannot-format-here"),
                point: None,
            })?;
        // The selection covers the same text; a caret stays a caret.
        let sel = if m.is_some() {
            let (a, h) = if m.is_some_and(|m| m > p) {
                (r.end, r.start)
            } else {
                (r.start, r.end)
            };
            org_edit::Selection { anchor: a, head: h }
        } else {
            org_edit::Selection::caret(tx.map(p, org_edit::Assoc::After))
        };
        Ok(tx.select(sel))
    })
}

fn rich_color(args: &Value, key: &str) -> Result<Option<crate::theme::Color>, CommandError> {
    let v = arg_str(args, key)?.trim();
    if v.is_empty() || v.eq_ignore_ascii_case("none") || v.eq_ignore_ascii_case("auto") {
        return Ok(None);
    }
    crate::rich::parse_color(v)
        .map(Some)
        .ok_or_else(|| CommandError::new(crate::tr!("msg-not-a-color", color = v)))
}

fn align_cmd(ctx: &mut EditorContext<'_>, align: crate::rich::Align) -> CommandResult {
    if strict_org(ctx) {
        return Ok(());
    }
    ctx.org(|d, p, m| {
        let root = d.parse().syntax();
        let text = root.text().to_string();
        let (s, e) = m.map_or((p, p), |m| (m.min(p), m.max(p)));
        crate::rich::set_align(&root, &text, s..e, align).ok_or_else(|| org_edit::EditError {
            message: crate::l10n::tr("msg-not-a-paragraph"),
            point: None,
        })
    })
}

/// Writes the Kalem document as strict Org beside it (`notes.klm` to
/// `notes.org`, with unsaved changes), without Kalem's additions, and says
/// what was left out. The document itself stays as it is.
fn save_as_org(ctx: &mut EditorContext<'_>, _: &Value) -> CommandResult {
    let doc = ctx.doc()?;
    let Some(path) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let target = path.with_extension("org");
    if target.exists() {
        return Err(CommandError::new(crate::tr!(
            "kind-exists",
            path = target.display().to_string()
        )));
    }
    let (text, counts) = crate::kinds::strip_markup(doc.text().as_str());
    std::fs::write(&target, text).map_err(|e| CommandError::new(e.to_string()))?;
    ctx.messages.push(crate::tr!(
        "kind-saved-org",
        name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        dropped = crate::kinds::dropped_summary(counts)
    ));
    Ok(())
}

/// Saves the `.org` document as a `.klm` Kalem document beside it (the
/// `.org` file goes), and turns the links to it in its project (or its
/// folder) to the new name.
fn make_kalem_document(ctx: &mut EditorContext<'_>, _: &Value) -> CommandResult {
    let options = ctx.config.save_options();
    let doc = ctx.doc()?;
    let Some(old) = doc.meta.path.clone() else {
        return Err(CommandError::new(crate::l10n::tr("msg-export-needs-file")));
    };
    let old = std::path::absolute(&old).unwrap_or(old);
    let new = old.with_extension("klm");
    if new.exists() {
        return Err(CommandError::new(crate::tr!(
            "kind-exists",
            path = new.display().to_string()
        )));
    }
    doc.save_as(&new, options)
        .map_err(|e| CommandError::new(e.to_string()))?;
    if old.exists() {
        std::fs::remove_file(&old).map_err(|e| CommandError::new(e.to_string()))?;
    }
    let root = kalem_project::list::detect_root(&new)
        .or_else(|| new.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default();
    let changed = crate::kinds::update_links(&root, &old, &new);
    ctx.messages.push(crate::tr!(
        "kind-made",
        name = new
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        count = changed.len()
    ));
    // The frontends watch the new file.
    ctx.requests.push(Request::Save);
    Ok(())
}

/// Puts color `c` first in the recent colors of setting `key` (six at
/// most), for the color menus.
fn remember_color(ctx: &mut EditorContext<'_>, key: &str, c: Option<crate::theme::Color>) {
    let Some(c) = c else { return };
    let (r, g, b) = c.rgb();
    let hex = format!("#{r:02x}{g:02x}{b:02x}");
    let mut list: Vec<String> = ctx
        .config
        .strings(key)
        .into_iter()
        .filter(|s| !s.eq_ignore_ascii_case(&hex))
        .map(str::to_string)
        .collect();
    list.insert(0, hex);
    list.truncate(6);
    if list != ctx.config.strings(key) {
        ctx.requests.push(Request::SetSetting {
            key: key.to_string(),
            value: list.into(),
            quiet: true,
        });
    }
}

/// Sets the space before (or after) the paragraphs of the selection to
/// `points` (`12`, `6.5`; `0` or `none` takes it away).
fn spacing_cmd(ctx: &mut EditorContext<'_>, points: &str, after: bool) -> CommandResult {
    let v = points.trim();
    let size = match crate::rich::parse_size(v) {
        Some(s) => Some(s),
        None if v.is_empty() || v == "0" || v.eq_ignore_ascii_case("none") => None,
        None => {
            return Err(CommandError::new(crate::tr!(
                "msg-not-a-spacing",
                spacing = v
            )));
        }
    };
    let change = Some(size);
    if strict_org(ctx) {
        return Ok(());
    }
    ctx.org(|d, p, m| {
        let root = d.parse().syntax();
        let text = root.text().to_string();
        let (s, e) = m.map_or((p, p), |m| (m.min(p), m.max(p)));
        let (before, after) = if after {
            (None, change)
        } else {
            (change, None)
        };
        crate::rich::set_spacing(&root, &text, s..e, before, after).ok_or_else(|| {
            org_edit::EditError {
                message: crate::l10n::tr("msg-not-a-paragraph"),
                point: None,
            }
        })
    })
}

/// Changes the document's own defaults (`#+KALEM:`).
fn doc_defaults(
    ctx: &mut EditorContext<'_>,
    change: impl FnOnce(&mut crate::rich::DocDefaults),
) -> CommandResult {
    if strict_org(ctx) {
        return Ok(());
    }
    ctx.org(|d, _, _| {
        let root = d.parse().syntax();
        let text = root.text().to_string();
        Ok(crate::rich::set_defaults(&root, &text, change))
    })
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
    for c in &mut all {
        c.args_schema = schemas
            .iter()
            .find(|(id, _)| *id == c.id)
            .map(|(_, s)| s.clone());
    }
    all
}

/// After leaving a field: the table's formulas again, when the document
/// (`#+KALEM: recalc=auto`) or `org.table_auto_recalc` asks for it and
/// the table has any. Errors stay quiet; F9 reports them.
fn auto_recalc(ctx: &mut EditorContext<'_>) {
    let setting = ctx.config.bool("org.table_auto_recalc");
    let Ok(doc) = ctx.doc() else { return };
    let Some((parse, _)) = doc.parse() else {
        return;
    };
    let auto = match crate::rich::kalem_option(&parse.keywords(), "recalc").as_deref() {
        Some("auto") => true,
        Some("manual") => false,
        _ => setting,
    };
    if !auto {
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
    let tool = crate::pdf::detect(engine, &search)
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
        pdf_result(&path, crate::pdf::compile(&tool, engine, &tex), open_after)
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
    let pandoc = crate::pandoc::find(&search)
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
    let pandoc = crate::pandoc::find(&search)
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
                let s = d.selection;
                let mark = (s.anchor != s.head).then_some(s.anchor);
                let tx = crate::input::newline(d.text().as_str(), s.head, mark);
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
                    && let Err(e) = crate::settings::remember_mode(&p, mode.name())
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
            "org.footnote.new",
            "New Footnote",
            "Footnotes",
            &["ctrl+alt+f"],
            Some(ORG),
            |ctx, _| {
                let s = footnote_settings(ctx.config);
                ctx.org(|d, p, _| org_edit::footnote::new(&text_of(d), p, &s))
            },
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
            |ctx, _| ctx.org(|d, p, _| org_edit::footnote::delete(&text_of(d), p)),
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
        // Kalem's formatting beyond Org (§9.5).
        cmd(
            "format.font",
            "Font",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let f = arg_str(args, "family")?.trim().to_string();
                let f = (!f.is_empty() && !f.eq_ignore_ascii_case("default")).then_some(f);
                rich_format(ctx, crate::rich::Change::Font(f))
            },
        ),
        cmd(
            "format.size",
            "Font Size",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "size")?.trim();
                let size =
                    if v.is_empty() || v.eq_ignore_ascii_case("default") {
                        None
                    } else {
                        Some(crate::rich::parse_size(v).ok_or_else(|| {
                            CommandError::new(crate::tr!("msg-not-a-size", size = v))
                        })?)
                    };
                rich_format(ctx, crate::rich::Change::Size(size))
            },
        ),
        cmd(
            "format.grow",
            "Grow Font",
            "Format",
            &["ctrl+]"],
            Some(ORG),
            |ctx, _| {
                let base = (ctx.config.int("editor.font_size").clamp(6, 72) * 10) as u16;
                rich_format(ctx, crate::rich::Change::Grow { down: false, base })
            },
        ),
        cmd(
            "format.shrink",
            "Shrink Font",
            "Format",
            &["ctrl+["],
            Some(ORG),
            |ctx, _| {
                let base = (ctx.config.int("editor.font_size").clamp(6, 72) * 10) as u16;
                rich_format(ctx, crate::rich::Change::Grow { down: true, base })
            },
        ),
        cmd(
            "format.color",
            "Text Color",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let c = rich_color(args, "color")?;
                rich_format(ctx, crate::rich::Change::Color(c))?;
                remember_color(ctx, "format.recent_colors", c);
                Ok(())
            },
        ),
        cmd(
            "format.highlight",
            "Highlight",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let c = rich_color(args, "color")?;
                rich_format(ctx, crate::rich::Change::Highlight(c))?;
                remember_color(ctx, "format.recent_highlights", c);
                Ok(())
            },
        ),
        cmd(
            "format.clear",
            "Clear Formatting",
            "Format",
            &["ctrl+space"],
            Some(ORG),
            |ctx, _| rich_format(ctx, crate::rich::Change::Clear),
        ),
        cmd(
            "format.align",
            "Align",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "align")?;
                let a = crate::rich::Align::from_name(v).ok_or_else(|| {
                    CommandError::new(crate::tr!("msg-not-an-alignment", align = v))
                })?;
                align_cmd(ctx, a)
            },
        ),
        cmd(
            "format.alignLeft",
            "Align Left",
            "Format",
            &["ctrl+l"],
            Some(ORG),
            |ctx, _| align_cmd(ctx, crate::rich::Align::Left),
        ),
        cmd(
            "format.alignCenter",
            "Center",
            "Format",
            &["ctrl+e"],
            Some(ORG),
            |ctx, _| align_cmd(ctx, crate::rich::Align::Center),
        ),
        cmd(
            "format.alignRight",
            "Align Right",
            "Format",
            &["ctrl+r"],
            Some(ORG),
            |ctx, _| align_cmd(ctx, crate::rich::Align::Right),
        ),
        cmd(
            "format.documentFont",
            "Document Font",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let f = arg_str(args, "family")?.trim().to_string();
                let f = (!f.is_empty() && !f.eq_ignore_ascii_case("default"))
                    .then(|| crate::rich::FontName::new(&f));
                doc_defaults(ctx, |d| d.font = f)
            },
        ),
        cmd(
            "format.documentSize",
            "Document Font Size",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "size")?.trim();
                let size =
                    if v.is_empty() || v.eq_ignore_ascii_case("default") {
                        None
                    } else {
                        Some(crate::rich::parse_size(v).ok_or_else(|| {
                            CommandError::new(crate::tr!("msg-not-a-size", size = v))
                        })?)
                    };
                doc_defaults(ctx, |d| d.size = size)
            },
        ),
        cmd(
            "format.lineSpacing",
            "Line Spacing",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "spacing")?.trim();
                let spacing = match v.parse::<f32>() {
                    Ok(x) if (0.5..=5.).contains(&x) => Some((x * 10.).round() as u16),
                    _ if v.is_empty() || v.eq_ignore_ascii_case("default") => None,
                    _ => {
                        return Err(CommandError::new(crate::tr!(
                            "msg-not-a-spacing",
                            spacing = v
                        )));
                    }
                };
                let spacing = spacing.filter(|s| *s != 10);
                doc_defaults(ctx, |d| d.spacing = spacing)
            },
        ),
        cmd(
            "format.allowMarkup",
            "Allow Kalem's Formatting in This File",
            "Format",
            &[],
            Some("fileKind == org"),
            |ctx, _| {
                ctx.org(|d, _, _| {
                    let root = d.parse().syntax();
                    let text = root.text().to_string();
                    Ok(crate::rich::set_kalem_option(&root, &text, "markup", "yes"))
                })
            },
        ),
        cmd(
            "file.saveAsOrg",
            "Save as Org",
            "File",
            &[],
            Some("fileKind == klm"),
            save_as_org,
        ),
        cmd(
            "file.makeKalemDocument",
            "Make Kalem Document (.klm)",
            "File",
            &[],
            Some("fileKind == org"),
            make_kalem_document,
        ),
        cmd(
            "format.spaceBefore",
            "Space Before Paragraph",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "points")?.to_string();
                spacing_cmd(ctx, &v, false)
            },
        ),
        cmd(
            "format.spaceAfter",
            "Space After Paragraph",
            "Format",
            &[],
            Some(ORG),
            |ctx, args| {
                let v = arg_str(args, "points")?.to_string();
                spacing_cmd(ctx, &v, true)
            },
        ),
        cmd(
            "format.justify",
            "Justify",
            "Format",
            &[],
            Some(ORG),
            |ctx, _| align_cmd(ctx, crate::rich::Align::Justify),
        ),
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
                let r = ctx.org(|d, p, _| {
                    let r = org_edit::recalc::recalculate(d, p, iterate)?;
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
                let content = std::fs::read_to_string(&path).map_err(|e| {
                    CommandError::new(crate::tr!(
                        "msg-cannot-read",
                        error = format!("{}: {e}", path.display())
                    ))
                })?;
                let content = content.replace("\r\n", "\n");
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
        // The document's keyword wins over the setting, both ways.
        let kw = format!("#+KALEM: recalc=auto\n{t}");
        let out = run(&kw, kw.find('2').unwrap(), "", &["table.nextField"]);
        assert!(out.contains("| 2 | 3 | 6 |"), "{out}");
        let kw = format!("#+KALEM: recalc=manual\n{t}");
        let out = run(&kw, kw.find('2').unwrap(), on, &["table.nextField"]);
        assert!(!out.contains('6'), "{out}");
        // A table without formulas is left alone.
        let plain = "| a | b |\n";
        assert!(run(plain, 2, on, &["table.nextField"]).starts_with("| a | b |"));
    }

    /// The formatting commands with arguments that work.
    const FORMATTING: &[(&str, &str)] = &[
        ("format.font", r#"{"family": "Georgia"}"#),
        ("format.size", r#"{"size": "14"}"#),
        ("format.color", r#"{"color": "red"}"#),
        ("format.highlight", r#"{"color": "yellow"}"#),
        ("format.grow", "{}"),
        ("format.shrink", "{}"),
        ("format.clear", "{}"),
        ("format.align", r#"{"align": "right"}"#),
        ("format.alignLeft", "{}"),
        ("format.alignCenter", "{}"),
        ("format.alignRight", "{}"),
        ("format.justify", "{}"),
        ("format.documentFont", r#"{"family": "Georgia"}"#),
        ("format.documentSize", r#"{"size": "12"}"#),
        ("format.lineSpacing", r#"{"spacing": "1.5"}"#),
        ("format.spaceBefore", r#"{"points": "12"}"#),
        ("format.spaceAfter", r#"{"points": "6"}"#),
    ];

    /// Runs the formatting commands `ops` (a command of [`FORMATTING`]
    /// and a selection) on `text` saved as `path`; the text after.
    fn format_ops(text: &str, path: &str, ops: &[(usize, usize, usize)]) -> String {
        let mut d = doc(text, 0);
        d.meta.path = Some(std::path::PathBuf::from(path));
        let reg = CommandRegistry::with_builtins();
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::default();
        for &(c, a, h) in ops {
            let len = d.text().len();
            let fix = |p: usize| {
                let t = d.text().as_str();
                let mut p = p % (len + 1);
                while !t.is_char_boundary(p) {
                    p -= 1;
                }
                p
            };
            let (a, h) = (fix(a), fix(h));
            d.selection = org_edit::Selection { anchor: a, head: h };
            let (id, args) = FORMATTING[c % FORMATTING.len()];
            let mut ctx = EditorContext {
                document: Some(&mut d),
                clipboard: &mut clip,
                config: &config,
                now: Instant::now(),
                clock: jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0),
                messages: Vec::new(),
                requests: Vec::new(),
            };
            let _ = reg.execute(id, &mut ctx, &serde_json::from_str(args).unwrap());
        }
        d.text().as_str().to_string()
    }

    const FORMAT_TEXT: &str =
        "#+TITLE: T\n\nSome words here, and there.\n\n| a | b |\n\n* Head\nMore çok text.\n";

    #[test]
    fn formatting_writes_markup_only_where_allowed() {
        // Every command writes Kalem's markup into a Kalem document…
        for (c, (id, _)) in FORMATTING.iter().enumerate() {
            // Clearing, and aligning left as paragraphs are, write nothing.
            if matches!(*id, "format.clear" | "format.alignLeft") {
                continue;
            }
            let out = format_ops(FORMAT_TEXT, "/k/n.klm", &[(c, 12, 17)]);
            // (Centering writes Org's own center block.)
            let center = out.contains("#+begin_center");
            assert!(
                center || !crate::kinds::markup(&org_syntax::parse(&out).syntax()).is_empty(),
                "{id}: {out}"
            );
        }
        // …and into a `.org` file that opted in.
        let opted = format!("#+KALEM: markup=yes\n{FORMAT_TEXT}");
        let out = format_ops(&opted, "/k/n.org", &[(2, 32, 37)]);
        assert!(out.contains("@@kalem:"), "{out}");
    }

    /// Every construct Kalem's formatting commands write, in
    /// `tests/corpus/klm/written.klm`, which the differential test against
    /// Emacs parses (`kalem diff-emacs`): each is one that org-element
    /// reads as Kalem does. `KALEM_WRITE_CORPUS=1` writes the file anew.
    #[test]
    fn constructs_kalem_writes() {
        let text = "* Heading\n\nOne two three four five six seven *bold* /it/ and =code= here.\n\nLeft.\n\nRight.\n\nCentered.\n\nJustified.\n\nSpaced.\n\n| cell | other |\n\n- item words\n";
        let at = |w: &str| text.find(w).unwrap();
        let word = |w: &str, c: usize| (c, at(w), at(w) + w.len());
        let idx = |id: &str| FORMATTING.iter().position(|(c, _)| *c == id).unwrap();
        let ops = [
            word("two", idx("format.font")),
            word("three", idx("format.size")),
            word("four", idx("format.color")),
            word("five", idx("format.highlight")),
            word("six", idx("format.grow")),
            word("*bold* /it", idx("format.color")),
            word("=code=", idx("format.highlight")),
            word("Heading", idx("format.color")),
            word("words", idx("format.color")),
            word("Right", idx("format.alignRight")),
            word("Centered", idx("format.alignCenter")),
            word("Justified", idx("format.justify")),
            word("Spaced", idx("format.spaceBefore")),
            word("Spaced", idx("format.spaceAfter")),
            (idx("format.documentFont"), 0, 0),
            (idx("format.documentSize"), 0, 0),
            (idx("format.lineSpacing"), 0, 0),
        ];
        // Each operation on the text as the ones before left it.
        let mut out = text.to_string();
        for (c, a, h) in ops {
            let w = &text[a..h];
            let (a, h) = if a == h {
                (0, 0)
            } else {
                let i = out.find(w).unwrap();
                (i, i + w.len())
            };
            out = format_ops(&out, "/k/n.klm", &[(c, a, h)]);
        }
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/corpus/klm/written.klm");
        if std::env::var_os("KALEM_WRITE_CORPUS").is_some() {
            std::fs::write(&file, &out).unwrap();
        }
        assert_eq!(out, std::fs::read_to_string(&file).unwrap_or_default());
        let (strict, counts) = crate::kinds::strip_markup(&out);
        assert!(counts.iter().all(|&n| n > 0), "{counts:?}");
        assert!(
            !strict.contains("kalem") && !strict.contains("KALEM"),
            "{strict}"
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
        /// A `.org` file that did not opt in never gets Kalem's markup,
        /// whatever formatting is asked for.
        #[test]
        fn strict_org_stays_strict(
            ops in proptest::collection::vec((0usize..64, 0usize..96, 0usize..96), 1..8),
        ) {
            let out = format_ops(FORMAT_TEXT, "/k/n.org", &ops);
            proptest::prop_assert_eq!(out, FORMAT_TEXT);
            let out = format_ops(FORMAT_TEXT, "/k/n.ORG_ARCHIVE", &ops);
            proptest::prop_assert_eq!(out, FORMAT_TEXT);
        }
    }

    #[test]
    fn colors_used_are_remembered() {
        use crate::command::Request;
        let mut d = doc("Some words here.\n", 0);
        d.selection = org_edit::Selection { anchor: 0, head: 4 };
        let reg = CommandRegistry::with_builtins();
        let mut clip = Clipboard::default();
        let config = crate::settings::Config::from_layers(&[(
            crate::settings::Layer::User,
            None,
            "[format]\nrecent_colors = [\"#1f5fbf\", \"#c00000\"]\n",
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
        reg.execute("format.color", &mut ctx, &json!({"color": "red"}))
            .unwrap();
        assert_eq!(
            ctx.requests,
            vec![Request::SetSetting {
                key: "format.recent_colors".into(),
                value: json!(["#c00000", "#1f5fbf"]),
                quiet: true,
            }]
        );
        // The prompt starts with the color used last.
        let mut d2 = doc("x\n", 0);
        assert_eq!(
            crate::command::argument_default_with(
                "format.color",
                "color",
                &json!({}),
                &mut d2,
                &config
            ),
            "#1f5fbf"
        );
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
