//! A plugin's panel in the window (`kalem_core::extensions`, D11): its
//! widgets drawn with gpui's elements, at the side or at the bottom; a
//! click on a button, an entry, a checkbox or an input reaches the plugin.
//! A text too long for the panel wraps over a few lines at its widget's
//! indent, then ends in an ellipsis; the panel scrolls, and takes the keys
//! as the terminal's does (`kalem_tui::panels::PluginPanelView`).

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, Keystroke, ParentElement, ScrollHandle,
    SharedString, StatefulInteractiveElement, Styled, Window, div, point, px, relative,
};
use kalem_core::extensions::{Panel, TextStyle, WidgetKind};

use crate::editor::Editor;

/// The most lines a widget's text takes, as in the terminal.
const WIDGET_LINES: usize = 4;

/// The keys' side of a plugin's panel: whether it has them, the widget
/// they chose (by key), and how far the panel is scrolled.
#[derive(Debug, Default)]
pub struct PanelKeys {
    /// Keys go to the panel.
    pub focus: bool,
    /// The widget chosen by keys, by key.
    pub selected: Option<String>,
    /// The panel's scrolling: its children are the title, then a line for
    /// each of `Panel::lines`.
    pub scroll: ScrollHandle,
}

/// Whether a user acts on widget kind `kind`.
fn acted_on(kind: &WidgetKind) -> bool {
    matches!(
        kind,
        WidgetKind::Button { .. }
            | WidgetKind::Input { .. }
            | WidgetKind::Checkbox { .. }
            | WidgetKind::Item { .. }
    )
}

