//! Babel on export (Org's `org-babel-exp-process-buffer`, ob-exp.el): with
//! `export.use_babel` (`org-export-use-babel`, on), an export first runs
//! each source block of the document whose `:exports` is `results` or
//! `both`, in order, on a copy of its text, its results written there with
//! `:results replace`; the export reads the copy, and the document is left
//! as it is. A block runs as one run by hand does, under the export's
//! `:eval` values too (`never-export` and `no-export` refuse,
//! `query-export` asks); a block of a language not loaded is not run, and
//! Emacs's warning says so. Inline source blocks and `#+CALL:` lines are not
//! run.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use org_syntax::SyntaxKind::SRC_BLOCK;
use serde_json::Value as Json;

use super::{Gate, Info, Job};
use crate::command::{CommandError, CommandResult, EditorContext, Request};
use crate::extensions::ProcessExit;

/// The setting.
pub const USE_BABEL: &str = "export.use_babel";

/// What runs a block: Babel's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// `babel.load_languages`.
    pub loaded: Vec<String>,
    /// `babel.python_command`.
    pub python: String,
    /// `babel.confirm_evaluate`.
    pub confirm: bool,
}

impl Settings {
    /// The settings of `config`.
    pub fn of(config: &crate::settings::Config) -> Settings {
        Settings {
            loaded: config
                .strings(super::LANGUAGES)
                .into_iter()
                .map(String::from)
                .collect(),
            python: config.str(super::PYTHON).to_string(),
            confirm: config.bool(super::CONFIRM),
        }
    }
}

/// The starts of `text`'s source blocks an export runs, in order: those
/// whose `:exports` is `results` or `both`, inside `range` (a subtree's)
/// when given.
pub fn blocks(text: &str, range: Option<&Range<usize>>) -> Vec<usize> {
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let keywords = parse.keywords();
    root.descendants()
        .filter(|n| n.kind() == SRC_BLOCK)
        .map(|n| usize::from(n.text_range().start()))
        .filter(|s| range.is_none_or(|r| r.contains(s)))
        .filter(|&s| {
            matches!(
                super::block_at(&root, &keywords, s),
                Some((_, Ok(info))) if info.params.exports.iter().any(|e| e == "results" || e == "both")
            )
        })
        .collect()
}

/// The runner of `info`'s block on export, or why it is left: its
/// language not loaded (Emacs's warning), or its `:eval` refusing.
fn check(info: &Info, settings: &Settings) -> Result<super::Runner, String> {
    let runner = super::runner(&info.lang, &settings.loaded)
        .map_err(|_| crate::tr!("babel-export-no-function", lang = info.lang.as_str()))?;
    if let Gate::Disabled(m) = super::gate(info, true, settings.confirm) {
        return Err(m);
    }
    Ok(runner)
}

/// What an export does with a block.
#[derive(Debug)]
pub enum Plan {
    /// Leaves it as it is, saying why where Emacs says it.
    Skip(Option<String>),
    /// Asks the user first: the question, and what a no says.
    Ask { question: String, aborted: String },
    /// Runs it: what it is, and its run.
    Run(Box<(Info, Job)>),
}

/// What an export does with the block at `start` of `text`, the file
/// `path`'s; `answered` when the user said yes to it.
pub fn plan(
    text: &str,
    start: usize,
    path: Option<&Path>,
    settings: &Settings,
    answered: bool,
) -> Plan {
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let keywords = parse.keywords();
    let Some((_, info)) = super::block_at(&root, &keywords, start) else {
        return Plan::Skip(None);
    };
    let mut info = match info {
        Ok(i) => i,
        Err(e) => return Plan::Skip(Some(e)),
    };
    // `org-babel-exp-results`: a language without its executor is left,
    // before any question.
    let runner = match check(&info, settings) {
        Ok(r) => r,
        Err(m) => return Plan::Skip(Some(m)),
    };
    if let Err(e) = info.params.add(":results", "replace") {
        return Plan::Skip(Some(e));
    }
    if let Gate::Query(question) = super::gate(&info, true, settings.confirm)
        && !answered
    {
        return Plan::Ask {
            question,
            aborted: super::aborted(&info),
        };
    }
    let dir = super::block_dir(&info, path);
    match super::job(&info, &runner, &root, path, &dir, &settings.python) {
        Ok(job) => Plan::Run(Box::new((info, job))),
        Err(e) => Plan::Skip(Some(e)),
    }
}

