//! Terminal capabilities (§7.6): environment hints first, then queries
//! the terminal answers itself: XTVERSION (name and version), DECRQM 2026
//! (synchronized output) and the kitty keyboard protocol, with DA1, which
//! every terminal answers, as the last one.

use std::time::Duration;

/// What the terminal can do.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Caps {
    /// Name and version from XTVERSION, or `TERM_PROGRAM`.
    pub terminal: Option<String>,
    /// 24-bit color.
    pub true_color: bool,
    /// Italic text.
    pub italic: bool,
    /// Struck-through text.
    pub strikethrough: bool,
    /// OSC 8 hyperlinks.
    pub hyperlinks: bool,
    /// Synchronized output (mode 2026), for flicker-free frames.
    pub synchronized_output: bool,
    /// The kitty keyboard protocol, which reports every key chord.
    pub kitty_keyboard: bool,
    /// DA1 attributes (4 means sixel graphics).
    pub da1: Vec<u32>,
    /// A cell's size in pixels (width, height), from `CSI 16 t`.
    pub cell_size: Option<(u16, u16)>,
    /// `NO_COLOR` is set.
    pub no_color: bool,
    /// Only ASCII glyphs (`TERM=linux`, or asked for).
    pub ascii: bool,
    /// Whether the terminal answered the queries.
    pub answered: bool,
    /// How long the queries took to answer (or to give up on).
    pub query_time: std::time::Duration,
    /// The background color, from OSC 11.
    pub background: Option<(u8, u8, u8)>,
    /// The theme's colors, used on true-color terminals instead of the
    /// terminal's palette (set by the application, not detected).
    pub colors: Option<std::sync::Arc<kalem_core::theme::ThemeColors>>,
}

fn env(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

/// Reads stdin until `done` holds or the time is up, or until the terminal,
/// once it has begun to answer, is silent for `idle`: it answers its
/// questions in one burst, and one that leaves the last unanswered (or
/// answered as `done` does not read it) kept the editor waiting for the
/// whole timeout at every start (half a second in iTerm2). Raw mode must
/// be on.
#[cfg(unix)]
fn read_until(timeout: Duration, idle: Duration, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use std::os::fd::AsFd;
    let stdin = std::io::stdin();
    let fd = stdin.as_fd();
    let end = std::time::Instant::now() + timeout;
    let mut out = Vec::new();
    while !done(&out) {
        let left = end.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            break;
        }
        let wait = if out.is_empty() { left } else { left.min(idle) };
        let ts = Timespec {
            tv_sec: wait.as_secs() as _,
            tv_nsec: wait.subsec_nanos() as _,
        };
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        match poll(&mut fds, Some(&ts)) {
            Ok(n) if n > 0 => {}
            _ => break,
        }
        let mut buf = [0u8; 256];
        match rustix::io::read(fd, &mut buf) {
            Ok(n) if n > 0 => out.extend_from_slice(&buf[..n]),
            _ => break,
        }
    }
    out
}

/// The DA1 reply `ESC [ ? a ; b ; … c`, among the replies.
fn da1(reply: &str) -> Option<Vec<u32>> {
    reply.split("\x1b[?").skip(1).find_map(|part| {
        let end = part.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
        (part[end..].starts_with('c') && end > 0).then(|| {
            part[..end]
                .split(';')
                .filter_map(|x| x.parse().ok())
                .collect()
        })
    })
}

/// Reads the replies to [`query`]'s questions.
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
    // `CSI 6 ; height ; width t`.
    c.cell_size = reply.split("\x1b[6;").nth(1).and_then(|r| {
        let end = r.find('t')?;
        let (h, w) = r[..end].split_once(';')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    });
    // `OSC 11 ; rgb:RRRR/GGGG/BBBB ST`, one to four digits a channel.
    c.background = reply.split("\x1b]11;rgb:").nth(1).and_then(|r| {
        let end = r.find(['\x1b', '\x07'])?;
        let mut parts = r[..end].split('/').map(|h| {
            let v = u32::from_str_radix(h, 16).ok()?;
            let max = 16u32.checked_pow(h.len() as u32)? - 1;
            (!h.is_empty() && h.len() <= 4).then(|| (v * 255 / max) as u8)
        });
        Some((parts.next()??, parts.next()??, parts.next()??))
    });
}

