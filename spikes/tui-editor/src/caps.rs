//! Terminal capability detection (T0.8.2).
//!
//! Environment hints first, then queries answered by the terminal itself:
//! XTVERSION (name and version), DECRQM 2026 (synchronized output) and the
//! kitty keyboard protocol, with DA1 as the sentinel every terminal answers.
//! Graphics protocols and the cell size come from ratatui-image's own query.

use std::io::Write;
use std::time::{Duration, Instant};

#[derive(Debug, Default, Clone)]
pub struct Caps {
    /// Name and version from XTVERSION, or `TERM_PROGRAM`.
    pub terminal: Option<String>,
    pub true_color: bool,
    pub italic: bool,
    pub strikethrough: bool,
    pub hyperlinks: bool,
    pub synchronized_output: bool,
    pub kitty_keyboard: bool,
    /// DA1 attributes, for example 4 for sixel.
    pub da1: Vec<u32>,
    pub graphics: String,
    pub cell_size: Option<(u16, u16)>,
    pub no_color: bool,
    /// Whether the terminal answered the queries at all.
    pub answered: bool,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// Reads bytes from stdin until `done` returns true or the timeout passes.
#[allow(unsafe_code)]
fn read_until(timeout: Duration, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let mut out = Vec::new();
    let end = Instant::now() + timeout;
    while Instant::now() < end && !done(&out) {
        let left = (end - Instant::now()).as_millis() as i32;
        let mut pfd = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        // SAFETY: one valid pollfd; stdin stays open.
        let n = unsafe { libc::poll(&mut pfd, 1, left.max(1)) };
        if n <= 0 {
            break;
        }
        let mut buf = [0u8; 256];
        // SAFETY: the buffer is valid for its length.
        let r = unsafe { libc::read(0, buf.as_mut_ptr().cast(), buf.len()) };
        if r <= 0 {
            break;
        }
        out.extend_from_slice(&buf[..r as usize]);
    }
    out
}

/// The DA1 reply `ESC [ ? a ; b ; ... c`, if present among the replies.
fn da1(reply: &str) -> Option<Vec<u32>> {
    reply.split("\x1b[?").skip(1).find_map(|part| {
        let end = part.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
        (part[end..].starts_with('c') && end > 0).then(|| part[..end].split(';').filter_map(|x| x.parse().ok()).collect())
    })
}

/// Reads the terminal's replies to the queries sent by [`query`].
pub fn parse_reply(reply: &str, c: &mut Caps) {
    c.answered = !reply.is_empty();
    if let Some(i) = reply.find("\x1bP>|") {
        let rest = &reply[i + 4..];
        let end = rest.find("\x1b\\").unwrap_or(rest.len());
        c.terminal = Some(rest[..end].to_string());
    }
    c.synchronized_output = reply.contains("\x1b[?2026;1$y") || reply.contains("\x1b[?2026;2$y");
    c.kitty_keyboard = reply.split("\x1b[?").any(|p| {
        let end = p.find('u');
        end.is_some_and(|e| e > 0 && p[..e].bytes().all(|b| b.is_ascii_digit()))
    });
    c.da1 = da1(reply).unwrap_or_default();
}

/// Queries the terminal. Raw mode must be enabled by the caller.
pub fn query() -> Caps {
    let mut c = Caps::default();
    let term = env("TERM").unwrap_or_default();
    let program = env("TERM_PROGRAM");
    c.no_color = env("NO_COLOR").is_some();

    let mut out = std::io::stdout();
    // XTVERSION, DECRQM 2026, kitty keyboard flags, then DA1 as the sentinel.
    let _ = out.write_all(b"\x1b[>0q\x1b[?2026$p\x1b[?u\x1b[c");
    let _ = out.flush();
    let bytes = read_until(Duration::from_millis(1000), |b| da1(&String::from_utf8_lossy(b)).is_some());
    let reply = String::from_utf8_lossy(&bytes).into_owned();
    parse_reply(&reply, &mut c);
    if c.terminal.is_none() {
        c.terminal = program.clone().map(|p| match env("TERM_PROGRAM_VERSION") {
            Some(v) => format!("{p} {v}"),
            None => p,
        });
    }
    let name = c.terminal.clone().unwrap_or_default().to_lowercase();
    let known = |list: &[&str]| list.iter().any(|k| name.contains(k) || term.contains(k));

    c.true_color = matches!(env("COLORTERM").as_deref(), Some("truecolor" | "24bit"))
        || known(&["iterm", "kitty", "wezterm", "ghostty", "alacritty", "foot", "konsole", "vscode", "xterm.js", "windows terminal", "contour", "rio"]);
    // The Linux console and old terminals lack italics; everything that
    // answers XTVERSION has them.
    c.italic = term != "linux" && (c.terminal.is_some() || term.contains("256color") || known(&["kitty", "alacritty"]));
    c.strikethrough = c.italic;
    // OSC 8 cannot be queried; use the known list.
    c.hyperlinks = known(&[
        "iterm", "kitty", "wezterm", "ghostty", "alacritty", "foot", "konsole", "vte", "gnome", "vscode", "xterm.js",
        "windows terminal", "contour", "rio", "tmux",
    ]) || env("WT_SESSION").is_some()
        || env("VTE_VERSION").and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v >= 5000);
    c
}

impl Caps {
    pub fn report(&self) -> String {
        let yes = |b: bool| if b { "yes" } else { "no" };
        [
            format!("terminal            {}", self.terminal.as_deref().unwrap_or("unknown")),
            format!("answered queries    {}", yes(self.answered)),
            format!("true color          {}", yes(self.true_color)),
            format!("italic              {}", yes(self.italic)),
            format!("strike-through      {}", yes(self.strikethrough)),
            format!("OSC 8 hyperlinks    {}", yes(self.hyperlinks)),
            format!("synchronized output {}", yes(self.synchronized_output)),
            format!("kitty keyboard      {}", yes(self.kitty_keyboard)),
            format!("DA1                 {:?} (4 = sixel)", self.da1),
            format!("graphics            {}", self.graphics),
            format!("cell size           {:?}", self.cell_size),
            format!("NO_COLOR            {}", yes(self.no_color)),
        ]
        .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_replies() {
        // kitty 0.36: XTVERSION, synchronized output, keyboard flags, DA1.
        let mut c = Caps::default();
        parse_reply("\x1bP>|kitty(0.36.4)\x1b\\\x1b[?2026;2$y\x1b[?0u\x1b[?62;c", &mut c);
        assert_eq!(c.terminal.as_deref(), Some("kitty(0.36.4)"));
        assert!(c.synchronized_output && c.kitty_keyboard);
        assert_eq!(c.da1, vec![62]);
        // xterm with sixel: no XTVERSION reply here, DA1 lists 4.
        let mut c = Caps::default();
        parse_reply("\x1b[?2026;0$y\x1b[?63;1;2;4;6;9;15;22c", &mut c);
        assert!(!c.synchronized_output && !c.kitty_keyboard);
        assert!(c.da1.contains(&4));
        // No answer at all.
        let mut c = Caps::default();
        parse_reply("", &mut c);
        assert!(!c.answered);
    }
}