/// The line of `p` (an index into `Panel::lines`) widget `j` is on.
fn line_of(p: &Panel, j: usize) -> Option<usize> {
    p.lines().iter().position(|&(i, _)| {
        i == j
            || (matches!(p.widgets[i].kind, WidgetKind::Row)
                && p.widgets[i].children.contains(&(j as u32)))
    })
}

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
        let chosen = self
            .plugin_keys
            .selected
            .as_ref()
            .filter(|_| self.plugin_keys.focus)
            .and_then(|k| p.widgets.iter().position(|w| &w.key == k));
        let mut rows = Vec::new();
        for (i, depth) in p.lines() {
            let parts: Vec<usize> = match p.widgets[i].kind {
                WidgetKind::Row => p.widgets[i].children.iter().map(|&c| c as usize).collect(),
                _ => vec![i],
            };
            // A row's widgets side by side, those that do not fit below.
            let row = div()
                .flex()
                .flex_row()
                .flex_wrap()
                .items_start()
                .gap(px(8.))
                .pl(px(10. + 14. * depth.min(8) as f32))
                .pr(px(10.))
                .py(px(2.))
                .children(
                    parts
                        .into_iter()
                        .map(|j| self.widget(&p, j, chosen == Some(j), cx)),
                );
            rows.push(row);
        }
        let title = div()
            .px(px(10.))
            .pb(px(6.))
            .font_weight(gpui::FontWeight::BOLD)
            .child(SharedString::from(p.title.clone()));
        let panel = div()
            .id("plugin-panel")
            .flex_none()
            .overflow_y_scroll()
            .track_scroll(&self.plugin_keys.scroll)
            .py(px(8.))
            .bg(theme.bar)
            .border_color(theme.border)
            .text_size(px(theme.size * 0.9))
            .child(if self.plugin_keys.focus {
                title.text_color(theme.link)
            } else {
                title
            })
            .children(rows)
            // A click away from its widgets gives the panel the keys.
            .on_click(cx.listener(|this, _, _, cx| {
                this.focus_plugin_panel(cx);
            }));
        let panel = if p.bottom {
            panel.w_full().h(px(220.)).border_t_1()
        } else {
            panel.w(px(260.)).h_full().border_l_1()
        };
        Some((panel.into_any_element(), p.bottom))
    }

    /// Widget `j` of panel `p`; `chosen` by keys.
    fn widget(&self, p: &Panel, j: usize, chosen: bool, cx: &mut Context<'_, Self>) -> AnyElement {
        let theme = &self.theme;
        let w = &p.widgets[j];
        // A text that wraps where its widget ends, a few lines at most.
        let wrapped = |t: String| {
            div()
                .min_w_0()
                .line_clamp(WIDGET_LINES)
                .text_ellipsis()
                .child(SharedString::from(t))
        };
        let base = div().id(("plugin-widget", j)).min_w_0();
        let el = match &w.kind {
            WidgetKind::Label { text: t, style } => {
                let el = base.flex_1().child(wrapped(t.clone()));
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
                .flex_none()
                .px(px(8.))
                .py(px(1.))
                .rounded(px(4.))
                .border_1()
                .border_color(if chosen { theme.caret } else { theme.border })
                .child(SharedString::from(label.clone())),
            WidgetKind::Input { value, placeholder } => {
                let empty = value.is_empty();
                base.flex_1()
                    .px(px(6.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if chosen { theme.caret } else { theme.border })
                    .text_color(if empty { theme.muted } else { theme.foreground })
                    .truncate()
                    .child(SharedString::from(if empty {
                        placeholder.clone().unwrap_or_default()
                    } else {
                        value.clone()
                    }))
            }
            // The arrow, the label wrapping past it, the detail after.
            WidgetKind::Item {
                label,
                detail,
                expanded,
                selected,
            } => {
                let arrow = expanded.map(|e| if e { "▾" } else { "▸" });
                let el = base
                    .flex_1()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(6.))
                    .children(arrow.map(|a| div().flex_none().child(a)))
                    .child(wrapped(label.clone()).flex_initial())
                    .children(detail.clone().map(|d| {
                        div()
                            .flex_none()
                            .text_color(theme.muted)
                            .child(SharedString::from(d))
                    }));
                if *selected {
                    el.bg(theme.selection)
                } else {
                    el
                }
            }
            WidgetKind::Checkbox { label, checked } => base
                .flex_1()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(6.))
                .child(div().flex_none().child(if *checked { "☑" } else { "☐" }))
                .child(wrapped(label.clone()).flex_initial()),
            WidgetKind::Progress { value, label } => base
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .flex_none()
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
                .children(label.clone().map(wrapped)),
            WidgetKind::Separator => base.w_full().h(px(1.)).bg(theme.border),
            _ => base
                .flex_1()
                .child(wrapped(kalem_core::extensions::widget_text(w))),
        };
        if !acted_on(&w.kind) {
            return el.into_any_element();
        }
        // The widget chosen by keys, framed (a button or an input in the
        // caret's color, above).
        let el = if matches!(w.kind, WidgetKind::Button { .. } | WidgetKind::Input { .. }) {
            el
        } else {
            el.rounded(px(3.)).border_1().border_color(if chosen {
                theme.caret
            } else {
                gpui::transparent_black()
            })
        };
        let id = p.id.clone();
        el.cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.panel_click(&id, j, window, cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    /// A click on widget `j` of panel `id`: the widget chosen, acted on.
    fn panel_click(&mut self, id: &str, j: usize, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(p) = kalem_core::extensions::panel(id) else {
            return;
        };
        self.plugin_keys.selected = p.widgets.get(j).map(|w| w.key.clone());
        if let Some(r) = kalem_core::extensions::activate(&p, j) {
            self.request(r, window, cx);
        }
        for (id, args) in kalem_core::extensions::take_runs() {
            self.run_command(&id, args, window, cx);
        }
        cx.notify();
    }

    /// Gives the keys to the plugin's panel, a widget chosen.
    pub fn focus_plugin_panel(&mut self, cx: &mut Context<'_, Self>) {
        let Some(p) = self
            .plugin_panel
            .as_deref()
            .and_then(kalem_core::extensions::panel)
        else {
            return;
        };
        let k = &mut self.plugin_keys;
        k.focus = true;
        let chosen = k
            .selected
            .as_ref()
            .is_some_and(|s| p.widgets.iter().any(|w| &w.key == s));
        if !chosen {
            // The first that shows.
            let top = k.scroll.top_item().saturating_sub(1);
            let actions = p.actions();
            let first = actions
                .iter()
                .find(|&&i| line_of(&p, i).is_some_and(|l| l >= top))
                .or(actions.first());
            k.selected = first.map(|&i| p.widgets[i].key.clone());
        }
        cx.notify();
    }

    /// A move of the focus (`pane.focus`) toward direction `d`, from the
    /// focused plugin panel or, `edge` true, from the pane at the edge the
    /// panel stands at (the right, or the bottom): it gives the panel the
    /// keys, or gives them back to the text; whether it did.
    pub fn plugin_panel_focus(
        &mut self,
        d: kalem_core::layout::Dir,
        edge: bool,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        use kalem_core::layout::Dir;
        let Some(p) = self
            .plugin_panel
            .as_deref()
            .and_then(kalem_core::extensions::panel)
        else {
            return false;
        };
        let (toward, back) = if p.bottom {
            (Dir::Down, Dir::Up)
        } else {
            (Dir::Right, Dir::Left)
        };
        if self.plugin_keys.focus {
            // Back to the text; the other ways lead nowhere.
            if d == back {
                self.plugin_keys.focus = false;
                cx.notify();
            }
            return true;
        }
        if !edge || d != toward {
            return false;
        }
        self.focus_plugin_panel(cx);
        true
    }

    /// Keys for the focused plugin panel, as the terminal's: arrows (and
    /// Vim's `j` `k` `g` `G`, Emacs's Ctrl+N and Ctrl+P) choose among the
    /// widgets a user acts on, Page Up and Page Down (Ctrl+U, Ctrl+D)
    /// scroll, Enter (or Right, `l`, Space but in Vim's normal mode) acts
    /// on the widget chosen, Escape (or Tab, Left, `h`, `q`) gives the
    /// keys back. A key sequence begun goes on (Vim's Ctrl+W, the leader).
    /// `false` for keys it leaves alone.
    pub fn plugin_panel_key(
        &mut self,
        ks: &Keystroke,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let vim_idle = self.vim.as_ref().map(|v| v.idle_command());
        let begun = !self.pending.is_empty()
            || self
                .vim
                .as_ref()
                .is_some_and(|v| !v.idle_command() && !v.takes_text());
        if begun || (ks.key == "space" && vim_idle == Some(true)) {
            return false;
        }
        let Some(p) = self
            .plugin_panel
            .as_deref()
            .and_then(kalem_core::extensions::panel)
        else {
            self.plugin_keys.focus = false;
            return false;
        };
        let m = ks.modifiers;
        let ctrl = m.control;
        let plain = !m.control && !m.alt && !m.platform && !m.function;
        let actions = p.actions();
        let k = &mut self.plugin_keys;
        let at = k
            .selected
            .as_ref()
            .and_then(|s| actions.iter().position(|&i| &p.widgets[i].key == s));
        // The first widget a user acts on that shows, from line `top`.
        let shown = |top: usize| {
            actions
                .iter()
                .position(|&i| line_of(&p, i).is_some_and(|l| l >= top))
        };
        let first = shown(k.scroll.top_item().saturating_sub(1)).unwrap_or(0);
        let last = actions.len().saturating_sub(1);
        let (mut choose, mut page, mut act) = (None, None, None);
        let key = if ks.key == "g" && m.shift {
            "G"
        } else {
            ks.key.as_str()
        };
        match (key, plain, ctrl) {
            ("down", ..) | ("j", true, _) | ("n", _, true) => {
                choose = Some(at.map_or(first, |a| (a + 1).min(last)));
            }
            ("up", ..) | ("k", true, _) | ("p", _, true) => {
                choose = Some(at.map_or(first, |a| a.saturating_sub(1)));
            }
            ("pagedown", ..) | ("d", _, true) => page = Some(1.),
            ("pageup", ..) | ("u", _, true) => page = Some(-1.),
            ("home", ..) | ("g", true, _) => choose = Some(0),
            ("end", ..) | ("G", _, false) => choose = Some(last),
            ("enter" | "right", ..) | ("space" | "l", true, _) => act = at,
            ("escape" | "tab" | "left", ..) | ("h" | "q", true, _) => k.focus = false,
            // Other keys run commands (to close the panel, say) but do not
            // type.
            _ => return plain && ks.key_char.is_some(),
        }
        if let Some(sign) = page {
            let s = &k.scroll;
            let height = s.bounds().size.height;
            let y = (s.offset().y - height * (sign * 0.9)).clamp(-s.max_offset().y, px(0.));
            s.set_offset(point(s.offset().x, y));
            // The widget chosen scrolled away: the first that shows.
            let top = s.top_item().saturating_sub(1);
            let bottom = s.bottom_item().saturating_sub(1);
            let gone = at
                .and_then(|a| line_of(&p, actions[a]))
                .is_none_or(|l| l < top || l > bottom);
            if gone && let Some(n) = shown(top) {
                k.selected = Some(p.widgets[actions[n]].key.clone());
            }
        }
        if let Some(&i) = choose.and_then(|n| actions.get(n)) {
            k.selected = Some(p.widgets[i].key.clone());
            // Into view: its line's child, after the title.
            if let Some(l) = line_of(&p, i) {
                k.scroll.scroll_to_item(l + 1);
            }
        }
        if let Some(a) = act {
            self.panel_click(&p.id, actions[a], window, cx);
        }
        cx.notify();
        true
    }
}