/// What writing a block's results into the copy did.
#[derive(Debug, Clone, PartialEq)]
pub struct Written {
    /// The text with them.
    pub text: String,
    /// The range of the text before that they replaced, and the length of
    /// what took its place.
    pub edit: Option<(Range<usize>, usize)>,
    /// The program's error output and exit status, to show.
    pub error: Option<(String, Option<i32>)>,
}

/// `text` with the results of `info`'s block at `start`, which `job` ran
/// and which ended as `exit` (none where it ran no program); or a plugin's
/// refusal.
pub fn write(
    text: &str,
    start: usize,
    info: &Info,
    job: &Job,
    exit: Option<&ProcessExit>,
) -> Result<Written, String> {
    let outcome = super::outcome(job, exit)?;
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let keywords = parse.keywords();
    let Some((node, _)) = super::block_at(&root, &keywords, start) else {
        return Ok(Written {
            text: text.to_string(),
            edit: None,
            error: outcome.error,
        });
    };
    let (range, new, _) = super::results_edit(text, &root, &node, info, &outcome.value);
    Ok(Written {
        text: format!("{}{}{}", &text[..range.start], new, &text[range.end..]),
        edit: Some((range, new.len())),
        error: outcome.error,
    })
}

/// The starts after the one at `next` moved by `edit`.
fn shift(starts: &mut [usize], next: usize, edit: &(Range<usize>, usize)) {
    let (range, len) = edit;
    for s in starts.iter_mut().skip(next + 1) {
        if *s >= range.start {
            *s = (*s + len).saturating_sub(range.len());
        }
    }
}

/// For the command line: `text`'s blocks an export runs (inside `range`
/// when given), run now in turn as the file `path`'s; `ask` is asked each
/// question and answers yes, no, or nothing where it cannot ask (a no);
/// `say` is told what Emacs would say. The text with their results.
pub fn evaluate_now(
    text: &str,
    path: Option<&Path>,
    range: Option<&Range<usize>>,
    settings: &Settings,
    ask: &mut dyn FnMut(&str) -> Option<bool>,
    say: &mut dyn FnMut(String),
) -> String {
    let mut text = text.to_string();
    let mut starts = blocks(&text, range);
    let mut i = 0;
    let mut answered = false;
    while i < starts.len() {
        match plan(&text, starts[i], path, settings, answered) {
            Plan::Skip(m) => {
                if let Some(m) = m {
                    say(m);
                }
            }
            Plan::Ask { question, aborted } => {
                if ask(&question) == Some(true) {
                    answered = true;
                    continue;
                }
                say(aborted);
            }
            Plan::Run(run) => {
                let (info, job) = *run;
                let stop = std::sync::atomic::AtomicBool::new(false);
                let exit = match &job.request {
                    Some(r) => {
                        match crate::extensions::run_program(
                            r,
                            &stop,
                            crate::extensions::RUN_TIMEOUT,
                        ) {
                            Ok(e) => Some(e),
                            Err(why) => {
                                say(why);
                                i += 1;
                                answered = false;
                                continue;
                            }
                        }
                    }
                    None => None,
                };
                match write(&text, starts[i], &info, &job, exit.as_ref()) {
                    Ok(w) => {
                        if let Some((stderr, code)) = &w.error {
                            if !stderr.is_empty() {
                                say(stderr.trim_end().to_string());
                            }
                            say(match code {
                                Some(c) => crate::tr!("babel-exited", code = *c),
                                None => crate::tr!("babel-exited-abnormally"),
                            });
                        }
                        if let Some(edit) = &w.edit {
                            shift(&mut starts, i, edit);
                        }
                        text = w.text;
                    }
                    Err(why) => say(why),
                }
            }
        }
        i += 1;
        answered = false;
    }
    text
}

