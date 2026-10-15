//! An extension plugin for tests (API 0.2.10): its layer `ids` hides every
//! `id::` line, shows `((x))` as `X` and styles `#tags`; its command
//! `layered.now` answers the clock's time and its offset (API 0.2.11) and
//! tells the editor its layer changed. Its completer `refs` (API 0.2.11) offers `ab` and `cd` after
//! an open `((`, with the closing `))`. By `layer-2` (API 0.2.13) its
//! layer draws a line `embedded …` under each line with `{{embed …}}`,
//! which by `layer-3` (API 0.2.15) shows the text after `{{embed `, to
//! be typed into. Its formatter `lines` (API 0.2.16) takes the blanks off
//! the ends of lines, and refuses a tab, at its place. Its command
//! `layered.later` asks for `layered.now` later (API 0.2.17), and for a
//! command not its own, refused. Its outline `hashes` (API 0.2.18)
//! takes the lines that start with `#`s and a blank as headings, a level
//! a `#`. By `layer-4` (API 0.2.18) the text an embed's line shows starts
//! a block of the note's outline.

use kalem_plugin::completer::{self, Item, ItemKind, Request};
use kalem_plugin::kalem::{self, Plugin, Scope};
use kalem_plugin::layer::{
    Below, Below3, Below4, LineEffect, Lines, Outline, OverlaySet, OverlaySet2, OverlaySet3,
    OverlaySet4, Piece, Replacement, Source, Source4, Span, SpanEffect, SpanStyle, VirtualLine,
    VirtualLine3, VirtualLine4,
};

struct Layered;

impl Plugin for Layered {
    fn activate() -> Result<(), String> {
        kalem::command(kalem::spec("layered.now", "Now", Scope::all()), |_| {
            kalem_plugin::layer::refresh();
            let random = kalem_plugin::clock::random();
            let now = kalem_plugin::clock::now();
            // What it keeps between sessions (API 0.2.17): its visits.
            let visits = kalem_plugin::state::get("visits")
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(0)
                + 1;
            kalem_plugin::state::set("visits", Some(&visits.to_string()))?;
            Ok(format!(
                "{{\"now\":{},\"zone\":\"{}\",\"random\":{},\"offset\":{},\"visits\":{}}}",
                now,
                kalem_plugin::clock::timezone(),
                u64::from(random != 0),
                kalem_plugin::clock::offset(now),
                visits
            ))
        })?;
        // Its own command run later (API 0.2.17), another's refused; then
        // dropped.
        kalem::command(kalem::spec("layered.later", "Later", Scope::all()), |_| {
            let run = kalem_plugin::later::run("layered.now", "null", 5)?;
            let other = kalem_plugin::later::run("file.save", "null", 0).unwrap_err();
            kalem_plugin::later::cancel(run);
            Ok(format!("{{\"run\":{run},\"other\":{other:?}}}"))
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
                let e = line[s..]
                    .find(char::is_whitespace)
                    .map_or(line.len(), |x| s + x);
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

    fn overlays_3(layer: &str, path: Option<&str>, text: &str) -> OverlaySet3 {
        let o = Self::overlays_2(layer, path, text);
        // Each embed's line shows the text after `{{embed ` on its line.
        let starts: Vec<usize> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let below = o
            .below
            .into_iter()
            .map(|b| {
                let at = b.at as usize;
                let line = starts.partition_point(|s| *s <= at) - 1;
                let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
                let from = at + "{{embed ".len();
                Below3 {
                    at: b.at,
                    lines: b
                        .lines
                        .into_iter()
                        .map(|l: VirtualLine| VirtualLine3 {
                            indent: l.indent,
                            pieces: l.pieces,
                            source: path.map(|p| Source {
                                path: p.to_string(),
                                line: line as u32,
                                column: (from - starts[line]) as u32,
                                text: text[from..end].to_string(),
                            }),
                        })
                        .collect(),
                }
            })
            .collect();
        OverlaySet3 {
            spans: o.spans,
            lines: o.lines,
            below,
        }
    }

    fn overlays_4(layer: &str, path: Option<&str>, text: &str) -> OverlaySet4 {
        let o = Self::overlays_3(layer, path, text);
        // The embed's line a block of its own, under nothing, before
        // nothing: Tab and Shift+Tab leave it.
        let below = o
            .below
            .into_iter()
            .map(|b| Below4 {
                at: b.at,
                lines: b
                    .lines
                    .into_iter()
                    .map(|l| VirtualLine4 {
                        indent: l.indent,
                        pieces: l.pieces,
                        source: l.source.map(|s| Source4 {
                            outline: Some(Outline {
                                end: s.line + 1,
                                bullet: "- ".into(),
                                step: "\t".into(),
                                indent: false,
                                outdent: false,
                            }),
                            path: s.path,
                            line: s.line,
                            column: s.column,
                            text: s.text,
                        }),
                    })
                    .collect(),
            })
            .collect();
        OverlaySet4 {
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

    fn format(
        id: &str,
        request: &kalem_plugin::formatter::Request,
    ) -> Result<String, kalem_plugin::formatter::Refusal> {
        use kalem_plugin::formatter::refusal;
        if id != "lines" {
            return Err(refusal("no such formatter", None));
        }
        if let Some(at) = request.text.find('\t') {
            return Err(refusal("a tab", Some(at)));
        }
        Ok(request
            .text
            .split_inclusive('\n')
            .map(|l| {
                let feed = if l.ends_with('\n') { "\n" } else { "" };
                format!("{}{feed}", l.trim_end())
            })
            .collect())
    }

    fn outline(
        id: &str,
        request: &kalem_plugin::outline::Request,
    ) -> Vec<kalem_plugin::outline::Heading> {
        if id != "hashes" {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut at = 0;
        for line in request.text.split_inclusive('\n') {
            let level = line.bytes().take_while(|&b| b == b'#').count();
            if level > 0 && line[level..].starts_with(' ') {
                let title = line[level..].trim();
                out.push(kalem_plugin::outline::heading(level as u32, title, at));
            }
            at += line.len();
        }
        out
    }
}

kalem_plugin::export_plugin!(Layered);
