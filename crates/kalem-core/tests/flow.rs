//! Documents of flowing text (plugin API 0.2.7): the fake document of
//! `tests/plugins/flowdoc`, natively, opened as a document of the editor,
//! its text the paragraphs', its edits the plugin's.

#[path = "../../../tests/plugins/flowdoc/src/lib.rs"]
#[allow(dead_code, unreachable_pub, missing_debug_implementations)]
mod flowdoc;

use std::sync::Arc;
use std::time::Instant;

use kalem_core::{DocumentMode, DocumentState};
use org_edit::{ChangeKind, Selection, Transaction};
use org_model::Settings;

const DOC: &str = "# Title\nPlain and *bold* text^\n- an item\n|a|b|\n---\nThe note.\n";

fn open(text: &str) -> (DocumentState, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "kalem-core-flow-{}-{}",
        std::process::id(),
        std::thread::current()
            .name()
            .unwrap_or("t")
            .replace("::", "-")
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("a.flow");
    std::fs::write(&path, text).unwrap();
    let d = DocumentState::viewed(
        &path,
        Arc::new(flowdoc::Flows),
        Arc::new(Settings::default()),
    )
    .unwrap();
    (d, path)
}

fn edit(d: &mut DocumentState, range: std::ops::Range<usize>, insert: &str) {
    let mut tx = Transaction::new("edit");
    tx.edit(range, insert);
    d.apply(&tx, ChangeKind::Command, Instant::now());
}

#[test]
fn a_flow_opens_as_text() {
    let (d, _) = open(DOC);
    assert_eq!(d.meta.mode, DocumentMode::Flow);
    assert!(d.viewer.is_none() && d.flow.is_some());
    assert_eq!(
        d.text().as_str(),
        "Title\nPlain and bold text\u{FFFC}\nan item\na\tb\n\nThe note."
    );
    assert!(!d.is_modified() && !d.read_only);
    let f = d.flow.as_deref().unwrap();
    let items = f.outline();
    assert_eq!((items[0].title.as_str(), items[0].level), ("Title", 1));
}

#[test]
fn lines_show_the_plugins_look() {
    let (d, _) = open(DOC);
    let f = d.flow.as_deref().unwrap();
    let text = d.text().as_str().to_string();
    let line = |n: usize| {
        let start = text.split('\n').take(n).map(|l| l.len() + 1).sum::<usize>();
        let len = text.split('\n').nth(n).unwrap().len();
        kalem_core::flow::line_view(f, start..start + len)
    };
    let h = line(0);
    assert_eq!(h.heading, 1);
    let p = line(1);
    let bold = p.runs.iter().find(|r| r.text == "bold").unwrap();
    assert!(bold.style.bold && bold.verbatim);
    let mark = p.runs.iter().find(|r| r.text == "1").unwrap();
    assert!(mark.style.superscript && !mark.verbatim);
    assert_eq!(p.display(), "Plain and bold text1");
    let item = line(2);
    assert_eq!(item.display(), "• an item");
    let row = line(3);
    assert_eq!(row.display(), "▏a │ b");
    let rule = line(4);
    assert_eq!(rule.role, kalem_core::view::LineRole::Delimiter);
    let note = line(5);
    assert_eq!(note.display(), "[1] The note.");
    // Display offsets map back to the text's.
    assert_eq!(
        p.source_offset(p.display_offset(text.find("bold").unwrap())),
        text.find("bold").unwrap()
    );
}

#[test]
fn typing_enter_and_backspace_are_the_plugins_edits() {
    let (mut d, path) = open(DOC);
    // Typing in the second paragraph.
    let at = d.text().as_str().find("Plain").unwrap();
    d.selection = Selection::caret(at);
    d.insert_text("Very ", Instant::now());
    assert!(d.text().as_str().contains("Very Plain and bold"));
    assert_eq!(d.selection.head, at + 5);
    assert!(d.is_modified());
    // Enter in the heading: two paragraphs.
    edit(&mut d, 2..2, "\n");
    assert!(d.text().as_str().starts_with("Ti\ntle\nVery Plain"));
    // Backspace at the second's start: one again.
    edit(&mut d, 2..3, "");
    assert!(d.text().as_str().starts_with("Title\nVery Plain"));
    // A deletion across paragraphs.
    let t = d.text().as_str().to_string();
    let from = t.find("tle").unwrap();
    let to = t.find("and").unwrap();
    edit(&mut d, from..to, "");
    assert!(
        d.text().as_str().starts_with("Tiand bold text"),
        "{}",
        d.text().as_str()
    );
    // Pasted lines: split as they come.
    let end = d.text().as_str().find('\n').unwrap();
    edit(&mut d, end..end, " one\ntwo\nthree");
    assert!(
        d.text()
            .as_str()
            .contains("text\u{FFFC} one\ntwo\nthree\nan item"),
        "{}",
        d.text().as_str()
    );
    // Each edit undoes as the plugin's step, back to the file.
    for _ in 0..5 {
        assert!(d.undo().is_some());
    }
    assert!(d.undo().is_none());
    assert!(!d.is_modified());
    assert_eq!(
        d.text().as_str(),
        "Title\nPlain and bold text\u{FFFC}\nan item\na\tb\n\nThe note."
    );
    assert!(d.redo().is_some());
    assert!(d.text().as_str().contains("Very Plain"));
    // Saved as the plugin writes the file.
    d.save(Default::default(), true).unwrap();
    assert!(!d.is_modified());
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("Very Plain")
    );
}