/// An export waiting on its blocks.
struct ExportRun {
    /// The export command, run again with the copy once its blocks ran.
    command: String,
    args: Json,
    /// The document's file.
    path: Option<PathBuf>,
    /// The copy.
    text: String,
    starts: Vec<usize>,
    /// The block at hand.
    next: usize,
    /// The block running, its run, and how it ended once it has.
    running: Option<(Info, Job, Option<super::Ended>)>,
    /// Every block done: the copy is the export's.
    done: bool,
}

static EXPORTS: Mutex<BTreeMap<u64, ExportRun>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

fn exports() -> std::sync::MutexGuard<'static, BTreeMap<u64, ExportRun>> {
    EXPORTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The text export command `command`, run with `args`, reads from the
/// active document (its subtree at the cursor with `subtree`): the
/// document's own where no block is to run; the copy its blocks ran in
/// where the command runs again after them (`args`' `babel`). `None` while
/// the blocks run first: the command is run again with the copy when they
/// end.
pub fn source(
    ctx: &mut EditorContext<'_>,
    command: &str,
    args: &Json,
    subtree: bool,
) -> Result<Option<String>, CommandError> {
    let use_babel = ctx.config.bool(USE_BABEL);
    let settings = Settings::of(ctx.config);
    let doc = ctx.doc()?;
    let text = doc.text().as_str().to_string();
    let path = doc.meta.path.clone();
    let head = doc.selection.head;
    if let Some(n) = args.get("babel").and_then(Json::as_u64) {
        let Some(run) = exports().remove(&n).filter(|r| r.done) else {
            return Ok(Some(text));
        };
        if run.path != path {
            return Err(CommandError::new(crate::tr!("babel-export-other-document")));
        }
        return Ok(Some(run.text));
    }
    if !use_babel {
        return Ok(Some(text));
    }
    let parse = org_syntax::parse(&text);
    let root = parse.syntax();
    let keywords = parse.keywords();
    let range = subtree
        .then(|| crate::stats::subtree_at(&root, head))
        .flatten();
    let mut starts = blocks(&text, range.as_ref());
    // Blocks of languages not loaded, or that may not run, are said and
    // left at once.
    starts.retain(|&s| match super::block_at(&root, &keywords, s) {
        Some((_, Ok(info))) => match check(&info, &settings) {
            Ok(_) => true,
            Err(m) => {
                ctx.messages.push(m);
                false
            }
        },
        _ => false,
    });
    if starts.is_empty() {
        return Ok(Some(text));
    }
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    exports().insert(
        n,
        ExportRun {
            command: command.to_string(),
            args: args.clone(),
            path,
            text,
            starts,
            next: 0,
            running: None,
            done: false,
        },
    );
    advance(ctx, n, None)?;
    Ok(None)
}

/// `org.babel.exportStep`: export `job`'s next step, an answer to its
/// question (`answer`) or a block's run ended (`ran`).
pub fn step(ctx: &mut EditorContext<'_>, args: &Json) -> CommandResult {
    let Some(n) = args.get("job").and_then(Json::as_u64) else {
        return Ok(());
    };
    if args.get("ran").and_then(Json::as_bool) == Some(true) {
        let run = exports().remove(&n);
        let Some(mut run) = run else {
            return Ok(());
        };
        finish(ctx, &mut run);
        exports().insert(n, run);
    }
    let answer = args
        .get("answer")
        .and_then(Json::as_str)
        .map(|a| a.trim().to_lowercase());
    advance(ctx, n, answer)
}

/// The block that ran in `run` written into its copy, its error output
/// shown; on to the next.
fn finish(ctx: &mut EditorContext<'_>, run: &mut ExportRun) {
    let Some((info, job, Some(exit))) = run.running.take() else {
        return;
    };
    let start = run.starts[run.next];
    run.next += 1;
    let exit = match exit {
        Ok(e) => e,
        Err(why) => {
            super::report_error(ctx, &why, None);
            return;
        }
    };
    match write(&run.text, start, &info, &job, exit.as_ref()) {
        Ok(w) => {
            if let Some((stderr, code)) = &w.error {
                super::report_error(ctx, stderr, *code);
            }
            if let Some(edit) = &w.edit {
                shift(&mut run.starts, run.next - 1, edit);
            }
            run.text = w.text;
        }
        Err(why) => ctx.messages.push(why),
    }
}

