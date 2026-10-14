//! Formatting by a plugin (API 0.2.16): its languages' documents written
//! as they write them, for Format Document and `editor.format_on_save`. A
//! plugin declares its formatters in its manifest (`"formatters": [{"id",
//! "types"}]`), builds with the feature `formatter`, and answers
//! [`crate::kalem::Plugin::format`]: from the text alone, quickly, as the
//! editor waits for it.
//!
//! ```ignore
//! fn format(_id: &str, r: &formatter::Request) -> Result<String, formatter::Refusal> {
//!     Ok(r.text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n") + "\n")
//! }
//! ```

#[cfg(feature = "formatter")]
pub use crate::extension::exports::kalem::plugin::formatter::{Refusal, Request};

/// A refusal saying `message`, at byte `at` of the text when given (the
/// editor puts the cursor there).
#[cfg(feature = "formatter")]
pub fn refusal(message: &str, at: Option<usize>) -> Refusal {
    Refusal {
        message: message.to_string(),
        at: at.map(|a| a as u64),
    }
}
