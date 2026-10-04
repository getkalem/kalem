//! The crash report (design §14, roadmap R2.2): a process that started
//! logging and then panics leaves `crash-DATE.txt` beside its log, with
//! the report's header, the panic's message and where the log is. The
//! test runs itself again as a child that panics on purpose.

use std::path::Path;
use std::process::Command;

use kalem_core::logging::{LogOptions, init};

const CHILD: &str = "KALEM_CRASH_TEST_DIR";

/// In the child: start logging into the folder given, then panic.
#[test]
fn child_panics() {
    let Some(dir) = std::env::var_os(CHILD) else {
        return;
    };
    let dir = Path::new(&dir);
    init(&LogOptions {
        file: Some(dir.join("kalem.log")),
        stderr: false,
        filter: Some("info".into()),
        crash_report: Some("Kalem test\nPlatform: test\n".into()),
    })
    .unwrap();
    panic!("a panic on purpose");
}

#[test]
fn a_panic_leaves_a_crash_report() {
    let dir = std::env::temp_dir().join(format!("kalem-crash-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["child_panics", "--exact", "--test-threads=1"])
        .env(CHILD, &dir)
        .output()
        .unwrap();
    assert!(!out.status.success(), "the child did not panic");
    let reports: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
        })
        .collect();
    assert_eq!(reports.len(), 1, "{reports:?}");
    let text = std::fs::read_to_string(&reports[0]).unwrap();
    assert!(text.starts_with("Kalem test\nPlatform: test\n"), "{text}");
    assert!(text.contains("Log: "), "{text}");
    assert!(text.contains("a panic on purpose"), "{text}");
    // The log has the panic too.
    let log = std::fs::read_to_string(dir.join("kalem.log")).unwrap();
    assert!(log.contains("a panic on purpose"), "{log}");
    std::fs::remove_dir_all(&dir).unwrap();
}
