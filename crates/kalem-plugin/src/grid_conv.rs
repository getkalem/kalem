// The grid's types between the Rust contract (`kv`, `kalem-viewer`) and
// the WIT bindings (`g`, the `grid` interface), both ways. Included by
// `kalem-plugin`'s adapter (wit-bindgen's types) and by `kalem-script`'s
// host (wasmtime's): the two generate the same names from the same WIT
// (D6), so one list of fields serves both. Each includer defines `kv` and
// `g` before including.

/// A conversion of one side's value into the other's.
pub trait Conv<T> {
    #[allow(missing_docs)]
    fn conv(self) -> T;
}

macro_rules! same {
    ($($t:ty),*) => {
        $(impl Conv<$t> for $t {
            fn conv(self) -> $t {
                self
            }
        })*
    };
}
same!(bool, u8, u16, u32, i64, f32, f64, String);

impl Conv<u32> for usize {
    fn conv(self) -> u32 {
        self as u32
    }
}

impl Conv<usize> for u32 {
    fn conv(self) -> usize {
        self as usize
    }
}

impl<A: Conv<B>, B> Conv<Option<B>> for Option<A> {
    fn conv(self) -> Option<B> {
        self.map(Conv::conv)
    }
}

impl<A: Conv<B>, B> Conv<Vec<B>> for Vec<A> {
    fn conv(self) -> Vec<B> {
        self.into_iter().map(Conv::conv).collect()
    }
}

impl<A: Conv<X>, B: Conv<Y>, X, Y> Conv<(X, Y)> for (A, B) {
    fn conv(self) -> (X, Y) {
        (self.0.conv(), self.1.conv())
    }
}

impl<A: Conv<X>, B: Conv<Y>, C: Conv<Z>, X, Y, Z> Conv<(X, Y, Z)> for (A, B, C) {
    fn conv(self) -> (X, Y, Z) {
        (self.0.conv(), self.1.conv(), self.2.conv())
    }
}

impl Conv<g::Rgb> for [u8; 3] {
    fn conv(self) -> g::Rgb {
        let [r, g, b] = self;
        g::Rgb { r, g, b }
    }
}

impl Conv<[u8; 3]> for g::Rgb {
    fn conv(self) -> [u8; 3] {
        [self.r, self.g, self.b]
    }
}

impl<A: Conv<B>, B> Conv<(B, B, B, B)> for [A; 4] {
    fn conv(self) -> (B, B, B, B) {
        let [a, b, c, d] = self;
        (a.conv(), b.conv(), c.conv(), d.conv())
    }
}

impl<A: Conv<B>, B> Conv<[B; 4]> for (A, A, A, A) {
    fn conv(self) -> [B; 4] {
        [self.0.conv(), self.1.conv(), self.2.conv(), self.3.conv()]
    }
}

/// A record both ways, field by field.
macro_rules! record {
    ($a:path, $b:path { $($f:ident),* $(,)? }) => {
        impl Conv<$b> for $a {
            fn conv(self) -> $b {
                use $b as B;
                B { $($f: self.$f.conv()),* }
            }
        }
        impl Conv<$a> for $b {
            fn conv(self) -> $a {
                use $a as A;
                A { $($f: self.$f.conv()),* }
            }
        }
    };
}

/// An enum without payloads both ways, variant by variant.
macro_rules! cases {
    ($a:path, $b:path { $($v:ident),* $(,)? }) => {
        impl Conv<$b> for $a {
            fn conv(self) -> $b {
                use $a as A;
                use $b as B;
                match self { $(A::$v => B::$v),* }
            }
        }
        impl Conv<$a> for $b {
            fn conv(self) -> $a {
                use $a as A;
                use $b as B;
                match self { $(B::$v => A::$v),* }
            }
        }
    };
}

