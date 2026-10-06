//! Panes (T2.7i.5): how a window is divided, shared by both frontends.
//! A layout is a tree of splits whose leaves are panes; each frontend
//! keeps what each pane shows by its [`PaneId`] and asks the layout for
//! the panes' rectangles in its own units (pixels, cells). The
//! operations are those of Doom's `SPC w`, each one undoable.

/// A pane, named for as long as it lives.
pub type PaneId = u64;

/// How a split divides its space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Side by side.
    Row,
    /// One above another.
    Column,
}

/// A direction on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// To the left.
    Left,
    /// To the right.
    Right,
    /// Above.
    Up,
    /// Below.
    Down,
}

impl Dir {
    /// The direction named `name` (`left`, `right`, `up`, `down`).
    pub fn parse(name: &str) -> Option<Dir> {
        Some(match name {
            "left" => Dir::Left,
            "right" => Dir::Right,
            "up" => Dir::Up,
            "down" => Dir::Down,
            _ => return None,
        })
    }

    fn axis(self) -> Axis {
        match self {
            Dir::Left | Dir::Right => Axis::Row,
            Dir::Up | Dir::Down => Axis::Column,
        }
    }

    fn first(self) -> bool {
        matches!(self, Dir::Left | Dir::Up)
    }
}

/// A rectangle in the frontend's units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// The left edge.
    pub x: f32,
    /// The top edge.
    pub y: f32,
    /// The width.
    pub w: f32,
    /// The height.
    pub h: f32,
}

impl Rect {
    fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// A node of the tree.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    /// A pane.
    Leaf(PaneId),
    /// A split: its children with their shares of the space.
    Split {
        /// How it divides.
        axis: Axis,
        /// The children and their weights.
        children: Vec<(Node, f32)>,
    },
}

impl Node {
    fn panes(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(p) => out.push(*p),
            Node::Split { children, .. } => children.iter().for_each(|(c, _)| c.panes(out)),
        }
    }

    fn contains(&self, p: PaneId) -> bool {
        match self {
            Node::Leaf(q) => *q == p,
            Node::Split { children, .. } => children.iter().any(|(c, _)| c.contains(p)),
        }
    }

    fn rects(&self, r: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Node::Leaf(p) => out.push((*p, r)),
            Node::Split { axis, children } => {
                let total: f32 = children
                    .iter()
                    .map(|(_, w)| *w)
                    .sum::<f32>()
                    .max(f32::EPSILON);
                let mut at = 0.0;
                for (c, w) in children {
                    let share = w / total;
                    let sub = match axis {
                        Axis::Row => Rect {
                            x: r.x + r.w * at,
                            y: r.y,
                            w: r.w * share,
                            h: r.h,
                        },
                        Axis::Column => Rect {
                            x: r.x,
                            y: r.y + r.h * at,
                            w: r.w,
                            h: r.h * share,
                        },
                    };
                    c.rects(sub, out);
                    at += share;
                }
            }
        }
    }

    /// The tree without pane `p`; `None` when nothing is left.
    fn without(self, p: PaneId) -> Option<Node> {
        match self {
            Node::Leaf(q) if q == p => None,
            Node::Leaf(q) => Some(Node::Leaf(q)),
            Node::Split { axis, children } => {
                let mut kept: Vec<(Node, f32)> = children
                    .into_iter()
                    .filter_map(|(c, w)| c.without(p).map(|c| (c, w)))
                    .collect();
                match kept.len() {
                    0 => None,
                    1 => kept.pop().map(|(c, _)| c),
                    _ => Some(Node::Split {
                        axis,
                        children: kept,
                    }),
                }
            }
        }
    }

    /// Pane `p` split along `axis`, `new` after it (before it with
    /// `before`).
    fn split_at(&mut self, p: PaneId, axis: Axis, new: PaneId, before: bool) -> bool {
        match self {
            Node::Leaf(q) if *q == p => {
                let mut children = vec![(Node::Leaf(p), 1.0), (Node::Leaf(new), 1.0)];
                if before {
                    children.reverse();
                }
                *self = Node::Split { axis, children };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { axis: a, children } => {
                // In a split of the same axis the new pane is a sibling.
                if *a == axis
                    && let Some(i) = children
                        .iter()
                        .position(|(c, _)| matches!(c, Node::Leaf(q) if *q == p))
                {
                    let w = children[i].1 / 2.0;
                    children[i].1 = w;
                    let at = if before { i } else { i + 1 };
                    children.insert(at, (Node::Leaf(new), w));
                    return true;
                }
                children
                    .iter_mut()
                    .any(|(c, _)| c.split_at(p, axis, new, before))
            }
        }
    }

    fn balance(&mut self) {
        if let Node::Split { children, .. } = self {
            for (c, w) in children {
                *w = 1.0;
                c.balance();
            }
        }
    }

    /// Grows pane `p` by `delta` (a share of its split) along `axis`, in
    /// the innermost split of that axis holding it.
    fn resize(&mut self, p: PaneId, axis: Axis, delta: f32) -> bool {
        let Node::Split { axis: a, children } = self else {
            return false;
        };
        let Some(i) = children.iter().position(|(c, _)| c.contains(p)) else {
            return false;
        };
        if children[i].0.resize(p, axis, delta) {
            return true;
        }
        if *a != axis || children.len() < 2 {
            return false;
        }
        let total: f32 = children.iter().map(|(_, w)| *w).sum();
        let others = total - children[i].1;
        let grow = (delta * total).clamp(
            -(children[i].1 - total * 0.05),
            others - total * 0.05 * (children.len() - 1) as f32,
        );
        children[i].1 += grow;
        for (j, (_, w)) in children.iter_mut().enumerate() {
            if j != i {
                *w -= grow * *w / others.max(f32::EPSILON);
            }
        }
        true
    }

    /// The panes renamed by `f` (rotation, swaps).
    fn rename(&mut self, f: &dyn Fn(PaneId) -> PaneId) {
        match self {
            Node::Leaf(p) => *p = f(*p),
            Node::Split { children, .. } => children.iter_mut().for_each(|(c, _)| c.rename(f)),
        }
    }
}

