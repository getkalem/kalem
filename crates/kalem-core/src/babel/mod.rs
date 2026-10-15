//! Org Babel (R5.6): the source block at the cursor run and its results
//! written under it, as Emacs's Org 9.7 does (`org-babel-execute-src-block`),
//! with Emacs's guards:
//!
//! - `:eval` (`never` or `no` refuse, `query` asks, `never-export`,
//!   `no-export` and `query-export` concern export only);
//! - `babel.confirm_evaluate` (`org-confirm-babel-evaluate`, on): ask
//!   before each run, the answer typed `yes` or `no`;
//! - `babel.load_languages` (`org-babel-load-languages`, none): a
//!   language runs only once named there, `shell` (sh, bash, zsh and the
//!   other shells), `python`, or a plugin's ([`plugins`], API 0.2.21);
//!   else, as Emacs says, "No org-babel-execute function for python!".
//!
//! Nothing runs on its own: a block runs when the user asks for it. The
//! program runs beside the editor; its results are written when it ends,
//! one step to undo, and what it says on its error output goes to the
//! `*Org-Babel Error Output*` document, as in Emacs.

pub mod plugins;
pub mod results;
pub mod table;
pub mod value;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use org_syntax::SyntaxKind::*;
use org_syntax::{SyntaxNode, ast};
use serde_json::Value as Json;

use crate::command::{CommandError, CommandResult, EditorContext, Request};
use value::Value;

/// The settings.
pub const CONFIRM: &str = "babel.confirm_evaluate";
pub const LANGUAGES: &str = "babel.load_languages";
pub const PYTHON: &str = "babel.python_command";

/// `org-babel-default-header-args`.
const DEFAULTS: &[(&str, &str)] = &[
    (":session", "none"),
    (":results", "replace"),
    (":exports", "code"),
    (":cache", "no"),
    (":noweb", "no"),
    (":hlines", "no"),
    (":tangle", "no"),
];

/// `:results`' words, each group's words excluding each other.
const RESULTS_GROUPS: &[&[&str]] = &[
    &["file", "list", "vector", "table", "scalar", "verbatim"],
    &[
        "raw", "html", "latex", "org", "code", "pp", "drawer", "link", "graphics",
    ],
    &["replace", "silent", "none", "discard", "append", "prepend"],
    &["output", "value"],
];

/// `:exports`' words.
const EXPORTS_GROUPS: &[&[&str]] = &[&["code", "results", "both", "none"]];

/// The shells `ob-shell` runs (`org-babel-shell-names`).
const SHELLS: &[&str] = &[
    "sh", "bash", "zsh", "fish", "csh", "ash", "dash", "ksh", "mksh", "posh",
];

/// A block's header arguments merged (`org-babel-merge-params`): a later
/// value replaces an earlier one; `:var`s add up by name; `:results` and
/// `:exports` add up their words, a word replacing those of its group.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Params {
    pairs: Vec<(String, String)>,
    /// `:var`s: the name, and what follows `=`.
    vars: Vec<(String, String)>,
    results: Vec<String>,
    exports: Vec<String>,
}

fn merge_words(out: &mut Vec<String>, words: &str, groups: &[&[&str]]) {
    for w in words.split_whitespace() {
        if let Some(g) = groups.iter().find(|g| g.contains(&w)) {
            out.retain(|o| !g.contains(&o.as_str()));
        }
        out.retain(|o| o != w);
        out.push(w.to_string());
    }
}

impl Params {
    /// The defaults merged with `headers`, in order.
    pub fn merged(headers: &[String]) -> Result<Params, String> {
        let mut p = Params::default();
        for (k, v) in DEFAULTS {
            p.add(k, v)?;
        }
        for h in headers {
            for (k, v) in org_export::babel::header_arguments(h) {
                p.add(&k, &v)?;
            }
        }
        Ok(p)
    }

    fn add(&mut self, key: &str, value: &str) -> Result<(), String> {
        match key {
            ":var" => {
                let v = value.trim();
                let name_end = v
                    .find(|c: char| c == '=' || c.is_whitespace())
                    .unwrap_or(v.len());
                let after = v[name_end..].trim_start_matches([' ', '\t']);
                if name_end > 0 && after.starts_with('=') {
                    let name = v[..name_end].to_string();
                    let rest = after[1..].trim().to_string();
                    self.vars.retain(|(n, _)| *n != name);
                    self.vars.push((name, rest));
                } else if let Some(slot) = self.vars.first_mut() {
                    // A value without a name: the first variable's.
                    slot.1 = v.to_string();
                } else {
                    return Err(crate::tr!("babel-var-no-default", var = v));
                }
            }
            ":results" => merge_words(&mut self.results, value, RESULTS_GROUPS),
            ":exports" => merge_words(&mut self.exports, value, EXPORTS_GROUPS),
            _ => {
                self.pairs.retain(|(k, _)| k != key);
                self.pairs.push((key.to_string(), value.to_string()));
            }
        }
        Ok(())
    }

