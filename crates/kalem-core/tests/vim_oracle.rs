//! The Vim layer against Vim itself: every case of `tests/vim/cases.json`
//! (a text and keys in Vim's notation) gives the text and the cursor Vim
//! gave, recorded in `tests/vim/expected.json` by `tools/vim-expected.py`
//! (`vim -Nu NONE`, Vim's own defaults). Keys the layer leaves alone are
//! typed, as the frontends do.

use std::time::Instant;

use kalem_core::vim::{Host, Key, Vim};
use kalem_core::{DocumentMode, DocumentState, LineEnding, Metadata};
use serde_json::Value;

/// A host with Vim's screen in a terminal of 24 rows and 80 columns: 23
/// lines of text.
#[derive(Default)]
struct TestHost {
    clip: Option<String>,
    top: usize,
    lines: usize,
}

const ROWS: usize = 23;

impl Host for TestHost {
    fn clipboard(&mut self) -> Option<String> {
        self.clip.clone()
    }

    fn set_clipboard(&mut self, text: &str) {
        self.clip = Some(text.to_string());
    }

    fn page_lines(&self) -> usize {
        ROWS
    }

    fn visible_lines(&self) -> Option<(usize, usize)> {
        Some((self.top, self.top + ROWS - 1))
    }

    fn scroll(&mut self, by: isize) -> Option<(usize, usize)> {
        let top = (self.top as isize + by).clamp(0, self.lines.saturating_sub(1) as isize);
        self.top = top as usize;
        self.visible_lines()
    }
}

/// `keys` in Vim's notation as keys.
fn parse_keys(keys: &str) -> Vec<Key> {
    let mut out = Vec::new();
    let mut rest = keys;
    while let Some(c) = rest.chars().next() {
        if c == '<'
            && let Some(end) = rest.find('>')
        {
            let name = &rest[1..end];
            let low = name.to_ascii_lowercase();
            let key = match low.as_str() {
                "esc" => Some(Key::Esc),
                "cr" | "enter" => Some(Key::Enter),
                "bs" => Some(Key::Backspace),
                "tab" => Some(Key::Tab),
                "lt" => Some(Key::Char('<')),
                "space" => Some(Key::Char(' ')),
                "nl" => Some(Key::Ctrl('j')),
                // An undo step for Vim's sake (see tools/vim-expected.py).
                "sync" => {
                    rest = &rest[end + 1..];
                    continue;
                }
                _ if low.starts_with("c-") && name.chars().count() == 3 => {
                    Some(Key::Ctrl(low.chars().nth(2).unwrap()))
                }
                _ => None,
            };
            if let Some(k) = key {
                out.push(k);
                rest = &rest[end + 1..];
                continue;
            }
        }
        out.push(Key::Char(c));
        rest = &rest[c.len_utf8()..];
    }
    out
}

/// The text and the cursor (line and byte column, from 1) after `keys`.
fn run(text: &str, keys: &str) -> (String, usize, usize) {
    let meta = Metadata {
        path: None,
        mode: DocumentMode::Text { language: None },
        line_ending: LineEnding::Lf,
        bom: false,
        encoding: encoding_rs::UTF_8,
        lossy: false,
    };
    let mut d = DocumentState::new(text, meta, std::sync::Arc::default());
    let mut v = Vim::new();
    // Vim's own keys: no leader (Kalem's is Space).
    v.leader = None;
    let mut host = TestHost {
        lines: text.lines().count(),
        ..TestHost::default()
    };
    let mut all = parse_keys(keys);
    all.extend([Key::Esc, Key::Esc]);
    for key in all {
        let out = v.key(&mut d, key, &mut host);
        if out.handled {
            continue;
        }
        let now = Instant::now();
        match key {
            Key::Char(c) => d.insert_text(&c.to_string(), now),
            Key::Enter => d.insert_text("\n", now),
            Key::Tab => d.insert_text("\t", now),
            Key::Backspace => {
                let _ = d.delete_backward(now);
            }
            _ => {}
        }
    }
    let t = d.text().as_str().to_string();
    let head = d.selection.head.min(t.len());
    let line_start = t[..head].rfind('\n').map_or(0, |i| i + 1);
    let line = t[..head].matches('\n').count() + 1;
    (t, line, head - line_start + 1)
}

fn root() -> std::path::PathBuf {
    std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/vim"))
}

#[test]
fn agrees_with_vim() {
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(root().join("cases.json")).unwrap()).unwrap();
    let expected: Value =
        serde_json::from_str(&std::fs::read_to_string(root().join("expected.json")).unwrap())
            .unwrap();
    let mut differ = Vec::new();
    for c in &cases {
        let name = c["name"].as_str().unwrap();
        let text = c["text"].as_str().unwrap();
        let keys = c["keys"].as_str().unwrap();
        let e = &expected[name];
        let want = (
            e["text"].as_str().unwrap().to_string(),
            e["line"].as_u64().unwrap() as usize,
            e["col"].as_u64().unwrap() as usize,
        );
        let got = std::panic::catch_unwind(|| run(text, keys));
        match got {
            Ok(got) if got == want => {}
            Ok(got) => differ.push(format!(
                "{name} ({keys}):\n  vim:   {:?} {}:{}\n  kalem: {:?} {}:{}",
                want.0, want.1, want.2, got.0, got.1, got.2
            )),
            Err(_) => differ.push(format!("{name} ({keys}): panicked")),
        }
    }
    if let Ok(out) = std::env::var("KALEM_VIM_REPORT") {
        std::fs::write(out, differ.join("\n")).unwrap();
    }
    // The cases known to differ, until they are done: the list only
    // shrinks (a case that agrees now comes off it).
    let known: Vec<String> = std::fs::read_to_string(root().join("known-differences.txt"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    let names: Vec<&str> = differ
        .iter()
        .map(|d| d.split(' ').next().unwrap_or(""))
        .collect();
    let new: Vec<&String> = differ
        .iter()
        .filter(|d| {
            !known
                .iter()
                .any(|k| d.split(' ').next() == Some(k.as_str()))
        })
        .collect();
    let fixed: Vec<&String> = known
        .iter()
        .filter(|k| !names.contains(&k.as_str()))
        .collect();
    assert!(
        new.is_empty() && fixed.is_empty(),
        "{}/{} cases differ from Vim; not in tests/vim/known-differences.txt:\n{}\nagree now, to take off the list: {fixed:?}",
        differ.len(),
        cases.len(),
        new.iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
}
