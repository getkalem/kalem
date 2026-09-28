# tui-rich-text

Rich text lines for terminal editors on [ratatui](https://ratatui.rs).

A document is lines of styled graphemes, each mapped back to a range of the source text. The crate:

- wraps lines into rows at spaces, with hanging indents and tab stops;
- scrolls through lines of any height and keeps the cursor in view;
- moves the cursor up and down at a kept column, across wrapped rows and folded lines;
- draws the cursor, a selection, marked ranges (search matches) and OSC 8 hyperlinks (each cell carries its link, grouped by an `id`, so partial redraws keep it);
- turns mouse positions back into source offsets.

The document implements the `Lines` trait: which lines exist and show, and the glyphs of each. Glyphs that stand for no source (decorations, indentation) have an empty source range; each glyph can carry data of the caller's type, such as the widget it draws.

It incubates in the [Kalem](https://github.com/kalem-editor/kalem) repository, which uses it for its Org editor, and will move to its own repository once its API settles (see §4.7 of Kalem's design document).

License: MIT OR Apache-2.0.
