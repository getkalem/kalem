//! Building LaTeX documents (T2.7h.22, T2.7h.23): the root document of a
//! project compiled by `latexmk` or the engine it names (`% !TEX
//! program`, the `latex.engine` setting, or XeLaTeX for documents that
//! load `fontspec`), `biber` or `bibtex` between runs, into an output
//! folder if one is set; and the problems of the log with the file and
//! line each comes from (LaTeX's log opens a file with `(` and closes it
//! with `)`; `-file-line-error` names it on error lines).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::pdf::{self, Engine, Tool};

/// How bad a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The build stopped or the output is wrong.
    Error,
    /// LaTeX warned (an undefined reference).
    Warning,
    /// An overfull or underfull box.
    BadBox,
}

/// A problem in a LaTeX log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file, as the log names it (`./chapters/one.tex`).
    pub file: Option<String>,
    /// The line in it.
    pub line: Option<usize>,
    /// What LaTeX says, with the install hint for a missing package.
    pub message: String,
    /// How bad.
    pub severity: Severity,
}

/// A root document and the problems of its last build, each with the
/// file it is in.
type Recorded = (PathBuf, Vec<(PathBuf, Problem)>);

/// The problems of the last build of each root document, and a count of
/// the builds recorded.
static RECORDED: std::sync::Mutex<(u64, Vec<Recorded>)> = std::sync::Mutex::new((0, Vec::new()));

/// Keeps the problems of a build of `root`, for the editor to show in the
/// files they are in (a problem without a file is the root's).
pub fn record(root: &Path, problems: &[Problem]) {
    let dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    let canon = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let located: Vec<(PathBuf, Problem)> = problems
        .iter()
        .map(|p| {
            let file = match &p.file {
                Some(f) => canon(&dir.join(f)),
                None => canon(root),
            };
            (file, p.clone())
        })
        .collect();
    if let Ok(mut r) = RECORDED.lock() {
        let root = canon(root);
        r.1.retain(|(k, _)| *k != root);
        r.1.push((root, located));
        r.0 += 1;
    }
}

/// How many builds were recorded: the editor shows their problems again
/// when it changes.
pub fn recorded() -> u64 {
    RECORDED.lock().map_or(0, |r| r.0)
}

/// The problems the last builds found in `file`.
pub fn problems_in(file: &Path) -> Vec<Problem> {
    let file = dunce::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    RECORDED.lock().map_or(Vec::new(), |r| {
        r.1.iter()
            .flat_map(|(_, v)| v.iter())
            .filter(|(f, _)| *f == file)
            .map(|(_, p)| p.clone())
            .collect()
    })
}

/// `% !TEX program = NAME` among the first lines.
fn magic_program(text: &str) -> Option<String> {
    text.lines().take(20).find_map(|l| {
        let l = l.trim_start().strip_prefix('%')?.trim_start();
        let l = l.strip_prefix('!').unwrap_or(l).trim_start();
        let (k, v) = l.split_once('=')?;
        let k: Vec<String> = k.split_whitespace().map(str::to_lowercase).collect();
        (k == ["tex", "program"] || k == ["tex", "ts-program"]).then(|| v.trim().to_lowercase())
    })
}

/// The engine for a root document with text `text`: `% !TEX program`,
/// else `setting` (`latex.engine`) unless `auto`, else XeLaTeX when the
/// document loads `fontspec`, `unicode-math` or `polyglossia`, else
/// pdfLaTeX.
pub fn engine(text: &str, model: &latex_model::Model, setting: &str) -> Engine {
    let named = magic_program(text)
        .or_else(|| (setting != "auto" && !setting.is_empty()).then(|| setting.to_lowercase()));
    if let Some(n) = named {
        return Engine::from_keyword(Some(&n));
    }
    let unicode = model
        .packages
        .iter()
        .any(|p| matches!(p.name.as_str(), "fontspec" | "unicode-math" | "polyglossia"));
    if unicode {
        Engine::XeLatex
    } else {
        Engine::PdfLatex
    }
}

/// The TeX distribution installed, for install hints.
fn distribution(path: &std::ffi::OsStr) -> Option<&'static str> {
    if pdf::find("tlmgr", path).is_some() {
        Some("texlive")
    } else if pdf::find("mpm", path).is_some() || pdf::find("miktex", path).is_some() {
        Some("miktex")
    } else {
        None
    }
}

/// The install hint for a missing `.sty` or `.cls` file.
fn install_hint(file: &str, path: &std::ffi::OsStr) -> Option<String> {
    let package = file.rsplit_once('.').map_or(file, |(s, _)| s);
    Some(match distribution(path)? {
        "texlive" => crate::tr!("latex-install-texlive", package = package),
        _ => crate::tr!("latex-install-miktex", package = package),
    })
}

