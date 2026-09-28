# gpui-rich-text

Rich text lines for editors on [gpui](https://www.gpui.rs), Zed's UI framework.

gpui's `shape_text` wraps text but cannot reserve space for inline boxes, which a rich text editor needs for checkboxes, formulas, images and similar widgets in running text. This crate lays out one line of pieces:

- text with gpui style runs (fonts, colors, backgrounds, underlines, strike-throughs);
- widget boxes of a given size that stand for some bytes of the display text, for the caller to paint;
- superscripts and subscripts, smaller and on a raised or lowered baseline;
- spacers that grow to fill a short row (tags pushed to the right edge, centered text).

It breaks the line into rows at spaces, around widgets and between CJK characters, with a hanging indent for wrapped rows (list items), paints the text, and answers what an editor asks of a line: the caret box for an offset, the offset under the mouse, the rectangles of a selection or a search match, and where each widget is.

```rust
use gpui_rich_text::{InlineLayout, Piece};

let pieces = vec![
    Piece::Text { text: "- [ ] ".into(), runs: vec![run(6)] },
    Piece::Widget { len: 3, size: size(px(14.), px(14.)), ascent: px(12.) },
    Piece::Text { text: " write the report".into(), runs: vec![run(17)] },
];
let layout = InlineLayout::new(&pieces, px(16.), px(23.), Some(width), Some(2), window);
layout.paint(origin, window, cx);
let caret = layout.caret(offset);
let hit = layout.index_for_position(mouse - origin);
```

Offsets are byte offsets into the display text: the concatenated text of the pieces, with each widget counting its `len`. Mapping them to a document's source is the caller's business.

It incubates in the [Kalem](https://github.com/kalem-editor/kalem) repository, which uses it for its Org editor, and will move to its own repository once its API settles and gpui is released with the APIs it uses (see §4.7 of Kalem's design document). Its terminal counterpart is `tui-rich-text`.

License: MIT OR Apache-2.0.
