//! Source blocks run by a plugin (API 0.2.21): Org's Babel for the
//! languages of its manifest's `executors` (`"executors": [{"language":
//! "ledger", "headers": ":results output :cmdline bal"}]`), as an
//! `ob-LANG.el` library gives Emacs its `org-babel-execute:LANG`. A plugin
//! builds with the feature `executor` and answers
//! [`crate::kalem::Plugin::prepare_block`] with the program a block runs
//! (one its manifest names, `subprocess:NAME`), which Kalem runs beside
//! the editor, and [`crate::kalem::Plugin::finish_block`] with the
//! block's value from what the program wrote. Kalem asks the user first,
//! as Emacs does, and writes the value under the block's `#+RESULTS:`.
//!
//! ```ignore
//! fn prepare_block(b: &executor::Block) -> Result<executor::Step, String> {
//!     let cmdline = executor::header(b, ":cmdline").unwrap_or("bal");
//!     let mut args = vec!["-f".to_string(), "-".to_string()];
//!     args.extend(cmdline.split_whitespace().map(String::from));
//!     Ok(executor::run("ledger", args, Some(&b.body)))
//! }
//!
//! fn finish_block(_: &executor::Block, out: &str, _: Option<i32>) -> Result<executor::Value, String> {
//!     Ok(executor::Value::Text(out.to_string()))
//! }
//! ```

#[cfg(feature = "executor")]
pub use crate::extension::exports::kalem::plugin::executor::{Block, Command, Row, Step, Value};

/// The value of `block`'s header argument `name` (`:cmdline`), the last
/// given; none when it is not given.
#[cfg(feature = "executor")]
pub fn header<'a>(block: &'a Block, name: &str) -> Option<&'a str> {
    block
        .headers
        .iter()
        .rev()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// Whether `block`'s `:results` has the word `word` (`output`).
#[cfg(feature = "executor")]
pub fn results_has(block: &Block, word: &str) -> bool {
    block.results.iter().any(|r| r == word)
}

/// The step that runs `program` with `args`, `stdin` written to its
/// standard input.
#[cfg(feature = "executor")]
pub fn run(program: &str, args: Vec<String>, stdin: Option<&str>) -> Step {
    Step::Run(Command {
        program: program.to_string(),
        args,
        stdin: stdin.map(str::to_string),
        env: Vec::new(),
    })
}
