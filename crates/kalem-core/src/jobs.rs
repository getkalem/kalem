//! Work that runs in the background for a command, such as compiling a
//! PDF: the command starts it, and both frontends show its status while
//! it runs and its message when it ends ([`status`], [`take_finished`]).

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

/// What a finished job tells the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    /// The message.
    pub message: String,
    /// Whether it failed.
    pub error: bool,
    /// What to open afterwards (the PDF, when `export.open_after` is on).
    pub open: Option<crate::input::LinkAction>,
}

struct Job {
    status: String,
    rx: Receiver<Finished>,
}

static JOBS: Mutex<Vec<Job>> = Mutex::new(Vec::new());

/// Runs `work` on a thread; `status` is shown while it runs.
pub fn spawn(status: String, work: impl FnOnce() -> Finished + Send + 'static) {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let out = work();
        let _ = tx.send(out);
    });
    if let Ok(mut jobs) = JOBS.lock() {
        jobs.push(Job { status, rx });
    }
}

/// The status of the oldest job still running.
pub fn status() -> Option<String> {
    JOBS.lock().ok()?.first().map(|j| j.status.clone())
}

/// Whether a job is running.
pub fn running() -> bool {
    JOBS.lock().is_ok_and(|j| !j.is_empty())
}

/// The jobs that ended since the last call, in the order they ended.
pub fn take_finished() -> Vec<Finished> {
    let Ok(mut jobs) = JOBS.lock() else {
        return Vec::new();
    };
    let mut done = Vec::new();
    jobs.retain(|j| match j.rx.try_recv() {
        Ok(f) => {
            done.push(f);
            false
        }
        Err(TryRecvError::Empty) => true,
        Err(TryRecvError::Disconnected) => {
            done.push(Finished {
                message: crate::l10n::tr("msg-job-failed"),
                error: true,
                open: None,
            });
            false
        }
    });
    done
}

/// Waits until every job has ended and returns what they said (tests,
/// batch use).
pub fn wait_all() -> Vec<Finished> {
    let mut all = Vec::new();
    while running() {
        all.extend(take_finished());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    all.extend(take_finished());
    all
}
