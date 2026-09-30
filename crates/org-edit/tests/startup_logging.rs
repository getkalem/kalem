//! The `#+STARTUP` logging and footnote options, against what Emacs
//! (Org 9.7) writes for the same commands, the clock at 2026-09-28 Mon
//! 10:00.

use org_edit::footnote::{self, AutoLabel, FootnoteSettings};
use org_edit::todo::{LogKind, Planning, PlanningChange, TodoSettings, schedule, store_log_note};
use org_model::Document;

fn now() -> jiff::civil::DateTime {
    jiff::civil::date(2026, 9, 28).at(10, 0, 0, 0)
}

fn doc(t: &str) -> Document {
    Document::new(org_syntax::parse(t))
}

fn date(s: &str, time: bool) -> PlanningChange {
    let (d, t) = s.split_once(' ').unwrap_or((s, "00:00"));
    PlanningChange::Set(format!("{d}T{t}").parse().unwrap(), time, None)
}

fn plan(text: &str, kind: Planning, change: PlanningChange, settings: &TodoSettings) -> String {
    let (tx, _, note) = schedule(&doc(text), 0, kind, &change, settings, now()).unwrap();
    assert!(note.is_none());
    tx.apply(text)
}

#[test]
fn reschedule_and_redeadline() {
    let s = TodoSettings::default().for_document(&doc("#+STARTUP: logreschedule logredeadline\n"));
    let t = "* TODO Task\nSCHEDULED: <2026-10-01 Thu>\nBody\n";
    assert_eq!(
        plan(t, Planning::Scheduled, date("2026-10-05", false), &s),
        "* TODO Task\nSCHEDULED: <2026-10-05 Mon>\n- Rescheduled from \"[2026-10-01 Thu]\" on [2026-09-28 Mon 10:00]\nBody\n"
    );
    assert_eq!(
        plan(t, Planning::Scheduled, PlanningChange::Remove, &s),
        "* TODO Task\n- Not scheduled, was \"[2026-10-01 Thu]\" on [2026-09-28 Mon 10:00]\nBody\n"
    );
    // The same date: nothing to log.
    assert_eq!(
        plan(t, Planning::Scheduled, date("2026-10-01", false), &s),
        t
    );
    // No date before: nothing to log.
    assert_eq!(
        plan(
            "* TODO Task\nBody\n",
            Planning::Scheduled,
            date("2026-10-05", false),
            &s
        ),
        "* TODO Task\nSCHEDULED: <2026-10-05 Mon>\nBody\n"
    );
    let t = "* TODO Task\nDEADLINE: <2026-10-01 Thu>\nBody\n";
    assert_eq!(
        plan(t, Planning::Deadline, date("2026-10-05 12:00", true), &s),
        "* TODO Task\nDEADLINE: <2026-10-05 Mon 12:00>\n- New deadline from \"[2026-10-01 Thu]\" on [2026-09-28 Mon 10:00]\nBody\n"
    );
    assert_eq!(
        plan(t, Planning::Deadline, PlanningChange::Remove, &s),
        "* TODO Task\n- Removed deadline, was \"[2026-10-01 Thu]\" on [2026-09-28 Mon 10:00]\nBody\n"
    );
    // A repeater is kept and shown in the old date.
    let t = "* TODO Task\nSCHEDULED: <2026-10-01 Thu 09:00 +1w>\n- old\nBody\n";
    assert_eq!(
        plan(t, Planning::Scheduled, date("2026-10-05", false), &s),
        "* TODO Task\nSCHEDULED: <2026-10-05 Mon +1w>\n- Rescheduled from \"[2026-10-01 Thu 09:00 +1w]\" on [2026-09-28 Mon 10:00]\n- old\nBody\n"
    );
    // Into the drawer.
    let d = TodoSettings {
        log_into_drawer: Some("LOGBOOK".into()),
        ..s.clone()
    };
    assert_eq!(
        plan(
            "* TODO Task\nSCHEDULED: <2026-10-01 Thu>\nBody\n",
            Planning::Scheduled,
            date("2026-10-05", false),
            &d
        ),
        "* TODO Task\nSCHEDULED: <2026-10-05 Mon>\n:LOGBOOK:\n- Rescheduled from \"[2026-10-01 Thu]\" on [2026-09-28 Mon 10:00]\n:END:\nBody\n"
    );
    // Off by default.
    let t = "* TODO Task\nSCHEDULED: <2026-10-01 Thu>\nBody\n";
    assert_eq!(
        plan(
            t,
            Planning::Scheduled,
            date("2026-10-05", false),
            &TodoSettings::default()
        ),
        "* TODO Task\nSCHEDULED: <2026-10-05 Mon>\nBody\n"
    );
}

