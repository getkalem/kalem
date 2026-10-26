//! Markdown mode's commands (T2.7c): emphasis and the other marks, links,
//! headings, lists and task items, tables (moving, aligning, formulas),
//! front matter, and the conversion to and from Org. Registered with the
//! other built-in commands (`crate::builtin::commands`).

use serde_json::Value;

use crate::builtin::lines_command;
use crate::command::{
    Command, CommandError, CommandResult, EditorContext, Handler, Request, arg_bool, arg_str, cmd,
    request, scoped,
};

pub(crate) fn markdown_commands() -> Vec<Command> {
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
                |ctx, _| md_emphasis(ctx, crate::markdown::MdKind::Strong, "**", "**"),
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
                |ctx, _| md_emphasis(ctx, crate::markdown::MdKind::Emphasis, "*", "*"),
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
                |ctx, _| md_emphasis(ctx, crate::markdown::MdKind::Code, "`", "`"),
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
                |ctx, _| md_emphasis(ctx, crate::markdown::MdKind::Strikethrough, "~~", "~~"),
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
        // Enter of an Org table, a row down; no key, since Enter in a
        // Markdown table stays a line break for typing a row as text.
        md_table("markdown.table.nextRow", "Next Row", &[], |ctx, _| {
            md_table_run(ctx, crate::markdown_table::next_row)
        }),
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
                            detail: String::new(),
                            keys: String::new(),
                            also: String::new(),
                        })
                        .collect();
                    items.push(crate::palette::PaletteItem {
                        id: "markdown.frontMatter.set".into(),
                        title: crate::tr!("front-matter-add"),
                        category: category.clone(),
                        detail: String::new(),
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
                        detail: String::new(),
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
                |ctx, _| {
                    let now = ctx.now;
                    let d = ctx.doc()?;
                    let s = d.selection;
                    if s.anchor == s.head {
                        return md_table_run(ctx, crate::markdown::newline);
                    }
                    // Over a selection: the selection goes, then Enter as
                    // without one, in one step.
                    let (a, b) = (s.anchor.min(s.head), s.anchor.max(s.head));
                    let text = d.text().as_str();
                    let rest = format!("{}{}", &text[..a], &text[b..]);
                    let md = crate::markdown::parsed(d).reparse(text, &rest);
                    let tx = crate::markdown::newline(&md, &rest, a)
                        .and_then(|tx| {
                            let caret = tx.selection_after?;
                            crate::lines::replace_differing(text, &tx.apply(&rest), "New Line")
                                .map(|t| t.select(caret))
                        })
                        .unwrap_or_else(|| crate::input::newline(text, s.head, Some(s.anchor)));
                    d.apply(&tx, org_edit::ChangeKind::Command, now);
                    Ok(())
                },
            ),
            Scope::only(&["markdown"]),
        ),
    ]
}

/// Bold, italic, code or strike-through at the selection of a Markdown
/// document ([`crate::markdown::wrap`]).
fn md_emphasis(
    ctx: &mut EditorContext<'_>,
    kind: crate::markdown::MdKind,
    open: &str,
    close: &str,
) -> CommandResult {
    let md = crate::markdown::parsed(ctx.doc()?);
    lines_command(ctx, |t, s| {
        crate::markdown::wrap(&md, t, s, kind, open, close)
    })
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
