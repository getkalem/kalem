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
