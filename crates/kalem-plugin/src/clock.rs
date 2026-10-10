//! The time and chance (`clock`, granted to extensions since API 0.2.10):
//! the day a journal is for, a new block's ID.

use crate::extension::kalem::plugin::clock as api;

/// Milliseconds since January 1, 1970, UTC.
pub fn now() -> i64 {
    api::now()
}

/// The user's time zone, as the IANA database names it
/// (`Europe/Istanbul`), or `UTC` when it is unknown.
pub fn timezone() -> String {
    api::timezone()
}

/// 64 random bits.
pub fn random() -> u64 {
    api::random()
}
