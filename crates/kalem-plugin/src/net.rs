//! `kalem.net` of design §11.4: HTTP, reached only with a permission of
//! the manifest (`net:fetch:DOMAIN`). A fetch does not wait: `done` gets
//! the response later.

use crate::extension::kalem::plugin::net as api;
use crate::kalem::HANDLERS;

pub use crate::extension::kalem::plugin::http::{Header, Request, Response};

/// Sends `request`; `done` gets the response, or why there is none.
pub fn fetch(
    request: &Request,
    done: impl FnOnce(Result<Response, String>) + 'static,
) -> Result<(), String> {
    let id = api::fetch(request)?;
    HANDLERS.with(|h| h.borrow_mut().responses.insert(id, Box::new(done)));
    Ok(())
}

/// A `GET` of `url` without headers.
pub fn get(url: &str) -> Request {
    Request {
        method: "GET".into(),
        url: url.into(),
        headers: Vec::new(),
        body: None,
    }
}
