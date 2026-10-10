// The `flow-2` interface's types (API 0.2.8) between the Rust contract
// (`kv`) and the WIT bindings (`f2`), both ways. Included after
// `flow_conv.rs` (its `Cross` for the alignment `flow-2` uses from
// `flow`) by `kalem-plugin`'s adapter and by `kalem-script`'s host.

impl Cross<f2::LineSpacing> for kv::LineSpacing {
    fn cross(self) -> f2::LineSpacing {
        match self {
            kv::LineSpacing::Multiple(v) => f2::LineSpacing::Multiple(v),
            kv::LineSpacing::AtLeast(v) => f2::LineSpacing::AtLeast(v),
            kv::LineSpacing::Exactly(v) => f2::LineSpacing::Exactly(v),
        }
    }
}

impl Cross<kv::LineSpacing> for f2::LineSpacing {
    fn cross(self) -> kv::LineSpacing {
        match self {
            f2::LineSpacing::Multiple(v) => kv::LineSpacing::Multiple(v),
            f2::LineSpacing::AtLeast(v) => kv::LineSpacing::AtLeast(v),
            f2::LineSpacing::Exactly(v) => kv::LineSpacing::Exactly(v),
        }
    }
}

impl Cross<f2::ListKind> for kv::ListKind {
    fn cross(self) -> f2::ListKind {
        match self {
            kv::ListKind::Bullet => f2::ListKind::Bullet,
            kv::ListKind::Numbered(s) => f2::ListKind::Numbered(s),
        }
    }
}

impl Cross<kv::ListKind> for f2::ListKind {
    fn cross(self) -> kv::ListKind {
        match self {
            f2::ListKind::Bullet => kv::ListKind::Bullet,
            f2::ListKind::Numbered(s) => kv::ListKind::Numbered(s),
        }
    }
}

impl Cross<f2::ParagraphChange> for kv::ParagraphChange {
    fn cross(self) -> f2::ParagraphChange {
        match self {
            kv::ParagraphChange::Align(a) => f2::ParagraphChange::Align(a.cross()),
            kv::ParagraphChange::IndentStart(v) => f2::ParagraphChange::IndentStart(v),
            kv::ParagraphChange::IndentEnd(v) => f2::ParagraphChange::IndentEnd(v),
            kv::ParagraphChange::FirstLine(v) => f2::ParagraphChange::FirstLine(v),
            kv::ParagraphChange::SpaceBefore(v) => f2::ParagraphChange::SpaceBefore(v),
            kv::ParagraphChange::SpaceAfter(v) => f2::ParagraphChange::SpaceAfter(v),
            kv::ParagraphChange::LineSpacing(v) => f2::ParagraphChange::LineSpacing(v.cross()),
            kv::ParagraphChange::List(v) => f2::ParagraphChange::List(v.cross()),
            kv::ParagraphChange::ListLevel(v) => f2::ParagraphChange::ListLevel(v),
            kv::ParagraphChange::Clear => f2::ParagraphChange::Clear,
        }
    }
}

impl Cross<kv::ParagraphChange> for f2::ParagraphChange {
    fn cross(self) -> kv::ParagraphChange {
        match self {
            f2::ParagraphChange::Align(a) => kv::ParagraphChange::Align(a.cross()),
            f2::ParagraphChange::IndentStart(v) => kv::ParagraphChange::IndentStart(v),
            f2::ParagraphChange::IndentEnd(v) => kv::ParagraphChange::IndentEnd(v),
            f2::ParagraphChange::FirstLine(v) => kv::ParagraphChange::FirstLine(v),
            f2::ParagraphChange::SpaceBefore(v) => kv::ParagraphChange::SpaceBefore(v),
            f2::ParagraphChange::SpaceAfter(v) => kv::ParagraphChange::SpaceAfter(v),
            f2::ParagraphChange::LineSpacing(v) => kv::ParagraphChange::LineSpacing(v.cross()),
            f2::ParagraphChange::List(v) => kv::ParagraphChange::List(v.cross()),
            f2::ParagraphChange::ListLevel(v) => kv::ParagraphChange::ListLevel(v),
            f2::ParagraphChange::Clear => kv::ParagraphChange::Clear,
        }
    }
}
