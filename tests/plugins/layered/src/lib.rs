//! An extension plugin for tests (API 0.2.10): its layer `ids` hides every
//! `id::` line, shows `((x))` as `X` and styles `#tags`; its command
//! `layered.now` answers the clock's time and its offset (API 0.2.11) and
//! tells the editor its layer changed. Its completer `refs` (API 0.2.11) offers `ab` and `cd` after
//! an open `((`, with the closing `))`. By `layer-2` (API 0.2.13) its
//! layer draws a line `embedded …` under each line with `{{embed …}}`.

use kalem_plugin::completer::{self, Item, ItemKind, Request};
use kalem_plugin::kalem::{self, Plugin, Scope};
use kalem_plugin::layer::{
    Below, LineEffect, Lines, OverlaySet, OverlaySet2, Piece, Replacement, Span, SpanEffect,
    SpanStyle, VirtualLine,
};

struct Layered;

impl Plugin for Layered {
    fn activate() -> Result<(), String> {
        kalem::command(kalem::spec("layered.now", "Now", Scope::all()), |_| {
            kalem_plugin::layer::refresh();
            let random = kalem_plugin::clock::random();
            let now = kalem_plugin::clock::now();
            Ok(format!(
                "{{\"now\":{},\"zone\":\"{}\",\"random\":{},\"offset\":{}}}",
                now,
                kalem_plugin::clock::timezone(),
                u64::from(random != 0),
                kalem_plugin::clock::offset(now)
            ))
        })?;
        Ok(())
    }

    fn overlays(layer: &str, path: Option<&str>, text: &str) -> OverlaySet {
        let mut set = OverlaySet {
            spans: Vec::new(),
            lines: Vec::new(),
        };
        if layer != "ids" || path.is_none() {
            return set;
        }
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            if line.trim_start().starts_with("id::") {
                set.lines.push(Lines {
                    start: at as u64,
                    end: (at + line.len()) as u64,
                    effect: LineEffect::Hidden,
                });
            }
            let mut from = 0;
            while let Some(p) = line[from..].find("((") {
                let s = from + p;
                let Some(q) = line[s..].find("))") else { break };
                let inner = &line[s + 2..s + q];
                set.spans.push(Span {
                    start: (at + s) as u64,
                    end: (at + s + q + 2) as u64,
                    effect: SpanEffect::Replace(Replacement {
                        text: inner.to_uppercase(),
                        style: SpanStyle::LINK,
                    }),
                });
                from = s + q + 2;
            }
            if let Some(p) = line.find(" #") {
                let s = p + 1;
                let e = line[s..].find(char::is_whitespace).map_or(line.len(), |x| s + x);
                set.spans.push(Span {
                    start: (at + s) as u64,
                    end: (at + e) as u64,
                    effect: SpanEffect::Style(SpanStyle::TAG),
                });
            }
            at += line.len();
        }
        set.spans.sort_by_key(|s| s.start);
        set
    }

    fn overlays_2(layer: &str, path: Option<&str>, text: &str) -> OverlaySet2 {
        let o = Self::overlays(layer, path, text);
        let mut below = Vec::new();
        if layer == "ids" && path.is_some() {
            let mut at = 0;
            for line in text.split_inclusive('\n') {
                if let Some(p) = line.find("{{embed ") {
                    let what = line[p + 8..].split("}}").next().unwrap_or("");
                    below.push(Below {
                        at: (at + p) as u64,
                        lines: vec![VirtualLine {
                            indent: 2,
                            pieces: vec![
                                Piece {
                                    text: "embedded ".into(),
                                    style: SpanStyle::DIM,
                                },
                                Piece {
                                    text: what.into(),
                                    style: SpanStyle::LINK,
                                },
                            ],
                        }],
                    });
                }
                at += line.len();
            }
        }
        OverlaySet2 {
            spans: o.spans,
            lines: o.lines,
            below,
        }
    }

    fn complete(id: &str, request: &Request) -> Vec<Item> {
        if id != "refs" {
            return Vec::new();
        }
        let at = (request.cursor - request.base) as usize;
        let before = &request.text[..at];
        let line = &before[before.rfind('\n').map_or(0, |i| i + 1)..];
        let Some(open) = line.rfind("((") else {
            return Vec::new();
        };
        let typed = &line[open + 2..];
        if typed.contains("))") {
            return Vec::new();
        }
        let start = request.cursor - typed.len() as u64;
        ["ab", "cd"]
            .iter()
            .filter(|r| r.starts_with(typed))
            .map(|r| {
                let mut item = completer::item(r, &format!("{r}))"), start, ItemKind::Link);
                item.detail = "a block".into();
                item
            })
            .collect()
    }
}

kalem_plugin::export_plugin!(Layered);
