//! The settings panel (T1.5.19): theme, keys, text size and font. A
//! change is written to the user's `settings.toml` (comments kept) and
//! applies to every window at once.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use gpui::{
    Context, InteractiveElement, IntoElement, Keystroke, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use kalem_core::l10n::tr;
use kalem_core::settings::{self, Config, Layer};
use serde_json::Value;

use crate::editor::{Editor, Shared};
use crate::theme::Theme;
use crate::workspace::Workspace;

/// The open settings panel.
#[derive(Debug, Clone, Default)]
pub struct SettingsPanel {
    /// Typed text that filters the fonts.
    pub filter: String,
    fonts: Vec<String>,
}

impl SettingsPanel {
    /// The fonts matching the filter, the system font first.
    pub fn fonts(&self) -> Vec<&str> {
        let f = self.filter.to_lowercase();
        let mut out = vec![""];
        out.extend(
            self.fonts
                .iter()
                .map(String::as_str)
                .filter(|n| n.to_lowercase().contains(&f)),
        );
        out.truncate(9);
        out
    }
}

/// Settings, commands and keys for `config`, keeping what `old` was
/// given from outside (tests replace the clipboard reader).
pub fn rebuild(old: &Shared, config: Config) -> Shared {
    Shared {
        html_clipboard: old.html_clipboard,
        settings_path: old.settings_path.clone(),
        jobs: old.jobs.clone(),
        ..crate::shared(config)
    }
}