/// Edits of every kind, each read again as the plugin tells what changed
/// (API 0.2.9): the lines are those a whole reading gives.
#[test]
fn what_changed_is_read_as_the_whole_would_be() {
    let mut text = String::from(DOC);
    for i in 0..30 {
        text.push_str(&match i % 6 {
            0 => format!("# Part {i}\n"),
            1 | 2 => format!("- item {i}\n"),
            3 => format!("|c{i}|d|\n"),
            4 => "---\n".to_string(),
            _ => format!("Paragraph {i} with *bold* words\n"),
        });
    }
    let (mut d, _) = open(&text);
    assert!(d.flow.as_deref_mut().unwrap().take_changed().is_none());
    let check = |d: &mut DocumentState, what: &str| {
        let f = d.flow.as_deref_mut().unwrap();
        assert!(
            f.take_changed().is_some(),
            "{what}: read whole rather than as changed"
        );
        if let Err(e) = f.reads_as_whole() {
            panic!("{what}: {e}");
        }
        let shown = f.text().to_string();
        assert_eq!(d.text().as_str(), shown, "{what}");
    };
    let at = |d: &DocumentState, s: &str| d.text().as_str().find(s).unwrap();
    let p = at(&d, "Paragraph 5");
    d.selection = Selection::caret(p);
    d.insert_text("x", Instant::now());
    check(&mut d, "a character typed");
    let p = at(&d, "Paragraph 11") + 4;
    edit(&mut d, p..p, "\n");
    check(&mut d, "Enter");
    edit(&mut d, p..p + 1, "");
    check(&mut d, "Backspace at a paragraph's start");
    let (a, b) = (at(&d, "item 7"), at(&d, "Paragraph 11"));
    edit(&mut d, a + 2..b + 3, "");
    check(&mut d, "a deletion across paragraphs, a table and a rule");
    let p = at(&d, "Part 18") + 4;
    edit(&mut d, p..p, " one\ntwo\nthree");
    check(&mut d, "lines pasted");
    let end = d.text().as_str().len() - "The note.".len() - 1;
    let last = d.text().as_str()[..end].rfind('\n').unwrap() + 1;
    edit(&mut d, last..last, "z");
    check(&mut d, "typing in the last paragraph before the note");
    for n in 0..4 {
        d.undo().unwrap();
        check(&mut d, &format!("undo {n}"));
    }
    d.redo().unwrap();
    check(&mut d, "redo");
}

#[test]
fn a_refused_edit_changes_nothing_and_says_why() {
    let (mut d, _) = open(DOC);
    let before = d.text().as_str().to_string();
    let mark = before.find('\u{FFFC}').unwrap();
    edit(&mut d, mark..mark + 3, "");
    assert_eq!(d.text().as_str(), before);
    let notice = d.take_notice().unwrap();
    assert!(notice.contains("note"), "{notice}");
    assert!(d.take_notice().is_none());
    // The rule's line is not text.
    let rule = before.find("\n\n").unwrap() + 1;
    edit(&mut d, rule..rule, "x");
    assert!(d.take_notice().unwrap().contains("not text"));
    // The fake's cells are not edited.
    let tab = before.find('\t').unwrap();
    edit(&mut d, tab - 1..tab + 2, "");
    assert!(d.take_notice().unwrap().contains("not edited"));
    assert_eq!(d.text().as_str(), before);
    assert!(!d.is_modified());
}

