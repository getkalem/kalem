//! Building LaTeX documents (T2.7h.22, T2.7h.23): the root document of a
//! project compiled by `latexmk` or the engine it names (`% !TEX
//! program`, the `latex.engine` setting, or XeLaTeX for documents that
//! load `fontspec`), `biber` or `bibtex` between runs, into an output
//! folder if one is set; and the problems of the log with the file and
//! line each comes from (LaTeX's log opens a file with `(` and closes it
//! with `)`; `-file-line-error` names it on error lines).

use std::path::{Path, PathBuf};
use std::process::Command;

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
        // `./chapters/one.tex:12: Undefined control sequence.`
        if let Some((file, rest)) = l.split_once(".tex:")
            && !file.contains(' ')
            && let Some((n, msg)) = rest.split_once(": ")
            && let Ok(n) = n.parse::<usize>()
        {
            let mut message = msg.trim().to_string();
            if let Some(f) = missing_file(&message)
                && let Some(h) = install_hint(&f, &search)
            {
                message = format!("{message} {h}");
            }
            out.push(Problem {
                file: Some(format!("{file}.tex")),
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

/// Builds the root document `root` with `engine`, the output in
/// `out_dir` (relative to the root's folder) when given. Without
/// `latexmk` the engine runs, then `biber` or `bibtex` if the document
/// has a bibliography, then the engine twice more.
pub fn build(root: &Path, engine: Engine, out_dir: Option<&Path>) -> Result<Built, String> {
    let search = std::env::var_os("PATH").unwrap_or_default();
    let tool = pdf::detect(engine, &search).ok_or_else(|| crate::l10n::tr("msg-no-latex"))?;
    let dir = root.parent().map(Path::to_path_buf).unwrap_or_default();
    if let Some(d) = out_dir {
        std::fs::create_dir_all(dir.join(d)).map_err(|e| e.to_string())?;
    }
    let run = || {
        build_command(&tool, engine, root, out_dir)
            .status()
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
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
                let _ = Command::new(p)
                    .arg(&stem)
                    .current_dir(&out)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
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