#[test]
fn reschedule_with_a_note() {
    let s = TodoSettings {
        log_reschedule: Some(LogKind::Note),
        ..TodoSettings::default()
    };
    let t = "* TODO Task\nSCHEDULED: <2026-10-01 Thu>\nBody\n";
    let (tx, _, note) = schedule(
        &doc(t),
        0,
        Planning::Scheduled,
        &date("2026-10-05", false),
        &s,
        now(),
    )
    .unwrap();
    let t2 = tx.apply(t);
    assert_eq!(t2, "* TODO Task\nSCHEDULED: <2026-10-05 Mon>\nBody\n");
    let t3 = store_log_note(&doc(&t2), 0, &note.unwrap(), "why", &s).apply(&t2);
    assert_eq!(
        t3,
        "* TODO Task\nSCHEDULED: <2026-10-05 Mon>\n- Rescheduled from \"[2026-10-01 Thu]\" on [2026-09-28 Mon 10:00] \\\\\n  why\nBody\n"
    );
}

#[test]
fn refile_logged() {
    let s = TodoSettings::default().for_document(&doc("#+STARTUP: logrefile\n"));
    let t = "* A\n* B\nbody\n";
    let (tx, note) = org_edit::archive::refile_logged(&doc(t), 4, 0, &s, now()).unwrap();
    assert!(note.is_none());
    assert_eq!(
        tx.apply(t),
        "* A\n** B\n- Refiled on [2026-09-28 Mon 10:00]\nbody\n"
    );
}

fn fn_new(t: &str, point: usize, s: &FootnoteSettings, answer: Option<&str>) -> (String, usize) {
    let tx = footnote::new_labeled(t, point, s, answer).unwrap();
    (tx.apply(t), tx.selection_after.map_or(0, |s| s.head))
}

#[test]
fn footnote_startup() {
    let base = FootnoteSettings::default();
    let s = base.for_text("#+STARTUP: fninline fnconfirm fnadjust fnlocal\n");
    assert!(s.define_inline && s.auto_adjust && s.section.is_none());
    assert_eq!(s.auto_label, AutoLabel::Confirm);
    let t = "Some text here.\n";
    let inline = FootnoteSettings {
        define_inline: true,
        ..base.clone()
    };
    assert_eq!(
        fn_new(t, 9, &inline, None),
        ("Some text[fn:1:] here.\n".into(), 15)
    );
    let anon = FootnoteSettings {
        auto_label: AutoLabel::Prompt,
        ..inline
    };
    assert_eq!(
        fn_new(t, 9, &anon, Some("")),
        ("Some text[fn::] here.\n".into(), 14)
    );
    let confirm = FootnoteSettings {
        auto_label: AutoLabel::Confirm,
        ..base.clone()
    };
    assert_eq!(
        fn_new(t, 9, &confirm, Some("mine")),
        (
            "Some text[fn:mine] here.\n\n* Footnotes\n\n[fn:mine] \n".into(),
            48
        )
    );
    let adjust = FootnoteSettings {
        auto_adjust: true,
        ..base
    };
    let (t2, p) = fn_new(
        "B[fn:2] text A.\n\n* Footnotes\n\n[fn:2] two\n",
        13,
        &adjust,
        None,
    );
    assert_eq!(
        t2,
        "B[fn:1] text [fn:2]A.\n\n* Footnotes\n\n[fn:1] two\n\n[fn:2] \n"
    );
    assert_eq!(p, 55);
    let t = "A[fn:1] B[fn:2] C[fn:3].\n\n* Footnotes\n\n[fn:1] one\n\n[fn:2] two\n\n[fn:3] three\n";
    let tx = footnote::delete_adjusted(t, 10, &adjust).unwrap();
    assert_eq!(
        tx.apply(t),
        "A[fn:1] B C[fn:2].\n\n* Footnotes\n\n[fn:1] one\n\n[fn:2] three\n"
    );
    assert_eq!(tx.selection_after.map(|s| s.head), Some(9));
}
