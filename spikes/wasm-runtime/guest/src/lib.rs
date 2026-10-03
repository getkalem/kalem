//! The spike's guest: a Markdown parser as a component.

wit_bindgen::generate!({ world: "parser", path: "../wit" });

use pulldown_cmark::{Event, Options, Parser, Tag};

struct Component;

fn kind(t: &Tag<'_>) -> u8 {
    match t {
        Tag::Paragraph => 1,
        Tag::Heading { .. } => 2,
        Tag::BlockQuote(_) => 3,
        Tag::CodeBlock(_) => 4,
        Tag::List(_) => 5,
        Tag::Item => 6,
        Tag::Emphasis => 7,
        Tag::Strong => 8,
        Tag::Link { .. } => 9,
        Tag::Image { .. } => 10,
        Tag::Table(_) => 11,
        _ => 0,
    }
}

impl Guest for Component {
    fn parse(text: String) -> Vec<Span> {
        Parser::new_ext(&text, Options::all())
            .into_offset_iter()
            .filter_map(|(ev, r)| match ev {
                Event::Start(t) => Some(Span {
                    start: r.start as u32,
                    end: r.end as u32,
                    kind: kind(&t),
                }),
                _ => None,
            })
            .collect()
    }

    fn spin(n: u64) -> u64 {
        let mut x = 0u64;
        for i in 0..n {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(i);
        }
        x
    }

    fn grow(mb: u32) -> u32 {
        let v: Vec<Vec<u8>> = (0..mb).map(|i| vec![i as u8; 1 << 20]).collect();
        v.iter().map(|b| b[0] as u32).sum::<u32>() + v.len() as u32
    }
}

export!(Component);
