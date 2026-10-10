//! An extension plugin for tests (API 0.2.10): its layer `ids` hides every
//! `id::` line, shows `((x))` as `X` and styles `#tags`; its command
//! `layered.now` answers the clock's time and tells the editor its layer
//! changed.

use kalem_plugin::kalem::{self, Plugin, Scope};
use kalem_plugin::layer::{LineEffect, Lines, OverlaySet, Replacement, Span, SpanEffect, SpanStyle};

struct Layered;

impl Plugin for Layered {
    fn activate() -> Result<(), String> {
        kalem::command(kalem::spec("layered.now", "Now", Scope::all()), |_| {
            kalem_plugin::layer::refresh();
            let random = kalem_plugin::clock::random();
            Ok(format!(
                "{{\"now\":{},\"zone\":\"{}\",\"random\":{}}}",
                kalem_plugin::clock::now(),
                kalem_plugin::clock::timezone(),
                u64::from(random != 0)
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
}

kalem_plugin::export_plugin!(Layered);
