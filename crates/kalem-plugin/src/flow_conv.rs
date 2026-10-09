// The flow's types between the Rust contract (`kv`, `kalem-viewer`) and
// the WIT bindings (`f`, the `flow` interface), both ways. Included after
// `annotations_conv.rs` (its `Cross` and `cases!`) by `kalem-plugin`'s
// adapter and by `kalem-script`'s host; each includer defines `kv` and `f`
// before including.

impl Cross<f::Rgb> for [u8; 3] {
    fn cross(self) -> f::Rgb {
        let [r, g, b] = self;
        f::Rgb { r, g, b }
    }
}

impl Cross<[u8; 3]> for f::Rgb {
    fn cross(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
}

impl Cross<f::FlowLayout> for kv::FlowLayout {
    fn cross(self) -> f::FlowLayout {
        f::FlowLayout {
            items: self.items,
            version: self.version,
            editable: self.editable,
        }
    }
}

impl Cross<kv::FlowLayout> for f::FlowLayout {
    fn cross(self) -> kv::FlowLayout {
        kv::FlowLayout {
            items: self.items,
            version: self.version,
            editable: self.editable,
        }
    }
}

cases!(
    kv::FlowRole,
    f::Role,
    Body,
    Title,
    Subtitle,
    Heading,
    ListItem,
    Quote,
    Code,
    Caption
);
cases!(kv::FlowAlign, f::Align, Start, Center, End, Justify);
cases!(kv::Script, f::Script, Baseline, Superscript, Subscript);
cases!(kv::AsideKind, f::AsideKind, Header, Footer, Footnote, Endnote, Frame, Sidebar);
cases!(kv::FlowStyleKind, f::StyleKind, Paragraph, Character);
impl Cross<f::Marks> for kv::Marks {
    fn cross(self) -> f::Marks {
        f::Marks {
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strike: self.strike,
            double_strike: self.double_strike,
            caps: self.caps,
            small_caps: self.small_caps,
            hidden: self.hidden,
            script: self.script.cross(),
            color: self.color.cross(),
            highlight: self.highlight.cross(),
            size: self.size,
            face: self.face,
        }
    }
}

impl Cross<kv::Marks> for f::Marks {
    fn cross(self) -> kv::Marks {
        kv::Marks {
            bold: self.bold,
            italic: self.italic,
            underline: self.underline,
            strike: self.strike,
            double_strike: self.double_strike,
            caps: self.caps,
            small_caps: self.small_caps,
            hidden: self.hidden,
            script: self.script.cross(),
            color: self.color.cross(),
            highlight: self.highlight.cross(),
            size: self.size,
            face: self.face,
        }
    }
}

impl Cross<f::Picture> for kv::FlowPicture {
    fn cross(self) -> f::Picture {
        f::Picture {
            id: self.id,
            width: self.width,
            height: self.height,
            alt: self.alt,
        }
    }
}

impl Cross<kv::FlowPicture> for f::Picture {
    fn cross(self) -> kv::FlowPicture {
        kv::FlowPicture {
            id: self.id,
            width: self.width,
            height: self.height,
            alt: self.alt,
        }
    }
}

impl Cross<f::Piece> for kv::Piece {
    fn cross(self) -> f::Piece {
        match self {
            kv::Piece::Text => f::Piece::Text,
            kv::Piece::Tab => f::Piece::Tab,
            kv::Piece::LineBreak => f::Piece::LineBreak,
            kv::Piece::PageBreak => f::Piece::PageBreak,
            kv::Piece::ColumnBreak => f::Piece::ColumnBreak,
            kv::Piece::NoteMark(id) => f::Piece::NoteMark(id),
            kv::Piece::Picture(p) => f::Piece::Picture(p.cross()),
            kv::Piece::Placeholder => f::Piece::Placeholder,
        }
    }
}

impl Cross<kv::Piece> for f::Piece {
    fn cross(self) -> kv::Piece {
        match self {
            f::Piece::Text => kv::Piece::Text,
            f::Piece::Tab => kv::Piece::Tab,
            f::Piece::LineBreak => kv::Piece::LineBreak,
            f::Piece::PageBreak => kv::Piece::PageBreak,
            f::Piece::ColumnBreak => kv::Piece::ColumnBreak,
            f::Piece::NoteMark(id) => kv::Piece::NoteMark(id),
            f::Piece::Picture(p) => kv::Piece::Picture(p.cross()),
            f::Piece::Placeholder => kv::Piece::Placeholder,
        }
    }
}

impl Cross<f::Run> for kv::FlowRun {
    fn cross(self) -> f::Run {
        f::Run {
            text: self.text,
            piece: self.piece.cross(),
            marks: self.marks.cross(),
            source: (self.source.start, self.source.end),
            link: self.link,
            annotations: self.annotations,
            locked: self.locked,
        }
    }
}

impl Cross<kv::FlowRun> for f::Run {
    fn cross(self) -> kv::FlowRun {
        kv::FlowRun {
            text: self.text,
            piece: self.piece.cross(),
            marks: self.marks.cross(),
            source: self.source.0..self.source.1,
            link: self.link,
            annotations: self.annotations,
            locked: self.locked,
        }
    }
}

impl Cross<f::Paragraph> for kv::FlowParagraph {
    fn cross(self) -> f::Paragraph {
        f::Paragraph {
            index: self.index,
            role: self.role.cross(),
            level: self.level,
            style: self.style,
            label: self.label.map(|(t, m)| (t, m.cross())),
            align: self.align.cross(),
            indent: self.indent,
            spacing: self.spacing,
            background: self.background.cross(),
            text: self.text,
            runs: self.runs.cross(),
            annotations: self.annotations,
        }
    }
}

impl Cross<kv::FlowParagraph> for f::Paragraph {
    fn cross(self) -> kv::FlowParagraph {
        kv::FlowParagraph {
            index: self.index,
            role: self.role.cross(),
            level: self.level,
            style: self.style,
            label: self.label.map(|(t, m)| (t, m.cross())),
            align: self.align.cross(),
            indent: self.indent,
            spacing: self.spacing,
            background: self.background.cross(),
            text: self.text,
            runs: self.runs.cross(),
            annotations: self.annotations,
        }
    }
}

impl Cross<f::Border> for kv::FlowBorder {
    fn cross(self) -> f::Border {
        f::Border {
            color: self.color.cross(),
            width: self.width,
            style: self.style,
        }
    }
}

impl Cross<kv::FlowBorder> for f::Border {
    fn cross(self) -> kv::FlowBorder {
        kv::FlowBorder {
            color: self.color.cross(),
            width: self.width,
            style: self.style,
        }
    }
}

impl Cross<f::Cell> for kv::FlowCell {
    fn cross(self) -> f::Cell {
        let [t, s, b, e] = self.borders;
        f::Cell {
            columns: self.columns,
            merged: self.merged,
            background: self.background.cross(),
            borders: (t.cross(), s.cross(), b.cross(), e.cross()),
        }
    }
}

impl Cross<kv::FlowCell> for f::Cell {
    fn cross(self) -> kv::FlowCell {
        let (t, s, b, e) = self.borders;
        kv::FlowCell {
            columns: self.columns,
            merged: self.merged,
            background: self.background.cross(),
            borders: [t.cross(), s.cross(), b.cross(), e.cross()],
        }
    }
}

impl Cross<f::Aside> for kv::Aside {
    fn cross(self) -> f::Aside {
        f::Aside {
            kind: self.kind.cross(),
            id: self.id,
            label: self.label,
        }
    }
}

impl Cross<kv::Aside> for f::Aside {
    fn cross(self) -> kv::Aside {
        kv::Aside {
            kind: self.kind.cross(),
            id: self.id,
            label: self.label,
        }
    }
}

impl Cross<f::Item> for kv::FlowItem {
    fn cross(self) -> f::Item {
        match self {
            kv::FlowItem::Paragraph(p) => f::Item::Paragraph(p.cross()),
            kv::FlowItem::TableStart(t) => f::Item::TableStart(f::Table {
                columns: t.columns,
                style: t.style,
            }),
            kv::FlowItem::RowStart(r) => f::Item::RowStart(f::Row { header: r.header }),
            kv::FlowItem::CellStart(c) => f::Item::CellStart(c.cross()),
            kv::FlowItem::CellEnd => f::Item::CellEnd,
            kv::FlowItem::RowEnd => f::Item::RowEnd,
            kv::FlowItem::TableEnd => f::Item::TableEnd,
            kv::FlowItem::AsideStart(a) => f::Item::AsideStart(a.cross()),
            kv::FlowItem::AsideEnd => f::Item::AsideEnd,
            kv::FlowItem::Rule(r) => f::Item::Rule(r),
            kv::FlowItem::Placeholder(p) => f::Item::Placeholder(p),
        }
    }
}

impl Cross<kv::FlowItem> for f::Item {
    fn cross(self) -> kv::FlowItem {
        match self {
            f::Item::Paragraph(p) => kv::FlowItem::Paragraph(p.cross()),
            f::Item::TableStart(t) => kv::FlowItem::TableStart(kv::FlowTable {
                columns: t.columns,
                style: t.style,
            }),
            f::Item::RowStart(r) => kv::FlowItem::RowStart(kv::FlowRow { header: r.header }),
            f::Item::CellStart(c) => kv::FlowItem::CellStart(c.cross()),
            f::Item::CellEnd => kv::FlowItem::CellEnd,
            f::Item::RowEnd => kv::FlowItem::RowEnd,
            f::Item::TableEnd => kv::FlowItem::TableEnd,
            f::Item::AsideStart(a) => kv::FlowItem::AsideStart(a.cross()),
            f::Item::AsideEnd => kv::FlowItem::AsideEnd,
            f::Item::Rule(r) => kv::FlowItem::Rule(r),
            f::Item::Placeholder(p) => kv::FlowItem::Placeholder(p),
        }
    }
}

impl Cross<f::Place> for kv::FlowPlace {
    fn cross(self) -> f::Place {
        f::Place {
            paragraph: self.paragraph,
            offset: self.offset,
        }
    }
}

impl Cross<kv::FlowPlace> for f::Place {
    fn cross(self) -> kv::FlowPlace {
        kv::FlowPlace {
            paragraph: self.paragraph,
            offset: self.offset,
        }
    }
}

impl Cross<f::MarkChange> for kv::MarkChange {
    fn cross(self) -> f::MarkChange {
        match self {
            kv::MarkChange::Bold(v) => f::MarkChange::Bold(v),
            kv::MarkChange::Italic(v) => f::MarkChange::Italic(v),
            kv::MarkChange::Underline(v) => f::MarkChange::Underline(v),
            kv::MarkChange::Strike(v) => f::MarkChange::Strike(v),
            kv::MarkChange::Script(v) => f::MarkChange::Script(v.cross()),
            kv::MarkChange::Color(v) => f::MarkChange::Color(v.cross()),
            kv::MarkChange::Highlight(v) => f::MarkChange::Highlight(v.cross()),
            kv::MarkChange::Size(v) => f::MarkChange::Size(v),
            kv::MarkChange::Face(v) => f::MarkChange::Face(v),
            kv::MarkChange::Clear => f::MarkChange::Clear,
        }
    }
}

impl Cross<kv::MarkChange> for f::MarkChange {
    fn cross(self) -> kv::MarkChange {
        match self {
            f::MarkChange::Bold(v) => kv::MarkChange::Bold(v),
            f::MarkChange::Italic(v) => kv::MarkChange::Italic(v),
            f::MarkChange::Underline(v) => kv::MarkChange::Underline(v),
            f::MarkChange::Strike(v) => kv::MarkChange::Strike(v),
            f::MarkChange::Script(v) => kv::MarkChange::Script(v.cross()),
            f::MarkChange::Color(v) => kv::MarkChange::Color(v.cross()),
            f::MarkChange::Highlight(v) => kv::MarkChange::Highlight(v.cross()),
            f::MarkChange::Size(v) => kv::MarkChange::Size(v),
            f::MarkChange::Face(v) => kv::MarkChange::Face(v),
            f::MarkChange::Clear => kv::MarkChange::Clear,
        }
    }
}

impl Cross<f::Style> for kv::FlowStyle {
    fn cross(self) -> f::Style {
        f::Style {
            id: self.id,
            name: self.name,
            kind: self.kind.cross(),
            shown: self.shown,
        }
    }
}

impl Cross<kv::FlowStyle> for f::Style {
    fn cross(self) -> kv::FlowStyle {
        kv::FlowStyle {
            id: self.id,
            name: self.name,
            kind: self.kind.cross(),
            shown: self.shown,
        }
    }
}

impl Cross<f::Bitmap> for kv::Bitmap {
    fn cross(self) -> f::Bitmap {
        f::Bitmap {
            width: self.width,
            height: self.height,
            rgba: std::sync::Arc::unwrap_or_clone(self.rgba),
        }
    }
}

impl Cross<kv::Bitmap> for f::Bitmap {
    fn cross(self) -> kv::Bitmap {
        kv::Bitmap::new(self.width, self.height, self.rgba)
    }
}

