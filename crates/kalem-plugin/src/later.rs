//! The plugin's own commands run later (API 0.2.17): at an editor's tick
//! once some milliseconds have passed, after the editor has drawn and
//! taken the keys typed meanwhile. Long work goes a slice at a time, each
//! slice asking for the next:
//!
//! ```ignore
//! kalem_plugin::later::run("notes.index", "null", 0)?;
//! ```

use crate::extension::kalem::plugin::later as api;

/// Runs the plugin's own command `id` with `args` as JSON (`"null"` for
/// none) once `after_ms` milliseconds have passed, at an editor's tick,
/// never inside the call that asks; its number, for [`cancel`]. Refused
/// for another's command, or past 64 waiting.
pub fn run(id: &str, args: &str, after_ms: u32) -> Result<u64, String> {
    api::run(id, args, after_ms)
}

/// Drops run `run` if it has not run yet.
pub fn cancel(run: u64) {
    api::cancel(run)
}
