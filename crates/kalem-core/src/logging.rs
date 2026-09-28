//! Logging (`tracing`) to a log file in the state directory, and a
//! diagnostics report for bug reports.
//!
//! The level comes from `KALEM_LOG` (`debug`, or per module:
//! `kalem_core=debug,info`), else the `log.level` setting. Each session
//! starts a new `kalem.log` and keeps the two before it as `kalem.log.1`
//! and `kalem.log.2`. The terminal frontend must not log to stderr, which
//! would draw over the screen; the others may.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Mutex;

use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{Layer, Registry};

use crate::settings::Config;

/// The directory for logs and other state: `$KALEM_STATE_DIR`, or `kalem`
/// in `$XDG_STATE_HOME`, `%LOCALAPPDATA%` on Windows, or `~/.local/state`.
pub fn state_dir() -> Option<PathBuf> {
    let var = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(d) = var("KALEM_STATE_DIR") {
        return Some(d);
    }
    let base = var("XDG_STATE_HOME")
        .or_else(|| cfg!(windows).then(|| var("LOCALAPPDATA")).flatten())
        .or_else(|| var("HOME").map(|h| h.join(".local").join("state")))?;
    Some(base.join("kalem"))
}

/// The log file in `dir`.
pub fn log_path(dir: &Path) -> PathBuf {
    dir.join("kalem.log")
}

/// Starts a new log file: the current one becomes `.1`, `.1` becomes `.2`.
pub fn rotate(path: &Path) -> io::Result<()> {
    let numbered = |n: u32| {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(format!(".{n}"));
        path.with_file_name(name)
    };
    if !path.exists() {
        return Ok(());
    }
    let _ = std::fs::remove_file(numbered(2));
    if numbered(1).exists() {
        std::fs::rename(numbered(1), numbered(2))?;
    }
    std::fs::rename(path, numbered(1))
}

/// Where logs go.
#[derive(Debug, Clone, Default)]
pub struct LogOptions {
    /// The log file, if any.
    pub file: Option<PathBuf>,
    /// Also log to stderr.
    pub stderr: bool,
    /// The filter (`KALEM_LOG` syntax); `info` if `None`.
    pub filter: Option<String>,
}

impl LogOptions {
    /// The usual options: the log file in [`state_dir`], the filter from
    /// `KALEM_LOG` or `log.level`.
    pub fn standard(config: &Config, stderr: bool) -> LogOptions {
        LogOptions {
            file: state_dir().map(|d| log_path(&d)),
            stderr,
            filter: std::env::var("KALEM_LOG")
                .ok()
                .filter(|f| !f.is_empty())
                .or_else(|| Some(config.str("log.level").to_string())),
        }
    }
}

/// Why logging could not start.
#[derive(Debug)]
pub enum LogError {
    /// The filter is invalid.
    Filter(String),
    /// The log file cannot be written.
    Io(io::Error),
    /// Logging was started before.
    AlreadyStarted,
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogError::Filter(e) => write!(f, "Invalid log filter: {e}"),
            LogError::Io(e) => write!(f, "Cannot write the log file: {e}"),
            LogError::AlreadyStarted => f.write_str("Logging was already started"),
        }
    }
}

impl std::error::Error for LogError {}

/// The subscriber for `options`, opening (and rotating) the log file.
pub fn subscriber(
    options: &LogOptions,
) -> Result<impl tracing::Subscriber + Send + Sync, LogError> {
    let filter = Targets::from_str(options.filter.as_deref().unwrap_or("info"))
        .map_err(|e| LogError::Filter(e.to_string()))?;
    let file = match &options.file {
        Some(p) => {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir).map_err(LogError::Io)?;
            }
            rotate(p).map_err(LogError::Io)?;
            Some(File::create(p).map_err(LogError::Io)?)
        }
        None => None,
    };
    let file_layer = file.map(|f| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_writer(Mutex::new(f))
            .with_filter(filter.clone())
    });
    let stderr_layer = options.stderr.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(io::stderr)
            .with_filter(filter)
    });
    Ok(Registry::default().with(file_layer).with(stderr_layer))
}

/// Starts logging for the process, and logs panics with a backtrace.
pub fn init(options: &LogOptions) -> Result<(), LogError> {
    let s = subscriber(options)?;
    tracing::subscriber::set_global_default(s).map_err(|_| LogError::AlreadyStarted)?;
    install_panic_hook();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "Kalem started");
    Ok(())
}

/// Logs panics, with a backtrace, before the previous hook runs (which a
/// terminal frontend uses to restore the terminal).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        tracing::error!(target: "panic", "{info}\n{backtrace}");
        previous(info);
    }));
}

/// A report for bug reports: version, platform, settings files and their
/// problems, and the end of the log.
pub fn diagnostics_report(config: &Config, log: Option<&Path>, extra: &[(&str, String)]) -> String {
    use std::fmt::Write;
    let mut r = String::new();
    let _ = writeln!(r, "Kalem {}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(
        r,
        "Platform: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let dir = crate::settings::config_dir();
    let _ = writeln!(
        r,
        "Configuration: {}",
        dir.as_deref()
            .map_or("unknown".into(), |d| d.display().to_string())
    );
    for (layer, path) in config.sources() {
        if let Some(p) = path {
            let _ = writeln!(r, "Settings ({layer:?}): {}", p.display());
        }
    }
    for i in config.issues() {
        let _ = writeln!(r, "Settings problem: {}", i.message);
    }
    for (k, v) in extra {
        let _ = writeln!(r, "{k}: {v}");
    }
    if let Some(p) = log {
        let _ = writeln!(r, "Log: {}", p.display());
        if let Ok(text) = std::fs::read_to_string(p) {
            let lines: Vec<&str> = text.lines().collect();
            let tail = &lines[lines.len().saturating_sub(200)..];
            let _ = writeln!(r, "--- last {} log lines ---", tail.len());
            for l in tail {
                let _ = writeln!(r, "{l}");
            }
        }
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_file_rotation_and_report() {
        let dir = std::env::temp_dir().join(format!("kalem-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = log_path(&dir);
        for session in 0..4 {
            let options = LogOptions {
                file: Some(path.clone()),
                stderr: false,
                filter: Some("kalem_core=debug,warn".into()),
            };
            let s = subscriber(&options).unwrap();
            tracing::subscriber::with_default(s, || {
                tracing::debug!(session, "debug from kalem_core");
                tracing::info!(target: "other", "filtered out");
            });
        }
        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("debug from kalem_core session=3"), "{log}");
        assert!(!log.contains("filtered out"));
        assert!(
            std::fs::read_to_string(dir.join("kalem.log.2"))
                .unwrap()
                .contains("session=1")
        );
        assert!(!dir.join("kalem.log.3").exists());
        let report = diagnostics_report(
            &Config::default(),
            Some(&path),
            &[("Frontend", "test".into())],
        );
        assert!(report.starts_with("Kalem "));
        assert!(report.contains("Frontend: test"));
        assert!(report.contains("session=3"));
        assert!(matches!(
            subscriber(&LogOptions {
                filter: Some("=+=".into()),
                ..LogOptions::default()
            }),
            Err(LogError::Filter(_))
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
