//! A plugin's question in the terminal: a message longer than the pane
//! is wide stands once above its answers, wrapped, and each answer shows
//! whole. In a test binary of its own, as the plugins' questions are the
//! process's.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use kalem_core::extensions::{self, Question};
use kalem_core::settings::Config;
use kalem_tui::app::App;
use kalem_tui::caps::Caps;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

fn screen(term: &mut Terminal<TestBackend>, app: &mut App) -> Vec<String> {
    term.draw(|f| app.draw(f)).unwrap();
    let buf = term.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn a_long_confirm_shows_its_answers_whole() {
    // The git plugin's Revert…: each answer drawn as "MESSAGE: Yes" was
    // cut at the pane's width ("…: Ye").
    let message = "Revert crates/kalem-tui/src/panels.rs and crates/kalem-ui/src/panels.rs, \
                   staged and not? Its changes are in no commit, and are lost.";
    let mut app = App::with_keymap(None, Config::default(), Caps::full(), &[], Vec::new()).unwrap();
    let mut term = Terminal::new(TestBackend::new(60, 20)).unwrap();
    extensions::ask(41_001, Question::Confirm(message.into()));
    app.tick(Instant::now());
    let shown = screen(&mut term, &mut app);
    let all = shown.join("\n");
    assert!(message.len() > 60, "longer than the pane");
    let alone = |answer: &str| shown.iter().filter(|l| l.trim() == answer).count();
    assert_eq!((alone("Yes"), alone("No")), (1, 1), "{all}");
    // The message once, every word of it: wrapped, not cut.
    let words = shown
        .iter()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(words.matches(message).count(), 1, "{all}");
    // Above what is typed, and the answers under it.
    let row = |what: &str| {
        shown
            .iter()
            .position(|l| l.contains(what))
            .unwrap_or_else(|| panic!("{what} shown: {all}"))
    };
    assert!(row("lost.") < row(">") && row(">") < row("Yes"), "{all}");
    // Enter answers the question: the list goes.
    assert!(extensions::asking());
    app.key(KeyEvent::from(KeyCode::Enter));
    assert!(!extensions::asking(), "answered");
    assert!(!screen(&mut term, &mut app).join("\n").contains("lost."));
}