/// A window's panes.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    root: Node,
    focus: PaneId,
    /// The pane focused before the last change of focus (`SPC w p`).
    previous: Option<PaneId>,
    next: PaneId,
    /// The layouts before each change, to undo; the ones undone, to redo.
    undo: Vec<(Node, PaneId)>,
    redo: Vec<(Node, PaneId)>,
    /// The layout `only` replaced, to go back to.
    zoomed: Option<(Node, PaneId)>,
}

impl Default for Layout {
    fn default() -> Self {
        Layout::new()
    }
}

/// How many layouts undo remembers.
const UNDO: usize = 50;

impl Layout {
    /// One pane, numbered 0.
    pub fn new() -> Layout {
        Layout {
            root: Node::Leaf(0),
            focus: 0,
            previous: None,
            next: 1,
            undo: Vec::new(),
            redo: Vec::new(),
            zoomed: None,
        }
    }

    /// The tree.
    pub fn root(&self) -> &Node {
        &self.root
    }

    /// The focused pane.
    pub fn focus(&self) -> PaneId {
        self.focus
    }

    /// The panes in order: left to right, top to bottom within each split.
    pub fn panes(&self) -> Vec<PaneId> {
        let mut v = Vec::new();
        self.root.panes(&mut v);
        v
    }

    /// Whether there is more than one pane.
    pub fn is_split(&self) -> bool {
        matches!(self.root, Node::Split { .. })
    }

    /// Whether `only` hid other panes.
    pub fn is_zoomed(&self) -> bool {
        self.zoomed.is_some()
    }

    /// Each pane's rectangle within `area`.
    pub fn rects(&self, area: Rect) -> Vec<(PaneId, Rect)> {
        let mut v = Vec::new();
        self.root.rects(area, &mut v);
        v
    }

