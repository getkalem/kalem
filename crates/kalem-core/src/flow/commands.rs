//! The commands of a document of flowing text (plugin API 0.2.7): the
//! paragraphs' alignment, lists, indentation and spacing, the marks and
//! fonts of a selection, styles, and the comments and tracked changes of
//! its annotations, each an edit the plugin makes. Registered with the
//! other built-in commands (`crate::builtin::commands`).

use crate::command::{Command, CommandError, CommandResult, EditorContext, Handler, cmd, scoped};

/// The range a comment goes on: the selection, else the word at the
/// cursor (as Word comments on it).
fn comment_range(d: &crate::DocumentState) -> Option<std::ops::Range<usize>> {
    let s = d.selection;
    if s.anchor != s.head {
        return Some(s.anchor.min(s.head)..s.anchor.max(s.head));
    }
    let text = d.text().as_str();
    let at = s.head.min(text.len());
    let is_word = |c: char| c.is_alphanumeric() || c == '\'' || c == '’';
    let start = text[..at]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map_or(at, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .find(|(_, c)| !is_word(*c))
        .map_or(text.len(), |(i, _)| at + i);
    (start < end).then_some(start..end)
}

/// The annotation of `kind` (a comment when `comment`, else a tracked
/// change) at the cursor of a document of flowing text.
fn flow_annotation_at(ctx: &mut EditorContext<'_>, comment: bool) -> Result<String, CommandError> {
    let d = ctx.doc()?;
    let at = d.selection.head;
    let f = d
        .flow()
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-flow")))?;
    f.annotations_at(at)
        .into_iter()
        .find(|a| (a.kind == kalem_viewer::AnnotationKind::Comment) == comment)
        .map(|a| a.id.clone())
        .ok_or_else(|| {
            CommandError::new(crate::l10n::tr(if comment {
                "msg-no-comment-here"
            } else {
                "msg-no-change-here"
            }))
        })
}

/// Moves the cursor to the next (or previous) comment or tracked change.
fn flow_step(ctx: &mut EditorContext<'_>, comment: bool, back: bool) -> CommandResult {
    let d = ctx.doc()?;
    let at = d.selection.head;
    let f = d
        .flow()
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-flow")))?;
    let list = if comment { f.comments() } else { f.changes() };
    let mut starts: Vec<usize> = list.iter().filter_map(|a| f.start_of(&a.id)).collect();
    starts.sort();
    starts.dedup();
    let next = if back {
        starts.iter().rev().find(|s| **s < at).copied()
    } else {
        starts.iter().find(|s| **s > at).copied()
    };
    let to = next.ok_or_else(|| {
        CommandError::new(crate::l10n::tr(if comment {
            "msg-no-more-comments"
        } else {
            "msg-no-more-tracked-changes"
        }))
    })?;
    d.move_cursor(to, false);
    Ok(())
}

/// Runs an annotation's function on a document of flowing text.
fn on_flow<R>(
    ctx: &mut EditorContext<'_>,
    f: impl FnOnce(&mut crate::flow::FlowState) -> Result<R, String>,
) -> Result<R, CommandError> {
    ctx.doc()?.on_flow(f).map_err(CommandError::new)
}

/// The commands of documents of flowing text (plugin API 0.2.7): their
/// comments and tracked changes, as a review has them.
pub(crate) fn flow_commands() -> Vec<Command> {
    use crate::command::Scope;
    let flow = || Scope::only(&["flow"]);
    vec![
        scoped(
            cmd(
                "flow.comment.new",
                "New Comment",
                "Review",
                &["ctrl+alt+m"],
                None,
                |ctx, args| {
                    let text = args["text"].as_str().unwrap_or_default().to_string();
                    let range = comment_range(ctx.doc()?).ok_or_else(|| {
                        CommandError::new(crate::l10n::tr("msg-select-to-comment"))
                    })?;
                    on_flow(ctx, |f| f.comment(range, &text)).map(|_| ())
                },
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.comment.reply",
                "Reply to Comment",
                "Review",
                &[],
                None,
                |ctx, args| {
                    let text = args["text"].as_str().unwrap_or_default().to_string();
                    let id = flow_annotation_at(ctx, true)?;
                    on_flow(ctx, |f| f.reply(&id, &text)).map(|_| ())
                },
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.comment.edit",
                "Edit Comment",
                "Review",
                &[],
                None,
                |ctx, args| {
                    let text = args["text"].as_str().unwrap_or_default().to_string();
                    let id = flow_annotation_at(ctx, true)?;
                    on_flow(ctx, |f| f.set_comment_text(&id, &text))
                },
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.comment.resolve",
                "Resolve Comment",
                "Review",
                &[],
                None,
                |ctx, _| {
                    let id = flow_annotation_at(ctx, true)?;
                    let done = ctx
                        .doc()?
                        .flow()
                        .and_then(|f| f.annotations.get(&id))
                        .is_some_and(|a| a.resolved);
                    on_flow(ctx, |f| f.resolve(&id, !done))
                },
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.comment.delete",
                "Delete Comment",
                "Review",
                &[],
                None,
                |ctx, _| {
                    let id = flow_annotation_at(ctx, true)?;
                    on_flow(ctx, |f| f.remove_comment(&id))
                },
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.comment.next",
                "Next Comment",
                "Review",
                &[],
                None,
                |ctx, _| flow_step(ctx, true, false),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.comment.previous",
                "Previous Comment",
                "Review",
                &[],
                None,
                |ctx, _| flow_step(ctx, true, true),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.change.accept",
                "Accept Change",
                "Review",
                &[],
                None,
                |ctx, _| {
                    let id = flow_annotation_at(ctx, false)?;
                    on_flow(ctx, |f| f.decide(&id, true))
                },
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.change.reject",
                "Reject Change",
                "Review",
                &[],
                None,
                |ctx, _| {
                    let id = flow_annotation_at(ctx, false)?;
                    on_flow(ctx, |f| f.decide(&id, false))
                },
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.change.acceptAll",
                "Accept All Changes",
                "Review",
                &[],
                None,
                |ctx, _| on_flow(ctx, |f| f.decide_all(true)),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.change.rejectAll",
                "Reject All Changes",
                "Review",
                &[],
                None,
                |ctx, _| on_flow(ctx, |f| f.decide_all(false)),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.change.next",
                "Next Change",
                "Review",
                &[],
                None,
                |ctx, _| flow_step(ctx, false, false),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.change.previous",
                "Previous Change",
                "Review",
                &[],
                None,
                |ctx, _| flow_step(ctx, false, true),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.trackChanges",
                "Track Changes",
                "Review",
                &[],
                None,
                |ctx, _| {
                    let on = ctx.doc()?.flow().and_then(|f| f.tracking);
                    let Some(on) = on else {
                        return Err(CommandError::new(crate::l10n::tr("msg-no-tracking")));
                    };
                    on_flow(ctx, |f| f.set_tracking(!on))?;
                    ctx.messages.push(crate::l10n::tr(if on {
                        "msg-tracking-off"
                    } else {
                        "msg-tracking-on"
                    }));
                    Ok(())
                },
            ),
            flow(),
        ),
        // Formatting: the look of the selected text (the word at the
        // cursor when none is), and the paragraphs' style.
        flow_toggle_cmd("flow.format.bold", "Bold", &["ctrl+b"], |ctx, _| {
            flow_toggle(ctx, FlowToggle::Bold)
        }),
        flow_toggle_cmd("flow.format.italic", "Italic", &["ctrl+i"], |ctx, _| {
            flow_toggle(ctx, FlowToggle::Italic)
        }),
        flow_toggle_cmd(
            "flow.format.underline",
            "Underline",
            &["ctrl+u"],
            |ctx, _| flow_toggle(ctx, FlowToggle::Underline),
        ),
        flow_toggle_cmd(
            "flow.format.strikeThrough",
            "Strike Through",
            &[],
            |ctx, _| flow_toggle(ctx, FlowToggle::Strike),
        ),
        flow_toggle_cmd("flow.format.superscript", "Superscript", &[], |ctx, _| {
            flow_toggle(ctx, FlowToggle::Superscript)
        }),
        flow_toggle_cmd("flow.format.subscript", "Subscript", &[], |ctx, _| {
            flow_toggle(ctx, FlowToggle::Subscript)
        }),
        scoped(
            cmd(
                "flow.format.font",
                "Font…",
                "Format",
                &[],
                None,
                |ctx, args| flow_font(ctx, args, false),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.format.fontSize",
                "Font Size…",
                "Format",
                &[],
                None,
                |ctx, args| flow_font(ctx, args, true),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.format.color",
                "Text Color…",
                "Format",
                &[],
                None,
                |ctx, args| flow_color(ctx, args, false),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.format.highlight",
                "Highlight Color…",
                "Format",
                &[],
                None,
                |ctx, args| flow_color(ctx, args, true),
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.format.style",
                "Paragraph Style…",
                "Format",
                &[],
                None,
                flow_style,
            ),
            flow(),
        ),
        scoped(
            cmd(
                "flow.format.clear",
                "Clear Formatting",
                "Format",
                &["ctrl+space"],
                Some("editorMode == flow"),
                |ctx, _| flow_marks(ctx, vec![kalem_viewer::MarkChange::Clear]),
            ),
            flow(),
        ),
        // Paragraphs and lists (API 0.2.8, `flow-2`): the selection's
        // paragraphs, or the cursor's.
        flow_key_cmd(
            "flow.paragraph.alignLeft",
            "Align Left",
            &["ctrl+l"],
            |ctx, _| flow_align(ctx, kalem_viewer::FlowAlign::Start),
        ),
        flow_key_cmd(
            "flow.paragraph.alignCenter",
            "Center",
            &["ctrl+e"],
            |ctx, _| flow_align(ctx, kalem_viewer::FlowAlign::Center),
        ),
        flow_key_cmd(
            "flow.paragraph.alignRight",
            "Align Right",
            &["ctrl+r"],
            |ctx, _| flow_align(ctx, kalem_viewer::FlowAlign::End),
        ),
        flow_key_cmd(
            "flow.paragraph.justify",
            "Justify",
            &["ctrl+j"],
            |ctx, _| flow_align(ctx, kalem_viewer::FlowAlign::Justify),
        ),
        flow_key_cmd(
            "flow.list.bullets",
            "Bullets",
            &["ctrl+shift+l"],
            |ctx, _| flow_list(ctx, kalem_viewer::ListKind::Bullet, true),
        ),
        flow_key_cmd(
            "flow.list.numbering",
            "Numbering",
            &[],
            |ctx, args| match args.get("format").and_then(|f| f.as_str()) {
                Some(f) => flow_list(ctx, kalem_viewer::ListKind::Numbered(f.to_string()), false),
                None => flow_list(
                    ctx,
                    kalem_viewer::ListKind::Numbered("decimal".into()),
                    true,
                ),
            },
        ),
        flow_key_cmd(
            "flow.list.numberingStyle",
            "Numbering Style…",
            &[],
            flow_numbering_style,
        ),
        flow_key_cmd(
            "flow.paragraph.increaseIndent",
            "Increase Indent",
            &[],
            |ctx, _| flow_indent(ctx, true),
        ),
        flow_key_cmd(
            "flow.paragraph.decreaseIndent",
            "Decrease Indent",
            &[],
            |ctx, _| flow_indent(ctx, false),
        ),
        flow_key_cmd(
            "flow.paragraph.lineSpacing",
            "Line Spacing…",
            &[],
            |ctx, args| flow_spacing(ctx, args, FlowSpacing::Lines),
        ),
        flow_key_cmd(
            "flow.paragraph.spaceBefore",
            "Space Before…",
            &[],
            |ctx, args| flow_spacing(ctx, args, FlowSpacing::Before),
        ),
        flow_key_cmd(
            "flow.paragraph.spaceAfter",
            "Space After…",
            &[],
            |ctx, args| flow_spacing(ctx, args, FlowSpacing::After),
        ),
        flow_key_cmd(
            "flow.paragraph.clear",
            "Clear Paragraph Formatting",
            &[],
            |ctx, _| flow_paragraphs(ctx, vec![kalem_viewer::ParagraphChange::Clear]),
        ),
    ]
}

/// A command of documents of flowing text in the Format category, its
/// keys only there.
fn flow_key_cmd(id: &str, title: &str, keys: &[&str], handler: Handler) -> Command {
    flow_toggle_cmd(id, title, keys, handler)
}

/// The text a paragraph command acts on: the selection, or the cursor's
/// paragraph.
fn flow_paragraph_range(
    ctx: &mut EditorContext<'_>,
) -> Result<std::ops::Range<usize>, CommandError> {
    let d = ctx.doc()?;
    if d.flow().is_none() {
        return Err(CommandError::new(crate::l10n::tr("msg-not-flow")));
    }
    let s = d.selection;
    Ok(s.anchor.min(s.head)..s.anchor.max(s.head))
}

/// `changes` made on the paragraphs a paragraph command acts on.
fn flow_paragraphs(
    ctx: &mut EditorContext<'_>,
    changes: Vec<kalem_viewer::ParagraphChange>,
) -> CommandResult {
    let range = flow_paragraph_range(ctx)?;
    on_flow(ctx, |f| f.set_paragraphs(range, &changes))
}

fn flow_align(ctx: &mut EditorContext<'_>, a: kalem_viewer::FlowAlign) -> CommandResult {
    flow_paragraphs(ctx, vec![kalem_viewer::ParagraphChange::Align(a)])
}

/// Bullets and Numbering: the paragraphs made items of a list of `kind`;
/// with `toggle`, taken out of their list when all are items of one of
/// that kind already.
fn flow_list(
    ctx: &mut EditorContext<'_>,
    kind: kalem_viewer::ListKind,
    toggle: bool,
) -> CommandResult {
    use kalem_viewer::{ListKind, ParagraphChange};
    let range = flow_paragraph_range(ctx)?;
    let numbered = matches!(kind, ListKind::Numbered(_));
    let all = {
        let f = ctx
            .doc()?
            .flow()
            .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-flow")))?;
        let spans = f.spans(range.clone());
        !spans.is_empty()
            && spans
                .iter()
                .all(|(p, _)| crate::flow::list_kind(p) == Some(numbered))
    };
    let change = if toggle && all {
        ParagraphChange::List(None)
    } else {
        ParagraphChange::List(Some(kind))
    };
    on_flow(ctx, |f| f.set_paragraphs(range, &[change]))
}

/// Numbering Style: the numbering styles every format has, to pick.
fn flow_numbering_style(ctx: &mut EditorContext<'_>, _: &serde_json::Value) -> CommandResult {
    flow_paragraph_range(ctx)?;
    let category = crate::l10n::tr("cmd-flow-list-numberingStyle");
    let items = [
        ("decimal", "1. 2. 3."),
        ("lower-alpha", "a. b. c."),
        ("upper-alpha", "A. B. C."),
        ("lower-roman", "i. ii. iii."),
        ("upper-roman", "I. II. III."),
    ]
    .iter()
    .map(|(format, title)| {
        flow_item(
            "flow.list.numbering",
            serde_json::json!({ "format": format }),
            title,
            &category,
        )
    })
    .collect();
    ctx.requests.push(crate::command::Request::Choose(items));
    Ok(())
}

/// Increase and Decrease Indent, as a word processor's: a list item a
/// level deeper or shallower, another paragraph's indent a step (half an
/// inch) further or back.
fn flow_indent(ctx: &mut EditorContext<'_>, deeper: bool) -> CommandResult {
    use kalem_viewer::{FlowRole, ParagraphChange};
    const STEP: f32 = 36.0;
    let range = flow_paragraph_range(ctx)?;
    on_flow(ctx, |f| {
        f.set_each_paragraph(range, |p| {
            if p.role == FlowRole::ListItem {
                let level = p.level.saturating_sub(1);
                match (deeper, level) {
                    (true, l) if l < 8 => vec![ParagraphChange::ListLevel(l + 1)],
                    (false, l) if l > 0 => vec![ParagraphChange::ListLevel(l - 1)],
                    _ => Vec::new(),
                }
            } else {
                let start = p.indent.0;
                let new = if deeper {
                    ((start / STEP).floor() + 1.0) * STEP
                } else {
                    ((start / STEP).ceil() - 1.0).max(0.0) * STEP
                };
                if (new - start).abs() < 0.01 {
                    Vec::new()
                } else {
                    vec![ParagraphChange::IndentStart(new)]
                }
            }
        })
    })
}

/// Which spacing a spacing command gives.
#[derive(Debug, Clone, Copy)]
enum FlowSpacing {
    Lines,
    Before,
    After,
}

/// Line Spacing, Space Before and Space After: the value given, else a
/// list of the common ones and one typed.
fn flow_spacing(
    ctx: &mut EditorContext<'_>,
    args: &serde_json::Value,
    which: FlowSpacing,
) -> CommandResult {
    use kalem_viewer::{LineSpacing, ParagraphChange};
    let id = match which {
        FlowSpacing::Lines => "flow.paragraph.lineSpacing",
        FlowSpacing::Before => "flow.paragraph.spaceBefore",
        FlowSpacing::After => "flow.paragraph.spaceAfter",
    };
    let given = args.get("value").and_then(|x| {
        x.as_str()
            .map(str::to_string)
            .or_else(|| x.as_f64().map(|n| n.to_string()))
    });
    match given.as_deref() {
        Some("custom") => {
            ctx.requests.push(crate::command::Request::Ask {
                command: id.into(),
                args: serde_json::json!({}),
                arg: "value".into(),
            });
            Ok(())
        }
        Some(v) => {
            let t = v.replace(',', ".");
            let n = t.trim().trim_end_matches("pt").trim().parse::<f32>().ok();
            let change = match (which, n) {
                (FlowSpacing::Lines, Some(n)) if (0.25..=10.0).contains(&n) => {
                    ParagraphChange::LineSpacing(LineSpacing::Multiple(n))
                }
                (FlowSpacing::Before, Some(n)) if (0.0..=1584.0).contains(&n) => {
                    ParagraphChange::SpaceBefore(n)
                }
                (FlowSpacing::After, Some(n)) if (0.0..=1584.0).contains(&n) => {
                    ParagraphChange::SpaceAfter(n)
                }
                _ => {
                    return Err(CommandError::new(crate::l10n::tr_args(
                        "msg-not-spacing",
                        &[("value", crate::l10n::Arg::Str(v.to_string()))],
                    )));
                }
            };
            flow_paragraphs(ctx, vec![change])
        }
        None => {
            flow_paragraph_range(ctx)?;
            let category = crate::l10n::tr(match which {
                FlowSpacing::Lines => "cmd-flow-paragraph-lineSpacing",
                FlowSpacing::Before => "cmd-flow-paragraph-spaceBefore",
                FlowSpacing::After => "cmd-flow-paragraph-spaceAfter",
            });
            let values: &[&str] = match which {
                FlowSpacing::Lines => &["1.0", "1.15", "1.5", "2.0", "2.5", "3.0"],
                _ => &["0", "3", "6", "12", "18", "24"],
            };
            let mut items: Vec<_> = values
                .iter()
                .map(|v| {
                    let title = match which {
                        FlowSpacing::Lines => v.to_string(),
                        _ => format!("{v} pt"),
                    };
                    flow_item(id, serde_json::json!({ "value": v }), &title, &category)
                })
                .collect();
            items.push(flow_item(
                id,
                serde_json::json!({ "value": "custom" }),
                &crate::l10n::tr("flow-custom"),
                &category,
            ));
            ctx.requests.push(crate::command::Request::Choose(items));
            Ok(())
        }
    }
}

/// A mark a formatting command turns on, or off where the whole
/// selection has it.
#[derive(Debug, Clone, Copy)]
enum FlowToggle {
    Bold,
    Italic,
    Underline,
    Strike,
    Superscript,
    Subscript,
}

fn flow_toggle_cmd(id: &str, title: &str, keys: &[&str], handler: Handler) -> Command {
    let mut c = cmd(
        id,
        title,
        "Format",
        keys,
        (!keys.is_empty()).then_some("editorMode == flow"),
        handler,
    );
    c.scope = Some(crate::command::Scope::only(&["flow"]));
    c
}

/// The text a formatting command acts on: the selection, else the word
/// at the cursor.
fn flow_format_range(ctx: &mut EditorContext<'_>) -> Result<std::ops::Range<usize>, CommandError> {
    let d = ctx.doc()?;
    if d.flow().is_none() {
        return Err(CommandError::new(crate::l10n::tr("msg-not-flow")));
    }
    comment_range(d).ok_or_else(|| CommandError::new(crate::l10n::tr("msg-select-to-format")))
}

/// `changes` made on the text a formatting command acts on.
fn flow_marks(
    ctx: &mut EditorContext<'_>,
    changes: Vec<kalem_viewer::MarkChange>,
) -> CommandResult {
    let range = flow_format_range(ctx)?;
    on_flow(ctx, |f| f.set_marks(range, &changes))
}

fn flow_toggle(ctx: &mut EditorContext<'_>, t: FlowToggle) -> CommandResult {
    use kalem_viewer::{MarkChange, Script};
    let range = flow_format_range(ctx)?;
    let all = {
        let f = ctx
            .doc()?
            .flow()
            .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-flow")))?;
        let runs = f.runs_in(range.clone());
        !runs.is_empty()
            && runs.iter().all(|r| {
                let m = &r.marks;
                match t {
                    FlowToggle::Bold => m.bold,
                    FlowToggle::Italic => m.italic,
                    FlowToggle::Underline => m.underline.is_some(),
                    FlowToggle::Strike => m.strike || m.double_strike,
                    FlowToggle::Superscript => m.script == Script::Superscript,
                    FlowToggle::Subscript => m.script == Script::Subscript,
                }
            })
    };
    let change = match t {
        FlowToggle::Bold => MarkChange::Bold(!all),
        FlowToggle::Italic => MarkChange::Italic(!all),
        FlowToggle::Underline => MarkChange::Underline((!all).then(|| "single".to_string())),
        FlowToggle::Strike => MarkChange::Strike(!all),
        FlowToggle::Superscript if all => MarkChange::Script(Script::Baseline),
        FlowToggle::Superscript => MarkChange::Script(Script::Superscript),
        FlowToggle::Subscript if all => MarkChange::Script(Script::Baseline),
        FlowToggle::Subscript => MarkChange::Script(Script::Subscript),
    };
    on_flow(ctx, |f| f.set_marks(range, &[change]))
}

/// A palette line running `id` with `args`.
fn flow_item(
    id: &str,
    args: serde_json::Value,
    title: &str,
    category: &str,
) -> crate::palette::PaletteItem {
    crate::palette::PaletteItem {
        id: crate::palette::invocation(id, &args),
        title: title.into(),
        category: category.into(),
        detail: String::new(),
        keys: String::new(),
        also: title.into(),
    }
}

/// Font and Font Size: the value given, else a list to pick from (the
/// document's typefaces and the common ones; the common sizes), with the
/// style's own and one typed.
fn flow_font(ctx: &mut EditorContext<'_>, args: &serde_json::Value, size: bool) -> CommandResult {
    use kalem_viewer::MarkChange;
    let id = if size {
        "flow.format.fontSize"
    } else {
        "flow.format.font"
    };
    let category = crate::l10n::tr(if size {
        "cmd-flow-format-fontSize"
    } else {
        "cmd-flow-format-font"
    });
    let given = args.get("value").and_then(|x| {
        x.as_str()
            .map(str::to_string)
            .or_else(|| x.as_f64().map(|n| n.to_string()))
    });
    match given.as_deref() {
        Some("custom") => {
            ctx.requests.push(crate::command::Request::Ask {
                command: id.into(),
                args: serde_json::json!({}),
                arg: "value".into(),
            });
            Ok(())
        }
        Some("default") => {
            let change = if size {
                MarkChange::Size(None)
            } else {
                MarkChange::Face(None)
            };
            flow_marks(ctx, vec![change])
        }
        Some(v) if size => {
            let t = v.replace(',', ".");
            match t.trim().trim_end_matches("pt").trim().parse::<f32>() {
                Ok(n) if (1.0..=1638.0).contains(&n) => {
                    flow_marks(ctx, vec![MarkChange::Size(Some(n))])
                }
                _ => Err(CommandError::new(crate::l10n::tr_args(
                    "msg-not-font-size",
                    &[("value", crate::l10n::Arg::Str(v.to_string()))],
                ))),
            }
        }
        Some(v) if !v.trim().is_empty() => {
            flow_marks(ctx, vec![MarkChange::Face(Some(v.trim().to_string()))])
        }
        Some(_) => Ok(()),
        None => {
            flow_format_range(ctx)?;
            let mut items = vec![flow_item(
                id,
                serde_json::json!({ "value": "default" }),
                &crate::l10n::tr("flow-style-default"),
                &category,
            )];
            if size {
                for n in [
                    8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 36.0,
                    48.0, 72.0_f32,
                ] {
                    let label = format!("{n}");
                    items.push(flow_item(
                        id,
                        serde_json::json!({ "value": label }),
                        &format!("{label} pt"),
                        &category,
                    ));
                }
            } else {
                let mut faces: Vec<String> = Vec::new();
                if let Some(f) = ctx.doc()?.flow() {
                    for line in &f.lines {
                        for seg in &line.segs {
                            for r in &seg.para.runs {
                                if let Some(face) = &r.marks.face
                                    && !faces.contains(face)
                                {
                                    faces.push(face.clone());
                                }
                            }
                        }
                    }
                }
                faces.sort();
                for common in [
                    "Aptos",
                    "Calibri",
                    "Cambria",
                    "Arial",
                    "Times New Roman",
                    "Georgia",
                    "Garamond",
                    "Verdana",
                    "Tahoma",
                    "Segoe UI",
                    "Helvetica",
                    "Courier New",
                    "Consolas",
                ] {
                    if !faces.iter().any(|f| f == common) {
                        faces.push(common.to_string());
                    }
                }
                for f in faces {
                    items.push(flow_item(
                        id,
                        serde_json::json!({ "value": f }),
                        &f,
                        &category,
                    ));
                }
            }
            items.push(flow_item(
                id,
                serde_json::json!({ "value": "custom" }),
                &crate::l10n::tr("flow-custom"),
                &category,
            ));
            ctx.requests.push(crate::command::Request::Choose(items));
            Ok(())
        }
    }
}

fn flow_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    Some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

/// Text Color and Highlight Color: Word's colors, its automatic color or
/// no highlight, and a color typed as #RRGGBB.
fn flow_color(
    ctx: &mut EditorContext<'_>,
    args: &serde_json::Value,
    highlight: bool,
) -> CommandResult {
    use kalem_viewer::MarkChange;
    let id = if highlight {
        "flow.format.highlight"
    } else {
        "flow.format.color"
    };
    let category = crate::l10n::tr(if highlight {
        "cmd-flow-format-highlight"
    } else {
        "cmd-flow-format-color"
    });
    let wrap = |c: Option<[u8; 3]>| {
        if highlight {
            MarkChange::Highlight(c)
        } else {
            MarkChange::Color(c)
        }
    };
    match args.get("color").and_then(|c| c.as_str()) {
        Some("auto" | "none") => flow_marks(ctx, vec![wrap(None)]),
        Some("custom") => {
            ctx.requests.push(crate::command::Request::Ask {
                command: id.into(),
                args: serde_json::json!({}),
                arg: "color".into(),
            });
            Ok(())
        }
        Some(c) => match flow_hex(c) {
            Some(rgb) => flow_marks(ctx, vec![wrap(Some(rgb))]),
            None => Err(CommandError::new(crate::l10n::tr_args(
                "msg-not-color",
                &[("value", crate::l10n::Arg::Str(c.to_string()))],
            ))),
        },
        None => {
            flow_format_range(ctx)?;
            // Word's standard colors, and the highlights it names.
            let colors: &[(&str, &str)] = if highlight {
                &[
                    ("flow-color-yellow", "#FFFF00"),
                    ("flow-color-bright-green", "#00FF00"),
                    ("flow-color-turquoise", "#00FFFF"),
                    ("flow-color-pink", "#FF00FF"),
                    ("flow-color-blue", "#0000FF"),
                    ("flow-color-red", "#FF0000"),
                    ("flow-color-dark-blue", "#000080"),
                    ("flow-color-teal", "#008080"),
                    ("flow-color-green", "#008000"),
                    ("flow-color-violet", "#800080"),
                    ("flow-color-dark-red", "#800000"),
                    ("flow-color-dark-yellow", "#808000"),
                    ("flow-color-gray-50", "#808080"),
                    ("flow-color-gray-25", "#C0C0C0"),
                    ("flow-color-black", "#000000"),
                ]
            } else {
                &[
                    ("flow-color-dark-red", "#C00000"),
                    ("flow-color-red", "#FF0000"),
                    ("flow-color-orange", "#FFC000"),
                    ("flow-color-yellow", "#FFFF00"),
                    ("flow-color-light-green", "#92D050"),
                    ("flow-color-green", "#00B050"),
                    ("flow-color-light-blue", "#00B0F0"),
                    ("flow-color-blue", "#0070C0"),
                    ("flow-color-dark-blue", "#002060"),
                    ("flow-color-purple", "#7030A0"),
                ]
            };
            let mut items = vec![flow_item(
                id,
                serde_json::json!({ "color": if highlight { "none" } else { "auto" } }),
                &crate::l10n::tr(if highlight {
                    "flow-color-none"
                } else {
                    "flow-color-auto"
                }),
                &category,
            )];
            for (key, hex) in colors {
                items.push(flow_item(
                    id,
                    serde_json::json!({ "color": hex }),
                    &format!("{} {hex}", crate::l10n::tr(key)),
                    &category,
                ));
            }
            items.push(flow_item(
                id,
                serde_json::json!({ "color": "custom" }),
                &crate::l10n::tr("flow-custom"),
                &category,
            ));
            ctx.requests.push(crate::command::Request::Choose(items));
            Ok(())
        }
    }
}

/// Paragraph Style: the style given, else the document's to pick from.
fn flow_style(ctx: &mut EditorContext<'_>, args: &serde_json::Value) -> CommandResult {
    let d = ctx.doc()?;
    let s = d.selection;
    let range = s.anchor.min(s.head)..s.anchor.max(s.head);
    if let Some(style) = args.get("style").and_then(|v| v.as_str()) {
        let style = style.to_string();
        return on_flow(ctx, |f| f.set_style(range, &style));
    }
    let f = d
        .flow_mut()
        .ok_or_else(|| CommandError::new(crate::l10n::tr("msg-not-flow")))?;
    let category = crate::l10n::tr("cmd-flow-format-style");
    let items: Vec<_> = f
        .paragraph_styles()
        .into_iter()
        .map(|st| {
            flow_item(
                "flow.format.style",
                serde_json::json!({ "style": st.id }),
                &st.name,
                &category,
            )
        })
        .collect();
    if items.is_empty() {
        return Err(CommandError::new(crate::l10n::tr("msg-no-styles")));
    }
    ctx.requests.push(crate::command::Request::Choose(items));
    Ok(())
}
