//! The system around the editor (T2.7e.12): a file opened with its
//! application, shown in the system's file manager, and shell commands
//! on files as Dired's `!` runs them.

use std::path::{Path, PathBuf};

/// A program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

fn plan(program: &str, args: Vec<String>) -> Plan {
    Plan {
        program: program.into(),
        args,
    }
}

/// How `path` opens with its application on `os`
/// (`std::env::consts::OS`).
pub fn open_plan(path: &Path, os: &str) -> Plan {
    let file = path.display().to_string();
    match os {
        "macos" => plan("open", vec![file]),
        "windows" => plan("explorer", vec![file]),
        _ => plan("xdg-open", vec![file]),
    }
}

/// How `path` is shown selected in the system's file manager on `os`;
/// `dbus` when the Freedesktop file manager service can be asked (else its
/// folder opens).
pub fn reveal_plan(path: &Path, os: &str, dbus: bool) -> Plan {
    let file = path.display().to_string();
    match os {
        "macos" => plan("open", vec!["-R".into(), file]),
        "windows" => plan("explorer", vec![format!("/select,{file}")]),
        _ if dbus => plan(
            "dbus-send",
            vec![
                "--session".into(),
                "--dest=org.freedesktop.FileManager1".into(),
                "--type=method_call".into(),
                "/org/freedesktop/FileManager1".into(),
                "org.freedesktop.FileManager1.ShowItems".into(),
                format!("array:string:{}", file_url(path)),
                "string:".into(),
            ],
        ),
        _ => plan(
            "xdg-open",
            vec![
                path.parent()
                    .unwrap_or(Path::new("/"))
                    .display()
                    .to_string(),
            ],
        ),
    }
}

