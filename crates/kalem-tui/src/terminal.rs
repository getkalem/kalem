//! Taking over the terminal and giving it back, also after a panic.

use std::io::{self, Stdout, Write};

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::caps::Caps;

/// The terminal in use; dropping it restores the terminal.
#[derive(Debug)]
pub struct Session {
    keyboard: bool,
}

/// Puts the terminal back: keyboard mode, mouse, paste, screen, raw mode.
fn restore(keyboard: bool) -> io::Result<()> {
    let mut out = io::stdout();
    if keyboard {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        out,
        DisableFocusChange,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::SetCursorStyle::DefaultUserShape,
        crossterm::cursor::Show
    );
    terminal::disable_raw_mode()?;
    out.flush()
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = restore(self.keyboard);
    }
}

/// Raw mode on, for the capability queries.
pub fn raw_mode() -> io::Result<()> {
    terminal::enable_raw_mode()
}

/// Takes over the terminal: alternate screen, mouse, bracketed paste,
/// focus events, and the kitty keyboard protocol where there is one. A
/// panic restores the terminal before it is reported.
pub fn start(caps: &Caps) -> io::Result<(Terminal<CrosstermBackend<Stdout>>, Session)> {
    terminal::enable_raw_mode()?;
    let mut out = io::stdout();
    execute!(
        out,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        EnableFocusChange
    )?;
    let keyboard = caps.kitty_keyboard;
    if keyboard {
        execute!(
            out,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        )?;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore(keyboard);
        previous(info);
    }));
    let terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    Ok((terminal, Session { keyboard }))
}
