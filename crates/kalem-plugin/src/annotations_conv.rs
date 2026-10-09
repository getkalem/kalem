// The annotations' types between the Rust contract (`kv`, `kalem-viewer`)
// and the WIT bindings (`a`, the `annotations` interface), both ways, and
// the conversion trait `flow_conv.rs` uses too. Included by
// `kalem-plugin`'s adapter (wit-bindgen's types) and by `kalem-script`'s
// host (wasmtime's), as `grid_conv.rs` is: the two generate the same
// names from the same WIT (D6). Each includer defines `kv` and `a` before
// including.

/// A conversion of one side's value into the other's (`Cross`, not
/// `grid_conv`'s `Conv`, so that both may be in scope).
pub trait Cross<T> {
    #[allow(missing_docs)]
    fn cross(self) -> T;
}

macro_rules! same {
    ($($t:ty),*) => {
        $(impl Cross<$t> for $t {
            fn cross(self) -> $t {
                self
            }
        })*
    };
}
same!(bool, u8, u32, u64, f32, String);

impl<A: Cross<B>, B> Cross<Option<B>> for Option<A> {
    fn cross(self) -> Option<B> {
        self.map(Cross::cross)
    }
}

impl<A: Cross<B>, B> Cross<Vec<B>> for Vec<A> {
    fn cross(self) -> Vec<B> {
        self.into_iter().map(Cross::cross).collect()
    }
}

impl<A: Cross<X>, B: Cross<Y>, X, Y> Cross<(X, Y)> for (A, B) {
    fn cross(self) -> (X, Y) {
        (self.0.cross(), self.1.cross())
    }
}

/// An enum's cases both ways, case for case.
macro_rules! cases {
    ($kv:ty, $w:ty, $($case:ident),*) => {
        impl Cross<$w> for $kv {
            fn cross(self) -> $w {
                match self {
                    $(<$kv>::$case => <$w>::$case,)*
                }
            }
        }
        impl Cross<$kv> for $w {
            fn cross(self) -> $kv {
                match self {
                    $(<$w>::$case => <$kv>::$case,)*
                }
            }
        }
    };
}

cases!(
    kv::AnnotationKind,
    a::AnnotationKind,
    Comment,
    Insertion,
    Deletion,
    Formatting,
    MoveFrom,
    MoveTo
);

fn text_place(p: kv::FlowPlace) -> a::TextPlace {
    a::TextPlace {
        paragraph: p.paragraph,
        offset: p.offset,
    }
}

fn flow_place(p: a::TextPlace) -> kv::FlowPlace {
    kv::FlowPlace {
        paragraph: p.paragraph,
        offset: p.offset,
    }
}

impl Cross<a::Anchor> for kv::Anchor {
    fn cross(self) -> a::Anchor {
        match self {
            kv::Anchor::Flow { unit, from, to } => {
                a::Anchor::Flow((unit as u32, text_place(from), text_place(to)))
            }
            kv::Anchor::Text { unit, range } => {
                a::Anchor::Text((unit as u32, range.start as u32, range.end as u32))
            }
            kv::Anchor::Cell { unit, row, col } => a::Anchor::Cell((unit as u32, row, col)),
            kv::Anchor::Area { unit, rect } => {
                let [x, y, width, height] = rect;
                a::Anchor::Area((
                    unit as u32,
                    a::Rect {
                        x,
                        y,
                        width,
                        height,
                    },
                ))
            }
            kv::Anchor::Unit(u) => a::Anchor::Unit(u as u32),
        }
    }
}

impl Cross<kv::Anchor> for a::Anchor {
    fn cross(self) -> kv::Anchor {
        match self {
            a::Anchor::Flow((unit, from, to)) => kv::Anchor::Flow {
                unit: unit as usize,
                from: flow_place(from),
                to: flow_place(to),
            },
            a::Anchor::Text((unit, start, end)) => kv::Anchor::Text {
                unit: unit as usize,
                range: start as usize..end as usize,
            },
            a::Anchor::Cell((unit, row, col)) => kv::Anchor::Cell {
                unit: unit as usize,
                row,
                col,
            },
            a::Anchor::Area((unit, r)) => kv::Anchor::Area {
                unit: unit as usize,
                rect: [r.x, r.y, r.width, r.height],
            },
            a::Anchor::Unit(u) => kv::Anchor::Unit(u as usize),
        }
    }
}

impl Cross<a::Annotation> for kv::Annotation {
    fn cross(self) -> a::Annotation {
        a::Annotation {
            id: self.id,
            kind: self.kind.cross(),
            author: self.author,
            date: self.date,
            text: self.text,
            parent: self.parent,
            resolved: self.resolved,
            anchors: self.anchors.cross(),
        }
    }
}

impl Cross<kv::Annotation> for a::Annotation {
    fn cross(self) -> kv::Annotation {
        kv::Annotation {
            id: self.id,
            kind: self.kind.cross(),
            author: self.author,
            date: self.date,
            text: self.text,
            parent: self.parent,
            resolved: self.resolved,
            anchors: self.anchors.cross(),
        }
    }
}
