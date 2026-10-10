//! A project's run and test commands (`SPC p R`, `SPC p T`): the language
//! plugin's `commands.run`, `commands.test` and `commands.testAtPoint`
//! (T3.8.4), or a command asked for, run in the project's root with their
//! output in a read-only document that follows it as it comes. The status
//! bar says it runs and how it ended; the same command run again stops
//! the one under way, as closing its document does.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The output kept at most: past it, the start goes.
const MAX_OUTPUT: usize = 4 << 20;

/// The commands under way, by their document's key, so that the same
/// command started again stops the one before.
static RUNNING: Mutex<Vec<(String, Arc<Mutex<Child>>)>> = Mutex::new(Vec::new());

/// The plugin name and kind of the documents runs write.
const OWNER: &str = "kalem";
const KIND: &str = "kalem-output";

/// `command` as a program and its arguments for the system's shell: `sh
/// -c`, or `cmd /C` on Windows.
pub fn shell(command: &str) -> Vec<String> {
    if cfg!(windows) {
        vec!["cmd".into(), "/C".into(), command.into()]
    } else {
        vec!["sh".into(), "-c".into(), command.into()]
    }
}

/// Runs `argv` in `dir` in the background, its output (standard output and
/// standard error as they come) in a read-only document titled after it,
/// written again as it grows; the document's number. The program is found
/// as a language server's is (on the `PATH`, or a path); `Err` when it is
/// not.
pub fn start(argv: Vec<String>, dir: PathBuf) -> Result<u64, String> {
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| crate::l10n::tr("run-nothing"))?;
    let found = kalem_lsp::find_program(program, Some(&dir), &[])
        .ok_or_else(|| crate::tr!("run-not-found", program = program.as_str()))?;
    let shown = shown_command(&argv);
    let place = dir.file_name().map_or_else(
        || dir.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let key = format!("{}\u{0}{}", dir.display(), argv.join("\u{0}"));
    // The same command under way is stopped: its document is written anew.
    stop(&key);
    let child = Command::new(&found)
        .args(args)
        .current_dir(&dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            crate::tr!(
                "run-did-not-start",
                command = shown.as_str(),
                reason = e.to_string()
            )
        })?;
    let child = Arc::new(Mutex::new(child));
    RUNNING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push((key.clone(), child.clone()));
    let header = format!("{shown}\n{}\n\n", dir.display());
    let number = crate::extensions::open_generated(
        OWNER,
        crate::extensions::GeneratedSpec {
            id: "kalem.run".into(),
            key: key.clone(),
            title: crate::tr!(
                "run-title",
                command = shown.as_str(),
                place = place.as_str()
            ),
            kind: KIND.into(),
            language: None,
        },
        header.clone(),
        Some(header.len()),
        Vec::new(),
    )?;
    let status = crate::tr!(
        "run-running",
        command = shown.as_str(),
        place = place.as_str()
    );
    crate::jobs::spawn(status, move || follow(child, key, number, header, shown));
    Ok(number)
}

/// The command as it is shown: its program's name and its arguments.
fn shown_command(argv: &[String]) -> String {
    let mut parts = argv.iter();
    let program = parts.next().map_or("", |p| {
        Path::new(p)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(p)
    });
    std::iter::once(program)
        .chain(parts.map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Stops the command of document key `key` if it is under way.
fn stop(key: &str) {
    let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
    running.retain(|(k, child)| {
        if k == key {
            let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
            false
        } else {
            true
        }
    });
}

/// Reads `stream` into `out` until it ends.
fn read_into(
    mut stream: impl Read + Send + 'static,
    out: Arc<Mutex<Vec<u8>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = stream.read(&mut buf) {
            if n == 0 {
                break;
            }
            let mut o = out.lock().unwrap_or_else(|e| e.into_inner());
            o.extend_from_slice(&buf[..n]);
            if o.len() > MAX_OUTPUT {
                // The start goes, from a line's start.
                let cut = o.len() - MAX_OUTPUT;
                let cut = o[cut..]
                    .iter()
                    .position(|b| *b == b'\n')
                    .map_or(cut, |i| cut + i + 1);
                o.drain(..cut);
            }
        }
    })
}

/// Follows the run of `child`: its output written to document `number`
/// as it comes, a last line saying how it ended. Closing the document
/// stops it.
fn follow(
    child: Arc<Mutex<Child>>,
    key: String,
    number: u64,
    header: String,
    shown: String,
) -> crate::jobs::Finished {
    let out = Arc::new(Mutex::new(Vec::new()));
    let readers = {
        let mut c = child.lock().unwrap_or_else(|e| e.into_inner());
        let mut readers = Vec::new();
        if let Some(s) = c.stdout.take() {
            readers.push(read_into(s, out.clone()));
        }
        if let Some(s) = c.stderr.take() {
            readers.push(read_into(s, out.clone()));
        }
        readers
    };
    let text = |out: &Arc<Mutex<Vec<u8>>>| {
        let o = out.lock().unwrap_or_else(|e| e.into_inner());
        format!("{header}{}", String::from_utf8_lossy(&o))
    };
    let mut written = 0usize;
    let mut closed = false;
    let status = loop {
        let exited = child
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_wait()
            .ok()
            .flatten();
        // Its document closed: the command stops.
        if !closed && !crate::extensions::generated_open(number) {
            closed = true;
            let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
        }
        let len = out.lock().unwrap_or_else(|e| e.into_inner()).len();
        if len != written && !closed {
            written = len;
            let t = text(&out);
            let end = t.len();
            if crate::extensions::set_generated(OWNER, number, t, Some(end), Vec::new()).is_err() {
                closed = true;
                let _ = child.lock().unwrap_or_else(|e| e.into_inner()).kill();
            }
        }
        if let Some(s) = exited {
            break Some(s);
        }
        if closed {
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    for r in readers {
        let _ = r.join();
    }
    // Stopped when its document closed, or when the same command started
    // again took it off the list (the exit code of a process killed is
    // the system's: a signal, or 1 on Windows).
    let stopped = {
        let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        let before = running.len();
        running.retain(|(k, c)| !(k == &key && Arc::ptr_eq(c, &child)));
        closed || running.len() == before
    };
    let (message, error) = match status.map(|s| s.code()) {
        _ if stopped => (crate::tr!("run-stopped", command = shown.as_str()), true),
        Some(Some(0)) => (crate::tr!("run-done", command = shown.as_str()), false),
        Some(Some(code)) => (
            crate::tr!("run-failed", command = shown.as_str(), code = code),
            true,
        ),
        _ => (crate::tr!("run-stopped", command = shown.as_str()), true),
    };
    if !closed {
        let mut t = text(&out);
        if !t.ends_with('\n') {
            t.push('\n');
        }
        t.push('\n');
        t.push_str(&message);
        t.push('\n');
        let end = t.len();
        let _ = crate::extensions::set_generated(OWNER, number, t, Some(end), Vec::new());
    }
    crate::jobs::Finished {
        message,
        error,
        open: None,
    }
}

/// How many commands are under way.
pub fn running() -> usize {
    RUNNING.lock().unwrap_or_else(|e| e.into_inner()).len()
}
