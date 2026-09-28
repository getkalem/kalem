//! Before and after snapshots of every command (T1.2.12).
//!
//! Each case is a document with the cursor written as `‸` (and a mark as
//! `⁁` when the command takes a region); the snapshot shows the document
//! after the command, with the new cursor, or the error. The Emacs
//! differential (`emacs_diff.rs`) checks behavior against Emacs; these
//! snapshots make every command's effect readable and catch changes in
//! Kalem-only commands too.

use jiff::civil::DateTime;
use org_edit::{EditError, Transaction};
use org_model::Document;

const CURSOR: char = '‸';
const MARK: char = '⁁';

fn now() -> DateTime {
    jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0)
}

/// The document without markers, the cursor and the mark.
fn split(marked: &str) -> (String, usize, Option<usize>) {
    let mut text = String::new();
    let (mut point, mut mark) = (0, None);
    for c in marked.chars() {
        match c {
            CURSOR => point = text.len(),
            MARK => mark = Some(text.len()),
            c => text.push(c),
        }
    }
    (text, point, mark)
}

fn render(text: &str, result: Result<Transaction, EditError>) -> String {
    match result {
        Ok(t) => {
            let after = t.apply(text);
            let head = t.selection_after.map_or(0, |s| s.head);
            format!("{}{CURSOR}{}", &after[..head], &after[head..])
        }
        Err(e) => format!("error: {}", e.message),
    }
}

/// Runs `command` on the marked document and snapshots the result.
fn check(
    name: &str,
    marked: &str,
    command: impl FnOnce(&Document, &str, usize, Option<usize>) -> Result<Transaction, EditError>,
) {
    let (text, point, mark) = split(marked);
    let doc = Document::new(org_syntax::parse(&text));
    let out = render(&text, command(&doc, &text, point, mark));
    insta::assert_snapshot!(name, format!("{marked}\n----\n{out}"));
}

const OUTLINE: &str = "* TODO [#B] Top :work:\n** Child ‸b\n** Child a\n*** Deep\n* Other\n";

#[test]
fn headlines() {
    use org_edit::headline::*;
    let ctx = || org_syntax::parse("").context().clone();
    check("promote", OUTLINE, |_, t, p, _| promote(t, p, &ctx()));
    check("demote", OUTLINE, |_, t, p, _| demote(t, p, &ctx()));
    check("promote_subtree", OUTLINE, |_, t, p, _| {
        promote_subtree(t, p, &ctx())
    });
    check("demote_subtree", OUTLINE, |_, t, p, _| {
        demote_subtree(t, p, &ctx())
    });
    check("move_subtree_down", OUTLINE, |_, t, p, _| {
        move_subtree(t, p, true, &ctx())
    });
    check("move_subtree_up", OUTLINE, |_, t, p, _| {
        move_subtree(t, p, false, &ctx())
    });
    check("cut_subtree", OUTLINE, |_, t, p, _| {
        cut_subtree(t, p, &ctx()).map(|(t, _)| t)
    });
    check("paste_subtree", "* A\n** B\n‸* C\n", |_, t, p, _| {
        paste_subtree(t, p, "* Pasted\n** Its child\n", &ctx())
    });
}

#[test]
fn sorting() {
    use org_edit::sort::*;
    let doc = "* P‸\n** b\n** C\n** a\n";
    // Distinct names: snapshot files must not differ only in case.
    for (name, c) in [
        ("sort_alpha", 'a'),
        ("sort_alpha_reversed", 'A'),
        ("sort_numeric", 'n'),
    ] {
        check(name, doc, |d, _, p, m| {
            sort_entries(d, p, m, &SortOptions::from_char(c, None).unwrap(), now())
        });
    }
}

