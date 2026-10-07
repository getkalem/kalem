//! Compares editing commands with Emacs: `tests/edit/expected.json` holds
//! what Emacs produced for the cases in `tests/edit/cases.json` (regenerate
//! both with `tools/edit-expected.sh`).

#![allow(clippy::print_stderr)]

use org_edit::{EditError, Transaction};
use serde_json::Value;

/// Runs the Kalem command for a case, or `None` if there is none yet.
fn run(
    cmd: &str,
    args: &[Value],
    text: &str,
    point: usize,
    mark: Option<usize>,
) -> Option<Result<Transaction, EditError>> {
    let parse = org_syntax::parse(text);
    let ctx = parse.context();
    use org_edit::headline::*;
    let arg = |i: usize| args.get(i).and_then(Value::as_u64).unwrap_or(0) as usize;
    Some(match cmd {
        "org-do-promote" => promote(text, point, ctx),
        "org-do-demote" => demote(text, point, ctx),
        "org-promote-subtree" => promote_subtree(text, point, ctx),
        "org-demote-subtree" => demote_subtree(text, point, ctx),
        "org-move-subtree-down" => move_subtree(text, point, true, ctx),
        "org-move-subtree-up" => move_subtree(text, point, false, ctx),
        "org-cut-subtree" => cut_subtree(text, point, ctx).map(|(t, _)| t),
        "ts-change" => {
            use org_edit::timestamp::{TsField, timestamp_change};
            let n = args[0].as_i64().unwrap_or(1);
            let what = (args[1] == "day").then_some(TsField::Day);
            timestamp_change(text, point, n, what, args[2].as_bool().unwrap_or(false))
        }
        "insert-heading" => {
            let place = |a: &Value| match a.as_str() {
                Some("after") => HeadingPlace::AfterSubtree,
                Some("parent") => HeadingPlace::AfterParent,
                _ => HeadingPlace::Here,
            };
            match args[0].as_str() {
                Some("sub") => insert_subheading(text, point, ctx),
                Some("todo") => {
                    // In a list `org-insert-todo-heading` inserts an item,
                    // unless it is told to insert a heading.
                    let doc = org_model::Document::new(org_syntax::parse(text));
                    let item = (args[1] != "after")
                        .then(|| org_edit::list::insert_item(&doc, point, true))
                        .flatten();
                    match item {
                        Some(t) => Ok(t),
                        None => insert_todo_heading(
                            text,
                            point,
                            place(&args[1]),
                            args[2].as_bool().unwrap_or(false),
                            ctx,
                        ),
                    }
                }
                _ => insert_heading(text, point, place(&args[0]), ctx),
            }
        }
        "copy-paste" => {
            copy_subtree(text, point, ctx).and_then(|clip| paste_subtree(text, arg(0), &clip, ctx))
        }
        "copy-paste-line2" => {
            let line2 = text.find('\n').map_or(0, |i| i + 1);
            copy_subtree(text, line2, ctx).and_then(|clip| paste_subtree(text, arg(0), &clip, ctx))
        }
        "sort" => {
            let t = args[0]
                .as_str()
                .and_then(|s| s.chars().next())
                .unwrap_or('a');
            let mut opts = org_edit::sort::SortOptions::from_char(t, args[2].as_str())?;
            opts.with_case = args[1].as_bool().unwrap_or(false);
            let doc = org_model::Document::new(org_syntax::parse(text));
            // The clock `tests/emacs/edit.el` fixes.
            let now = jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0);
            org_edit::sort::sort_entries(&doc, point, mark, &opts, now)
        }
        "todo" => return Some(todo_case(&args[0], text, point)),
        "fn-new" | "fn-sort" | "fn-normalize" | "fn-renumber" | "fn-delete" | "fn-action" => {
            use org_edit::footnote::*;
            let settings = FootnoteSettings {
                section: match args.first().and_then(Value::as_str) {
                    Some("local") => None,
                    _ => Some("Footnotes".into()),
                },
                ..FootnoteSettings::default()
            };
            match cmd {
                "fn-new" => new(text, point, &settings),
                "fn-sort" => sort(text, point, &settings),
                "fn-normalize" => normalize(text, point, &settings),
                "fn-renumber" => renumber(text, point),
                "fn-delete" => delete(text, point),
                _ => action(text, point, &settings),
            }
        }
        "schedule" => {
            use org_edit::todo::{Planning, PlanningChange, TodoSettings, schedule};
            let a = &args[0];
            let kind = if a["kind"] == "deadline" {
                Planning::Deadline
            } else {
                Planning::Scheduled
            };
            let change = if a["remove"].as_bool() == Some(true) {
                PlanningChange::Remove
            } else {
                let d = a["date"].as_str().unwrap();
                let (date, time) = d.split_once(' ').unwrap_or((d, "00:00"));
                let dt: jiff::civil::DateTime = format!("{date}T{time}").parse().unwrap();
                PlanningChange::Set(
                    dt,
                    a["time"].as_bool().unwrap_or(false),
                    a["repeater"].as_str().map(str::to_string),
                )
            };
            let settings = TodoSettings {
                adapt_indentation: a["adapt"].as_bool().unwrap_or(false),
                ..TodoSettings::default()
            };
            let doc = org_model::Document::new(org_syntax::parse(text));
            schedule(
                &doc,
                point,
                kind,
                &change,
                &settings,
                jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0),
            )
            .map(|(t, _, _)| t)
        }
        "drawer" => org_edit::insert::insert_drawer(text, point, mark, args[0].as_str().unwrap()),
        "archive-tag" | "archive-sibling" | "refile" => {
            use org_edit::archive::*;
            let doc = org_model::Document::new(org_syntax::parse(text));
            match cmd {
                "archive-tag" => toggle_archive_tag(&doc, point).map(|(t, _)| t),
                // The clock `tests/emacs/edit.el` fixes.
                "archive-sibling" => archive_to_sibling(&doc, point, "2026-09-28 Mon 10:00"),
                _ => refile(&doc, point, arg(0)),
            }
        }
        "toggle-ordered" => {
            let doc = org_model::Document::new(org_syntax::parse(text));
            org_edit::property::toggle_ordered(&doc, point).map(|(t, _)| t)
        }
        "narrow" => {
            use org_edit::narrow::*;
            let doc = org_model::Document::new(org_syntax::parse(text));
            let r = match args[0].as_str().unwrap() {
                "subtree" => subtree(&doc, point),
                "element" => element(&doc, point),
                _ => block(&doc, point),
            };
            r.map(|r| {
                let mut t = Transaction::new("narrow marks");
                t.replace(r.end..r.end, "]").unwrap();
                t.replace(r.start..r.start, "[").unwrap();
                t.select(org_edit::Selection::caret(r.start + 1))
            })
        }
        "narrowed" => {
            use org_edit::narrow::*;
            let doc = org_model::Document::new(org_syntax::parse(text));
            let range = match subtree(&doc, point) {
                Ok(r) => r,
                Err(e) => return Some(Err(e)),
            };
            let what = args[0].as_str().unwrap().to_string();
            narrowed(&doc, range, point, |sub, p| {
                let ctx = sub.parse().context();
                let sub_text = sub.parse().syntax().to_string();
                use org_edit::headline::*;
                match what.as_str() {
                    "sort" => {
                        let opts = org_edit::sort::SortOptions::from_char('a', None).unwrap();
                        org_edit::sort::sort_entries(
                            sub,
                            p,
                            None,
                            &opts,
                            jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0),
                        )
                    }
                    "move-down" => move_subtree(&sub_text, p, true, ctx),
                    "move-up" => move_subtree(&sub_text, p, false, ctx),
                    "promote" => promote_subtree(&sub_text, p, ctx),
                    _ => {
                        let settings = org_edit::todo::TodoSettings::default().for_document(sub);
                        let opts = org_edit::todo::TodoOptions {
                            arg: org_edit::todo::TodoArg::Next,
                            settings: &settings,
                            now: jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0),
                            remembered_head: None,
                            repeated: false,
                            force_note: false,
                            inhibit_note: false,
                        };
                        org_edit::todo::todo(sub, p, &opts).map(|o| o.transaction)
                    }
                }
            })
        }
        "convert" => {
            use org_table::csv::Separator;
            let doc = org_model::Document::new(org_syntax::parse(text));
            let sep = match args[0].as_str().unwrap() {
                "comma" => Separator::Comma,
                "tab" => Separator::Tab,
                "spaces2" => Separator::Spaces(2),
                _ => Separator::Auto,
            };
            org_edit::recalc::convert_region(&doc, 0, text.len(), sep)
        }
        "export" => {
            let doc = org_model::Document::new(org_syntax::parse(text));
            let format = if args[0].as_str() == Some("csv") {
                org_table::csv::Format::Csv
            } else {
                org_table::csv::Format::Tsv
            };
            org_edit::recalc::export(&doc, point, format).map(|s| {
                // As the case inserts it: point after the text.
                let mut t = Transaction::new("Export");
                let end = s.len();
                t.replace(0..text.len(), s).unwrap();
                t.select(org_edit::Selection::caret(end))
            })
        }
        "table" => {
            use org_edit::table::*;
            let doc = org_model::Document::new(org_syntax::parse(text));
            let flag = || args[1].as_bool().unwrap();
            match args[0].as_str().unwrap() {
                "align" => align_table(&doc, point),
                "insert-row" => insert_row(&doc, point, flag()),
                "kill-row" => kill_row(&doc, point),
                "move-row" => move_row(&doc, point, flag()),
                "hline" => insert_hline(&doc, point, flag()),
                "insert-column" => insert_column(&doc, point),
                "delete-column" => delete_column(&doc, point),
                "move-column" => move_column(&doc, point, flag()),
                "next-field" => next_field(&doc, point),
                "create" => create_table(
                    &doc,
                    point,
                    args[1].as_u64().unwrap() as usize,
                    args[2].as_u64().unwrap() as usize,
                ),
                "previous-field" => previous_field(&doc, point),
                "sort-rows" => sort_rows(
                    &doc,
                    point,
                    args[1].as_str().unwrap().chars().next().unwrap(),
                    args.get(2).and_then(Value::as_bool).unwrap_or(false),
                ),
                _ => next_row(&doc, point),
            }
        }
        "type" => {
            use org_edit::typing::*;
            let doc = org_model::Document::new(org_syntax::parse(text));
            // Emacs deletes one character.
            let before = text[..point].chars().next_back().map_or(0, char::len_utf8);
            let after = text[point..].chars().next().map_or(0, char::len_utf8);
            match args[0].as_str().unwrap() {
                "insert" => self_insert(
                    &doc,
                    point,
                    args[1].as_str().unwrap(),
                    args[2].as_bool().unwrap(),
                ),
                "backspace" if point == 0 => Err(org_edit::EditError {
                    message: "Beginning of buffer".into(),
                    point: None,
                }),
                "backspace" => Ok(delete_backward(&doc, point, before)),
                _ if point == text.len() => Err(org_edit::EditError {
                    message: "End of buffer".into(),
                    point: None,
                }),
                _ => Ok(delete_forward(&doc, point, after)),
            }
        }
        "insert" => {
            use org_edit::insert::*;
            let doc = org_model::Document::new(org_syntax::parse(text));
            match args[0].as_str().unwrap() {
                "link" => insert_link(
                    &doc,
                    point,
                    mark,
                    args[1].as_str().unwrap(),
                    args[2].as_str(),
                ),
                "block" => insert_structure_template(&doc, point, mark, args[1].as_str().unwrap()),
                _ => Ok(insert_timestamp(
                    &doc,
                    point,
                    jiff::civil::date(2026, 10, 5).at(14, 30, 0, 0),
                    args[1].as_bool().unwrap(),
                    args[2].as_bool().unwrap(),
                )),
            }
        }
        "emphasize" => {
            let doc = org_model::Document::new(org_syntax::parse(text));
            let c = args[0].as_str().unwrap().chars().next().unwrap();
            org_edit::emphasis::emphasize(&doc, point, mark, (c != ' ').then_some(c))
        }
        "list" => {
            use org_edit::list::*;
            let doc = org_model::Document::new(org_syntax::parse(text));
            match args[0].as_str().unwrap() {
                "indent" => indent_item(
                    &doc,
                    point,
                    mark,
                    args[1].as_bool().unwrap(),
                    args[2].as_bool().unwrap(),
                ),
                "bullet" => {
                    let which = match args[1].as_str().unwrap() {
                        "next" => BulletChoice::Next,
                        "previous" => BulletChoice::Previous,
                        b => BulletChoice::Bullet(b.to_string()),
                    };
                    cycle_bullet(&doc, point, which)
                }
                "checkbox" => {
                    let action = match args[1].as_str().unwrap() {
                        "toggle" => CheckboxAction::Toggle,
                        "presence" => CheckboxAction::Presence,
                        _ => CheckboxAction::Partial,
                    };
                    toggle_checkbox(&doc, point, mark, action)
                }
                "move" => move_item(&doc, point, args[1].as_bool().unwrap()),
                "repair" => repair(&doc, point),
                _ => Ok(
                    insert_item(&doc, point, args[1].as_bool().unwrap()).unwrap_or_else(|| {
                        Transaction::new("none").select(org_edit::Selection::caret(point))
                    }),
                ),
            }
        }
        "tags" => {
            use org_edit::tags::*;
            let doc = org_model::Document::new(org_syntax::parse(text));
            let s = |v: &Value| v.as_str().unwrap().to_string();
            match args[0].as_str().unwrap() {
                "set" => {
                    let tags: Vec<String> = args[1].as_array().unwrap().iter().map(s).collect();
                    set_tags(&doc, point, &tags)
                }
                "toggle" => {
                    toggle_tag(&doc, point, &s(&args[1]), args[2].as_bool()).map(|(t, _)| t)
                }
                "region" => Ok(change_tag_in_region(
                    &doc,
                    point,
                    arg(1),
                    arg(2),
                    &s(&args[3]),
                    args[4].as_bool().unwrap(),
                )),
                _ => Ok(align_all_tags(&doc, point)),
            }
        }
        "entry-put" => {
            let doc = org_model::Document::new(org_syntax::parse(text));
            let adapt = args.get(2).and_then(Value::as_bool).unwrap_or(false);
            org_edit::property::set_property(
                &doc,
                point,
                args[0].as_str().unwrap(),
                args[1].as_str().unwrap(),
                adapt,
            )
        }
        _ => return None,
    })
}