    fn save(&mut self) {
        self.undo.push((self.root.clone(), self.focus));
        if self.undo.len() > UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Focuses pane `p`.
    pub fn set_focus(&mut self, p: PaneId) -> bool {
        if !self.root.contains(p) {
            return false;
        }
        if p != self.focus {
            self.previous = Some(self.focus);
            self.focus = p;
        }
        true
    }

    /// Splits the focused pane: the new pane beside it (`Axis::Row`, to
    /// the right) or below it (`Axis::Column`), and focused.
    pub fn split(&mut self, axis: Axis) -> PaneId {
        self.save();
        self.zoomed = None;
        let new = self.next;
        self.next += 1;
        let focus = self.focus;
        self.root.split_at(focus, axis, new, false);
        self.set_focus(new);
        new
    }

    /// Closes pane `p`, the last one excepted; the focus goes to the pane
    /// that took its place. `false` when it is the last.
    pub fn close(&mut self, p: PaneId) -> bool {
        let panes = self.panes();
        if panes.len() < 2 || !panes.contains(&p) {
            return false;
        }
        self.save();
        let at = panes.iter().position(|q| *q == p).unwrap_or(0);
        let root = std::mem::replace(&mut self.root, Node::Leaf(0));
        self.root = root.without(p).unwrap_or(Node::Leaf(panes[0]));
        if self.focus == p {
            let rest = self.panes();
            let next = rest[at.min(rest.len() - 1)];
            self.focus = next;
        }
        if self.previous == Some(p) {
            self.previous = None;
        }
        true
    }

    /// Only the focused pane, or (again) the panes as they were.
    pub fn toggle_only(&mut self) {
        match self.zoomed.take() {
            Some((root, focus)) => {
                self.save();
                self.root = root;
                self.focus = focus;
            }
            None if self.is_split() => {
                self.save();
                self.zoomed = Some((self.root.clone(), self.focus));
                self.root = Node::Leaf(self.focus);
            }
            None => {}
        }
    }

    /// The panes that `only` hides, as a list: they are kept, not closed.
    pub fn hidden(&self) -> Vec<PaneId> {
        let Some((root, _)) = &self.zoomed else {
            return Vec::new();
        };
        let mut v = Vec::new();
        root.panes(&mut v);
        v.retain(|p| *p != self.focus);
        v
    }

    /// Focuses the next pane in order (`back`: the one before), wrapping.
    pub fn cycle(&mut self, back: bool) -> PaneId {
        let panes = self.panes();
        let i = panes.iter().position(|p| *p == self.focus).unwrap_or(0);
        let n = panes.len();
        let j = if back { (i + n - 1) % n } else { (i + 1) % n };
        self.set_focus(panes[j]);
        self.focus
    }

    /// Focuses the pane focused before (`SPC w p`).
    pub fn focus_previous(&mut self) -> Option<PaneId> {
        let p = self.previous?;
        self.set_focus(p).then_some(p)
    }

    /// The pane next to the focused one in `dir`: the nearest whose
    /// rectangle lies that way and overlaps it across.
    pub fn neighbour(&self, dir: Dir) -> Option<PaneId> {
        let rects = self.rects(Rect {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        let (_, here) = *rects.iter().find(|(p, _)| *p == self.focus)?;
        let (cx, cy) = here.center();
        let eps = 1e-4;
        rects
            .iter()
            .filter(|(p, _)| *p != self.focus)
            .filter(|(_, r)| match dir {
                Dir::Left => r.x + r.w <= here.x + eps && overlaps(r.y, r.h, here.y, here.h),
                Dir::Right => r.x >= here.x + here.w - eps && overlaps(r.y, r.h, here.y, here.h),
                Dir::Up => r.y + r.h <= here.y + eps && overlaps(r.x, r.w, here.x, here.w),
                Dir::Down => r.y >= here.y + here.h - eps && overlaps(r.x, r.w, here.x, here.w),
            })
            .min_by(|(_, a), (_, b)| {
                let d = |r: &Rect| {
                    let (x, y) = r.center();
                    (x - cx).abs() + (y - cy).abs()
                };
                d(a).total_cmp(&d(b))
            })
            .map(|(p, _)| *p)
    }

    /// Focuses the pane in `dir`.
    pub fn focus_dir(&mut self, dir: Dir) -> Option<PaneId> {
        let p = self.neighbour(dir)?;
        self.set_focus(p);
        Some(p)
    }

    /// Moves the focused pane to the edge of the window in `dir`, as
    /// tall (or wide) as the window (Vim's `C-w H`).
    pub fn move_to_edge(&mut self, dir: Dir) -> bool {
        if !self.is_split() {
            return false;
        }
        self.save();
        let p = self.focus;
        let root = std::mem::replace(&mut self.root, Node::Leaf(0));
        let Some(rest) = root.without(p) else {
            self.root = Node::Leaf(p);
            return false;
        };
        let mut children = vec![(rest, 3.0), (Node::Leaf(p), 1.0)];
        if dir.first() {
            children.reverse();
        }
        self.root = Node::Split {
            axis: dir.axis(),
            children,
        };
        true
    }

    /// Makes all the panes of each split the same size.
    pub fn balance(&mut self) {
        self.save();
        self.root.balance();
    }

    /// Grows the focused pane by `delta`, a share of its split (negative
    /// shrinks), along `axis`. `false` when no split of that axis holds it.
    pub fn resize(&mut self, axis: Axis, delta: f32) -> bool {
        let before = (self.root.clone(), self.focus);
        let focus = self.focus;
        if self.root.resize(focus, axis, delta) {
            self.undo.push(before);
            self.redo.clear();
            true
        } else {
            false
        }
    }

    /// Swaps the focused pane with the next one in order, the focus going
    /// with it.
    pub fn swap_next(&mut self) -> bool {
        let panes = self.panes();
        if panes.len() < 2 {
            return false;
        }
        self.save();
        let i = panes.iter().position(|p| *p == self.focus).unwrap_or(0);
        let (a, b) = (panes[i], panes[(i + 1) % panes.len()]);
        self.root.rename(&|p| {
            if p == a {
                b
            } else if p == b {
                a
            } else {
                p
            }
        });
        true
    }

    /// Moves every pane to the next place in order (`back`: the place
    /// before), the focus staying with its pane.
    pub fn rotate(&mut self, back: bool) -> bool {
        let panes = self.panes();
        let n = panes.len();
        if n < 2 {
            return false;
        }
        self.save();
        let map: std::collections::HashMap<PaneId, PaneId> = panes
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let from = if back { (i + 1) % n } else { (i + n - 1) % n };
                (*p, panes[from])
            })
            .collect();
        self.root.rename(&|p| map.get(&p).copied().unwrap_or(p));
        true
    }

    /// The layout before the last change.
    pub fn undo(&mut self) -> bool {
        let Some((root, focus)) = self.undo.pop() else {
            return false;
        };
        self.redo
            .push((std::mem::replace(&mut self.root, root), self.focus));
        self.focus = focus;
        true
    }

    /// The layout undone last, again.
    pub fn redo(&mut self) -> bool {
        let Some((root, focus)) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push((std::mem::replace(&mut self.root, root), self.focus));
        self.focus = focus;
        true
    }

    /// The panes gone from the layout since `before` (closed ones, or
    /// ones an undo took away), for the frontend to forget.
    pub fn removed_since(&self, before: &[PaneId]) -> Vec<PaneId> {
        let now = self.panes();
        let hidden = self.hidden();
        before
            .iter()
            .filter(|p| !now.contains(p) && !hidden.contains(p))
            .copied()
            .collect()
    }
}

fn overlaps(a: f32, al: f32, b: f32, bl: f32) -> bool {
    a < b + bl - 1e-4 && b < a + al - 1e-4
}

/// A change of a window's panes, as the `pane.*` commands ask it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneOp {
    /// Split the focused pane: beside it or below.
    Split(Axis),
    /// Focus the pane in a direction.
    Focus(Dir),
    /// Move the focused pane to the window's edge.
    Move(Dir),
    /// Close the focused pane (with its document when `true`).
    Close(bool),
    /// Only the focused pane, or back.
    Only,
    /// The next pane, or the one before.
    Cycle(bool),
    /// The pane focused before.
    Previous,
    /// Every pane the same size.
    Balance,
    /// Grow (or, negative, shrink) along an axis, by percent of the split.
    Resize(Axis, i32),
    /// Swap with the next pane.
    Swap,
    /// Rotate the panes (back when `true`).
    Rotate(bool),
    /// Undo the last change of the layout.
    Undo,
    /// Redo it.
    Redo,
    /// Split, the new pane on a new empty document.
    New,
    /// Vim's `:q`: close the focused pane, else its document (`force`,
    /// `:q!`: losing the document's changes), quitting only when nothing
    /// is left to close.
    CloseOrQuit { force: bool },
}

