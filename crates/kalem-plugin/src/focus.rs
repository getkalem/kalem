//! The document the user works in (API 0.2.18): the plugin's handlers
//! are called each time another document gets the focus, a switch back
//! to one open already included, which `document-open` does not tell.
//! [`export_plugin!`](crate::export_plugin) exports the `focus` interface
//! that the editor calls.
//!
//! A Kalem of an earlier API calls none, and runs the plugin as before: a
//! plugin following the current document learns from the first call that
//! it is told, and until then takes the document opened last for it.
//!
//! ```ignore
//! kalem_plugin::focus::on(|document, path| {
//!     // Show what belongs to the document of `path`.
//! });
//! ```

use std::cell::RefCell;

type FocusFn = Box<dyn FnMut(u64, Option<&str>)>;

thread_local! {
    static HANDLERS: RefCell<Vec<Option<FocusFn>>> = const { RefCell::new(Vec::new()) };
}

/// Calls `handler` with the number (as `document-open` gave it) and the
/// file, if any, of each document that gets the focus, in the order the
/// handlers were added. They stay until the plugin is deactivated.
pub fn on(handler: impl FnMut(u64, Option<&str>) + 'static) {
    HANDLERS.with(|h| h.borrow_mut().push(Some(Box::new(handler))));
}

/// Hands a document given the focus to the handlers (the host's
/// `focused`). Each is taken out while it runs, so that it may add others.
#[doc(hidden)]
pub fn dispatch_focused(document: u64, path: Option<&str>) {
    let count = HANDLERS.with(|h| h.borrow().len());
    for i in 0..count {
        let taken = HANDLERS.with(|h| h.borrow_mut().get_mut(i).and_then(Option::take));
        let Some(mut f) = taken else {
            continue;
        };
        f(document, path);
        HANDLERS.with(|h| {
            if let Some(slot @ None) = h.borrow_mut().get_mut(i) {
                *slot = Some(f);
            }
        });
    }
}

/// Forgets the handlers (the plugin's deactivation).
pub(crate) fn clear() {
    HANDLERS.with(|h| h.borrow_mut().clear());
}
