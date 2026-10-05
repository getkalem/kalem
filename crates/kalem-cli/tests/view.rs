//! `kalem view`: a unit of a file a viewer opens, as text or as PNG.

use std::path::Path;
use std::process::ExitCode;

#[test]
fn a_sheet_is_written_as_text_and_refused_as_a_picture() {
    let book = Path::new(env!("CARGO_MANIFEST_DIR")).join("../kalem-tui/tests/data/budget.xlsx");
    let dir = std::env::temp_dir().join(format!("kalem-view-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The viewers as the binary installs them, none of the user's.
    // SAFETY: the test's process has this one test, and sets the
    // variables before any thread reads them.
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("KALEM_CONFIG_DIR", dir.join("config"));
        std::env::set_var("KALEM_STATE_DIR", dir.join("state"));
    }
    kalem_cli::bundled_plugins();
    let run = |to: &str, out: &Path| {
        kalem_cli::run([
            "kalem".as_ref(),
            "view".as_ref(),
            book.as_os_str(),
            "--to".as_ref(),
            to.as_ref(),
            "-o".as_ref(),
            out.as_os_str(),
        ])
    };
    let txt = dir.join("budget.txt");
    assert_eq!(run("txt", &txt), ExitCode::SUCCESS);
    assert!(std::fs::read_to_string(&txt).unwrap().contains("Rent"));
    // It wrote a 1 × 1 PNG and said nothing.
    let png = dir.join("budget.png");
    assert_ne!(run("png", &png), ExitCode::SUCCESS);
    assert!(!png.exists());
    std::fs::remove_dir_all(&dir).ok();
}