impl Layout {
    /// Applies `op` but for what only the frontend can do (closing a
    /// document, opening a new one): whether the layout changed, and the
    /// new pane of a split.
    pub fn apply(&mut self, op: &PaneOp) -> (bool, Option<PaneId>) {
        match *op {
            PaneOp::Split(axis) => (true, Some(self.split(axis))),
            PaneOp::New => (true, Some(self.split(Axis::Row))),
            PaneOp::Focus(d) => (self.focus_dir(d).is_some(), None),
            PaneOp::Move(d) => (self.move_to_edge(d), None),
            PaneOp::Close(_) | PaneOp::CloseOrQuit { .. } => {
                let f = self.focus;
                (self.close(f), None)
            }
            PaneOp::Only => {
                self.toggle_only();
                (true, None)
            }
            PaneOp::Cycle(back) => {
                let before = self.focus;
                (self.cycle(back) != before, None)
            }
            PaneOp::Previous => (self.focus_previous().is_some(), None),
            PaneOp::Balance => {
                self.balance();
                (true, None)
            }
            PaneOp::Resize(axis, d) => (self.resize(axis, d as f32 / 100.0), None),
            PaneOp::Swap => (self.swap_next(), None),
            PaneOp::Rotate(back) => (self.rotate(back), None),
            PaneOp::Undo => (self.undo(), None),
            PaneOp::Redo => (self.redo(), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 100.0,
        h: 100.0,
    };

    fn rect(l: &Layout, p: PaneId) -> Rect {
        l.rects(UNIT).into_iter().find(|(q, _)| *q == p).unwrap().1
    }

    #[test]
    fn splits_and_focus_by_direction() {
        let mut l = Layout::new();
        let right = l.split(Axis::Row);
        assert_eq!(l.focus(), right);
        let below = l.split(Axis::Column);
        // 0 | right
        //   | below
        assert_eq!(l.panes(), [0, right, below]);
        assert_eq!(
            rect(&l, 0),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 50.0,
                h: 100.0
            }
        );
        assert_eq!(
            rect(&l, below),
            Rect {
                x: 50.0,
                y: 50.0,
                w: 50.0,
                h: 50.0
            }
        );
        assert_eq!(l.focus_dir(Dir::Up), Some(right));
        assert_eq!(l.focus_dir(Dir::Left), Some(0));
        assert_eq!(l.focus_dir(Dir::Left), None);
        assert_eq!(l.focus_dir(Dir::Right), Some(right));
        assert_eq!(l.focus_dir(Dir::Down), Some(below));
        // Splitting along the same axis makes siblings, not nesting.
        l.set_focus(0);
        let third = l.split(Axis::Row);
        assert_eq!(l.panes(), [0, third, right, below]);
        l.balance();
        assert!((rect(&l, 0).w - 100.0 / 3.0).abs() < 0.01);
        // Next and previous wrap; SPC w p goes back.
        assert_eq!(l.cycle(false), right);
        assert_eq!(l.cycle(true), third);
        assert_eq!(l.focus_previous(), Some(right));
    }

    #[test]
    fn close_only_undo() {
        let mut l = Layout::new();
        let a = l.split(Axis::Row);
        let b = l.split(Axis::Column);
        assert!(l.close(b));
        assert_eq!(l.panes(), [0, a]);
        assert_eq!(l.focus(), a);
        assert!(l.close(a));
        assert!(!l.close(0), "the last pane stays");
        assert_eq!(l.root(), &Node::Leaf(0));
        assert!(l.undo() && l.undo());
        assert_eq!(l.panes(), [0, a, b]);
        assert!(l.redo());
        assert_eq!(l.panes(), [0, a]);
        // Only, and back.
        let before = l.clone();
        l.toggle_only();
        assert_eq!(l.panes(), [a]);
        assert_eq!(l.hidden(), [0]);
        l.toggle_only();
        assert_eq!(l.panes(), before.panes());
        assert_eq!(l.removed_since(&[0, a, b]), [b]);
    }

    #[test]
    fn moving_swapping_rotating_resizing() {
        let mut l = Layout::new();
        let a = l.split(Axis::Row);
        let b = l.split(Axis::Row);
        // [0 a b]: b to the top, full width.
        assert!(l.move_to_edge(Dir::Up));
        assert_eq!(rect(&l, b).w, 100.0);
        assert_eq!(rect(&l, b).y, 0.0);
        assert!(rect(&l, 0).y > 0.0);
        assert!(l.undo());
        l.set_focus(0);
        assert!(l.swap_next());
        assert_eq!(l.panes(), [a, 0, b]);
        assert!(l.rotate(false));
        assert_eq!(l.panes(), [b, a, 0]);
        assert!(l.rotate(true));
        assert_eq!(l.panes(), [a, 0, b]);
        // Wider: the focused pane grows, its siblings shrink, sizes add up.
        let w0 = rect(&l, 0).w;
        assert!(l.resize(Axis::Row, 0.1));
        assert!(rect(&l, 0).w > w0);
        let sum: f32 = l.rects(UNIT).iter().map(|(_, r)| r.w).sum();
        assert!((sum - 100.0).abs() < 0.01);
        assert!(!l.resize(Axis::Column, 0.1), "no column split holds it");
        // A pane never shrinks to nothing.
        for _ in 0..50 {
            l.resize(Axis::Row, -0.2);
        }
        assert!(rect(&l, 0).w > 1.0);
    }

    #[test]
    fn apply_ops() {
        let mut l = Layout::new();
        assert_eq!(l.apply(&PaneOp::Split(Axis::Column)), (true, Some(1)));
        assert_eq!(l.apply(&PaneOp::Focus(Dir::Up)), (true, None));
        assert_eq!(l.focus(), 0);
        assert_eq!(l.apply(&PaneOp::New).1, Some(2));
        assert_eq!(l.apply(&PaneOp::Close(false)), (true, None));
        assert_eq!(l.panes(), [0, 1]);
    }
}
