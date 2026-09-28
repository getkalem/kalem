//! The outline sidebar (T1.5.15): the document's headings as a tree that
//! folds, a click jumps to a heading, and dragging one moves its subtree.

use std::collections::HashSet;

use gpui::{
    AppContext, Context, InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use kalem_core::view::OutlineItem;
use org_edit::{Assoc, Transaction};

use crate::editor::Editor;
use crate::theme::Theme;

/// The sidebar's state.
#[derive(Debug, Default)]
pub struct Outline {
    /// The headings.
    pub items: Vec<OutlineItem>,
    /// The text version of `items`.
    version: Option<u64>,
    /// Folded headings in the tree, by start.
    pub collapsed: HashSet<usize>,
}

impl Outline {
    /// Moves the folded headings through edits.
    pub fn map(&mut self, changes: &[Transaction]) {
        for tx in changes {
            self.collapsed = self
                .collapsed
                .iter()
                .map(|s| tx.map(*s, Assoc::After))
                .collect();
        }
    }

    /// Whether heading `i` has headings under it.
    pub fn has_children(&self, i: usize) -> bool {
        let level = self.items[i].level;
        self.items.get(i + 1).is_some_and(|n| n.level > level)
    }

    /// The headings that show: not under a folded one.
    pub fn shown(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut hide_below: Option<usize> = None;
        for (i, it) in self.items.iter().enumerate() {
            if let Some(l) = hide_below {
                if it.level > l {
                    continue;
                }
                hide_below = None;
            }
            out.push(i);
            if self.collapsed.contains(&it.start) {
                hide_below = Some(it.level);
            }
        }
        out
    }

    /// Where a heading dropped on heading `i` goes, and at what level:
    /// before `i` on its upper half; on its lower half after its subtree,
    /// or as its first child when its children show.
    pub fn drop_target(&self, i: usize, below: bool, len: usize) -> (usize, usize) {
        let it = &self.items[i];
        if !below {
            return (it.start, it.level);
        }
        if self.has_children(i) && !self.collapsed.contains(&it.start) {
            let child = &self.items[i + 1];
            return (child.start, child.level);
        }
        let end = self.items[i + 1..]
            .iter()
            .find(|n| n.level <= it.level)
            .map_or(len, |n| n.start);
        (end, it.level)
    }
}

/// A heading being dragged in the outline.
#[derive(Debug, Clone)]
pub struct DraggedHeading {
    /// Its start.
    pub start: usize,
    /// Its title, for the drag preview.
    pub title: SharedString,
}

/// What follows the pointer while a heading is dragged.
struct Preview(SharedString, Theme);

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
            .px(px(8.))
            .py(px(2.))
            .rounded(px(4.))
            .border_1()
            .border_color(self.1.border)
            .bg(self.1.bar)
            .text_color(self.1.foreground)
            .text_size(px(self.1.size * 0.9))
            .child(self.0.clone())
    }
}

impl Editor {
    /// Shows or hides the outline.
    pub fn toggle_outline(&mut self, cx: &mut Context<'_, Self>) {
        self.outline = match self.outline.take() {
            Some(_) => None,
            None => Some(Outline::default()),
        };
        cx.notify();
    }

    /// Jumps to the heading at `start`.
    pub fn jump_to(&mut self, start: usize, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.doc.move_cursor(start, false);
        self.after_change(cx);
        window.focus(&gpui::Focusable::focus_handle(self, cx), cx);
    }

    /// Moves the dragged heading to heading `i` (see
    /// [`Outline::drop_target`]). Headings folded in the tree stay folded,
    /// in the moved subtree too.
    pub fn drop_heading(
        &mut self,
        d: &DraggedHeading,
        i: usize,
        below: bool,
        cx: &mut Context<'_, Self>,
    ) {
        let len = self.doc.text().len();
        let Some(o) = self.outline.as_mut().filter(|o| i < o.items.len()) else {
            return;
        };
        let (to, level) = o.drop_target(i, below, len);
        // The moved headings' folds, in order: they go with the subtree.
        let mut moved = Vec::new();
        if let Some(f) = o.items.iter().position(|it| it.start == d.start) {
            let top = o.items[f].level;
            for (k, it) in o.items[f..].iter().enumerate() {
                if k > 0 && it.level <= top {
                    break;
                }
                moved.push((it.start, o.collapsed.remove(&it.start)));
            }
        }
        match self
            .doc
            .move_subtree(d.start, to, level, std::time::Instant::now())
        {
            Ok(()) => {
                let head = self.doc.selection.head;
                self.after_change(cx);
                if let Some(m) = self.doc.model()
                    && let Some(o) = &mut self.outline
                {
                    let items = kalem_core::view::outline_items(&m);
                    let now = items.iter().filter(|it| it.start >= head);
                    for (it, (_, folded)) in now.zip(&moved) {
                        if *folded {
                            o.collapsed.insert(it.start);
                        }
                    }
                }
            }
            Err(e) => {
                if let Some(o) = &mut self.outline {
                    o.collapsed
                        .extend(moved.iter().filter(|m| m.1).map(|m| m.0));
                }
                self.status = Some((e.message, true));
                cx.notify();
            }
        }
    }

