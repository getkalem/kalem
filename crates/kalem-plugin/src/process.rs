//! `kalem.process` (API 0.2.4): programs, reached only with a permission
//! of the manifest naming each (`subprocess:git`). A run does not wait:
//! `done` gets how it ended, later.
//!
//! ```ignore
//! use kalem_plugin::process::{self, Command};
//!
//! process::run(
//!     &Command::new("git").args(["status", "--porcelain=v2"]).cwd(root),
//!     |result| match result {
//!         Ok(exit) if exit.status == Some(0) => { /* read exit.stdout */ }
//!         Ok(exit) => { /* exit.stderr says why */ }
//!         Err(e) => { /* the program could not start */ }
//!     },
//! )?;
//! ```

use crate::extension::kalem::plugin::process as api;
use crate::kalem::HANDLERS;

pub use crate::extension::kalem::plugin::process::Exit;

/// A program to run.
#[derive(Debug, Clone)]
pub struct Command(api::Command);

impl Command {
    /// `program`, as the manifest's `subprocess:NAME` names it.
    pub fn new(program: &str) -> Command {
        Command(api::Command {
            program: program.to_string(),
            args: Vec::new(),
            cwd: None,
            stdin: None,
            env: Vec::new(),
        })
    }

    /// Its arguments, after those given before.
    pub fn args<I, S>(mut self, args: I) -> Command
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.0.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Its folder: absolute, inside a project Kalem knows.
    pub fn cwd(mut self, dir: &str) -> Command {
        self.0.cwd = Some(dir.to_string());
        self
    }

    /// What it reads on its standard input.
    pub fn stdin(mut self, bytes: Vec<u8>) -> Command {
        self.0.stdin = Some(bytes);
        self
    }

    /// A variable added to the user's environment.
    pub fn env(mut self, key: &str, value: &str) -> Command {
        self.0.env.push((key.to_string(), value.to_string()));
        self
    }
}

/// Starts `command`; `done` gets how it ended, or why it could not start.
/// Its number, for [`kill`].
pub fn run(
    command: &Command,
    done: impl FnOnce(Result<Exit, String>) + 'static,
) -> Result<u64, String> {
    let id = api::run(&command.0)?;
    HANDLERS.with(|h| h.borrow_mut().processes.insert(id, Box::new(done)));
    Ok(id)
}

/// Stops run `run`; `done` still gets its end, without a status.
pub fn kill(run: u64) {
    api::kill(run);
}