#[test]
fn todo_and_priority() {
    use org_edit::todo::*;
    let doc = "#+STARTUP: logdone\n#+TODO: TODO WAIT(w@) | DONE\n* Parent [0/1]\n** TODO Task‸\nSCHEDULED: <2026-09-21 Mon +1w>\n";
    let run = |arg: TodoArg| {
        move |d: &Document, _: &str, p: usize, _: Option<usize>| {
            let settings = TodoSettings::default().for_document(d);
            let opts = TodoOptions {
                arg,
                settings: &settings,
                now: now(),
                remembered_head: None,
                repeated: false,
                force_note: false,
                inhibit_note: false,
            };
            todo(d, p, &opts).map(|o| o.transaction)
        }
    };
    check("todo_cycle", doc, run(TodoArg::Cycle));
    check("todo_done_repeats", doc, run(TodoArg::Done));
    check("todo_state_wait", doc, run(TodoArg::State("WAIT".into())));
    check("todo_none", doc, run(TodoArg::None));
    check("todo_bogus", doc, run(TodoArg::State("BOGUS".into())));
    check("priority_up", doc, |d, _, p, _| {
        priority(d, p, PriorityAction::Up, false, &TodoSettings::default())
    });
    check("priority_set_a", doc, |d, _, p, _| {
        priority(
            d,
            p,
            PriorityAction::Set('A'),
            false,
            &TodoSettings::default(),
        )
    });
    check("priority_remove", "* [#A] x‸\n", |d, _, p, _| {
        priority(
            d,
            p,
            PriorityAction::Remove,
            false,
            &TodoSettings::default(),
        )
    });
    // A note entered by the user.
    check("todo_note", doc, |d, text, p, _| {
        let settings = TodoSettings::default().for_document(d);
        let opts = TodoOptions {
            arg: TodoArg::State("WAIT".into()),
            settings: &settings,
            now: now(),
            remembered_head: None,
            repeated: false,
            force_note: false,
            inhibit_note: false,
        };
        let outcome = todo(d, p, &opts)?;
        let mid = outcome.transaction.apply(text);
        let p2 = outcome.transaction.selection_after.map_or(p, |s| s.head);
        let d2 = Document::new(org_syntax::parse(&mid));
        let t2 = store_log_note(
            &d2,
            p2,
            outcome.note.as_ref().expect("a note"),
            "Waiting for the review",
            &settings,
        );
        let mut whole = Transaction::new("todo and note");
        whole.replace(0..text.len(), t2.apply(&mid)).unwrap();
        Ok(whole.select(t2.selection_after.unwrap()))
    });
}

#[test]
fn properties_and_tags() {
    use org_edit::tags::*;
    let doc = "* Heading‸ :a:\nText.\n";
    check("set_property", doc, |d, _, p, _| {
        org_edit::property::set_property(d, p, "EFFORT", "1:00", false)
    });
    check("set_tags", doc, |d, _, p, _| {
        set_tags(d, p, &["x".into(), "y".into()])
    });
    check("toggle_tag_on", doc, |d, _, p, _| {
        toggle_tag(d, p, "b", None).map(|(t, _)| t)
    });
    check("toggle_tag_off", doc, |d, _, p, _| {
        toggle_tag(d, p, "a", None).map(|(t, _)| t)
    });
    check(
        "change_tag_in_region",
        "⁁* A\n* B :x:\n* C‸\n",
        |d, _, p, m| Ok(change_tag_in_region(d, p, m.unwrap(), p, "new", false)),
    );
    check(
        "align_all_tags",
        "* A :x:\n* Longer‸ title :y:\n",
        |d, _, p, _| Ok(align_all_tags(d, p)),
    );
    let table = org_model::TagTable::parse("{ @home @work } laptop");
    let picked = select_tag(&["@home".into(), "laptop".into()], "@work", &table);
    insta::assert_snapshot!("select_tag_exclusive", format!("{picked:?}"));
}

#[test]
fn lists() {
    use org_edit::list::*;
    let doc = "- [ ] one\n- [X] two‸\n  - sub\n- three\n";
    check("indent_item", doc, |d, _, p, m| {
        indent_item(d, p, m, true, true)
    });
    check("indent_item_tree", doc, |d, _, p, m| {
        indent_item(d, p, m, true, false)
    });
    check("outdent_item_tree", "- a\n  - b‸\n", |d, _, p, m| {
        indent_item(d, p, m, false, false)
    });
    check("cycle_bullet", doc, |d, _, p, _| {
        cycle_bullet(d, p, BulletChoice::Next)
    });
    check("cycle_bullet_numbered", doc, |d, _, p, _| {
        cycle_bullet(d, p, BulletChoice::Bullet("1.".into()))
    });
    check("toggle_checkbox", doc, |d, _, p, m| {
        toggle_checkbox(d, p, m, CheckboxAction::Toggle)
    });
    check(
        "checkbox_cookies",
        "* Tasks [/] [%]\n- [ ] a‸\n- [X] b\n",
        |d, _, p, m| toggle_checkbox(d, p, m, CheckboxAction::Toggle),
    );
    check("move_item_down", doc, |d, _, p, _| move_item(d, p, true));
    check("move_item_up", doc, |d, _, p, _| move_item(d, p, false));
    check("insert_item", doc, |d, _, p, _| {
        Ok(insert_item(d, p, false).unwrap())
    });
    check("insert_item_checkbox", doc, |d, _, p, _| {
        Ok(insert_item(d, p, true).unwrap())
    });
    check("repair_numbering", "1. a‸\n5. b\n9. c\n", |d, _, p, _| {
        repair(d, p)
    });
}

