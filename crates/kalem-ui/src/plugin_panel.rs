//! A plugin's panel in the window (`kalem_core::extensions`, D11): its
//! widgets drawn with gpui's elements, at the side or at the bottom; a
//! click on a button, an entry, a checkbox or an input reaches the plugin.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use kalem_core::extensions::{Panel, TextStyle, WidgetKind};

use crate::editor::Editor;

impl Editor {
    /// The plugin's panel shown, and whether it stands at the bottom.
    pub fn plugin_panel_view(&mut self, cx: &mut Context<'_, Self>) -> Option<(AnyElement, bool)> {
        let id = self.plugin_panel.clone()?;
        let Some(p) = kalem_core::extensions::panel(&id) else {
            // Its plugin went.
            self.plugin_panel = None;
            return None;
        };
        let theme = self.theme.clone();
        let mut rows = Vec::new();
        for (i, depth) in p.lines() {
            let parts: Vec<usize> = match p.widgets[i].kind {
                WidgetKind::Row => p.widgets[i].children.iter().map(|&c| c as usize).collect(),
                _ => vec![i],
            };
            let row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .pl(px(10. + 14. * depth.min(8) as f32))
                .pr(px(10.))
                .py(px(2.))
                .children(parts.into_iter().map(|j| self.widget(&p, j, cx)));
            rows.push(row);
        }
        let panel = div()
            .id("plugin-panel")
            .flex_none()
            .overflow_y_scroll()
            .py(px(8.))
            .bg(theme.bar)
            .border_color(theme.border)
            .text_size(px(theme.size * 0.9))
            .child(
                div()
                    .px(px(10.))
                    .pb(px(6.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(SharedString::from(p.title.clone())),
            )
            .children(rows);
        let panel = if p.bottom {
            panel.w_full().h(px(220.)).border_t_1()
        } else {
            panel.w(px(260.)).h_full().border_l_1()
        };
        Some((panel.into_any_element(), p.bottom))
    }

    /// Widget `j` of panel `p`.
    fn widget(&self, p: &Panel, j: usize, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = &self.theme;
        let w = &p.widgets[j];
        let text = SharedString::from(kalem_core::extensions::widget_text(w));
        let base = div().id(("plugin-widget", j));
        let el = match &w.kind {
            WidgetKind::Label { text: t, style } => {
                let el = base.child(SharedString::from(t.clone()));
                match style {
                    TextStyle::Heading => el
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_size(px(theme.size)),
                    TextStyle::Strong => el.font_weight(gpui::FontWeight::BOLD),
                    TextStyle::Emphasis => el.italic(),
                    TextStyle::Muted => el.text_color(theme.muted),
                    TextStyle::Code => el.font_family(SharedString::from(theme.mono.clone())),
                    TextStyle::Error => el.text_color(theme.todo),
                    TextStyle::Normal => el,
                }
            }
            WidgetKind::Button { label, .. } => base
                .px(px(8.))
                .py(px(1.))
                .rounded(px(4.))
                .border_1()
                .border_color(theme.border)
                .child(SharedString::from(label.clone())),
            WidgetKind::Input { value, placeholder } => {
                let empty = value.is_empty();
                base.flex_1()
                    .px(px(6.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme.border)
                    .text_color(if empty { theme.muted } else { theme.foreground })
                    .child(SharedString::from(if empty {
                        placeholder.clone().unwrap_or_default()
                    } else {
                        value.clone()
                    }))
            }
            WidgetKind::Item { selected, .. } => {
                let el = base.child(text);
                if *selected {
                    el.bg(theme.selection)
                } else {
                    el
                }
            }
            WidgetKind::Progress { value, label } => base
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .w(px(120.))
                        .h(px(6.))
                        .rounded(px(3.))
                        .bg(theme.border)
                        .child(
                            div()
                                .h_full()
                                .rounded(px(3.))
                                .bg(theme.caret)
                                .w(relative(value.unwrap_or(0.0).clamp(0.0, 1.0))),
                        ),
                )
                .children(label.clone().map(SharedString::from)),
            WidgetKind::Separator => base.w_full().h(px(1.)).bg(theme.border),
            _ => base.child(text),
        };
        let acted_on = matches!(
            w.kind,
            WidgetKind::Button { .. }
                | WidgetKind::Input { .. }
                | WidgetKind::Checkbox { .. }
                | WidgetKind::Item { .. }
        );
        if !acted_on {
            return el.into_any_element();
        }
        let id = p.id.clone();
        el.cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.panel_click(&id, j, window, cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    /// A click on widget `j` of panel `id`.
    fn panel_click(&mut self, id: &str, j: usize, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(p) = kalem_core::extensions::panel(id) else {
            return;
        };
        if let Some(r) = kalem_core::extensions::activate(&p, j) {
            self.request(r, window, cx);
        }
        for (id, args) in kalem_core::extensions::take_runs() {
            self.run_command(&id, args, window, cx);
        }
        cx.notify();
    }
}
