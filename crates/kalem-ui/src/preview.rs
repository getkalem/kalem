//! The file manager's preview pane (T2.7e.11): beside a listing, the file
//! at the cursor (a picture, the first lines of a text, a folder's names),
//! or the listing's pictures as thumbnails (`image-dired`), a click on one
//! going to its line.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use gpui::{
    Context, InteractiveElement, IntoElement, ObjectFit, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, StyledImage, div, img, px,
};
use kalem_core::dired::Preview;

use crate::editor::Editor;

/// The preview read last: its file, the file's time, and what it shows.
pub type Cache = RefCell<Option<(PathBuf, Option<SystemTime>, Rc<Preview>)>>;

/// The pane's width.
const WIDTH: f32 = 360.;
/// A thumbnail's side.
const THUMB: f32 = 96.;

impl Editor {
    /// What the pane shows for `path`, read again when the file changes.
    fn preview_of(&self, path: &std::path::Path) -> Rc<Preview> {
        let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
        if let Some((p, t, v)) = self.preview_cache.borrow().as_ref()
            && p == path
            && *t == modified
        {
            return v.clone();
        }
        let v = Rc::new(kalem_core::dired::preview(path));
        *self.preview_cache.borrow_mut() = Some((path.to_path_buf(), modified, v.clone()));
        v
    }

    /// The preview pane, when shown in a listing.
    pub fn preview_panel(&mut self, cx: &mut Context<'_, Self>) -> Option<gpui::AnyElement> {
        let thumbnails = self.preview?;
        let d = self.doc.dired.as_deref()?;
        let theme = self.theme.clone();
        let pane = div()
            .id("preview")
            .debug_selector(|| "preview".into())
            .w(px(WIDTH))
            .flex_none()
            .h_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(theme.border)
            .bg(theme.bar)
            .p(px(8.))
            .text_size(px(theme.size * 0.85));
        if thumbnails {
            let pictures = kalem_core::dired::thumbnails(d);
            let mut grid = div().flex().flex_row().flex_wrap().gap(px(8.));
            for (i, p) in pictures.into_iter().enumerate() {
                let name: SharedString = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
                    .into();
                let mut cell = div()
                    .id(("thumbnail", i))
                    .debug_selector(move || format!("thumbnail-{i}"))
                    .w(px(THUMB))
                    .flex()
                    .flex_col()
                    .items_center()
                    .cursor_pointer();
                if let Some((image, _, _)) = self.shared.pictures.get(&p) {
                    cell = cell.child(
                        img(image)
                            .w(px(THUMB))
                            .h(px(THUMB))
                            .object_fit(ObjectFit::Contain),
                    );
                }
                let path = p.clone();
                cell = cell
                    .child(div().w_full().truncate().child(name))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let line = this.doc.dired.as_deref().and_then(|d| d.line_of(&path));
                        if let Some(line) = line {
                            let start = this.doc.text().line_start(line);
                            let col = this
                                .doc
                                .dired
                                .as_deref()
                                .and_then(|d| d.name_range(line))
                                .map_or(0, |r| r.start);
                            this.doc.move_cursor(start + col, false);
                            this.after_change(cx);
                        }
                    }));
                grid = grid.child(cell);
            }
            return Some(pane.child(grid).into_any_element());
        }
        let line = self.doc.text().line_of(self.doc.selection.head);
        let path = d.path_at(line)?;
        let body = match &*self.preview_of(&path) {
            Preview::Picture => match self.shared.pictures.get(&path) {
                Some((image, _, _)) => img(image)
                    .w(px(WIDTH - 16.))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                None => div()
                    .text_color(theme.muted)
                    .child(kalem_core::tr!("fm-preview-none"))
                    .into_any_element(),
            },
            Preview::Text(t) => div()
                .font_family(SharedString::from(theme.mono.clone()))
                .flex()
                .flex_col()
                .children(t.lines().map(|l| {
                    div().child(SharedString::from(if l.is_empty() {
                        " ".to_string()
                    } else {
                        l.to_string()
                    }))
                }))
                .into_any_element(),
            Preview::Folder(names) => div()
                .flex()
                .flex_col()
                .children(
                    names
                        .iter()
                        .map(|n| div().child(SharedString::from(n.clone()))),
                )
                .into_any_element(),
            Preview::Nothing(why) => div()
                .text_color(theme.muted)
                .child(SharedString::from(why.clone()))
                .into_any_element(),
        };
        Some(pane.child(body).into_any_element())
    }
}