/// The clock `tests/emacs/edit.el` fixes.
fn now() -> jiff::civil::DateTime {
    jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0)
}

/// `org-todo` and `org-priority` cases; a note in the case is typed into
/// the pending log entry.
fn todo_case(spec: &Value, text: &str, point: usize) -> Result<Transaction, EditError> {
    use org_edit::todo::*;
    let doc = org_model::Document::new(org_syntax::parse(text));
    let mut settings = TodoSettings::default().for_document(&doc);
    settings.adapt_indentation = spec["adapt"].as_bool().unwrap_or(false);
    settings.enforce_todo_dependencies = spec["enforce"].as_bool().unwrap_or(false);
    settings.enforce_todo_checkbox_dependencies =
        spec["enforce_checkbox"].as_bool().unwrap_or(false);
    for t in spec["triggers"].as_array().into_iter().flatten() {
        let trigger = match t[0].as_str().unwrap() {
            "" => TagTrigger::NoKeyword,
            "todo" => TagTrigger::Todo,
            "done" => TagTrigger::Done,
            k => TagTrigger::Keyword(k.to_string()),
        };
        let changes = t[1]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c[0].as_str().unwrap().to_string(), c[1].as_bool().unwrap()))
            .collect();
        settings.todo_state_tags_triggers.push((trigger, changes));
    }
    if let Some(p) = spec["priority"].as_str() {
        let action = match p {
            "up" => PriorityAction::Up,
            "down" => PriorityAction::Down,
            "remove" => PriorityAction::Remove,
            s => PriorityAction::Set(s.trim_start_matches("set:").chars().next().unwrap()),
        };
        return priority(&doc, point, action, false, &settings);
    }
    let t = spec["todo"].as_str().unwrap();
    let arg = match t {
        "cycle" => TodoArg::Cycle,
        "right" => TodoArg::Next,
        "left" => TodoArg::Previous,
        "done" => TodoArg::Done,
        "none" => TodoArg::None,
        "nextset" => TodoArg::NextSet,
        "previousset" => TodoArg::PreviousSet,
        s if s.starts_with("state:") => TodoArg::State(s[6..].to_string()),
        s => TodoArg::Nth(s.trim_start_matches("nth:").parse().unwrap()),
    };
    let opts = TodoOptions {
        arg,
        settings: &settings,
        now: now(),
        remembered_head: None,
        repeated: false,
        force_note: spec["force_note"].as_bool().unwrap_or(false),
        inhibit_note: spec["inhibit_note"].as_bool().unwrap_or(false),
    };
    let outcome = match todo(&doc, point, &opts) {
        // Called from Lisp, Emacs fails silently where it blocks a change.
        Err(e) if e.message.contains("blocked (by") => {
            return Ok(Transaction::new("blocked").select(org_edit::Selection::caret(point)));
        }
        r => r?,
    };
    let Some(pending) = outcome.note else {
        return Ok(outcome.transaction);
    };
    let Some(content) = spec["note_text"].as_str() else {
        return Ok(outcome.transaction);
    };
    // Apply the note on the text after the state change, and return one
    // transaction from the original text.
    let mid = outcome.transaction.apply(text);
    let mid_point = outcome
        .transaction
        .selection_after
        .map_or(point, |s| s.head);
    let doc2 = org_model::Document::new(org_syntax::parse(&mid));
    let t2 = store_log_note(&doc2, mid_point, &pending, content, &settings);
    let after = t2.apply(&mid);
    let mut whole = Transaction::new("todo with note");
    whole.replace(0..text.len(), &after).unwrap();
    Ok(whole.select(t2.selection_after.unwrap()))
}

