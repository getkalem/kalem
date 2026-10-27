//! Typing code in Vim's insert mode, as the terminal editor does (the
//! keys the Vim layer leaves typed as the document's text): a closing
//! bracket alone on its line goes back to the indentation of its opening
//! bracket's line, which Vim's 'autoindent' keeps the lines after at.

use std::time::Instant;

use kalem_core::{DocumentMode, DocumentState, LineEnding, Metadata};
use kalem_vim::{Host, Key, Vim};

#[derive(Default)]
struct TestHost {
    clip: Option<String>,
}

impl Host for TestHost {
    fn clipboard(&mut self) -> Option<String> {
        self.clip.clone()
    }

    fn set_clipboard(&mut self, text: &str) {
        self.clip = Some(text.to_string());
    }
}

/// `text`, a Rust file, after `A` on its last line and `typed` in Vim's
/// insert mode (a line feed is Enter).
fn typed_in_vim(text: &str, typed: &str) -> String {
    let meta = Metadata {
        path: None,
        mode: DocumentMode::Text {
            language: Some("rs".into()),
        },
        line_ending: LineEnding::Lf,
        bom: false,
        encoding: kalem_core::encoding_rs::UTF_8,
        lossy: false,
    };
    let mut d = DocumentState::new(text, meta, std::sync::Arc::default());
    d.move_cursor(text.len(), false);
    let mut v = Vim::new();
    let mut host = TestHost::default();
    let keys = typed
        .chars()
        .map(|c| if c == '\n' { Key::Enter } else { Key::Char(c) });
    for key in std::iter::once(Key::Char('A'))
        .chain(keys)
        .chain([Key::Esc])
    {
        if !v.key(&mut d, key, &mut host).handled
            && let Key::Char(c) = key
        {
            d.type_text(&c.to_string(), false, Instant::now());
        }
    }
    d.text().as_str().to_string()
}

#[test]
fn a_closing_brace_under_its_opening_line() {
    assert_eq!(
        typed_in_vim("fn main() {\n    if a {", "\nb();\n}"),
        "fn main() {\n    if a {\n    b();\n    }"
    );
    // Indented a level deeper by hand, the brace goes back to the line of
    // its opening brace.
    assert_eq!(
        typed_in_vim("fn main() {\n    if a {", "\n    b();\n}"),
        "fn main() {\n    if a {\n        b();\n    }"
    );
    // No opening brace: as typed.
    assert_eq!(typed_in_vim("    x", "\n}"), "    x\n    }");
}