/// `file://` and the path, its bytes outside the unreserved ones
/// percent-encoded.
fn file_url(path: &Path) -> String {
    let mut s = String::from("file://");
    for b in path.to_string_lossy().bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// The terminals to try, in order, for a window at `dir` on `os`;
/// `terminal` is `$TERMINAL`. Each starts in `dir` (its working folder is
/// set too, for those without an option).
pub fn terminal_plans(dir: &Path, os: &str, terminal: Option<&str>) -> Vec<Plan> {
    let d = dir.display().to_string();
    match os {
        "macos" => vec![plan("open", vec!["-a".into(), "Terminal".into(), d])],
        "windows" => vec![
            plan("wt", vec!["-d".into(), d.clone()]),
            plan(
                "cmd",
                vec![
                    "/c".into(),
                    "start".into(),
                    "cmd".into(),
                    "/K".into(),
                    format!("cd /d {}", quote(&d, os)),
                ],
            ),
        ],
        _ => {
            let mut v: Vec<Plan> = terminal
                .filter(|t| !t.trim().is_empty())
                .map(|t| plan(t.trim(), Vec::new()))
                .into_iter()
                .collect();
            v.extend([
                plan("x-terminal-emulator", Vec::new()),
                plan("gnome-terminal", vec![format!("--working-directory={d}")]),
                plan("konsole", vec!["--workdir".into(), d.clone()]),
                plan("xfce4-terminal", vec![format!("--working-directory={d}")]),
                plan("kitty", vec!["--directory".into(), d.clone()]),
                plan("alacritty", vec!["--working-directory".into(), d.clone()]),
                plan("xterm", Vec::new()),
            ]);
            v
        }
    }
}

/// Opens the system's terminal at `dir` (Doom's `SPC o t`; Kalem has no
/// terminal of its own).
pub fn open_terminal(dir: &Path) -> Result<(), String> {
    let terminal = std::env::var("TERMINAL").ok();
    let mut last = String::from("no terminal");
    for p in terminal_plans(dir, std::env::consts::OS, terminal.as_deref()) {
        let started = std::process::Command::new(&p.program)
            .args(&p.args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match started {
            Ok(_) => return Ok(()),
            Err(e) => last = format!("{}: {e}", p.program),
        }
    }
    Err(last)
}

/// Starts `plan`, not waiting for it.
pub fn spawn(plan: &Plan) -> Result<(), String> {
    std::process::Command::new(&plan.program)
        .args(&plan.args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("{}: {e}", plan.program))
}

/// Opens `path` with its application.
pub fn open(path: &Path) -> Result<(), String> {
    spawn(&open_plan(path, std::env::consts::OS))
}

/// Shows `path` in the system's file manager.
pub fn reveal(path: &Path) -> Result<(), String> {
    let dbus =
        crate::pdf::find("dbus-send", &std::env::var_os("PATH").unwrap_or_default()).is_some();
    spawn(&reveal_plan(path, std::env::consts::OS, dbus))
}

/// `s` quoted for the shell of `os`.
pub fn quote(s: &str, os: &str) -> String {
    if os == "windows" {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else if !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/+=:,@%".contains(&b))
    {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// The command lines `command` makes for `files`, as Dired's `!` does: a
/// `*` standing alone runs it once with every file there; a `?` standing
/// alone runs it once per file with the file there; otherwise once per
/// file with the file at the end. No files: the command as it is.
pub fn shell_commands(command: &str, files: &[PathBuf], os: &str) -> Vec<String> {
    let command = command.trim();
    if files.is_empty() {
        return vec![command.to_string()];
    }
    let q = |p: &PathBuf| quote(&p.to_string_lossy(), os);
    let alone = |c: char| {
        let pat = c.to_string();
        command.split_whitespace().any(|w| w == pat)
    };
    let replace = |with: &str, c: &str| {
        command
            .split(' ')
            .map(|w| if w == c { with } else { w })
            .collect::<Vec<_>>()
            .join(" ")
    };
    if alone('*') {
        let all: Vec<String> = files.iter().map(q).collect();
        vec![replace(&all.join(" "), "*")]
    } else if alone('?') {
        files.iter().map(|f| replace(&q(f), "?")).collect()
    } else {
        files
            .iter()
            .map(|f| format!("{command} {}", q(f)))
            .collect()
    }
}

/// Runs a shell command on files in their folder, waiting for it: the
/// message (the output's last line, or the error's) and whether it failed.
pub fn run_shell(req: &crate::command::ShellOp) -> (String, bool) {
    let os = std::env::consts::OS;
    let mut last = String::new();
    for line in shell_commands(&req.command, &req.files, os) {
        let mut c = if os == "windows" {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", &line]);
            c
        } else {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", &line]);
            c
        };
        let out = match c
            .current_dir(&req.dir)
            .stdin(std::process::Stdio::null())
            .output()
        {
            Ok(o) => o,
            Err(e) => return (e.to_string(), true),
        };
        let text = |b: &[u8]| {
            String::from_utf8_lossy(b)
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
                .to_string()
        };
        if !out.status.success() {
            let err = text(&out.stderr);
            let code = out.status.code().map_or("-".into(), |c| c.to_string());
            return (
                crate::tr!("fm-shell-failed", code = code, error = err),
                true,
            );
        }
        let t = text(&out.stdout);
        if !t.is_empty() {
            last = t;
        }
    }
    if last.is_empty() {
        (
            crate::tr!("fm-shell-done", command = req.command.trim()),
            false,
        )
    } else {
        (last, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans() {
        let p = Path::new("/w/a b.txt");
        assert_eq!(open_plan(p, "macos").program, "open");
        assert_eq!(open_plan(p, "linux").program, "xdg-open");
        assert_eq!(reveal_plan(p, "macos", false).args, ["-R", "/w/a b.txt"]);
        assert_eq!(
            reveal_plan(p, "windows", false).args,
            ["/select,/w/a b.txt"]
        );
        let dbus = reveal_plan(p, "linux", true);
        assert!(
            dbus.args
                .contains(&"array:string:file:///w/a%20b.txt".to_string())
        );
        assert_eq!(reveal_plan(p, "linux", false).args, ["/w"]);
    }

    #[test]
    fn shell_lines() {
        let f = vec![PathBuf::from("a.txt"), PathBuf::from("it's.txt")];
        assert_eq!(
            shell_commands("wc -l *", &f, "linux"),
            ["wc -l a.txt 'it'\\''s.txt'"]
        );
        assert_eq!(
            shell_commands("cp ? backup/", &f, "linux"),
            ["cp a.txt backup/", "cp 'it'\\''s.txt' backup/"]
        );
        assert_eq!(shell_commands("gzip", &f[..1], "linux"), ["gzip a.txt"]);
        assert_eq!(shell_commands("ls", &[], "linux"), ["ls"]);
        assert_eq!(quote("a b", "windows"), "\"a b\"");
    }

    #[cfg(unix)]
    #[test]
    fn runs_in_the_folder() {
        crate::l10n::set_language("en");
        let dir = std::env::temp_dir().join(format!("kalem-shell-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "1\n2\n").unwrap();
        let req = crate::command::ShellOp {
            command: "cat * | wc -l".into(),
            files: vec![PathBuf::from("a.txt")],
            dir: dir.clone(),
        };
        assert_eq!(run_shell(&req), ("2".to_string(), false));
        let bad = crate::command::ShellOp {
            command: "exit 3".into(),
            files: Vec::new(),
            dir: dir.clone(),
        };
        let (msg, error) = run_shell(&bad);
        assert!(error && msg.contains('3'), "{msg}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn terminals() {
        let d = Path::new("/w/notes");
        let mac = terminal_plans(d, "macos", None);
        assert_eq!(mac[0].args, ["-a", "Terminal", "/w/notes"]);
        let linux = terminal_plans(d, "linux", Some("foot"));
        assert_eq!(linux[0].program, "foot");
        assert!(linux.iter().any(|p| p.program == "gnome-terminal"));
        assert_eq!(terminal_plans(d, "windows", None)[0].program, "wt");
    }
}