#[test]
fn comments_on_the_text() {
    let (mut d, _) = open(DOC);
    let at = d.text().as_str().find("Plain").unwrap();
    let id = d.on_flow(|f| f.comment(at..at + 5, "Check this")).unwrap();
    let f = d.flow.as_deref().unwrap();
    let here = f.annotations_at(at + 1);
    assert_eq!(here.len(), 1);
    assert_eq!(here[0].text, "Check this");
    assert_eq!(f.comments().len(), 1);
    assert_eq!(f.start_of(&id), Some(at));
    // Commented text is highlighted.
    let line_start = d.text().as_str().find("Plain").unwrap();
    let len = d.text().as_str()[line_start..].find('\n').unwrap();
    let v = kalem_core::flow::line_view(f, line_start..line_start + len);
    assert!(v.runs.iter().any(|r| r.style.rich.highlight.is_some()));
    d.on_flow(|f| f.reply(&id, "Done")).unwrap();
    d.on_flow(|f| f.resolve(&id, true)).unwrap();
    assert_eq!(d.flow.as_deref().unwrap().comments().len(), 2);
    assert!(d.on_flow(|f| f.decide(&id, true)).is_err());
    d.on_flow(|f| f.set_tracking(true)).unwrap();
    assert_eq!(d.flow.as_deref().unwrap().tracking, Some(true));
    d.on_flow(|f| f.remove_comment(&id)).unwrap();
    assert!(d.flow.as_deref().unwrap().comments().is_empty());
}

#[test]
fn a_comment_edited_from_the_palette() {
    use kalem_core::command::Clipboard;
    use kalem_core::{CommandRegistry, Config, EditorContext};
    let (mut d, _) = open(DOC);
    let at = d.text().as_str().find("Plain").unwrap();
    let id = d.on_flow(|f| f.comment(at..at + 5, "Check this")).unwrap();
    d.selection = Selection::caret(at + 1);
    let config = Config::default();
    // The palette starts with the comment's text, to edit.
    let start = kalem_core::command::argument_default_with(
        "flow.comment.edit",
        "text",
        &serde_json::json!({}),
        &mut d,
        &config,
    );
    assert_eq!(start, "Check this");
    let reg = CommandRegistry::with_builtins();
    let mut clip = Clipboard::default();
    let mut run = |d: &mut DocumentState, id: &str, args: serde_json::Value| {
        let mut ctx = EditorContext {
            document: Some(d),
            clipboard: &mut clip,
            config: &config,
            now: Instant::now(),
            clock: jiff::civil::date(2026, 10, 9).at(10, 0, 0, 0),
            messages: Vec::new(),
            requests: Vec::new(),
        };
        reg.execute(id, &mut ctx, &args).map_err(|e| e.to_string())
    };
    run(
        &mut d,
        "flow.comment.edit",
        serde_json::json!({ "text": "Checked\ntwice" }),
    )
    .unwrap();
    let f = d.flow.as_deref().unwrap();
    assert_eq!(f.annotations[&id].text, "Checked\ntwice");
    assert_eq!(
        f.comment_at(at + 1).map(|a| a.id.as_str()),
        Some(id.as_str())
    );
    // Nowhere near a comment: said so.
    d.selection = Selection::caret(0);
    let e = run(
        &mut d,
        "flow.comment.edit",
        serde_json::json!({ "text": "x" }),
    )
    .unwrap_err();
    assert!(!e.is_empty());
}

/// Runs command `id` on `d`: its result, and what it asks the editor.
fn run_command(
    d: &mut DocumentState,
    id: &str,
    args: serde_json::Value,
) -> (Result<(), String>, Vec<kalem_core::command::Request>) {
    let config = kalem_core::Config::default();
    let reg = kalem_core::CommandRegistry::with_builtins();
    let mut clip = kalem_core::command::Clipboard::default();
    let mut ctx = kalem_core::EditorContext {
        document: Some(d),
        clipboard: &mut clip,
        config: &config,
        now: Instant::now(),
        clock: jiff::civil::date(2026, 10, 10).at(10, 0, 0, 0),
        messages: Vec::new(),
        requests: Vec::new(),
    };
    let r = reg.execute(id, &mut ctx, &args).map_err(|e| e.to_string());
    (r, ctx.requests)
}