/// Fills in what the environment tells.
pub fn from_env(c: &mut Caps) {
    let term = env("TERM").unwrap_or_default();
    c.no_color = env("NO_COLOR").is_some();
    c.ascii = term == "linux" || env("KALEM_ASCII").is_some();
    if c.terminal.is_none() {
        c.terminal = env("TERM_PROGRAM").map(|p| match env("TERM_PROGRAM_VERSION") {
            Some(v) => format!("{p} {v}"),
            None => p,
        });
    }
    let name = c.terminal.clone().unwrap_or_default().to_lowercase();
    let known = |list: &[&str]| list.iter().any(|k| name.contains(k) || term.contains(k));
    c.true_color = matches!(env("COLORTERM").as_deref(), Some("truecolor" | "24bit"))
        || known(&[
            "iterm",
            "kitty",
            "wezterm",
            "ghostty",
            "alacritty",
            "foot",
            "konsole",
            "vscode",
            "xterm.js",
            "windows terminal",
            "contour",
            "rio",
        ])
        || env("WT_SESSION").is_some();
    // The Linux console and old terminals lack italics; terminals that
    // answer XTVERSION have them.
    c.italic = term != "linux"
        && (c.terminal.is_some()
            || term.contains("256color")
            || known(&["kitty", "alacritty"])
            || cfg!(windows));
    c.strikethrough = c.italic;
    // OSC 8 cannot be asked for; use the known list.
    c.hyperlinks = known(&[
        "iterm",
        "kitty",
        "wezterm",
        "ghostty",
        "alacritty",
        "foot",
        "konsole",
        "vte",
        "gnome",
        "vscode",
        "xterm.js",
        "windows terminal",
        "contour",
        "rio",
        "tmux",
    ]) || env("WT_SESSION").is_some()
        || env("VTE_VERSION")
            .and_then(|v| v.parse::<u32>().ok())
            .is_some_and(|v| v >= 5000);
}

/// Queries the terminal; raw mode must be on. Without an answer within
/// `timeout` (a pipe, an old terminal) only the environment counts.
pub fn query(timeout: Duration) -> Caps {
    let mut c = Caps::default();
    #[cfg(unix)]
    {
        use std::io::Write;
        let mut out = std::io::stdout();
        // XTVERSION, DECRQM 2026, kitty keyboard flags, cell size, the
        // background color, then DA1.
        let _ = out.write_all(b"\x1b[>0q\x1b[?2026$p\x1b[?u\x1b[16t\x1b]11;?\x1b\\\x1b[c");
        let _ = out.flush();
        let start = std::time::Instant::now();
        let bytes = read_until(timeout, Duration::from_millis(100), |b| {
            da1(&String::from_utf8_lossy(b)).is_some()
        });
        c.query_time = start.elapsed();
        parse_reply(&String::from_utf8_lossy(&bytes), &mut c);
    }
    #[cfg(not(unix))]
    let _ = timeout;
    from_env(&mut c);
    c
}

/// The terminal image protocols Kalem uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Graphics {
    /// kitty's graphics protocol (kitty, WezTerm, Ghostty, Konsole).
    Kitty,
    /// iTerm2's inline images.
    Iterm2,
    /// Sixel (DA1 attribute 4).
    Sixel,
}

impl Caps {
    /// The image protocol to use, if any. Not inside tmux, which needs
    /// passthrough.
    pub fn graphics(&self) -> Option<Graphics> {
        if std::env::var_os("TMUX").is_some() {
            return None;
        }
        let name = self.terminal.clone().unwrap_or_default().to_lowercase();
        let term = env("TERM").unwrap_or_default();
        let has = |k: &str| name.contains(k) || term.contains(k);
        if ["kitty", "wezterm", "ghostty", "konsole"]
            .iter()
            .any(|k| has(k))
        {
            Some(Graphics::Kitty)
        } else if has("iterm") || env("TERM_PROGRAM").is_some_and(|p| p == "iTerm.app") {
            Some(Graphics::Iterm2)
        } else if self.da1.contains(&4) {
            Some(Graphics::Sixel)
        } else {
            None
        }
    }