/// The problems of a LaTeX log, with their files.
pub fn problems(log: &str) -> Vec<Problem> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    // LaTeX wraps log lines at 79 characters.
    let mut joined = String::new();
    for l in log.lines() {
        joined.push_str(l);
        if l.chars().count() != 79 {
            joined.push('\n');
        }
    }
    let lines: Vec<&str> = joined.lines().collect();
    let mut files: Vec<String> = Vec::new();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let l = lines[i];
        // `./chapters/one.tex:12: Undefined control sequence.`, or from a
        // package: `/…/babel.sty:1234: Package babel Error: …`.
        if let Some((file, n, msg)) = file_line_error(l) {
            let mut message = msg.trim().to_string();
            if let Some(f) = missing_file(&message)
                && let Some(h) = install_hint(&f, &search)
            {
                message = format!("{message} {h}");
            }
            out.push(Problem {
                file: Some(file.to_string()),
                line: Some(n),
                message,
                severity: Severity::Error,
            });
            i += 1;
            continue;
        }
        if let Some(msg) = l.strip_prefix("! ") {
            let n = lines[i + 1..]
                .iter()
                .take(12)
                .find_map(|x| x.strip_prefix("l.")?.split(' ').next()?.parse().ok());
            let mut message = msg.trim().to_string();
            if let Some(f) = missing_file(&message)
                && let Some(h) = install_hint(&f, &search)
            {
                message = format!("{message} {h}");
            }
            out.push(Problem {
                file: files.last().cloned(),
                line: n,
                message,
                severity: Severity::Error,
            });
            i += 1;
            continue;
        }
        let warning = l.contains("Warning:");
        let bad = l.starts_with("Overfull ") || l.starts_with("Underfull ");
        if warning || bad {
            let mut text = l.to_string();
            let mut j = i + 1;
            while warning && j < lines.len() && lines[j].starts_with("   ") && !text.ends_with('.')
            {
                text.push(' ');
                text.push_str(lines[j].trim());
                j += 1;
            }
            out.push(Problem {
                file: files.last().cloned(),
                line: line_number_in(&text),
                message: text.trim().to_string(),
                severity: if bad {
                    Severity::BadBox
                } else {
                    Severity::Warning
                },
            });
            track(l, &mut files);
            i = j;
            continue;
        }
        track(l, &mut files);
        i += 1;
    }
    out
}

/// `FILE:LINE: MESSAGE`, as `-file-line-error` writes errors: the file a
/// name with an extension and no blanks.
fn file_line_error(l: &str) -> Option<(&str, usize, &str)> {
    let mut from = 0;
    while let Some(i) = l[from..].find(':') {
        let colon = from + i;
        let rest = &l[colon + 1..];
        let digits = rest.len() - rest.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 && rest[digits..].starts_with(": ") {
            let file = &l[..colon];
            let ext = file.rsplit_once('.').map(|(_, e)| e);
            if !file.contains(' ')
                && ext.is_some_and(|e| {
                    !e.is_empty() && e.len() <= 4 && e.chars().all(|c| c.is_ascii_alphanumeric())
                })
            {
                let n = rest[..digits].parse().ok()?;
                return Some((file, n, rest[digits + 2..].trim()));
            }
        }
        from = colon + 1;
    }
    None
}

/// The file names opened and closed on a log line.
fn track(line: &str, files: &mut Vec<String>) {
    let b = line.as_bytes();
    let mut k = 0;
    while k < b.len() {
        match b[k] {
            b'(' => {
                let rest = &line[k + 1..];
                let end = rest
                    .find(|c: char| c.is_whitespace() || c == ')' || c == '(')
                    .unwrap_or(rest.len());
                let name = &rest[..end];
                let looks = name.starts_with("./") || name.starts_with('/') || name.contains('.');
                files.push(if looks {
                    name.to_string()
                } else {
                    String::new()
                });
                k += 1 + end;
                continue;
            }
            b')' => {
                files.pop();
            }
            _ => {}
        }
        k += 1;
    }
    // Empty entries stand for parentheses that were not files.
    while files.last().is_some_and(String::is_empty) && files.len() > 64 {
        files.pop();
    }
}

/// `File `foo.sty' not found.`
fn missing_file(message: &str) -> Option<String> {
    let rest = message.split("File `").nth(1)?;
    let (name, after) = rest.split_once('\'')?;
    after.contains("not found").then(|| name.to_string())
}