cases!(kv::Align, g::Align { General, Left, Center, Right });
cases!(kv::VAlign, g::Valign { Bottom, Middle, Top });
cases!(kv::PasteKind, g::PasteKind { All, Values, Formats, Formulas });
cases!(kv::BorderSet, g::BorderSet { All, Outside, ThickOutside, Bottom, Top, Left, Right, None });
cases!(kv::CompareOp, g::CompareOp {
    Greater, Less, GreaterOrEqual, LessOrEqual, Equal, NotEqual, Between, NotBetween
});
cases!(kv::ChartKind, g::ChartKind { Column, Bar, Line, Area, Pie, Doughnut, Scatter, Other });
cases!(kv::LegendPosition, g::LegendPosition { Bottom, Top, Left, Right, TopRight });
cases!(kv::ChartAxis, g::ChartAxis { Horizontal, Vertical });
cases!(kv::Aggregate, g::Aggregate { Sum, Count, Average, Max, Min });
cases!(kv::ValidationKind, g::ValidationKind {
    Any, Whole, Decimal, List, Date, Time, TextLength, Custom
});
cases!(kv::ErrorStyle, g::ErrorStyle { Stop, Warning, Information });
cases!(kv::FilterOp, g::FilterOp {
    Equal, NotEqual, Greater, GreaterOrEqual, Less, LessOrEqual, BeginsWith, EndsWith,
    Contains, NotContains
});

record!(kv::GridCell, g::Cell {
    text, numeric, bold, italic, underline, strike, color, fill, align, wrap, formula, note,
    bar, icon, font_size, face, valign, borders, border_thick, indent, rotation, shrink,
    center_across, unlocked, sparkline, thread,
});
cases!(kv::SparklineKind, g::SparklineKind { Line, Column, WinLoss });
record!(kv::Sparkline, g::Sparkline { kind, points, zero, color, marker, high, low });
record!(kv::SheetProtection, g::Protection {
    has_password, format_cells, format_columns, format_rows, insert_rows, insert_columns,
    delete_rows, delete_columns, sort, filter,
});
record!(kv::PageSetup, g::PageLayout {
    landscape, paper, margins, fit_width, print_area, title_rows, header, footer, row_breaks,
});
record!(kv::Drawing, g::Drawing { name, anchor, kind });
record!(kv::Scenario, g::Scenario { name, comment, cells });
record!(kv::ThreadComment, g::ThreadComment { author, text, time });
record!(kv::SheetView, g::ViewSettings { zoom, gridlines, headings, page_break_preview, split });
record!(kv::CommentThread, g::CommentThread { row, col, done, comments });

impl Conv<g::DrawingKind> for kv::DrawingKind {
    fn conv(self) -> g::DrawingKind {
        match self {
            kv::DrawingKind::Picture => g::DrawingKind::Picture,
            kv::DrawingKind::Shape {
                preset,
                fill,
                line,
                text,
                text_box,
            } => g::DrawingKind::Shape(g::Shape {
                preset,
                fill: fill.conv(),
                line: line.conv(),
                text,
                text_box,
            }),
        }
    }
}

impl Conv<kv::DrawingKind> for g::DrawingKind {
    fn conv(self) -> kv::DrawingKind {
        match self {
            g::DrawingKind::Picture => kv::DrawingKind::Picture,
            g::DrawingKind::Shape(s) => kv::DrawingKind::Shape {
                preset: s.preset,
                fill: s.fill.conv(),
                line: s.line.conv(),
                text: s.text,
                text_box: s.text_box,
            },
        }
    }
}
record!(kv::GridLayout, g::GridLayout {
    rows, cols, max_rows, max_cols, widths, default_width, heights, default_height,
    hidden_rows, hidden_cols, merged, frozen, editable, filter, filtered,
});
record!(kv::StyleChange, g::StyleChange {
    bold, italic, underline, strike, color, fill, size, face, align, valign, borders,
    number_format, indent, rotation, shrink, center_across, locked,
});
record!(kv::CondStyle, g::CondStyle { fill, color, bold });
record!(kv::ChartSeries, g::Series {
    name, values, x, color, point_colors, explosion, point_explosions,
});
record!(kv::AxisFont, g::AxisFont { size, bold, italic, color, face });
record!(kv::Gridlines, g::Gridlines {
    horizontal_major, horizontal_minor, vertical_major, vertical_minor,
});
record!(kv::AxisScale, g::AxisScale { min, max, major, log });
record!(kv::DataLabels, g::DataLabels { value, category, series, percent });
record!(kv::Chart, g::Chart {
    kind, title, categories, series, anchor, stacked, horizontal_title, vertical_title,
    legend, labels, scale, background, border, plot_background, plot_border, gridlines,
    axis_format, horizontal_font, vertical_font, title_font, legend_font,
});
record!(kv::PivotSpec, g::PivotSpec { range, rows, cols, values });
record!(kv::Validation, g::CellValidation {
    kind, op, value, value2, allow_blank, dropdown, prompt, error, list,
});
record!(kv::ValidationError, g::ValidationError { style, title, message });
record!(kv::MacroEntry, g::MacroEntry { name, event });
record!(kv::TableInfo, g::TableInfo { name, range, totals, style });
record!(kv::SortKey, g::SortKey { col, descending, list });

