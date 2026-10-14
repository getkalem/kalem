//! A document edited with Vim's keys is saved with a final line feed, as
//! Vim writes a file ('fixendofline'); one that does not ask for it is
//! written as its text is.

use kalem_core::files::SaveOptions;
use kalem_core::{DocumentMode, DocumentState, LineEnding, Metadata};

fn document(path: &std::path::Path, text: &str, line_ending: LineEnding) -> DocumentState {
    let meta = Metadata {
        path: Some(path.to_path_buf()),
        mode: DocumentMode::Text { language: None },
        line_ending,
        bom: false,
        encoding: encoding_rs::UTF_8,
        lossy: false,
    };
    DocumentState::new(text, meta, std::sync::Arc::default())
}

#[test]
fn saved_as_vim_writes_it() {
    let dir = std::env::temp_dir().join(format!("kalem-final-newline-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (fix, line_ending, want) in [
        (true, LineEnding::Lf, "abc\n"),
        (false, LineEnding::Lf, "abc"),
        (true, LineEnding::CrLf, "abc\r\n"),
    ] {
        let path = dir.join("t.txt");
        let mut d = document(&path, "abc", line_ending);
        d.final_newline = fix;
        d.save(SaveOptions::default(), true).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), want);
        // The text is what the file holds: nothing left to save.
        assert!(!d.is_modified());
        let text = if fix { "abc\n" } else { "abc" };
        assert_eq!(d.text().as_str(), text);
    }
    let _ = std::fs::remove_dir_all(&dir);
}