    /// The value of `key` (`:dir`), read as a header value is.
    pub fn get(&self, key: &str) -> Option<String> {
        self.pairs
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| org_export::babel::header_value(v))
    }

    /// Whether `key` is given at all, even without a value (`:wrap`).
    pub fn has(&self, key: &str) -> bool {
        self.pairs.iter().any(|(k, _)| k == key)
    }

    /// `:results`' words (`:result-params`).
    pub fn results(&self) -> &[String] {
        &self.results
    }

    /// Whether `:results` asks for the output rather than the value.
    pub fn output(&self) -> bool {
        self.results.iter().any(|w| w == "output")
    }

    /// The `:var`s, by name.
    pub fn vars(&self) -> &[(String, String)] {
        &self.vars
    }
}

/// A source block as running it needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    pub lang: String,
    pub name: Option<String>,
    pub params: Params,
    pub body: String,
    /// Where the block starts in the text (its affiliated keywords
    /// included), from 0.
    pub start: usize,
}

impl Info {
    /// Message `key` about the block, which names its language, and its
    /// name when it has one (`KEY-named`), as Emacs's messages do.
    fn say(&self, key: &str) -> String {
        match &self.name {
            Some(name) => crate::tr!(&format!("{key}-named"), lang = &self.lang, name = name),
            None => crate::tr!(key, lang = &self.lang),
        }
    }
}

/// `s` as Emacs's `capitalize` writes it: each word's first letter upper
/// case, the others lower case.
fn capitalize(s: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

/// The source block holding byte `pos` of the document `root` (of
/// `text`), and what running it needs; its header arguments' error. The
/// byte is the one after the cursor: at the start of `#+begin_src` the
/// block is there, at the start of the line after it not.
pub fn block_at(
    root: &SyntaxNode,
    keywords: &[(String, String)],
    pos: usize,
) -> Option<(SyntaxNode, Result<Info, String>)> {
    let at = root.token_at_offset(org_syntax::TextSize::from(
        pos.min(usize::from(root.text_range().end())) as u32,
    ));
    let token = at.clone().right_biased().or_else(|| at.left_biased())?;
    let node = token.parent_ancestors().find(|n| n.kind() == SRC_BLOCK)?;
    let src = org_export::babel::src_block_info(&node, keywords)?;
    // A plugin language's default header arguments before the document's
    // (`org-babel-default-header-args:LANG`).
    let mut headers: Vec<String> = src
        .lang
        .as_deref()
        .and_then(plugins::for_language)
        .map(|spec| spec.headers)
        .filter(|h| !h.trim().is_empty())
        .into_iter()
        .collect();
    headers.extend(src.headers.iter().cloned());
    let info = Params::merged(&headers).map(|params| Info {
        lang: src.lang.clone().unwrap_or_default(),
        name: src.name.clone(),
        params,
        body: src.body.clone(),
        start: usize::from(node.text_range().start()),
    });
    Some((node, info))
}

/// What may happen to a block asked to run (`org-babel-check-confirm-evaluate`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// It may not run: the message.
    Disabled(String),
    /// The user is asked first: the question.
    Query(String),
    /// It runs.
    Run,
}

/// The [`Gate`] of `info`, run interactively or (`export`) by an
/// export, with `org-confirm-babel-evaluate` `confirm`.
pub fn gate(info: &Info, export: bool, confirm: bool) -> Gate {
    let eval = info
        .params
        .get(":eval")
        .or_else(|| info.params.has(":noeval").then(|| "no".to_string()));
    let eval = eval.as_deref();
    let no = matches!(eval, Some("no" | "never"));
    let no_export = export && matches!(eval, Some("no-export" | "never-export"));
    if no || no_export {
        return Gate::Disabled(info.say("babel-disabled"));
    }
    let query = eval == Some("query") || (export && eval == Some("query-export")) || confirm;
    if query {
        Gate::Query(info.say("babel-query"))
    } else {
        Gate::Run
    }
}

/// What Emacs says when the answer is no.
pub fn aborted(info: &Info) -> String {
    info.say("babel-aborted")
}

/// How a language runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Runner {
    /// A shell, by its program.
    Shell(String),
    Python,
    /// A plugin's language (API 0.2.21).
    Plugin(plugins::PluginExecutorSpec),
}

/// The runner of `lang` once `loaded` (`babel.load_languages`) names its
/// language, as `org-babel-execute:LANG` exists once its library is
/// loaded; else Emacs's error.
pub fn runner(lang: &str, loaded: &[String]) -> Result<Runner, String> {
    let has = |l: &str| loaded.iter().any(|x| x == l);
    if has("shell") && SHELLS.contains(&lang) {
        return Ok(Runner::Shell(lang.to_string()));
    }
    if has("shell") && lang == "shell" {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        return Ok(Runner::Shell(shell));
    }
    if has("python") && lang == "python" {
        return Ok(Runner::Python);
    }
    if has(lang)
        && let Some(spec) = plugins::for_language(lang)
    {
        return Ok(Runner::Plugin(spec));
    }
    Err(crate::tr!("babel-no-function", lang = lang))
}