    /// The outline sidebar, when shown.
    pub fn outline_panel(&mut self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        let mut o = self.outline.take()?;
        let version = self.doc.version();
        if o.version != Some(version)
            && let Some(m) = self.doc.model()
        {
            o.items = kalem_core::view::outline_items(&m);
            o.version = Some(version);
            let starts: HashSet<usize> = o.items.iter().map(|i| i.start).collect();
            o.collapsed.retain(|s| starts.contains(s));
        }
        let head = self.doc.selection.head;
        let current = o.items.iter().rposition(|i| i.start <= head);
        let theme = self.theme.clone();
        let mut rows = Vec::new();
        for i in o.shown() {
            let it = &o.items[i];
            let start = it.start;
            let folded = o.collapsed.contains(&start);
            let title: SharedString = it.title.clone().into();
            let arrow = if !o.has_children(i) {
                ""
            } else if folded {
                "▸"
            } else {
                "▾"
            };
            let mut row = div()
                .id(("outline-row", i))
                .debug_selector(|| format!("outline-{i}"))
                .relative()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .pl(px(6. + 14. * (it.level.saturating_sub(1).min(8)) as f32))
                .pr(px(8.))
                .py(px(2.))
                .cursor_pointer()
                .child(
                    div()
                        .id(("outline-fold", i))
                        .debug_selector(|| format!("outline-fold-{i}"))
                        .w(px(12.))
                        .flex_none()
                        .text_color(theme.muted)
                        .child(arrow)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(o) = &mut this.outline
                                && !o.collapsed.remove(&start)
                            {
                                o.collapsed.insert(start);
                            }
                            cx.stop_propagation();
                            cx.notify();
                        })),
                );
            if Some(i) == current {
                row = row.bg(theme.selection);
            }
            if let Some((kw, done)) = &it.todo {
                row = row.child(
                    div()
                        .flex_none()
                        .text_color(if *done { theme.done } else { theme.todo })
                        .child(SharedString::from(kw.clone())),
                );
            }
            let color = theme.level(it.level.min(6) as u8);
            row = row.child(div().truncate().text_color(color).child(title.clone()));
            let zone = |below: bool| {
                let caret = theme.caret;
                let z = div()
                    .absolute()
                    .left_0()
                    .w_full()
                    .h(relative(0.5))
                    .drag_over::<DraggedHeading>(move |s, _, _, _| {
                        if below {
                            s.border_b_2().border_color(caret)
                        } else {
                            s.border_t_2().border_color(caret)
                        }
                    })
                    .on_drop(cx.listener(move |this, d: &DraggedHeading, _, cx| {
                        this.drop_heading(d, i, below, cx);
                    }));
                if below {
                    z.top(relative(0.5))
                } else {
                    z.top_0()
                }
            };
            let preview_theme = theme.clone();
            row = row
                .child(zone(false))
                .child(zone(true))
                .on_click(cx.listener(move |this, _, window, cx| this.jump_to(start, window, cx)))
                .on_drag(
                    DraggedHeading {
                        start,
                        title: title.clone(),
                    },
                    move |d, _, _, cx| {
                        let t = preview_theme.clone();
                        cx.new(|_| Preview(d.title.clone(), t))
                    },
                );
            rows.push(row);
        }
        let panel = div()
            .id("outline")
            .flex_none()
            .w(px(260.))
            .h_full()
            .overflow_y_scroll()
            .py(px(8.))
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.bar)
            .text_size(px(theme.size * 0.9))
            .children(rows)
            .into_any_element();
        self.outline = Some(o);
        Some(panel)
    }
}