#[test]
fn emphasis() {
    use org_edit::emphasis::*;
    check(
        "emphasize_region",
        "Some ⁁words‸ here.\n",
        |d, _, p, m| emphasize(d, p, m, Some('*')),
    );
    check("emphasize_empty", "Some ‸words.\n", |d, _, p, m| {
        emphasize(d, p, m, Some('/'))
    });
    check("emphasize_remove", "Some ⁁*words*‸.\n", |d, _, p, m| {
        emphasize(d, p, m, None)
    });
    check("toggle_bold_on", "a ⁁word‸ b\n", |d, _, p, m| {
        toggle_emphasis(d, m.unwrap(), p, Emphasis::Bold)
    });
    check("toggle_bold_off", "a *⁁word‸* b\n", |d, _, p, m| {
        toggle_emphasis(d, m.unwrap(), p, Emphasis::Bold)
    });
    check("toggle_nested", "a *bold ⁁word‸* b\n", |d, _, p, m| {
        toggle_emphasis(d, m.unwrap(), p, Emphasis::Italic)
    });
    check("toggle_in_code", "a ~co⁁de‸~ b\n", |d, _, p, m| {
        toggle_emphasis(d, m.unwrap(), p, Emphasis::Bold)
    });
}

#[test]
fn inserts() {
    use org_edit::insert::*;
    check("insert_link", "See ‸.\n", |d, _, p, m| {
        insert_link(d, p, m, "https://orgmode.org", Some("Org"))
    });
    check(
        "insert_link_region",
        "See ⁁the manual‸.\n",
        |d, _, p, m| insert_link(d, p, m, "https://orgmode.org", None),
    );
    check("insert_block", "Text\n‸", |d, _, p, m| {
        insert_structure_template(d, p, m, "src")
    });
    check(
        "insert_block_region",
        "⁁* heading-like line\ncode\n‸",
        |d, _, p, m| insert_structure_template(d, p, m, "example"),
    );
    check("insert_timestamp", "Meet ‸.\n", |d, _, p, _| {
        Ok(insert_timestamp(d, p, now(), true, false))
    });
    check(
        "replace_timestamp",
        "Meet <2026-09-01 Tue +1w>‸.\n",
        |d, _, p, _| Ok(insert_timestamp(d, p - 2, now(), false, false)),
    );
    check("insert_rule", "Text‸\n", |d, _, p, _| {
        Ok(insert_horizontal_rule(d, p))
    });
}

#[test]
fn tables() {
    use org_edit::table::*;
    let doc = "| a | bb |\n|-\n| 10 | x‸ |\n#+TBLFM: $2=$1*2\n";
    check("table_align", doc, |d, _, p, _| align_table(d, p));
    check("table_insert_row", doc, |d, _, p, _| {
        insert_row(d, p, false)
    });
    check("table_kill_row", doc, |d, _, p, _| kill_row(d, p));
    check("table_move_row_up", doc, |d, _, p, _| move_row(d, p, true));
    check("table_insert_hline", doc, |d, _, p, _| {
        insert_hline(d, p, false)
    });
    check("table_insert_column", doc, |d, _, p, _| insert_column(d, p));
    check("table_delete_column", doc, |d, _, p, _| delete_column(d, p));
    check("table_move_column_left", doc, |d, _, p, _| {
        move_column(d, p, true)
    });
    check("table_next_field", doc, |d, _, p, _| next_field(d, p));
    check("table_previous_field", doc, |d, _, p, _| {
        previous_field(d, p)
    });
    check("table_next_row", doc, |d, _, p, _| next_row(d, p));
}

#[test]
fn narrowing() {
    use org_edit::narrow::*;
    let doc = "* B‸\n** z\n** a\n* A\n";
    check("narrowed_sort", doc, |d, _, p, _| {
        let r = subtree(d, p)?;
        narrowed(d, r, p, |sub, sp| {
            org_edit::sort::sort_entries(
                sub,
                sp,
                None,
                &org_edit::sort::SortOptions::from_char('a', None).unwrap(),
                now(),
            )
        })
    });
    let (text, _, _) = split(doc);
    let d = Document::new(org_syntax::parse(&text));
    let r = subtree(&d, 5).unwrap();
    insta::assert_snapshot!("narrow_subtree_range", format!("{:?}", &text[r]));
}

#[test]
fn style() {
    let text = "#+TITLE: T\n#+STARTUP: indent\n* A\n  SCHEDULED: <2026-01-01>\n  Body.\n\n* B                                                                   :x:\n";
    let s = org_edit::style::Style::infer(&org_syntax::parse(text));
    insta::assert_debug_snapshot!("style_infer", s);
}