fn line_number_in(s: &str) -> Option<usize> {
    for key in ["input line ", "at lines ", "at line "] {
        if let Some(i) = s.find(key) {
            let digits: String = s[i + key.len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(n) = digits.parse() {
                return Some(n);
            }
        }
    }
    None
}

/// What a build gave.
#[derive(Debug, Clone)]
pub struct Built {
    /// The PDF, if one was written.
    pub pdf: Option<PathBuf>,
    /// The problems, in the order of the log.
    pub problems: Vec<Problem>,
}

/// The command that builds `root` with `tool`, the output in `out_dir`.
fn build_command(tool: &Tool, engine: Engine, root: &Path, out_dir: Option<&Path>) -> Command {
    let name = root.file_name().map(PathBuf::from).unwrap_or_default();
    let mut cmd = match tool {
        Tool::Latexmk(p) => {
            let mut c = Command::new(p);
            c.args([
                "-pdf",
                "-interaction=nonstopmode",
                "-file-line-error",
                "-halt-on-error",
            ]);
            c.arg(match engine {
                Engine::PdfLatex => "-pdflatex",
                Engine::XeLatex => "-xelatex",
                Engine::LuaLatex => "-lualatex",
                // Not reached: tectonic is run on its own.
                Engine::Tectonic => "-xelatex",
            });
            if let Some(d) = out_dir {
                c.arg(format!("-outdir={}", d.display()));
            }
            c
        }
        Tool::Engine(p) => {
            let mut c = Command::new(p);
            c.args(["-interaction=nonstopmode", "-file-line-error"]);
            if let Some(d) = out_dir {
                c.arg(format!("-output-directory={}", d.display()));
            }
            c
        }
        Tool::Tectonic(p) => {
            let mut c = Command::new(p);
            c.arg("--keep-logs");
            if let Some(d) = out_dir {
                c.arg(format!("--outdir={}", d.display()));
            }
            c
        }
    };
    cmd.arg(name);
    if let Some(dir) = root.parent().filter(|d| !d.as_os_str().is_empty()) {
        cmd.current_dir(dir);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd
}

/// The flag of the build that runs, which [`cancel`] raises.
static CURRENT: std::sync::Mutex<Option<Arc<AtomicBool>>> = std::sync::Mutex::new(None);

/// Stops the build that runs, if one does (Cancel Build); `false` when
/// none does.
pub fn cancel() -> bool {
    match CURRENT.lock().ok().and_then(|c| c.clone()) {
        Some(flag) => {
            flag.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// Runs `cmd` to its end, or kills it when `cancelled` is raised.
fn run_to_end(cmd: &mut Command, cancelled: &AtomicBool) -> Result<(), String> {
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    loop {
        if cancelled.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(crate::l10n::tr("msg-build-cancelled"));
        }
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

/// Builds the root document `root` with `engine`, the output in
/// `out_dir` (relative to the root's folder) when given. Without
/// `latexmk` the engine runs, then `biber` or `bibtex` if the document
/// has a bibliography, then the engine twice more. [`cancel`] stops it.
pub fn build(root: &Path, engine: Engine, out_dir: Option<&Path>) -> Result<Built, String> {
    let flag = Arc::new(AtomicBool::new(false));
    if let Ok(mut c) = CURRENT.lock() {
        *c = Some(flag.clone());
    }
    let out = build_inner(root, engine, out_dir, &flag);
    if let Ok(mut c) = CURRENT.lock()
        && c.as_ref().is_some_and(|f| Arc::ptr_eq(f, &flag))
    {
        *c = None;
    }
    out
}

fn build_inner(
    root: &Path,
    engine: Engine,
    out_dir: Option<&Path>,
    cancelled: &AtomicBool,
) -> Result<Built, String> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let tool = pdf::detect(engine, &search).ok_or_else(|| crate::l10n::tr("msg-no-latex"))?;
    let dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    if let Some(d) = out_dir {
        std::fs::create_dir_all(dir.join(d)).map_err(|e| e.to_string())?;
    }
    let run = || run_to_end(&mut build_command(&tool, engine, root, out_dir), cancelled);
    run()?;
    let out = out_dir.map_or(dir.clone(), |d| dir.join(d));
    let stem = root.file_stem().map(PathBuf::from).unwrap_or_default();
    if let Tool::Engine(program) = &tool {
        let aux = out.join(&stem).with_extension("aux");
        let bcf = out.join(&stem).with_extension("bcf");
        let bib = if bcf.is_file() {
            Some("biber")
        } else if std::fs::read(&aux)
            .is_ok_and(|a| String::from_utf8_lossy(&a).contains("\\bibdata{"))
        {
            Some("bibtex")
        } else {
            None
        };
        let mut again = 1;
        if let Some(b) = bib {
            let mut paths: Vec<PathBuf> = program
                .parent()
                .map(Path::to_path_buf)
                .into_iter()
                .collect();
            paths.extend(std::env::split_paths(&search));
            if let Some(p) = std::env::join_paths(paths)
                .ok()
                .and_then(|j| pdf::find(b, &j))
            {
                run_to_end(
                    Command::new(p)
                        .arg(&stem)
                        .current_dir(&out)
                        .stdin(std::process::Stdio::null())
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null()),
                    cancelled,
                )?;
                again = 2;
            }
        }
        for _ in 0..again {
            run()?;
        }
    }
    let log = std::fs::read(out.join(&stem).with_extension("log"))
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let pdf = out.join(&stem).with_extension("pdf");
    Ok(Built {
        pdf: pdf.is_file().then_some(pdf),
        problems: problems(&log),
    })
}

/// The problems as a compiler prints them: `file:line: error: …`.
pub fn report(root: &Path, problems: &[Problem]) -> String {
    problems
        .iter()
        .map(|p| {
            let kind = match p.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
                Severity::BadBox => "info",
            };
            let file = p.file.clone().unwrap_or_else(|| root.display().to_string());
            match p.line {
                Some(n) => format!("{file}:{n}: {kind}: {}", p.message),
                None => format!("{file}: {kind}: {}", p.message),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn cancelling_stops_the_program() {
        let flag = Arc::new(AtomicBool::new(false));
        let raise = flag.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            raise.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let r = run_to_end(Command::new("sleep").arg("10"), &flag);
        t.join().unwrap();
        assert_eq!(r, Err(crate::l10n::tr("msg-build-cancelled")));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn engines() {
        let model = |t: &str| latex_model::Model::new(&latex_syntax::parse(t));
        let t = "% !TEX program = lualatex\n\\documentclass{article}";
        assert_eq!(engine(t, &model(t), "auto"), Engine::LuaLatex);
        let t = "\\usepackage{fontspec}";
        assert_eq!(engine(t, &model(t), "auto"), Engine::XeLatex);
        assert_eq!(engine(t, &model(t), "pdflatex"), Engine::PdfLatex);
        assert_eq!(engine("x", &model("x"), "auto"), Engine::PdfLatex);
    }

    #[test]
    fn log_problems_with_files() {
        let log = "This is pdfTeX\n(./main.tex (/usr/share/texlive/article.cls\n) (./chapters/one.tex\n\nLaTeX Warning: Reference `x' on page 1 undefined on input line 3.\n\n) [1]\nOverfull \\hbox (12.0pt too wide) in paragraph at lines 10--12\n./chapters/two.tex:7: Undefined control sequence.\nl.7 \\foo\n\n! LaTeX Error: File `nosuch.sty' not found.\n";
        let p = problems(log);
        assert_eq!(p.len(), 4, "{p:#?}");
        assert_eq!(
            (p[0].file.as_deref(), p[0].line, p[0].severity),
            (Some("./chapters/one.tex"), Some(3), Severity::Warning)
        );
        assert_eq!(
            (p[1].file.as_deref(), p[1].line, p[1].severity),
            (Some("./main.tex"), Some(10), Severity::BadBox)
        );
        assert_eq!(
            (p[2].file.as_deref(), p[2].line),
            (Some("./chapters/two.tex"), Some(7))
        );
        assert!(
            p[3].message
                .starts_with("LaTeX Error: File `nosuch.sty' not found.")
        );
    }

    #[test]
    fn builds_a_project() {
        let search = std::env::var_os("PATH").unwrap_or_default();
        if pdf::detect(Engine::PdfLatex, &search).is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("kalem-latex-build-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("ch")).unwrap();
        std::fs::write(
            dir.join("main.tex"),
            "\\documentclass{article}\\begin{document}\\input{ch/one}\\end{document}\n",
        )
        .unwrap();
        std::fs::write(dir.join("ch/one.tex"), "Hello \\ref{nope}.\n").unwrap();
        let built = build(
            &dir.join("main.tex"),
            Engine::PdfLatex,
            Some(Path::new("build")),
        )
        .unwrap();
        assert!(
            built
                .pdf
                .as_ref()
                .is_some_and(|p| p.ends_with("build/main.pdf")),
            "{built:?}"
        );
        assert!(
            built
                .problems
                .iter()
                .any(|p| p.message.contains("undefined")
                    && p.file.as_deref() == Some("./ch/one.tex")),
            "{:#?}",
            built.problems
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