/// The value of a `:var` written in a header (`org-babel-ref-resolve`): a
/// number, a string in double quotes, or the table, list or example of
/// that name in the document.
fn var_value(written: &str, root: &SyntaxNode) -> Result<Value, String> {
    let t = written.trim();
    if t.starts_with(['(', '\'', '`', '[']) {
        return Err(crate::tr!("babel-var-lisp", value = t));
    }
    let read = value::read(t);
    if !matches!(&read, Value::Str(s) if s == t) || t.starts_with('"') {
        return Ok(read);
    }
    let named = root
        .descendants()
        .find(|n| ast::affiliated_keywords(n).any(|k| k.key() == "NAME" && k.value().trim() == t));
    let Some(n) = named else {
        return Err(crate::tr!("babel-var-not-found", name = t));
    };
    match n.kind() {
        TABLE => {
            let text = n.text().to_string();
            let rows = text
                .lines()
                .filter(|l| l.trim_start().starts_with('|'))
                .map(|l| {
                    let l = l.trim();
                    if l.starts_with("|-") {
                        Value::Symbol("hline".into())
                    } else {
                        let inner = l.trim_start_matches('|').trim_end_matches('|');
                        Value::List(inner.split('|').map(|c| value::read(c.trim())).collect())
                    }
                })
                .collect();
            Ok(Value::List(rows))
        }
        PLAIN_LIST => Ok(Value::List(
            n.children()
                .filter(|c| c.kind() == ITEM)
                .map(|c| {
                    let t = c.text().to_string();
                    let first = t.lines().next().unwrap_or("").trim_start();
                    let item = first
                        .split_once(char::is_whitespace)
                        .map_or("", |(_, r)| r)
                        .trim();
                    value::read(item)
                })
                .collect(),
        )),
        EXAMPLE_BLOCK | FIXED_WIDTH => {
            let t = n.text().to_string();
            let body: Vec<&str> = t
                .lines()
                .filter(|l| {
                    let l = l.trim_start().to_ascii_lowercase();
                    !l.starts_with("#+")
                })
                .map(|l| {
                    l.trim_start()
                        .strip_prefix(": ")
                        .or(l.trim_start().strip_prefix(':'))
                        .unwrap_or(l)
                })
                .collect();
            Ok(Value::Str(body.join("\n")))
        }
        _ => Err(crate::tr!("babel-var-not-data", name = t)),
    }
}