impl Editor {
    /// Opens the settings panel.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let mut fonts = window.text_system().all_font_names();
        fonts.sort();
        fonts.dedup();
        fonts.retain(|f| !f.starts_with('.'));
        self.palette = None;
        self.completion = None;
        self.settings = Some(SettingsPanel {
            filter: String::new(),
            fonts,
        });
        cx.notify();
    }

    /// Typed text for the settings panel: the font filter.
    pub fn settings_input(&mut self, text: &str, cx: &mut Context<'_, Self>) -> bool {
        let Some(s) = &mut self.settings else {
            return false;
        };
        s.filter.push_str(text);
        cx.notify();
        true
    }

    /// Keys for the settings panel; `true` if used.
    pub fn settings_key(
        &mut self,
        k: &Keystroke,
        _window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> bool {
        let Some(s) = &mut self.settings else {
            return false;
        };
        match k.key.as_str() {
            "escape" => self.settings = None,
            "backspace" => {
                s.filter.pop();
            }
            "enter" => {
                if let Some(f) = s.fonts().get(1).map(|f| f.to_string()) {
                    self.set_setting("editor.font_family", Value::from(f), cx);
                }
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    /// Saves `key` in the user's settings and applies the settings to
    /// every window.
    pub fn set_setting(&mut self, key: &str, value: Value, cx: &mut Context<'_, Self>) {
        let Some(path) = self.shared.settings_path.clone() else {
            self.status = Some((tr("msg-no-settings-dir"), true));
            return;
        };
        if let Err(e) = settings::save_setting(&path, key, &value) {
            self.status = Some((e, true));
            return;
        }
        let workspace = self
            .shared
            .config
            .sources()
            .iter()
            .find(|(l, _)| *l == Layer::Workspace)
            .and_then(|(_, p)| p.clone());
        let config = Config::load(Some(&path), workspace.as_deref());
        let shared = Rc::new(rebuild(&self.shared, config));
        self.status = Some((kalem_core::tr!("msg-setting-saved", key = key), false));
        // After this update: every window, this one too.
        cx.defer(move |cx| apply(shared, cx));
    }

    /// The settings panel, when open.
    pub fn settings_view(&self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        let s = self.settings.as_ref()?;
        let theme = &self.theme;
        let config = &self.shared.config;
        let choice =
            |id: &'static str, label: String, on: bool, key: &'static str, value: Value| {
                div()
                    .id(id)
                    .debug_selector(|| id.to_string())
                    .px(px(10.))
                    .py(px(3.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(theme.border)
                    .cursor_pointer()
                    .when(on, |d| d.bg(theme.selection))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_setting(key, value.clone(), cx);
                    }))
            };
        let row = |label: String| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(div().w(px(90.)).text_color(theme.muted).child(label))
        };
        let current_theme = config.str("editor.theme");
        let language = config.str("ui.language");
        let profile = config.str("editor.keymap_profile");
        let size = config.int("editor.font_size");
        let family = config.str("editor.font_family").to_string();
        let fonts = s.fonts().into_iter().enumerate().map(|(i, f)| {
            let name = f.to_string();
            let label = if name.is_empty() {
                tr("settings-system-font")
            } else {
                name.clone()
            };
            div()
                .id(("settings-font", i))
                .debug_selector(|| format!("settings-font-{i}"))
                .px(px(8.))
                .py(px(1.))
                .cursor_pointer()
                .when(name == family, |d| d.bg(theme.selection))
                .child(SharedString::from(label))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_setting("editor.font_family", Value::from(name.clone()), cx);
                }))
        });
        let panel = div()
            .id("settings")
            .debug_selector(|| "settings".into())
            .occlude()
            .w(px(460.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(14.))
            .rounded(px(8.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.bar)
            .text_size(px(theme.size.min(18.) * 0.85))
            .child(
                div()
                    .text_color(theme.foreground)
                    .child(tr("settings-title")),
            )
            .child(
                row(tr("settings-theme"))
                    .child(choice(
                        "settings-theme-system",
                        tr("settings-theme-system"),
                        current_theme == "system",
                        "editor.theme",
                        "system".into(),
                    ))
                    .child(choice(
                        "settings-theme-light",
                        tr("settings-theme-light"),
                        current_theme == "light",
                        "editor.theme",
                        "light".into(),
                    ))
                    .child(choice(
                        "settings-theme-dark",
                        tr("settings-theme-dark"),
                        current_theme == "dark",
                        "editor.theme",
                        "dark".into(),
                    )),
            )
            .child(
                row(tr("settings-keys"))
                    .child(choice(
                        "settings-keys-word",
                        tr("settings-keys-word"),
                        profile == "word",
                        "editor.keymap_profile",
                        "word".into(),
                    ))
                    .child(choice(
                        "settings-keys-vim",
                        tr("settings-keys-vim"),
                        profile == "vim",
                        "editor.keymap_profile",
                        "vim".into(),
                    )),
            )
            .child(
                row(tr("settings-language"))
                    .child(choice(
                        "settings-language-auto",
                        tr("settings-language-auto"),
                        language == "auto",
                        "ui.language",
                        "auto".into(),
                    ))
                    .child(choice(
                        "settings-language-en",
                        "English".into(),
                        language == "en",
                        "ui.language",
                        "en".into(),
                    ))
                    .child(choice(
                        "settings-language-tr",
                        "Türkçe".into(),
                        language == "tr",
                        "ui.language",
                        "tr".into(),
                    )),
            )
            .child(
                row(tr("settings-size"))
                    .child(choice(
                        "settings-size-down",
                        "−".into(),
                        false,
                        "editor.font_size",
                        (size - 1).max(6).into(),
                    ))
                    .child(SharedString::from(size.to_string()))
                    .child(choice(
                        "settings-size-up",
                        "+".into(),
                        false,
                        "editor.font_size",
                        (size + 1).min(72).into(),
                    )),
            )
            .child(row(tr("settings-font")).child(SharedString::from(format!("{}▏", s.filter))))
            .child(div().flex().flex_col().pl(px(96.)).children(fonts))
            .child(
                div()
                    .id("settings-file")
                    .debug_selector(|| "settings-file".into())
                    .text_color(theme.link)
                    .cursor_pointer()
                    .child(tr("settings-open-file"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(p) = this.shared.settings_path.clone() {
                            let shared = this.shared.clone();
                            this.settings = None;
                            let _ = shared;
                            cx.emit(crate::editor::DocEvent::Open { path: p, at: None });
                        }
                    })),
            );
        Some(
            div()
                .absolute()
                .top(px(8.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(panel)
                .into_any_element(),
        )
    }
}

/// Gives every window the settings of `shared`: the language, commands
/// and keys, and the theme; the menus follow.
pub fn apply(shared: Rc<Shared>, cx: &mut gpui::App) {
    kalem_core::l10n::set_language(shared.config.str("ui.language"));
    cx.set_menus(crate::workspace::menus());
    cx.clear_key_bindings();
    cx.bind_keys(crate::workspace::menu_bindings(&shared));
    for w in cx.windows() {
        let Some(w) = w.downcast::<Workspace>() else {
            continue;
        };
        let shared = shared.clone();
        let _ = w.update(cx, |ws, window, cx| {
            let theme = Theme::from_config(&shared.config, window.appearance());
            ws.shared = shared.clone();
            ws.files_at =
                crate::workspace::FilesAt::from_setting(shared.config.str("ui.open_files"));
            ws.files_shown = ws.files_at != crate::workspace::FilesAt::Hidden;
            for e in ws.editors.clone() {
                let (shared, theme) = (shared.clone(), theme.clone());
                e.update(cx, |e, cx| {
                    e.shared = shared;
                    e.vim = None;
                    e.refresh_vim();
                    e.theme = theme;
                    e.relayout();
                    cx.notify();
                });
            }
            cx.notify();
        });
    }
}