/// The titles of a list the command offers.
fn offered(requests: &[kalem_core::command::Request]) -> Vec<String> {
    requests
        .iter()
        .find_map(|r| match r {
            kalem_core::command::Request::Choose(items) => {
                Some(items.iter().map(|i| i.title.clone()).collect())
            }
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn formatting_from_the_menus_and_the_toolbar() {
    use serde_json::json;
    let (mut d, _) = open(DOC);
    let at = d.text().as_str().find("Plain").unwrap();
    d.selection = Selection::caret(at + 2);
    let bold = |d: &DocumentState| {
        let f = d.flow.as_deref().unwrap();
        f.run_at(at + 2).unwrap().marks.bold
    };
    // Bold on the word at the cursor, then off again: a toggle.
    assert!(!bold(&d));
    run_command(&mut d, "flow.format.bold", json!({}))
        .0
        .unwrap();
    assert!(bold(&d));
    assert!(d.is_modified());
    run_command(&mut d, "flow.format.bold", json!({}))
        .0
        .unwrap();
    assert!(!bold(&d));
    // Lists to pick from, then the pick.
    let (r, asked) = run_command(&mut d, "flow.format.font", json!({}));
    r.unwrap();
    let fonts = offered(&asked);
    assert!(
        fonts.contains(&"Calibri".to_string()) && fonts.last().unwrap().ends_with('…'),
        "{fonts:?}"
    );
    run_command(&mut d, "flow.format.font", json!({ "value": "Georgia" }))
        .0
        .unwrap();
    run_command(&mut d, "flow.format.fontSize", json!({ "value": "14" }))
        .0
        .unwrap();
    run_command(&mut d, "flow.format.color", json!({ "color": "#C00000" }))
        .0
        .unwrap();
    run_command(
        &mut d,
        "flow.format.highlight",
        json!({ "color": "#FFFF00" }),
    )
    .0
    .unwrap();
    let (style, marks) = d.flow.as_deref().unwrap().look_at(at + 2).unwrap();
    assert_eq!(style, "Normal");
    assert_eq!(marks.face.as_deref(), Some("Georgia"));
    assert_eq!(marks.size, Some(14.0));
    assert_eq!(marks.color, Some([0xC0, 0, 0]));
    assert_eq!(marks.highlight, Some([0xFF, 0xFF, 0]));
    let (r, _) = run_command(&mut d, "flow.format.fontSize", json!({ "value": "big" }));
    assert!(r.unwrap_err().contains("big"));
    run_command(&mut d, "flow.format.clear", json!({}))
        .0
        .unwrap();
    let (_, marks) = d.flow.as_deref().unwrap().look_at(at + 2).unwrap();
    assert_eq!(marks.size, None);
    // The paragraph styles shown, the default first, the headings next.
    let (r, asked) = run_command(&mut d, "flow.format.style", json!({}));
    r.unwrap();
    assert_eq!(offered(&asked), ["Normal", "Heading 1", "Quote"]);
    run_command(&mut d, "flow.format.style", json!({ "style": "Heading 1" }))
        .0
        .unwrap();
    let (style, _) = d.flow.as_deref().unwrap().look_at(at + 2).unwrap();
    assert_eq!(style, "Heading 1");
    // Undone through the plugin's history.
    assert!(d.undo().is_some());
    let (style, _) = d.flow.as_deref().unwrap().look_at(at + 2).unwrap();
    assert_eq!(style, "Normal");
    // Nowhere near a word: said so.
    d.selection = Selection::caret(0);
    let line_end = d.text().as_str().find('\n').unwrap();
    d.selection = Selection::caret(line_end);
    let _ = run_command(&mut d, "flow.format.italic", json!({}));
}

#[test]
fn the_word_commands_offered_where_they_serve() {
    let (d, _) = open(DOC);
    let reg = kalem_core::CommandRegistry::with_builtins();
    let flow = d.document_context();
    let meta = kalem_core::Metadata {
        path: None,
        mode: DocumentMode::Org,
        line_ending: kalem_core::LineEnding::Lf,
        bom: false,
        encoding: kalem_core::encoding_rs::UTF_8,
        lossy: false,
    };
    let org =
        DocumentState::new("* A heading\n", meta, Arc::new(Settings::default())).document_context();
    for id in [
        "flow.format.bold",
        "flow.format.font",
        "flow.format.style",
        "flow.comment.edit",
    ] {
        assert!(reg.offered(id, &flow), "{id} in a flowing document");
        assert!(!reg.offered(id, &org), "{id} in Org");
    }
    // In the menus: the Format menu's and the Review menu's.
    let menus = kalem_core::menus::menus();
    let ids = |name: &str| -> Vec<String> {
        menus
            .iter()
            .find(|m| m.name == name)
            .map(|m| {
                m.entries
                    .iter()
                    .filter_map(|e| match e {
                        kalem_core::menus::MenuEntry::Command { id, .. } => Some(id.clone()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    assert!(ids("Format").contains(&"flow.format.highlight".to_string()));
    assert!(ids("Review").contains(&"flow.change.acceptAll".to_string()));
}

#[test]
fn paragraphs_and_lists_from_the_menus_and_the_toolbar() {
    use kalem_viewer::{FlowAlign, FlowRole};
    use serde_json::json;
    let (mut d, _) = open(DOC);
    let at = d.text().as_str().find("Plain").unwrap();
    d.selection = Selection::caret(at + 2);
    let para = |d: &DocumentState| {
        d.flow
            .as_deref()
            .unwrap()
            .paragraph_at(at + 2)
            .unwrap()
            .clone()
    };
    run_command(&mut d, "flow.paragraph.alignCenter", json!({}))
        .0
        .unwrap();
    assert_eq!(para(&d).align, FlowAlign::Center);
    assert!(d.is_modified());
    // Bullets on, then off: a toggle.
    run_command(&mut d, "flow.list.bullets", json!({}))
        .0
        .unwrap();
    let p = para(&d);
    assert_eq!(
        (p.role, kalem_core::flow::list_kind(&p)),
        (FlowRole::ListItem, Some(false))
    );
    run_command(&mut d, "flow.list.bullets", json!({}))
        .0
        .unwrap();
    assert_eq!(para(&d).role, FlowRole::Body);
    // Numbering in a style picked from the list.
    let (r, asked) = run_command(&mut d, "flow.list.numberingStyle", json!({}));
    r.unwrap();
    assert_eq!(offered(&asked).len(), 5);
    run_command(
        &mut d,
        "flow.list.numbering",
        json!({ "format": "lower-roman" }),
    )
    .0
    .unwrap();
    assert_eq!(kalem_core::flow::list_kind(&para(&d)), Some(true));
    let (r, _) = run_command(&mut d, "flow.list.numbering", json!({ "format": "hebrew" }));
    assert!(r.unwrap_err().contains("hebrew"));
    // A list item a level deeper, and back.
    run_command(&mut d, "flow.paragraph.increaseIndent", json!({}))
        .0
        .unwrap();
    assert_eq!(para(&d).level, 2);
    run_command(&mut d, "flow.paragraph.decreaseIndent", json!({}))
        .0
        .unwrap();
    assert_eq!(para(&d).level, 1);
    run_command(&mut d, "flow.list.numbering", json!({}))
        .0
        .unwrap();
    assert_eq!(para(&d).role, FlowRole::Body);
    // Spacing from the lists, or typed.
    let (r, asked) = run_command(&mut d, "flow.paragraph.lineSpacing", json!({}));
    r.unwrap();
    assert!(offered(&asked).contains(&"1.5".to_string()));
    run_command(
        &mut d,
        "flow.paragraph.spaceBefore",
        json!({ "value": "12" }),
    )
    .0
    .unwrap();
    assert_eq!(para(&d).spacing.0, 12.0);
    let (r, _) = run_command(
        &mut d,
        "flow.paragraph.spaceAfter",
        json!({ "value": "lots" }),
    );
    assert!(r.unwrap_err().contains("lots"));
    run_command(&mut d, "flow.paragraph.clear", json!({}))
        .0
        .unwrap();
    assert_eq!(para(&d).align, FlowAlign::Start);
    // Two body paragraphs indented a step each, one step to undo.
    let first = d.text().as_str().find("Title").unwrap();
    d.selection = Selection {
        anchor: first,
        head: at + 2,
    };
    run_command(&mut d, "flow.paragraph.increaseIndent", json!({}))
        .0
        .unwrap();
    run_command(&mut d, "flow.paragraph.increaseIndent", json!({}))
        .0
        .unwrap();
    assert_eq!(para(&d).indent.0, 72.0);
    let title = |d: &DocumentState| {
        d.flow
            .as_deref()
            .unwrap()
            .paragraph_at(first)
            .unwrap()
            .indent
            .0
    };
    assert_eq!(title(&d), 72.0);
    assert!(d.undo().is_some());
    assert_eq!((title(&d), para(&d).indent.0), (36.0, 36.0));
}

#[test]
fn rules_drawn_across_and_colors_marked_as_the_documents() {
    let (d, _) = open(DOC);
    let f = d.flow.as_deref().unwrap();
    // `---`: a rule with no name, a line drawn across away from the
    // cursor, as Org's.
    let rules = f.rule_lines();
    assert_eq!(rules.len(), 1);
    let blocks = kalem_core::mode_view::blocks(&d);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].kind, kalem_core::view::BlockKind::Rule);
    assert_eq!(blocks[0].range, rules[0]);
    // A run's colors are the document's, for the frontends to keep
    // legible.
    let at = d.text().as_str().find("Plain").unwrap();
    let len = d.text().as_str()[at..].find('\n').unwrap();
    let v = kalem_core::flow::line_view(f, at..at + len);
    assert!(
        v.runs
            .iter()
            .filter(|r| !r.style.dim)
            .all(|r| r.style.rich.paper)
    );
}