    /// Whether the terminal's background is dark, if it said.
    pub fn dark_background(&self) -> Option<bool> {
        self.background
            .map(|(r, g, b)| 299 * u32::from(r) + 587 * u32::from(g) + 114 * u32::from(b) < 128_000)
    }

    /// Capabilities for tests: everything but graphics.
    pub fn full() -> Caps {
        Caps {
            terminal: Some("test".into()),
            true_color: true,
            italic: true,
            strikethrough: true,
            hyperlinks: true,
            ..Caps::default()
        }
    }

    /// A report for `kalem tui --detect` and bug reports.
    pub fn report(&self) -> String {
        let yes = |b: bool| if b { "yes" } else { "no" };
        [
            format!(
                "terminal            {}",
                self.terminal.as_deref().unwrap_or("unknown")
            ),
            format!(
                "answered queries    {} ({} ms)",
                yes(self.answered),
                self.query_time.as_millis()
            ),
            format!("true color          {}", yes(self.true_color)),
            format!("italic              {}", yes(self.italic)),
            format!("strike-through      {}", yes(self.strikethrough)),
            format!("OSC 8 hyperlinks    {}", yes(self.hyperlinks)),
            format!("synchronized output {}", yes(self.synchronized_output)),
            format!("kitty keyboard      {}", yes(self.kitty_keyboard)),
            format!("DA1                 {:?} (4 = sixel)", self.da1),
            format!(
                "background          {}",
                match (self.background, self.dark_background()) {
                    (Some((r, g, b)), Some(dark)) => format!(
                        "#{r:02x}{g:02x}{b:02x} ({})",
                        if dark { "dark" } else { "light" }
                    ),
                    _ => "unknown".into(),
                }
            ),
            format!("cell size           {:?}", self.cell_size),
            format!("graphics            {:?}", self.graphics()),
            format!("NO_COLOR            {}", yes(self.no_color)),
            format!("ASCII only          {}", yes(self.ascii)),
        ]
        .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_replies() {
        // kitty: XTVERSION, synchronized output, keyboard flags, DA1.
        let mut c = Caps::default();
        parse_reply(
            "\x1bP>|kitty(0.36.4)\x1b\\\x1b[?2026;2$y\x1b[?0u\x1b[6;20;10t\x1b[?62;c",
            &mut c,
        );
        assert_eq!(c.terminal.as_deref(), Some("kitty(0.36.4)"));
        assert_eq!(c.cell_size, Some((10, 20)));
        assert!(c.synchronized_output && c.kitty_keyboard);
        assert_eq!(c.da1, vec![62]);
        assert_eq!(c.dark_background(), None);
        // The background: four hex digits a channel, or two, ended by ST or BEL.
        let mut c = Caps::default();
        parse_reply("\x1b]11;rgb:1e1e/1e1e/2e2e\x1b\\\x1b[?62;c", &mut c);
        assert_eq!(c.background, Some((0x1e, 0x1e, 0x2e)));
        assert_eq!(c.dark_background(), Some(true));
        parse_reply("\x1b]11;rgb:ff/fa/f0\x07\x1b[?62;c", &mut c);
        assert_eq!(c.dark_background(), Some(false));
        // xterm with sixel: DA1 lists 4.
        let mut c = Caps::default();
        parse_reply("\x1b[?2026;0$y\x1b[?63;1;2;4;6;9;15;22c", &mut c);
        assert!(!c.synchronized_output && !c.kitty_keyboard);
        assert!(c.da1.contains(&4));
        let mut c = Caps::default();
        parse_reply("", &mut c);
        assert!(!c.answered);
    }
}
