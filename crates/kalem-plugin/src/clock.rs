//! The time and chance (`clock`, granted to extensions since API 0.2.10):
//! the day a journal is for, a new block's ID; and the local time
//! (`clock-2`, API 0.2.11).

use crate::extension::kalem::plugin::clock as api;
use crate::extension::kalem::plugin::clock_2 as api2;

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

/// Seconds east of UTC in the user's time zone at `at` (milliseconds
/// since 1970, UTC), daylight saving time included; 0 where the zone is
/// not known (API 0.2.11).
pub fn offset(at: i64) -> i32 {
    api2::offset(at)
}

/// Milliseconds since January 1, 1970 in the user's local time: [`now`]
/// with the time zone's [`offset`] (API 0.2.11).
pub fn local_now() -> i64 {
    let now = now();
    now + i64::from(offset(now)) * 1000
}
