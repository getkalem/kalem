//! Work that runs in the background for a command, such as compiling a
//! PDF: the command starts it, and both frontends show its status while
//! it runs and its message when it ends ([`status`], [`take_finished`]).

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

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

/// Lists to choose from that background work offers (a plugin found, to
/// be confirmed): both frontends open them as `Request::Choose` does.
static OFFERS: Mutex<Vec<Vec<crate::palette::PaletteItem>>> = Mutex::new(Vec::new());

/// How long the keys rest before an offered list opens: one opened while
/// the user types (a plugin stopped, offering its restart) took the keys
/// typed next.
pub const OFFER_PAUSE: Duration = Duration::from_secs(1);

/// When a key was last typed in an editor ([`typed`]).
static TYPED: Mutex<Option<Instant>> = Mutex::new(None);

/// Offers `items` to choose from, from a job's thread.
pub fn offer(items: Vec<crate::palette::PaletteItem>) {
    if let Ok(mut o) = OFFERS.lock() {
        o.push(items);
    }
}

/// An editor took a key: the lists offered wait for the keys to rest
/// ([`OFFER_PAUSE`]).
pub fn typed() {
    if let Ok(mut t) = TYPED.lock() {
        *t = Some(Instant::now());
    }
}

/// The lists offered since the last call, once the keys have rested
/// [`OFFER_PAUSE`]; none before, those offered kept.
pub fn take_offers() -> Vec<Vec<crate::palette::PaletteItem>> {
    let last = TYPED.lock().ok().and_then(|t| *t);
    if !rested(last, Instant::now()) {
        return Vec::new();
    }
    OFFERS
        .lock()
        .map(|mut o| std::mem::take(&mut *o))
        .unwrap_or_default()
}

/// Whether the keys, last typed at `last`, have rested at `now`.
fn rested(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|t| now.saturating_duration_since(t) >= OFFER_PAUSE)
}

/// Lines for the status bar from background work that is not a job of
/// the user's (a plugin update found): text, and whether an error.
static NOTICES: Mutex<Vec<(String, bool)>> = Mutex::new(Vec::new());

/// Tells the user `text`, from any thread.
pub fn notice(text: String, error: bool) {
    if let Ok(mut n) = NOTICES.lock() {
        n.push((text, error));
    }
}

/// The notices since the last call.
pub fn take_notices() -> Vec<(String, bool)> {
    NOTICES
        .lock()
        .map(|mut n| std::mem::take(&mut *n))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_wait_for_the_keys_to_rest() {
        let now = Instant::now();
        assert!(rested(None, now), "nothing typed yet");
        assert!(!rested(Some(now), now));
        assert!(!rested(Some(now), now + OFFER_PAUSE / 2));
        assert!(rested(Some(now), now + OFFER_PAUSE));
    }
}