/// `org-babel-sh-var-to-string`: a table's rows by lines, its fields by
/// tabs; a list's items by lines; a value as `princ` writes it.
fn sh_string(v: &Value) -> String {
    let echo = |v: &Value| match v {
        Value::Str(s) => s.clone(),
        o => o.prin1(),
    };
    match v {
        Value::List(items)
            if items
                .iter()
                .any(|i| matches!(i, Value::List(_)) || i.is_hline()) =>
        {
            items
                .iter()
                .map(|row| match row {
                    Value::List(cells) => cells.iter().map(echo).collect::<Vec<_>>().join("\t"),
                    h if h.is_hline() => "hline".to_string(),
                    o => echo(o),
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        Value::List(items) if !items.is_empty() => {
            items.iter().map(echo).collect::<Vec<_>>().join("\n")
        }
        o => echo(o),
    }
}

/// A shell's quoting of `s` in single quotes.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\"'\"'"))
}

/// The shell lines assigning `vars`: a scalar `NAME='VALUE'` everywhere;
/// in bash a list an array and a table an associative array.
fn shell_assignments(vars: &[(String, Value)], bash: bool) -> Vec<String> {
    vars.iter()
        .map(|(name, v)| match v {
            Value::List(rows)
                if bash
                    && rows
                        .iter()
                        .any(|r| matches!(r, Value::List(c) if c.len() >= 2)) =>
            {
                let entries: Vec<String> = rows
                    .iter()
                    .filter_map(|r| match r {
                        Value::List(c) if !c.is_empty() => Some(format!(
                            "{name}[{}]={}",
                            sh_quote(&sh_string(&c[0])),
                            sh_quote(&sh_string(&Value::List(c[1..].to_vec())))
                        )),
                        _ => None,
                    })
                    .collect();
                format!("unset {name}\ndeclare -A {name}\n{}", entries.join("\n"))
            }
            Value::List(items) if bash && !items.is_empty() => format!(
                "unset {name}\ndeclare -a {name}=( {} )",
                items
                    .iter()
                    .map(|i| sh_quote(&sh_string(i)))
                    .collect::<Vec<_>>()
                    .join(" ")
            ),
            other => format!("{name}={}", sh_quote(&sh_string(other))),
        })
        .collect()
}

/// `org-babel-python-var-to-python`.
fn python_value(v: &Value) -> String {
    match v {
        Value::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(python_value)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        h if h.is_hline() => "None".into(),
        Value::Str(s) if s.contains(['\n', '\r']) => format!("\"\"{}\"\"", v.prin1()),
        o => o.prin1(),
    }
}

/// The Python lines writing a block's value to a file, as Org's
/// `org-babel-python--def-format-value` does: `str()` of it, a table's
/// dicts turned into rows, `pprint` for `pp`.
const PYTHON_VALUE: &str = r#"def _kalem_babel_value(value, path, params):
    with open(path, "w") as out:
        if "pp" in params:
            import pprint
            out.write(pprint.pformat(value))
            return
        if "list" in params and isinstance(value, dict):
            out.write(str(["{} :: {}".format(k, v) for k, v in value.items()]))
            return
        if not set(params) & {"scalar", "verbatim", "raw"}:
            if "table" in params:
                def rows(v):
                    if isinstance(v, dict):
                        return [(k, rows(x)) for k, x in v.items()]
                    if isinstance(v, (list, tuple)):
                        return [rows(x) for x in v]
                    return v
                value = rows(value)
            try:
                import pandas
                if isinstance(value, pandas.DataFrame) and "table" in params:
                    value = [[value.index.name or ""] + list(value.columns)] + [None] + [[i] + list(r) for i, r in value.iterrows()]
                elif isinstance(value, pandas.Series) and "table" in params:
                    value = list(value.items())
            except ImportError:
                pass
            try:
                import numpy
                if isinstance(value, numpy.ndarray):
                    value = value.tolist() if "table" in params else repr(value)
            except ImportError:
                pass
        out.write(str(value))
"#;

/// Python's body as a function's (`org-babel-python--shift-right`): each
/// line four columns further, but the lines inside a string of three
/// quotes and blank ones.
fn shift_right(body: &str) -> String {
    let mut in_string: Option<&str> = None;
    body.split_inclusive('\n')
        .map(|l| {
            let shifted = if in_string.is_some() || l.trim().is_empty() {
                l.to_string()
            } else {
                format!("    {l}")
            };
            for q in ["\"\"\"", "'''"] {
                let n = l.matches(q).count();
                if n % 2 == 1 {
                    in_string = match in_string {
                        Some(open) if open == q => None,
                        None => Some(q),
                        other => other,
                    };
                }
            }
            shifted
        })
        .collect()
}

/// A run ready to start: the program and how its result is read.
#[derive(Debug, Clone)]
pub struct Job {
    /// The program; none where a plugin gave the value at once.
    pub request: Option<crate::extensions::ProcessRequest>,
    /// Where Python writes a block's value.
    value_file: Option<PathBuf>,
    /// The shell's value is its exit status (`:results value`).
    exit_status: bool,
    runner: Runner,
    results: Vec<String>,
    /// A plugin's block, and the value it gave at once.
    plugin: Option<(plugins::Block, Option<plugins::Data>)>,
}

static NEXT_FILE: AtomicU64 = AtomicU64::new(0);

/// The [`Job`] of `info`, of the document of file `path`, run by `runner`
/// in `cwd`, its `:var`s read from `root`; Python run as `python`
/// (`babel.python_command`, else `python` where it is found, else
/// `python3`); a plugin's language as its plugin prepares it.
pub fn job(
    info: &Info,
    runner: &Runner,
    root: &SyntaxNode,
    path: Option<&Path>,
    cwd: &Path,
    python: &str,
) -> Result<Job, String> {
    let vars: Vec<(String, Value)> = info
        .params
        .vars()
        .iter()
        .map(|(n, w)| var_value(w, root).map(|v| (n.clone(), v)))
        .collect::<Result<_, _>>()?;
    let results = info.params.results().to_vec();
    let has = |w: &str| results.iter().any(|r| r == w);
    let find = |name: &str| kalem_lsp::find_program(name, Some(cwd), &[]);
    match runner {
        Runner::Shell(shell) => {
            // `org-babel-shell-results-defaults-to-output`: the value is
            // the exit status only when `:results value` says so.
            let exit_status = has("value");
            let bash = shell.ends_with("bash");
            let mut lines = shell_assignments(&vars, bash);
            lines.push(info.body.clone());
            let mut full = lines.join("\n");
            if exit_status {
                full.push_str("\necho $?");
            }
            let program = find(shell)
                .ok_or_else(|| crate::tr!("babel-program-not-found", program = shell.as_str()))?;
            Ok(Job {
                request: Some(crate::extensions::ProcessRequest {
                    program,
                    args: Vec::new(),
                    cwd: cwd.to_path_buf(),
                    stdin: Some(format!("{}\n", full.trim()).into_bytes()),
                    env: Vec::new(),
                }),
                value_file: None,
                exit_status,
                runner: runner.clone(),
                results,
                plugin: None,
            })
        }
        Runner::Python => {
            let command = if python.trim().is_empty() {
                if find("python").is_some() {
                    "python"
                } else {
                    "python3"
                }
                .to_string()
            } else {
                python.trim().to_string()
            };
            let mut words = command.split_whitespace();
            let name = words.next().unwrap_or("python3");
            let program =
                find(name).ok_or_else(|| crate::tr!("babel-program-not-found", program = name))?;
            let mut lines: Vec<String> = vars
                .iter()
                .map(|(n, v)| format!("{n}={}", python_value(v)))
                .collect();
            let mut body = info.body.clone();
            if let Some(r) = info.params.get(":return") {
                body.push_str(&format!("\nreturn {r}"));
            }
            lines.push(body);
            let full = lines.join("\n");
            let (stdin, value_file) = if info.params.output() {
                (format!("{full}\n"), None)
            } else {
                let file = std::env::temp_dir().join(format!(
                    "kalem-babel-{}-{}",
                    std::process::id(),
                    NEXT_FILE.fetch_add(1, Ordering::Relaxed)
                ));
                let params = format!(
                    "[{}]",
                    results
                        .iter()
                        .map(|r| Value::Str(r.clone()).prin1())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                let path = Value::Str(file.to_string_lossy().into_owned()).prin1();
                (
                    format!(
                        "{PYTHON_VALUE}\ndef main():\n{}\n\n_kalem_babel_value(main(), {path}, {params})\n",
                        shift_right(&full)
                    ),
                    Some(file),
                )
            };
            Ok(Job {
                request: Some(crate::extensions::ProcessRequest {
                    program,
                    args: words.map(String::from).collect(),
                    cwd: cwd.to_path_buf(),
                    stdin: Some(stdin.into_bytes()),
                    env: Vec::new(),
                }),
                value_file,
                exit_status: false,
                runner: runner.clone(),
                results,
                plugin: None,
            })
        }
        Runner::Plugin(spec) => {
            let mut headers: Vec<(String, String)> = info
                .params
                .pairs
                .iter()
                .map(|(k, v)| (k.clone(), org_export::babel::header_value(v)))
                .collect();
            headers.push((":exports".into(), info.params.exports.join(" ")));
            let block = plugins::Block {
                language: info.lang.clone(),
                body: info.body.clone(),
                headers,
                results: results.clone(),
                vars: vars
                    .iter()
                    .map(|(n, v)| (n.clone(), plugins::Data::of_value(v)))
                    .collect(),
                path: path.map(Path::to_path_buf),
                dir: cwd.to_path_buf(),
            };
            let (request, done) = match plugins::ask(spec, plugins::Call::Prepare(&block)) {
                plugins::Answer::Prepared(plugins::Prepared::Run(r)) => (Some(r), None),
                plugins::Answer::Prepared(plugins::Prepared::Done(d)) => (None, Some(d)),
                other => return Err(refusal(spec, other)),
            };
            Ok(Job {
                request,
                value_file: None,
                exit_status: false,
                runner: runner.clone(),
                results,
                plugin: Some((block, done)),
            })
        }
    }
}

/// What a plugin's answer `a` that is not the one asked for says.
fn refusal(spec: &plugins::PluginExecutorSpec, a: plugins::Answer) -> String {
    match a {
        plugins::Answer::Refused(m) | plugins::Answer::Failed(m) => m,
        plugins::Answer::Busy => crate::tr!("babel-plugin-busy", plugin = spec.plugin.as_str()),
        _ => crate::tr!("babel-plugin-gone", plugin = spec.plugin.as_str()),
    }
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub value: Value,
    /// What the program wrote on its error output and how it exited,
    /// when it failed or wrote there (`org-babel-eval-error-notify`).
    pub error: Option<(String, Option<i32>)>,
}

/// `org-babel-result-cond`: whether `params` keep a result a string.
fn scalar(params: &[String]) -> bool {
    let has = |w: &str| params.iter().any(|p| p == w);
    ["scalar", "verbatim", "html", "code", "pp", "file"]
        .iter()
        .any(|w| has(w))
        || (["output", "raw", "org", "drawer"].iter().any(|w| has(w)) && !has("table"))
}

/// The [`Outcome`] of `job` ended as `exit` (none where it ran no
/// program); a plugin's language's value as its plugin reads it, on the
/// editor's thread, or the plugin's refusal.
pub fn outcome(
    job: &Job,
    exit: Option<&crate::extensions::ProcessExit>,
) -> Result<Outcome, String> {
    let empty = crate::extensions::ProcessExit {
        status: Some(0),
        ..Default::default()
    };
    let exit = exit.unwrap_or(&empty);
    let stdout = String::from_utf8_lossy(&exit.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&exit.stderr).into_owned();
    let error = (exit.status != Some(0) || !stderr.is_empty()).then_some((stderr, exit.status));
    let params = &job.results;
    let value = match &job.runner {
        Runner::Shell(_) => {
            let mut raw = stdout;
            if job.exit_status {
                raw = raw
                    .lines()
                    .rfind(|l| !l.is_empty())
                    .unwrap_or("")
                    .to_string();
            }
            if scalar(params) {
                Value::Str(raw)
            } else {
                table::import(&raw)
            }
        }
        Runner::Python => {
            let raw = match &job.value_file {
                Some(f) => {
                    let r = std::fs::read_to_string(f).unwrap_or_default();
                    let _ = std::fs::remove_file(f);
                    r
                }
                None => stdout,
            };
            if scalar(params) || raw.starts_with('{') {
                Value::Str(raw)
            } else {
                match value::read_script(&raw) {
                    Value::List(items) => Value::List(
                        items
                            .into_iter()
                            .map(|e| match e {
                                Value::Symbol(s) if s == "None" => Value::Symbol("hline".into()),
                                o => o,
                            })
                            .collect(),
                    ),
                    o => o,
                }
            }
        }
        Runner::Plugin(spec) => {
            let Some((block, done)) = &job.plugin else {
                return Err(crate::tr!(
                    "babel-plugin-gone",
                    plugin = spec.plugin.as_str()
                ));
            };
            match done {
                Some(d) => d.clone().into_value(params),
                None => {
                    match plugins::ask(spec, plugins::Call::Finish(block, &stdout, exit.status)) {
                        plugins::Answer::Value(d) => d.into_value(params),
                        other => return Err(refusal(spec, other)),
                    }
                }
            }
        }
    };
    // A value wanted as a table or vector that is not a list: one cell.
    let has = |w: &str| params.iter().any(|p| p == w);
    let value =
        if !has("output") && (has("vector") || has("table")) && !matches!(value, Value::List(_)) {
            Value::List(vec![Value::List(vec![value])])
        } else {
            value
        };
    Ok(Outcome { value, error })
}

/// What running a block now did ([`run_now`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Ran {
    /// The text with the block's results.
    pub text: String,
    /// What Emacs would have said: a guard's refusal, a silent result.
    pub message: Option<String>,
    /// The error output and the exit status, when there was any.
    pub error: Option<(String, Option<i32>)>,
    /// The range of `text` as it was that the results replaced, and the
    /// length of what took its place; none when nothing was written.
    pub edit: Option<(std::ops::Range<usize>, usize)>,
}

/// The block at byte `pos` of the Org text `text` run now, in `dir`, as
/// C-c C-c runs it with `babel.confirm_evaluate` off and `loaded`
/// languages, and the text with its results: for the comparison with
/// Emacs and the command line.
pub fn run_now(
    text: &str,
    pos: usize,
    loaded: &[String],
    python: &str,
    dir: &Path,
) -> Result<Ran, String> {
    let parse = org_syntax::parse(text);
    let root = parse.syntax();
    let keywords = parse.keywords();
    let (node, info) = block_at(&root, &keywords, pos).ok_or("Not in a source block")?;
    let info = info?;
    let unchanged = |message: String| Ran {
        text: text.to_string(),
        message: Some(message),
        error: None,
        edit: None,
    };
    if let Gate::Disabled(m) = gate(&info, false, false) {
        return Ok(unchanged(m));
    }
    let runner = runner(&info.lang, loaded)?;
    let job = job(&info, &runner, &root, None, dir, python)?;
    let stop = std::sync::atomic::AtomicBool::new(false);
    let exit = match &job.request {
        Some(r) => Some(crate::extensions::run_program(
            r,
            &stop,
            crate::extensions::RUN_TIMEOUT,
        )?),
        None => None,
    };
    let outcome = outcome(&job, exit.as_ref())?;
    let params = info.params.results().to_vec();
    let has = |w: &str| params.iter().any(|r| r == w);
    if has("none") {
        return Ok(Ran {
            text: text.to_string(),
            message: None,
            error: outcome.error,
            edit: None,
        });
    }
    if has("silent") {
        return Ok(Ran {
            error: outcome.error.clone(),
            ..unchanged(outcome.value.prin1())
        });
    }
    let format = results::Format {
        params: params.clone(),
        wrap: info
            .params
            .has(":wrap")
            .then(|| info.params.get(":wrap").unwrap_or_default()),
        lang: info.lang.clone(),
        switches: info.params.get(":results_switches"),
    };
    let body = results::text(&outcome.value, &format);
    let (range, new) = results::edit(text, &root, &node, info.name.as_deref(), &body, &params);
    Ok(Ran {
        text: format!("{}{}{}", &text[..range.start], new, &text[range.end..]),
        message: None,
        error: outcome.error,
        edit: Some((range.clone(), new.len())),
    })
}
/// A run under way or ended, waiting to write its results.
struct Pending {
    path: Option<PathBuf>,
    start: usize,
    lang: String,
    body: String,
    job: Job,
    /// How its program ended, once it has; `Ok(None)` where it ran none.
    exit: Option<Result<Option<crate::extensions::ProcessExit>, String>>,
    began: std::time::Instant,
}

static PENDING: Mutex<BTreeMap<u64, Pending>> = Mutex::new(BTreeMap::new());
static NEXT: AtomicU64 = AtomicU64::new(1);

/// What `*Org-Babel Error Output*` holds, every error added.
static ERRORS: Mutex<String> = Mutex::new(String::new());

fn pending() -> std::sync::MutexGuard<'static, BTreeMap<u64, Pending>> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner())
}

/// `org.babel.execute` (`org-babel-execute-src-block`, C-c C-c in a
/// source block): the guards, then the run beside the editor.
pub fn execute(ctx: &mut EditorContext<'_>, args: &Json) -> CommandResult {
    let confirm = ctx.config.bool(CONFIRM);
    let loaded: Vec<String> = ctx
        .config
        .strings(LANGUAGES)
        .into_iter()
        .map(String::from)
        .collect();
    let python = ctx.config.str(PYTHON).to_string();
    let (path, root, keywords, pos) = {
        let doc = ctx.doc()?;
        let model = doc
            .model()
            .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
        let parse = model.parse();
        (
            doc.meta.path.clone(),
            parse.syntax(),
            parse.keywords(),
            doc.selection.head,
        )
    };
    let Some((_, info)) = block_at(&root, &keywords, pos) else {
        return Err(CommandError::new(crate::tr!("babel-not-in-block")));
    };
    let info = info.map_err(CommandError::new)?;
    match gate(&info, false, confirm) {
        Gate::Disabled(m) => {
            ctx.messages.push(m);
            return Ok(());
        }
        // `yes-or-no-p`: the answer's case does not matter.
        Gate::Query(question) => match args
            .get("answer")
            .and_then(Json::as_str)
            .map(|a| a.trim().to_lowercase())
            .as_deref()
        {
            Some("yes") => {}
            Some("no") => {
                ctx.messages.push(aborted(&info));
                return Ok(());
            }
            other => {
                let mut a = serde_json::json!({ "answer_label": question });
                if other.is_some() {
                    a["answer_label"] =
                        Json::String(crate::tr!("babel-answer-yes-or-no", question = &question));
                }
                ctx.requests.push(Request::Ask {
                    command: "org.babel.execute".into(),
                    args: a,
                    arg: "answer".into(),
                });
                return Ok(());
            }
        },
        Gate::Run => {}
    }
    let runner = runner(&info.lang, &loaded).map_err(CommandError::new)?;
    let cwd = match info.params.get(":dir") {
        Some(d) => {
            let d = PathBuf::from(crate::settings::expand_home(&d));
            match &path {
                Some(p) if d.is_relative() => p.parent().map(|f| f.join(&d)).unwrap_or(d),
                _ => d,
            }
        }
        None => path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| dirs_home().unwrap_or_else(|| PathBuf::from("."))),
    };
    let job =
        job(&info, &runner, &root, path.as_deref(), &cwd, &python).map_err(CommandError::new)?;
    let request = job.request.clone();
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    pending().insert(
        n,
        Pending {
            path,
            start: info.start,
            lang: info.lang.clone(),
            body: info.body.clone(),
            job,
            exit: None,
            began: std::time::Instant::now(),
        },
    );
    let lang = capitalize(&info.lang);
    ctx.messages.push(match &info.name {
        Some(name) => crate::tr!("babel-executing-named", lang = &lang, name = name),
        None => crate::tr!(
            "babel-executing-at",
            lang = &lang,
            position = info.start + 1
        ),
    });
    let Some(request) = request else {
        // A plugin's value at once.
        if let Some(p) = pending().get_mut(&n) {
            p.exit = Some(Ok(None));
        }
        return insert_result(ctx, &serde_json::json!({ "run": n }));
    };
    std::thread::spawn(move || {
        let stop = std::sync::atomic::AtomicBool::new(false);
        let exit = crate::extensions::run_program(&request, &stop, crate::extensions::RUN_TIMEOUT);
        if let Some(p) = pending().get_mut(&n) {
            p.exit = Some(exit.map(Some));
        }
        crate::extensions::queue_run("org.babel.insertResult", serde_json::json!({ "run": n }));
    });
    Ok(())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// `org.babel.insertResult`: run `run`'s results written under its block,
/// when its document is the active one and the block is still there.
pub fn insert_result(ctx: &mut EditorContext<'_>, args: &Json) -> CommandResult {
    let Some(n) = args.get("run").and_then(Json::as_u64) else {
        return Ok(());
    };
    let Some(p) = pending().remove(&n) else {
        return Ok(());
    };
    let outcome = match &p.exit {
        None => return Ok(()),
        Some(Ok(exit)) => outcome(&p.job, exit.as_ref()).map_err(CommandError::new)?,
        Some(Err(why)) => Outcome {
            value: Value::nil(),
            error: Some((why.clone(), None)),
        },
    };
    if let Some((stderr, code)) = &outcome.error {
        let mut all = ERRORS.lock().unwrap_or_else(|e| e.into_inner());
        if !all.is_empty() && !all.ends_with('\n') {
            all.push('\n');
        }
        all.push_str(stderr);
        if !all.is_empty() && !all.ends_with('\n') {
            all.push('\n');
        }
        let line = match code {
            Some(c) => format!("[ Babel evaluation exited with code {c} ]"),
            None => "[ Babel evaluation exited abnormally ]".to_string(),
        };
        all.push_str(&line);
        let _ = crate::extensions::open_generated(
            "kalem",
            crate::extensions::GeneratedSpec {
                id: "kalem.babel-errors".into(),
                key: "errors".into(),
                title: "*Org-Babel Error Output*".into(),
                kind: "kalem-output".into(),
                language: None,
            },
            all.clone(),
            None,
            Vec::new(),
        );
        ctx.messages.push(match code {
            Some(c) => crate::tr!("babel-exited", code = *c),
            None => crate::tr!("babel-exited-abnormally"),
        });
    }
    let (path, root, keywords, text) = {
        let doc = ctx.doc()?;
        let model = doc
            .model()
            .ok_or_else(|| CommandError::new(crate::tr!("msg-not-org")))?;
        let parse = model.parse();
        (
            doc.meta.path.clone(),
            parse.syntax(),
            parse.keywords(),
            parse.text(),
        )
    };
    if path != p.path {
        ctx.messages.push(crate::tr!("babel-other-document"));
        return Ok(());
    }
    let Some((node, Ok(info))) = block_at(&root, &keywords, p.start) else {
        ctx.messages.push(crate::tr!("babel-changed"));
        return Ok(());
    };
    if info.start != p.start || info.lang != p.lang || info.body != p.body {
        ctx.messages.push(crate::tr!("babel-changed"));
        return Ok(());
    }
    let params = info.params.results().to_vec();
    let has = |w: &str| params.iter().any(|r| r == w);
    if has("none") {
        return Ok(());
    }
    // The time when it shows more than 0.0 seconds.
    let took = p.began.elapsed().as_secs_f64();
    let time = (took > 0.05).then(|| format!("{took:.1}"));
    if has("silent") {
        ctx.messages.push(outcome.value.prin1());
        return Ok(());
    }
    let format = results::Format {
        params: params.clone(),
        wrap: info
            .params
            .has(":wrap")
            .then(|| info.params.get(":wrap").unwrap_or_default()),
        lang: info.lang.clone(),
        switches: info.params.get(":results_switches"),
    };
    let body = results::text(&outcome.value, &format);
    let (range, new) = results::edit(&text, &root, &node, info.name.as_deref(), &body, &params);
    let now = ctx.now;
    ctx.doc()?
        .run(now, |_, _, _| {
            let mut tx = org_edit::Transaction::new("Babel: Execute");
            let _ = tx.replace(range.clone(), new.clone());
            Ok(tx)
        })
        .map_err(CommandError::from)?;
    let key = if !(outcome.value.is_nil() || body.is_empty()) {
        "babel-complete"
    } else if has("value") {
        "babel-no-value"
    } else {
        "babel-no-output"
    };
    ctx.messages.push(match time {
        Some(t) => crate::tr!(&format!("{key}-took"), seconds = t),
        None => crate::tr!(key),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(header: &str) -> Info {
        Info {
            lang: "sh".into(),
            name: None,
            params: Params::merged(&[header.to_string()]).unwrap(),
            body: String::new(),
            start: 0,
        }
    }

    #[test]
    fn header_arguments_merged_as_emacs_merges_them() {
        let p = Params::merged(&[":results output".into(), ":results table".into()]).unwrap();
        assert_eq!(
            p.results(),
            ["replace", "output", "table"].map(String::from)
        );
        let p = Params::merged(&[":results output".into(), ":results value".into()]).unwrap();
        assert_eq!(p.results(), ["replace", "value"].map(String::from));
        let p = Params::merged(&[":var x=1 :var y=2".into(), ":var x=3".into()]).unwrap();
        assert_eq!(
            p.vars(),
            [("y".into(), "2".into()), ("x".into(), "3".into())]
        );
        assert!(Params::merged(&[":var 5".into()]).is_err());
    }

    #[test]
    fn the_block_at_the_cursor() {
        let text = "Before.\n#+begin_src sh\necho a\n#+end_src\nAfter.\n";
        let parse = org_syntax::parse(text);
        let root = parse.syntax();
        let at = |pos: usize| {
            block_at(&root, &[], pos).map(|(n, _)| usize::from(n.text_range().start()))
        };
        let start = text.find("#+begin").unwrap();
        assert_eq!(at(start), Some(start));
        assert_eq!(at(text.find("echo").unwrap()), Some(start));
        assert_eq!(at(text.find("After").unwrap() - 1), Some(start));
        assert_eq!(at(start - 1), None);
        assert_eq!(at(text.find("After").unwrap()), None);
    }

    #[test]
    fn the_guards_are_emacss() {
        assert_eq!(
            gate(&info(":eval never"), false, true),
            Gate::Disabled("Evaluation of this sh code block is disabled.".into())
        );
        assert_eq!(gate(&info(""), false, false), Gate::Run);
        assert_eq!(
            gate(&info(""), false, true),
            Gate::Query("Evaluate this sh code block on your system? (yes or no)".into())
        );
        assert_eq!(
            gate(&info(":eval query"), false, false),
            Gate::Query("Evaluate this sh code block on your system? (yes or no)".into())
        );
        // Export only.
        assert_eq!(gate(&info(":eval never-export"), false, false), Gate::Run);
        assert!(matches!(
            gate(&info(":eval never-export"), true, false),
            Gate::Disabled(_)
        ));
        let mut named = info("");
        named.name = Some("n".into());
        assert_eq!(
            aborted(&named),
            "Evaluation of this sh code block (n) is aborted."
        );
        assert_eq!(capitalize("emacs-lisp"), "Emacs-Lisp");
        assert_eq!(capitalize("SH"), "Sh");
        // Languages: none unless loaded.
        assert_eq!(
            runner("python", &[]),
            Err("No org-babel-execute function for python!".into())
        );
        assert_eq!(
            runner("bash", &["shell".into()]),
            Ok(Runner::Shell("bash".into()))
        );
        assert_eq!(runner("python", &["python".into()]), Ok(Runner::Python));
    }

    #[test]
    fn python_bodies() {
        assert_eq!(
            shift_right("x = 1\n\nreturn x"),
            "    x = 1\n\n    return x"
        );
        assert_eq!(
            shift_right("s = \"\"\"a\nb\"\"\"\nreturn s"),
            "    s = \"\"\"a\nb\"\"\"\n    return s"
        );
        assert_eq!(
            python_value(&Value::List(vec![Value::Int(1), Value::Str("a".into())])),
            "[1, \"a\"]"
        );
        assert_eq!(sh_quote("it's"), "'it'\"'\"'s'");
    }
}
