//! File operations in the background, with progress and cancellation.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::ops::{self, OpKind, Operation, Outcome, Report};

/// How far an operation is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Progress {
    /// Files (and links) to go through, when known.
    pub total_files: u64,
    /// Files done.
    pub done_files: u64,
    /// Bytes to copy, when known.
    pub total_bytes: u64,
    /// Bytes done.
    pub done_bytes: u64,
    /// The file being worked on.
    pub current: Option<PathBuf>,
}

impl Progress {
    /// Done, from 0 to 1, by bytes when known, else by files.
    pub fn fraction(&self) -> f32 {
        if self.total_bytes > 0 {
            (self.done_bytes as f64 / self.total_bytes as f64).min(1.0) as f32
        } else if self.total_files > 0 {
            (self.done_files as f64 / self.total_files as f64).min(1.0) as f32
        } else {
            0.0
        }
    }
}

#[derive(Debug, Default)]
struct Shared {
    progress: Mutex<Progress>,
    outcome: Mutex<Option<Outcome>>,
    cancel: AtomicBool,
}

impl Report for Shared {
    fn file(&self, path: &Path) {
        if let Ok(mut p) = self.progress.lock() {
            p.current = Some(path.to_path_buf());
        }
    }
    fn bytes(&self, n: u64) {
        if let Ok(mut p) = self.progress.lock() {
            p.done_bytes += n;
            p.done_files += 1;
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Counts the files and bytes under `path`.
fn measure(path: &Path, files: &mut u64, bytes: &mut u64) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    if meta.is_dir() {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                measure(&e.path(), files, bytes);
            }
        }
    } else {
        *files += 1;
        *bytes += meta.len();
    }
}

/// An operation running on its own thread.
#[derive(Debug)]
pub struct Job {
    /// What it does.
    pub kind: OpKind,
    shared: Arc<Shared>,
    handle: Option<JoinHandle<()>>,
}

impl Job {
    /// Starts `op` in the background.
    pub fn start(op: Operation) -> Job {
        let shared = Arc::new(Shared::default());
        let kind = op.kind;
        let s = shared.clone();
        let handle = std::thread::Builder::new()
            .name("kalem-fs job".into())
            .spawn(move || {
                let (mut files, mut bytes) = (0, 0);
                match op.kind {
                    OpKind::Copy => {
                        for (src, _) in &op.items {
                            measure(src, &mut files, &mut bytes);
                        }
                    }
                    _ => files = op.items.len() as u64,
                }
                if let Ok(mut p) = s.progress.lock() {
                    p.total_files = files;
                    p.total_bytes = if op.kind == OpKind::Copy { bytes } else { 0 };
                }
                let out = ops::run(&op, s.as_ref(), &s.cancel);
                tracing::info!(
                    ?kind,
                    done = out.done.len(),
                    skipped = out.skipped.len(),
                    errors = out.errors.len(),
                    cancelled = out.cancelled,
                    "file operation finished"
                );
                if let Ok(mut o) = s.outcome.lock() {
                    *o = Some(out);
                }
            })
            .ok();
        Job {
            kind,
            shared,
            handle,
        }
    }

    /// How far it is.
    pub fn progress(&self) -> Progress {
        self.shared
            .progress
            .lock()
            .map(|p| p.clone())
            .unwrap_or_default()
    }

    /// Asks it to stop after the current file.
    pub fn cancel(&self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
    }

    /// The result, once it has finished (only once).
    pub fn take_outcome(&mut self) -> Option<Outcome> {
        let out = self.shared.outcome.lock().ok()?.take()?;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        Some(out)
    }

    /// Waits for the end.
    pub fn wait(mut self) -> Outcome {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        self.shared
            .outcome
            .lock()
            .ok()
            .and_then(|mut o| o.take())
            .unwrap_or_default()
    }
}