impl Conv<g::FilterRule> for kv::FilterRule {
    fn conv(self) -> g::FilterRule {
        match self {
            kv::FilterRule::Values(v) => g::FilterRule::Values(v),
            kv::FilterRule::Custom { first, second } => g::FilterRule::Custom(g::CustomFilter {
                first: first.conv(),
                second: second.conv(),
            }),
            kv::FilterRule::Top {
                count,
                percent,
                bottom,
            } => g::FilterRule::Top(g::TopFilter {
                count,
                percent,
                bottom,
            }),
            kv::FilterRule::Average { above } => g::FilterRule::Average(above),
            kv::FilterRule::Fill(c) => g::FilterRule::Fill(c.conv()),
        }
    }
}

impl Conv<kv::FilterRule> for g::FilterRule {
    fn conv(self) -> kv::FilterRule {
        match self {
            g::FilterRule::Values(v) => kv::FilterRule::Values(v),
            g::FilterRule::Custom(c) => kv::FilterRule::Custom {
                first: c.first.conv(),
                second: c.second.conv(),
            },
            g::FilterRule::Top(t) => kv::FilterRule::Top {
                count: t.count,
                percent: t.percent,
                bottom: t.bottom,
            },
            g::FilterRule::Average(above) => kv::FilterRule::Average { above },
            g::FilterRule::Fill(c) => kv::FilterRule::Fill(c.conv()),
        }
    }
}
record!(kv::MacroOutcome, g::MacroOutcome { output, skipped, error, changed, question });

impl Conv<g::Paint> for kv::Paint {
    fn conv(self) -> g::Paint {
        match self {
            kv::Paint::Automatic => g::Paint::Automatic,
            kv::Paint::None => g::Paint::None,
            kv::Paint::Color(c) => g::Paint::Color(c.conv()),
        }
    }
}

impl Conv<kv::Paint> for g::Paint {
    fn conv(self) -> kv::Paint {
        match self {
            g::Paint::Automatic => kv::Paint::Automatic,
            g::Paint::None => kv::Paint::None,
            g::Paint::Color(c) => kv::Paint::Color(c.conv()),
        }
    }
}

impl Conv<g::GridEdit> for kv::GridEdit {
    fn conv(self) -> g::GridEdit {
        let ac = |at, count| g::AtCount { at, count };
        match self {
            kv::GridEdit::InsertRows { at, count } => g::GridEdit::InsertRows(ac(at, count)),
            kv::GridEdit::DeleteRows { at, count } => g::GridEdit::DeleteRows(ac(at, count)),
            kv::GridEdit::InsertCols { at, count } => g::GridEdit::InsertCols(ac(at, count)),
            kv::GridEdit::DeleteCols { at, count } => g::GridEdit::DeleteCols(ac(at, count)),
        }
    }
}

impl Conv<kv::GridEdit> for g::GridEdit {
    fn conv(self) -> kv::GridEdit {
        match self {
            g::GridEdit::InsertRows(a) => kv::GridEdit::InsertRows {
                at: a.at,
                count: a.count,
            },
            g::GridEdit::DeleteRows(a) => kv::GridEdit::DeleteRows {
                at: a.at,
                count: a.count,
            },
            g::GridEdit::InsertCols(a) => kv::GridEdit::InsertCols {
                at: a.at,
                count: a.count,
            },
            g::GridEdit::DeleteCols(a) => kv::GridEdit::DeleteCols {
                at: a.at,
                count: a.count,
            },
        }
    }
}

impl Conv<g::SheetEdit> for kv::SheetEdit {
    fn conv(self) -> g::SheetEdit {
        match self {
            kv::SheetEdit::Insert(i) => g::SheetEdit::Insert(i.conv()),
            kv::SheetEdit::Delete(i) => g::SheetEdit::Delete(i.conv()),
            kv::SheetEdit::Rename(i, n) => g::SheetEdit::Rename((i.conv(), n)),
            kv::SheetEdit::Move(a, b) => g::SheetEdit::Move((a.conv(), b.conv())),
            kv::SheetEdit::Hide(i, h) => g::SheetEdit::Hide((i.conv(), h)),
            kv::SheetEdit::Copy(a, b) => g::SheetEdit::Copy((a.conv(), b.conv())),
        }
    }
}