/// Export `n`'s blocks from the one at hand, `answer` the user's to its
/// question: each left, asked about, or run beside the editor; the export
/// run again with the copy after the last.
fn advance(ctx: &mut EditorContext<'_>, n: u64, mut answer: Option<String>) -> CommandResult {
    let Some(mut run) = exports().remove(&n) else {
        return Ok(());
    };
    let settings = Settings::of(ctx.config);
    loop {
        if run.next >= run.starts.len() {
            run.done = true;
            let mut args = run.args.clone();
            if !args.is_object() {
                args = serde_json::json!({});
            }
            args["babel"] = Json::from(n);
            ctx.requests.push(Request::Run {
                command: run.command.clone(),
                args,
            });
            exports().insert(n, run);
            return Ok(());
        }
        let yes = answer.as_deref() == Some("yes");
        match plan(
            &run.text,
            run.starts[run.next],
            run.path.as_deref(),
            &settings,
            yes,
        ) {
            Plan::Skip(m) => {
                if let Some(m) = m {
                    ctx.messages.push(m);
                }
                run.next += 1;
                answer = None;
            }
            Plan::Ask { question, aborted } => {
                if answer.as_deref() == Some("no") {
                    ctx.messages.push(aborted);
                    run.next += 1;
                    answer = None;
                    continue;
                }
                let label = if answer.is_some() {
                    crate::tr!("babel-answer-yes-or-no", question = &question)
                } else {
                    question
                };
                ctx.requests.push(Request::Ask {
                    command: "org.babel.exportStep".into(),
                    args: serde_json::json!({ "job": n, "answer_label": label }),
                    arg: "answer".into(),
                });
                exports().insert(n, run);
                return Ok(());
            }
            Plan::Run(b) => {
                let (info, job) = *b;
                answer = None;
                let lang = super::capitalize(&info.lang);
                ctx.messages.push(match &info.name {
                    Some(name) => crate::tr!("babel-executing-named", lang = &lang, name = name),
                    None => crate::tr!(
                        "babel-executing-at",
                        lang = &lang,
                        position = info.start + 1
                    ),
                });
                let Some(request) = job.request.clone() else {
                    // A plugin's value at once.
                    run.running = Some((info, job, Some(Ok(None))));
                    finish(ctx, &mut run);
                    continue;
                };
                run.running = Some((info, job, None));
                exports().insert(n, run);
                std::thread::spawn(move || {
                    let stop = std::sync::atomic::AtomicBool::new(false);
                    let exit = crate::extensions::run_program(
                        &request,
                        &stop,
                        crate::extensions::RUN_TIMEOUT,
                    );
                    if let Some(r) = exports().get_mut(&n)
                        && let Some(running) = r.running.as_mut()
                    {
                        running.2 = Some(exit.map(Some));
                    }
                    crate::extensions::queue_run(
                        "org.babel.exportStep",
                        serde_json::json!({ "job": n, "ran": true }),
                    );
                });
                return Ok(());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blocks_an_export_runs() {
        let text = "#+begin_src sh\necho code\n#+end_src\n\n#+begin_src sh :exports results\necho r\n#+end_src\n\n* H\n#+begin_src sh :exports both\necho b\n#+end_src\n#+begin_src sh :exports none\necho n\n#+end_src\n";
        let r = text.find("#+begin_src sh :exports results").unwrap();
        let b = text.find("#+begin_src sh :exports both").unwrap();
        assert_eq!(blocks(text, None), [r, b]);
        let h = text.find("* H").unwrap();
        assert_eq!(blocks(text, Some(&(h..text.len()))), [b]);
    }

    #[test]
    fn starts_moved_by_an_edit() {
        let mut starts = vec![0, 10, 20, 30];
        shift(&mut starts, 1, &(12..14, 7));
        assert_eq!(starts, [0, 10, 25, 35]);
    }
}