#[test]
fn commands_match_emacs() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/edit");
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(root.join("cases.json")).unwrap()).unwrap();
    let expected: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(root.join("expected.json")).unwrap())
            .unwrap();
    assert_eq!(
        cases.len(),
        expected.len(),
        "regenerate with tools/edit-expected.sh"
    );
    let (mut ok, mut missing) = (0, 0);
    let mut known = Vec::new();
    let mut failures = Vec::new();
    for (c, e) in cases.iter().zip(&expected) {
        let text = c["text"].as_str().unwrap();
        let point = c["point"].as_u64().unwrap() as usize;
        let cmd = c["cmd"].as_str().unwrap_or("");
        let mut args = c["args"].as_array().cloned().unwrap_or_default();
        if let (Some(a), Some(n)) = (
            args.first_mut().and_then(Value::as_object_mut),
            c["note"].as_str(),
        ) {
            a.insert("note_text".into(), Value::String(n.to_string()));
        }
        let mark = c["mark"].as_u64().map(|m| m as usize);
        if let Some(reason) = c["known"].as_str() {
            // A documented Emacs bug (`book/part-2/org-known-differences.org`).
            known.push(format!("{}: {reason}", c["name"].as_str().unwrap()));
            continue;
        }
        let Some(result) = run(cmd, &args, text, point, mark) else {
            missing += 1;
            continue;
        };
        let (got_text, got_point, got_err) = match result {
            Ok(t) => (
                t.apply(text),
                t.selection_after.map_or(point, |s| s.head),
                false,
            ),
            Err(e) => (text.to_string(), e.point.unwrap_or(point), true),
        };
        let want_text = e["text"].as_str().unwrap();
        let want_point = e["point"].as_u64().unwrap() as usize;
        let want_err = !e["error"].is_null();
        // A failing Emacs command may already have changed the text (a
        // final newline from `org-sort-entries`, a `COMMENT` removed by
        // `org-todo`); Kalem leaves the text unchanged when a command fails.
        let text_ok = got_text == want_text || (got_err && want_err);
        // When Emacs changed the text before failing, its point is in text
        // Kalem does not produce.
        let point_ok = got_point == want_point || (got_err && want_err && want_text != text);
        if text_ok && point_ok && got_err == want_err {
            ok += 1;
        } else {
            failures.push(format!(
                "{}\n  emacs: {:?} point {} error {:?}\n  kalem: {:?} point {} error {}",
                c["name"].as_str().unwrap(),
                want_text,
                want_point,
                e["error"],
                got_text,
                got_point,
                got_err
            ));
        }
    }
    eprintln!(
        "{ok} identical, {} different, {missing} without a Kalem command, {} known Emacs bugs",
        failures.len(),
        known.len()
    );
    for f in failures.iter().take(40) {
        eprintln!("{f}");
    }
    assert!(
        failures.is_empty(),
        "{} cases differ from Emacs",
        failures.len()
    );
}
