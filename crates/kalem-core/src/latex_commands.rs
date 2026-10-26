//! LaTeX mode's commands (§9.5, T2.7h): building the PDF (latexmk and the
//! engines, the build outputs kept out of git, the PDF at the cursor's
//! place), the problems of the last build, the environments, citations,
//! floats and insertions, and the export through pandoc. Registered with
//! the other built-in commands (`crate::builtin::commands`).

use serde_json::Value;

use crate::builtin::{file_url, import_file};
use crate::command::{
    Command, CommandError, CommandResult, EditorContext, Handler, Request, cmd, request, scoped,
};

/// LaTeX mode's commands: building the PDF and what goes with it, and the
/// editing commands of [`latex_commands`].
pub(crate) fn commands() -> Vec<Command> {
    let mut all = vec![
        scoped(
            cmd(
                "latex.build",
                "Build PDF",
                "LaTeX",
                &["f5"],
                None,
                |ctx, _| latex_build(ctx),
            ),
            crate::command::Scope::only(&["latex"]),
        ),
        scoped(
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
                    let message =
                        match crate::latex_build::ignore_build_outputs(&root, out.as_deref())
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
        ),
        scoped(
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
                    // No SyncTeX file, or the file not in it (the build left it
                    // out, as `\includeonly` does): said apart.
                    let place = match crate::synctex::Synctex::cached(&pdf) {
                        None => {
                            ctx.messages.push(crate::l10n::tr("msg-no-synctex"));
                            None
                        }
                        Some(st) => {
                            let place = st.forward(&path, line);
                            if place.is_none() {
                                ctx.messages.push(crate::l10n::tr("msg-synctex-not-built"));
                            }
                            place
                        }
                    };
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
        ),
    ];
    all.extend(latex_commands());
    all
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
                        detail: String::new(),
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
                detail: String::new(),
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