impl Conv<kv::SheetEdit> for g::SheetEdit {
    fn conv(self) -> kv::SheetEdit {
        match self {
            g::SheetEdit::Insert(i) => kv::SheetEdit::Insert(i.conv()),
            g::SheetEdit::Delete(i) => kv::SheetEdit::Delete(i.conv()),
            g::SheetEdit::Rename((i, n)) => kv::SheetEdit::Rename(i.conv(), n),
            g::SheetEdit::Move((a, b)) => kv::SheetEdit::Move(a.conv(), b.conv()),
            g::SheetEdit::Hide((i, h)) => kv::SheetEdit::Hide(i.conv(), h),
            g::SheetEdit::Copy((a, b)) => kv::SheetEdit::Copy(a.conv(), b.conv()),
        }
    }
}

impl Conv<g::CondRule> for kv::CondRule {
    fn conv(self) -> g::CondRule {
        match self {
            kv::CondRule::Compare { op, value, value2 } => g::CondRule::Compare(g::CompareRule {
                op: op.conv(),
                value,
                value2,
            }),
            kv::CondRule::TextContains(t) => g::CondRule::TextContains(t),
            kv::CondRule::Duplicates => g::CondRule::Duplicates,
            kv::CondRule::Unique => g::CondRule::Unique,
            kv::CondRule::Top {
                count,
                bottom,
                percent,
            } => g::CondRule::Top(g::TopRule {
                count,
                bottom,
                percent,
            }),
            kv::CondRule::Average { below } => g::CondRule::Average(below),
            kv::CondRule::Formula(f) => g::CondRule::Formula(f),
            kv::CondRule::ColorScale(c) => g::CondRule::ColorScale(c.conv()),
            kv::CondRule::DataBar(c) => g::CondRule::DataBar(c.conv()),
            kv::CondRule::IconSet(s) => g::CondRule::IconSet(s),
        }
    }
}

impl Conv<kv::CondRule> for g::CondRule {
    fn conv(self) -> kv::CondRule {
        match self {
            g::CondRule::Compare(r) => kv::CondRule::Compare {
                op: r.op.conv(),
                value: r.value,
                value2: r.value2,
            },
            g::CondRule::TextContains(t) => kv::CondRule::TextContains(t),
            g::CondRule::Duplicates => kv::CondRule::Duplicates,
            g::CondRule::Unique => kv::CondRule::Unique,
            g::CondRule::Top(r) => kv::CondRule::Top {
                count: r.count,
                bottom: r.bottom,
                percent: r.percent,
            },
            g::CondRule::Average(below) => kv::CondRule::Average { below },
            g::CondRule::Formula(f) => kv::CondRule::Formula(f),
            g::CondRule::ColorScale(c) => kv::CondRule::ColorScale(c.conv()),
            g::CondRule::DataBar(c) => kv::CondRule::DataBar(c.conv()),
            g::CondRule::IconSet(s) => kv::CondRule::IconSet(s),
        }
    }
}

impl Conv<g::MacroQuestion> for kv::MacroQuestion {
    fn conv(self) -> g::MacroQuestion {
        match self {
            kv::MacroQuestion::Message {
                prompt,
                buttons,
                title,
            } => g::MacroQuestion::Message(g::MessageQuestion {
                prompt,
                buttons,
                title,
            }),
            kv::MacroQuestion::Input {
                prompt,
                title,
                default,
            } => g::MacroQuestion::Input(g::InputQuestion {
                prompt,
                title,
                default,
            }),
        }
    }
}

impl Conv<kv::MacroQuestion> for g::MacroQuestion {
    fn conv(self) -> kv::MacroQuestion {
        match self {
            g::MacroQuestion::Message(q) => kv::MacroQuestion::Message {
                prompt: q.prompt,
                buttons: q.buttons,
                title: q.title,
            },
            g::MacroQuestion::Input(q) => kv::MacroQuestion::Input {
                prompt: q.prompt,
                title: q.title,
                default: q.default,
            },
        }
    }
}

/// A contract's changed units, or its error, as the interface's.
pub fn changed(r: kv::Result<Vec<usize>>) -> Result<Vec<u32>, String> {
    r.map(Conv::conv).map_err(|e| e.0)
}

/// The interface's changed units as the contract's.
pub fn unchanged(r: Result<Vec<u32>, String>) -> kv::Result<Vec<usize>> {
    r.map(Conv::conv).map_err(kv::ViewerError)
}
