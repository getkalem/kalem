//! TODO states and priorities: `org-todo` (cycling, selection, keyword
//! sets), with CLOSED timestamps, state logging and the statistics cookies
//! of parents, and `org-priority`.
//!
//! Logging follows Emacs: `#+STARTUP: logdone` adds a CLOSED timestamp,
//! keywords such as `WAIT(w@/!)` record state changes, and a `LOGGING`
//! property overrides both. A log entry that only needs a time is written
//! at once; one that needs a note is returned as a [`PendingNote`] for the
//! user interface to complete with [`store_log_note`].

use jiff::civil::DateTime;
use org_model::{Document, Inherit, complex_heading, time};
use org_syntax::{ParseContext, SyntaxKind};

use crate::buffer::{Buf, EditError};
use crate::headline::{align_tags, headings, org_back_to_heading};
use crate::property::{indentation, is_planning_line, property_drawer_at};
use crate::timestamp::{TsField, change};
use crate::transaction::Transaction;

/// What a log entry records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogKind {
    /// A timestamp.
    Time,
    /// A timestamp and a note from the user.
    Note,
}

/// The state a repeating task returns to (`org-todo-repeat-to-state`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepeatToState {
    /// The first keyword of the task's sequence (`nil`).
    Head,
    /// The state before DONE (`t`).
    Previous,
    /// This keyword.
    State(String),
}

/// Settings of the TODO commands. The defaults are those of `emacs -Q`;
/// [`TodoSettings::for_document`] applies the document's `#+STARTUP`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoSettings {
    /// `org-log-done`: what marking a task done records (a CLOSED
    /// timestamp, and a note if [`LogKind::Note`]).
    pub log_done: Option<LogKind>,
    /// `org-log-repeat`: what completing a repeating task records.
    pub log_repeat: Option<LogKind>,
    /// `org-log-into-drawer`: the drawer for log entries.
    pub log_into_drawer: Option<String>,
    /// `org-log-states-order-reversed`: newest entries first.
    pub log_states_order_reversed: bool,
    /// `org-closed-keep-when-no-todo`.
    pub closed_keep_when_no_todo: bool,
    /// `org-log-done-with-time`: CLOSED timestamps have a time.
    pub log_done_with_time: bool,
    /// `org-todo-repeat-to-state`.
    pub repeat_to_state: RepeatToState,
    /// `org-adapt-indentation`: new planning lines and log entries are
    /// indented to the headline's text.
    pub adapt_indentation: bool,
    /// `org-priority-start-cycle-with-default`.
    pub priority_start_cycle_with_default: bool,
}

impl Default for TodoSettings {
    fn default() -> TodoSettings {
        TodoSettings {
            log_done: None,
            log_repeat: Some(LogKind::Time),
            log_into_drawer: None,
            log_states_order_reversed: true,
            closed_keep_when_no_todo: false,
            log_done_with_time: true,
            repeat_to_state: RepeatToState::Head,
            adapt_indentation: false,
            priority_start_cycle_with_default: true,
        }
    }
}

impl TodoSettings {
    /// These settings with the document's `#+STARTUP` options applied.
    pub fn for_document(&self, doc: &Document) -> TodoSettings {
        let mut s = self.clone();
        for (k, v) in &doc.info().keywords {
            if k.eq_ignore_ascii_case("STARTUP") {
                for opt in v.split_whitespace() {
                    s.apply_startup(&opt.to_ascii_lowercase());
                }
            }
        }
        s
    }

    fn apply_startup(&mut self, opt: &str) -> bool {
        match opt {
            "logdone" => self.log_done = Some(LogKind::Time),
            "lognotedone" => self.log_done = Some(LogKind::Note),
            "nologdone" => self.log_done = None,
            "logrepeat" => self.log_repeat = Some(LogKind::Time),
            "lognoterepeat" => self.log_repeat = Some(LogKind::Note),
            "nologrepeat" => self.log_repeat = None,
            "logdrawer" => self.log_into_drawer = Some("LOGBOOK".into()),
            "nologdrawer" => self.log_into_drawer = None,
            "logstatesreversed" => self.log_states_order_reversed = true,
            "nologstatesreversed" => self.log_states_order_reversed = false,
            _ => return false,
        }
        true
    }
}

/// The argument of `org-todo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoArg {
    /// No argument: the next state, then no keyword after the last done
    /// state.
    Cycle,
    /// `right`: the next keyword of all sequences.
    Next,
    /// `left`: the previous keyword of all sequences.
    Previous,
    /// This keyword; the empty string removes the keyword.
    State(String),
    /// `none`: no keyword.
    None,
    /// `done`: the first done keyword of the sequence.
    Done,
    /// `nextset`: the first keyword of the next sequence.
    NextSet,
    /// `previousset`: the first keyword of the previous sequence.
    PreviousSet,
    /// A numeric prefix: the Nth keyword of all sequences.
    Nth(usize),
}

/// The options of [`todo`].
#[derive(Debug, Clone)]
pub struct TodoOptions<'a> {
    /// The requested change.
    pub arg: TodoArg,
    /// Settings, with the document's applied.
    pub settings: &'a TodoSettings,
    /// The current time.
    pub now: DateTime,
    /// The keyword sequence last used on this headline (see
    /// [`TodoOutcome::head`]), used to cycle from no keyword.
    pub remembered_head: Option<String>,
    /// The previous command was also a TODO change (`last-command`), which
    /// changes cycling through type keywords.
    pub repeated: bool,
    /// `C-u`: take a note whatever the settings.
    pub force_note: bool,
    /// Prefix 0: record a time where a note would be taken.
    pub inhibit_note: bool,
}

/// A log entry waiting for the user's note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingNote {
    /// The start of the headline, in the text after the command.
    pub heading: usize,
    /// What the note is about.
    pub purpose: NotePurpose,
    /// The new state.
    pub state: Option<String>,
    /// The previous state.
    pub previous_state: Option<String>,
    /// The time of the change.
    pub time: DateTime,
}

/// Why a log entry is written (a key of `org-log-note-headings`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotePurpose {
    /// `done`: "CLOSING NOTE".
    Done,
    /// `state`: a state change.
    State,
}

/// The result of [`todo`].
#[derive(Debug, Clone)]
pub struct TodoOutcome {
    /// The change.
    pub transaction: Transaction,
    /// The keyword sequence to remember for this headline (Emacs keeps it
    /// in the `org-todo-head` text property).
    pub head: Option<String>,
    /// A log entry that needs the user's note.
    pub note: Option<PendingNote>,
}

/// The keyword tables of `org-set-regexps-and-options`.
struct Keywords<'a> {
    ctx: &'a ParseContext,
    /// `org-todo-keywords-1`.
    all: Vec<&'a str>,
}

/// An entry of `org-todo-kwd-alist`: type, head, first and last done
/// keywords of a keyword's sequence.
struct SequenceInfo<'a> {
    types: bool,
    head: &'a str,
    done_word: Option<&'a str>,
    final_done_word: Option<&'a str>,
}

impl<'a> Keywords<'a> {
    fn new(ctx: &'a ParseContext) -> Keywords<'a> {
        let all = ctx
            .todo_sequences
            .iter()
            .flat_map(|s| s.keywords.iter().map(|k| k.name.as_str()))
            .collect();
        Keywords { ctx, all }
    }

    fn is_done(&self, k: Option<&str>) -> bool {
        k.is_some_and(|k| self.ctx.done_keywords.iter().any(|d| d == k))
    }

    fn is_not_done(&self, k: Option<&str>) -> bool {
        k.is_some_and(|k| self.all.contains(&k) && !self.is_done(Some(k)))
    }

    /// `(assoc head org-todo-kwd-alist)`.
    fn info(&self, name: Option<&str>) -> Option<SequenceInfo<'a>> {
        let name = name?;
        let seq = self
            .ctx
            .todo_sequences
            .iter()
            .find(|s| s.keywords.iter().any(|k| k.name == name))?;
        let done: Vec<&str> = seq
            .keywords
            .iter()
            .filter(|k| k.done)
            .map(|k| k.name.as_str())
            .collect();
        Some(SequenceInfo {
            types: matches!(seq.kind, org_syntax::TodoSequenceKind::Type),
            head: seq.keywords.first().map_or("", |k| k.name.as_str()),
            done_word: done.first().copied(),
            final_done_word: done.last().copied(),
        })
    }

    /// `org-get-todo-sequence-head`.
    fn head(&self, kwd: Option<&str>, remembered: Option<&'a str>) -> Option<&'a str> {
        match kwd {
            None => remembered,
            Some(k) if !self.all.contains(&k) => self.all.first().copied(),
            Some(k) => self.info(Some(k)).map(|i| i.head),
        }
    }

    /// `org-todo-heads`.
    fn heads(&self) -> Vec<&'a str> {
        let mut out: Vec<&str> = Vec::new();
        for s in &self.ctx.todo_sequences {
            if let Some(k) = s.keywords.first()
                && !out.contains(&k.name.as_str())
            {
                out.push(k.name.as_str());
            }
        }
        out
    }

    /// `org-todo-log-states`: (keyword, on entering, on leaving), with
    /// later keywords first as `assoc` finds them.
    fn log_states(&self) -> Vec<(String, Option<LogKind>, Option<LogKind>)> {
        let mut out: Vec<_> = self
            .ctx
            .todo_sequences
            .iter()
            .flat_map(|s| s.keywords.iter())
            .filter_map(|k| {
                let (a, b) = log_spec(k.spec.as_deref()?)?;
                Some((k.name.clone(), a, b))
            })
            .collect();
        out.reverse();
        out
    }
}

/// `org-extract-log-state-settings` for the text inside a keyword's
/// parentheses, such as `w@/!`.
fn log_spec(spec: &str) -> Option<(Option<LogKind>, Option<LogKind>)> {
    let mut chars = spec.chars().peekable();
    if chars.peek().is_some_and(|c| !matches!(c, '!' | '@' | '/')) {
        chars.next();
    }
    let kind = |c: char| {
        if c == '!' {
            LogKind::Time
        } else {
            LogKind::Note
        }
    };
    let enter = chars.next_if(|c| matches!(c, '!' | '@')).map(kind);
    let leave = if chars.next_if_eq(&'/').is_some() {
        Some(kind(chars.next().filter(|c| matches!(c, '!' | '@'))?))
    } else {
        None
    };
    (chars.next().is_none() && (enter.is_some() || leave.is_some())).then_some((enter, leave))
}

/// Logging settings of an entry after its `LOGGING` property
/// (`org-local-logging`).
struct Logging {
    done: Option<LogKind>,
    repeat: Option<LogKind>,
    states: Vec<(String, Option<LogKind>, Option<LogKind>)>,
}

fn logging(doc: &Document, heading: usize, settings: &TodoSettings, kw: &Keywords<'_>) -> Logging {
    let mut l = Logging {
        done: settings.log_done,
        repeat: settings.log_repeat,
        states: kw.log_states(),
    };
    let entry = doc.outline().entry_at(heading);
    if let Some(value) = doc.entry_get(entry, "LOGGING", Inherit::Yes, true) {
        l = Logging {
            done: None,
            repeat: None,
            states: Vec::new(),
        };
        for w in value.split_whitespace() {
            let mut s = TodoSettings {
                log_done: l.done,
                log_repeat: l.repeat,
                ..TodoSettings::default()
            };
            if s.apply_startup(&w.to_ascii_lowercase()) {
                // Only the logging variables are taken from the options.
                l.done = s.log_done;
                l.repeat = s.log_repeat;
            } else if let Some(open) = w.find('(')
                && w.ends_with(')')
                && let Some((a, b)) = log_spec(&w[open + 1..w.len() - 1])
                && kw.all.contains(&&w[..open])
            {
                l.states.insert(0, (w[..open].to_string(), a, b));
            }
        }
    }
    l
}

/// The drawer for log entries of the entry at `heading`
/// (`org-log-into-drawer`).
fn log_drawer(doc: &Document, heading: usize, settings: &TodoSettings) -> Option<String> {
    let entry = doc.outline().entry_at(heading);
    match doc
        .entry_get(entry, "LOG_INTO_DRAWER", Inherit::Yes, true)
        .as_deref()
    {
        Some("nil") => None,
        Some("t") => Some("LOGBOOK".into()),
        Some(p) => Some(p.to_string()),
        None => settings.log_into_drawer.clone(),
    }
}

fn line_end(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i)
}

/// `forward-line`: the start of the next line, or the end of the text.
fn next_line(text: &str, pos: usize) -> usize {
    text[pos..].find('\n').map_or(text.len(), |i| pos + i + 1)
}

fn bol_of(text: &str, pos: usize) -> usize {
    text[..pos].rfind('\n').map_or(0, |i| i + 1)
}

/// The `org-todo` regexp after the stars: ` +KEYWORD\( +\|[ \t]*$\)`.
/// Returns the keyword and the end of the match, or the end of the
/// spaces when there is no keyword.
fn keyword_at<'a>(text: &str, stars_end: usize, kw: &Keywords<'a>) -> (Option<&'a str>, usize) {
    let eol = line_end(text, stars_end);
    let line = &text[stars_end..eol];
    let spaces = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[spaces..];
    let found = kw
        .all
        .iter()
        .copied()
        .filter(|k| {
            spaces > 0
                && rest.strip_prefix(k).is_some_and(|r| {
                    r.starts_with(' ') || r.trim_start_matches([' ', '\t']).is_empty()
                })
        })
        .max_by_key(|k| k.len());
    match found {
        Some(k) => {
            let after = &rest[k.len()..];
            let end = if after.starts_with(' ') {
                stars_end + spaces + k.len() + (after.len() - after.trim_start_matches(' ').len())
            } else {
                eol
            };
            (Some(k), end)
        }
        None => (None, stars_end + spaces),
    }
}

fn is_commented(doc: &Document, heading: usize) -> bool {
    let root = doc.parse().syntax();
    root.descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::HEADLINE | SyntaxKind::INLINETASK))
        .find(|n| usize::from(n.text_range().start()) == heading)
        .is_some_and(|n| {
            n.children_with_tokens()
                .any(|t| t.kind() == SyntaxKind::COMMENT_KEYWORD)
        })
}

/// `org-toggle-comment` on the heading at `h`.
fn toggle_comment(buf: &mut Buf, h: usize, commented: bool, ctx: &ParseContext) {
    let eol = line_end(&buf.text, h);
    let Some(parts) = complex_heading(&buf.text[h..eol], ctx) else {
        return;
    };
    let mut p = h + parts.priority.or(parts.todo).unwrap_or(parts.stars).end;
    while matches!(buf.text.as_bytes().get(p), Some(b' ' | b'\t')) {
        p += 1;
    }
    if !matches!(
        buf.text.as_bytes().get(p.wrapping_sub(1)),
        Some(b' ' | b'\t')
    ) {
        buf.insert_before_point(p, " ");
        p += 1;
    }
    if commented {
        let eol = line_end(&buf.text, p);
        let mut e = buf.text[p..eol].find(' ').map_or(eol, |i| p + i + 1);
        while matches!(buf.text.as_bytes().get(e), Some(b' ' | b'\t')) {
            e += 1;
        }
        buf.delete(p, e);
    } else {
        buf.insert_before_point(p, "COMMENT");
        if p + 7 != line_end(&buf.text, p) {
            buf.insert_before_point(p + 7, " ");
        }
    }
}

/// A log entry set up by `org-add-log-setup`, written after the command.
#[derive(Debug, Clone)]
struct LogSetup {
    purpose: NotePurpose,
    how: LogKind,
    state: Option<String>,
    previous: Option<String>,
    /// Marker of the headline.
    heading: usize,
}

/// `org-todo` on the headline at `point`.
pub fn todo(
    doc: &Document,
    point: usize,
    opts: &TodoOptions<'_>,
) -> Result<TodoOutcome, EditError> {
    let text = doc.parse().syntax().to_string();
    let ctx = doc.parse().context();
    let mut buf = Buf::new(&text, point);
    let Some(h) = org_back_to_heading(&text, point, ctx) else {
        return Err(EditError::new(&format!(
            "Before first headline at position {}",
            point + 1
        )));
    };
    let mut setup = None;
    let head = change_state(&mut buf, doc, h, &opts.arg, opts, false, &mut setup)?;
    // `post-command-hook`: `org-add-log-note`.
    let mut note = None;
    if let Some(s) = setup {
        let pending = PendingNote {
            heading: buf.marker(s.heading),
            purpose: s.purpose,
            state: s.state,
            previous_state: s.previous,
            time: opts.now,
        };
        if s.how == LogKind::Time {
            let h0 = org_back_to_heading(&text, point, ctx).unwrap_or(0);
            let drawer = log_drawer(doc, h0, opts.settings);
            store_note(
                &mut buf,
                &pending,
                None,
                drawer.as_deref(),
                opts.settings,
                ctx,
            );
        } else {
            note = Some(pending);
        }
    }
    Ok(TodoOutcome {
        transaction: buf.transaction("Change TODO state"),
        head,
        note,
    })
}

/// The body of `org-todo` for the headline at `h`; `doc` is the document
/// of the text in `buf`. `nested` is the call from
/// `org-auto-repeat-maybe`, where logging is off unless a `LOGGING`
/// property turns it on. Returns the sequence head to remember.
#[allow(clippy::too_many_arguments)]
fn change_state(
    buf: &mut Buf,
    doc: &Document,
    h: usize,
    arg: &TodoArg,
    opts: &TodoOptions<'_>,
    nested: bool,
    setup: &mut Option<LogSetup>,
) -> Result<Option<String>, EditError> {
    let ctx = doc.parse().context();
    let kw = Keywords::new(ctx);
    // Edits above the headline (parents' statistics cookies) move it.
    let hm = buf.add_marker(h);
    let commented = is_commented(doc, h);
    if commented {
        toggle_comment(buf, h, true, ctx);
    }
    let stars = buf.text[h..].bytes().take_while(|b| *b == b'*').count();
    let stars_end = h + stars;
    let (this, m_end) = keyword_at(&buf.text, stars_end, &kw);
    let remembered = opts
        .remembered_head
        .as_deref()
        .and_then(|r| kw.all.iter().copied().find(|k| *k == r));
    let mut head = kw.head(this, remembered);
    let info = kw.info(head);
    let member = this.and_then(|t| kw.all.iter().position(|k| *k == t));
    let tail: &[&str] = member.map_or(&[], |i| &kw.all[i + 1..]);
    let first_done = || kw.all.iter().copied().find(|k| kw.is_done(Some(k)));
    let state: Option<&str> = match arg {
        TodoArg::Next => match this {
            Some(_) => tail.first().copied(),
            None => kw.all.first().copied(),
        },
        TodoArg::Previous => match member {
            Some(0) => None,
            Some(i) => kw.all.get(i - 1).copied(),
            None => kw.all.last().copied(),
        },
        TodoArg::State(s) if s.is_empty() => None,
        TodoArg::None => None,
        TodoArg::Done => info.as_ref().and_then(|i| i.done_word).or_else(first_done),
        TodoArg::NextSet | TodoArg::PreviousSet => {
            let mut heads = kw.heads();
            if *arg == TodoArg::PreviousSet {
                heads.reverse();
            }
            head.and_then(|hd| heads.iter().position(|x| *x == hd))
                .and_then(|i| heads.get(i + 1))
                .or(heads.first())
                .copied()
        }
        TodoArg::State(s) => match kw.all.iter().copied().find(|k| k == s) {
            Some(k) => Some(k),
            None => {
                return Err(EditError::new(&format!(
                    "State `{s}' not valid in this file"
                )));
            }
        },
        TodoArg::Nth(n) => n.checked_sub(1).and_then(|i| kw.all.get(i)).copied(),
        TodoArg::Cycle => {
            if member.is_none() {
                head.or(kw.all.first().copied())
            } else if (this.is_some() && info.as_ref().and_then(|i| i.final_done_word) == this)
                || tail.is_empty()
            {
                // After the final done state, or the last keyword: none.
                None
            } else if info.as_ref().is_some_and(|i| i.types) {
                if opts.repeated {
                    tail.first().copied()
                } else {
                    info.as_ref().and_then(|i| i.done_word).or_else(first_done)
                }
            } else {
                tail.first().copied()
            }
        }
    };
    let next = match state {
        Some(s) if !s.is_empty() => format!(" {s} "),
        _ => " ".to_string(),
    };
    buf.replace_before_markers(stars_end, m_end, &next);
    if head.is_none() {
        head = kw.head(state, None);
    }
    let now_done = kw.is_done(state) && !kw.is_done(this);

    // Logging.
    let mut log = logging(doc, h, opts.settings, &kw);
    if nested && !has_logging_property(doc, h) {
        log.done = None;
        log.states.clear();
    }
    let set_change = matches!(arg, TodoArg::NextSet | TodoArg::PreviousSet);
    let force = opts.force_note && !nested;
    if ((!log.states.is_empty() || log.done.is_some()) && !set_change) || force {
        let find = |k: Option<&str>, enter: bool| {
            let k = k?;
            let e = log.states.iter().find(|(n, _, _)| n == k)?;
            if enter { e.1 } else { e.2 }
        };
        let mut dolog = if force {
            Some(LogKind::Note)
        } else {
            find(state, true).or_else(|| find(this, false))
        };
        if dolog == Some(LogKind::Note) && opts.inhibit_note {
            dolog = Some(LogKind::Time);
        }
        if (state.is_none() && !opts.settings.closed_keep_when_no_todo)
            || (state.is_some() && kw.is_not_done(state) && !kw.is_not_done(this))
        {
            add_planning_info(buf, h, None, &[Planning::Closed], opts.settings);
        }
        if now_done && log.done.is_some() {
            add_planning_info(
                buf,
                h,
                Some((Planning::Closed, opts.now)),
                &[],
                opts.settings,
            );
            if dolog.is_none() && log.done == Some(LogKind::Note) {
                *setup = Some(LogSetup {
                    purpose: NotePurpose::Done,
                    how: LogKind::Note,
                    state: state.map(str::to_string),
                    previous: this.map(str::to_string),
                    heading: hm,
                });
            }
        }
        if let (Some(_), Some(how)) = (state, dolog) {
            *setup = Some(LogSetup {
                purpose: NotePurpose::State,
                how,
                state: state.map(str::to_string),
                previous: this.map(str::to_string),
                heading: hm,
            });
        }
    }
    align_tags(buf, h);
    update_parent_todo_statistics(buf, doc, h, stars);
    let h = buf.marker(hm);
    if !matches!(arg, TodoArg::Cycle) && !kw.is_done(state) {
        head = kw.head(state, remembered);
    }
    let repeated = if now_done {
        auto_repeat(buf, hm, state.unwrap_or(""), this, &log, opts, setup, ctx)?
    } else {
        false
    };
    let h = if repeated { buf.marker(hm) } else { h };

    // Cursor fixup, inside `save-excursion`: `just-one-space` after the
    // keyword, unless tags follow. After a repeat, point has left the
    // headline and nothing happens.
    if !repeated {
        let eol = line_end(&buf.text, h);
        let (after_kw, _) = keyword_line_end(&buf.text, h, eol, &kw);
        let tail_text = &buf.text[after_kw..eol];
        let spaces = tail_text.len() - tail_text.trim_start_matches(' ').len();
        let run = tail_text.len() - tail_text.trim_start_matches([' ', '\t']).len();
        if spaces > 0 && !tail_text[spaces..].starts_with(':') && run > 1 {
            buf.delete(after_kw + 1, after_kw + run);
        }
    }
    if commented {
        toggle_comment(buf, h, false, ctx);
    }
    Ok(head.map(str::to_string))
}

fn has_logging_property(doc: &Document, h: usize) -> bool {
    let entry = doc.outline().entry_at(h);
    doc.entry_get(entry, "LOGGING", Inherit::Yes, true)
        .is_some()
}

/// `org-repeat-re` matches in `from..to`: the timestamp start, the match
/// end and the repeater.
fn repeat_matches(text: &str, from: usize, to: usize) -> Vec<(usize, usize, String)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = from;
    while let Some(k) = text[i..to].find('<') {
        let s = i + k;
        let date = b.get(s + 1..s + 11).is_some_and(|d| {
            d.iter().enumerate().all(|(j, c)| {
                if j == 4 || j == 7 {
                    *c == b'-'
                } else {
                    c.is_ascii_digit()
                }
            })
        });
        if !date || b.get(s + 11) != Some(&b' ') {
            i = s + 1;
            continue;
        }
        // `[^>\n]*?` then `[.+]?\+[0-9]+[hdwmy]\(/[0-9]+[hdwmy]\)?`.
        let mut found = None;
        let mut j = s + 12;
        while j < to && !matches!(b[j], b'>' | b'\n') {
            let mut m = j;
            if matches!(b[m], b'.' | b'+') && b.get(m + 1) == Some(&b'+') {
                m += 1;
            }
            if b[m] == b'+' {
                let n = b[m + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                if n > 0 && matches!(b.get(m + 1 + n), Some(b'h' | b'd' | b'w' | b'm' | b'y')) {
                    let mut e = m + 2 + n;
                    if b.get(e) == Some(&b'/') {
                        let n2 = b[e + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                        if n2 > 0
                            && matches!(b.get(e + 1 + n2), Some(b'h' | b'd' | b'w' | b'm' | b'y'))
                        {
                            e += 2 + n2;
                        }
                    }
                    if e <= to {
                        found = Some((j, e));
                    }
                    break;
                }
            }
            j += text[j..].chars().next().map_or(1, char::len_utf8);
        }
        match found {
            Some((r, e)) => {
                out.push((s, e, text[r..e].to_string()));
                i = e;
            }
            None => i = s + 1,
        }
    }
    out
}

/// `org-auto-repeat-maybe` after the headline at marker `hm` was marked
/// `done_word` from `last_state`. Returns whether the entry repeats.
#[allow(clippy::too_many_arguments)]
fn auto_repeat(
    buf: &mut Buf,
    hm: usize,
    done_word: &str,
    last_state: Option<&str>,
    log: &Logging,
    opts: &TodoOptions<'_>,
    setup: &mut Option<LogSetup>,
    ctx: &ParseContext,
) -> Result<bool, EditError> {
    let h = buf.marker(hm);
    let entry_end = headings(&buf.text, None)
        .into_iter()
        .find(|(s, _)| *s > h)
        .map_or(buf.text.len(), |(s, _)| s);
    let parse = org_syntax::parse_with(&buf.text, ctx);
    let doc = Document::new(parse);
    let agenda =
        |text: &str, root: &org_syntax::SyntaxNode, s: usize| agenda_timestamp(text, root, h, s);
    let root = doc.parse().syntax();
    let Some((_, _, repeat)) = repeat_matches(&buf.text, h, entry_end)
        .into_iter()
        .find(|(s, _, _)| agenda(&buf.text, &root, *s))
    else {
        return Ok(false);
    };
    if org_model::string_to_number(&repeat[1..]) == 0.0 {
        return Ok(false);
    }
    let kw = Keywords::new(ctx);
    let last = last_state.unwrap_or("");
    let entry = doc.outline().entry_at(h);
    let to_state = doc
        .entry_get(entry, "REPEAT_TO_STATE", Inherit::Selective, false)
        .or_else(|| match &opts.settings.repeat_to_state {
            RepeatToState::State(s) => Some(s.clone()),
            RepeatToState::Previous => Some(last.to_string()),
            RepeatToState::Head => None,
        });
    let aa = kw.info(Some(last));
    let arg = match to_state {
        Some(t) if kw.all.contains(&t.as_str()) => TodoArg::State(t),
        _ if aa.as_ref().is_some_and(|a| a.types) => TodoArg::State(last.to_string()),
        _ => match aa {
            Some(a) => TodoArg::State(a.head.to_string()),
            None => TodoArg::None,
        },
    };
    change_state(buf, &doc, h, &arg, opts, true, setup)?;
    let h = buf.marker(hm);
    add_planning_info(buf, h, None, &[Planning::Closed], opts.settings);
    let entry_end_m = {
        let e = headings(&buf.text, None)
            .into_iter()
            .find(|(s, _)| *s > h)
            .map_or(buf.text.len(), |(s, _)| s);
        buf.add_marker(e)
    };
    let has_clock = doc.parse().syntax().descendants().any(|n| {
        n.kind() == SyntaxKind::CLOCK && {
            let s = usize::from(n.text_range().start());
            s > h && s < entry_end
        }
    });
    if log.repeat.is_some() || has_clock {
        let stamp = format!("[{}]", time::format(opts.now, true));
        crate::property::entry_put(
            buf,
            Some(h),
            "LAST_REPEAT",
            &stamp,
            opts.settings.adapt_indentation,
            ctx.inlinetask_min_level,
        )?;
    }
    if let Some(how) = log.repeat {
        match setup {
            Some(s) => {
                if how == LogKind::Note {
                    s.how = LogKind::Note;
                }
            }
            None => {
                *setup = Some(LogSetup {
                    purpose: NotePurpose::State,
                    how,
                    state: Some(if done_word.is_empty() {
                        ctx.done_keywords.first().cloned().unwrap_or_default()
                    } else {
                        done_word.to_string()
                    }),
                    previous: Some(last.to_string()),
                    heading: hm,
                });
            }
        }
    }
    // A SCHEDULED date without a repeater goes.
    let h = buf.marker(hm);
    remove_unrepeated_scheduled(buf, h);
    // Every repeating timestamp moves, last first so positions hold.
    let h = buf.marker(hm);
    let end = buf.marker(entry_end_m);
    let parse = org_syntax::parse_with(&buf.text, ctx);
    let root = parse.syntax();
    let text = buf.text.clone();
    let matches: Vec<(usize, usize, String)> = repeat_matches(&text, h, end)
        .into_iter()
        .filter(|(s, _, _)| agenda(&text, &root, *s))
        .collect();
    let today = opts.now.date();
    for (s, e, rep) in matches.into_iter().rev() {
        let ts = &text[s..e];
        let dot = rep.starts_with('.');
        let plusplus = rep.starts_with("++");
        let body = rep.trim_start_matches('.').trim_start_matches('+');
        let digits: String = body.chars().take_while(char::is_ascii_digit).collect();
        let mut n: i64 = digits.parse().unwrap_or(0);
        let unit = body[digits.len()..].chars().next().unwrap_or('d');
        let field = match unit {
            'h' => TsField::Hour,
            'm' => TsField::Month,
            'y' => TsField::Year,
            'w' => {
                n *= 7;
                TsField::Day
            }
            _ => TsField::Day,
        };
        if unit == 'h' && !has_time(ts) {
            return Err(EditError::new(&format!(
                "Cannot repeat in {n} hour(s) because no hour has been set"
            )));
        }
        let t0 =
            time::parse_time_string(ts).ok_or_else(|| EditError::new("Not an Org time string"))?;
        if dot {
            if unit == 'h' {
                let minutes = minutes_between(t0, opts.now).unwrap_or(0);
                change(buf, s + 1, minutes, TsField::Minute, false)?;
            } else {
                let days = i64::from((today - t0.date()).get_days());
                change(buf, s + 1, days, TsField::Day, false)?;
            }
        } else if plusplus {
            let mut t = t0;
            let mut shifted = false;
            while !shifted
                || if unit == 'h' {
                    opts.now >= t
                } else {
                    today >= t.date()
                }
            {
                shifted = true;
                change(buf, s + 1, n, field, false)?;
                t = crate::timestamp::shift(t, n, field)
                    .ok_or_else(|| EditError::new("Date out of range"))?;
            }
            change(buf, s + 1, -n, field, false)?;
        }
        change(buf, s + 1, n, field, true)?;
    }
    Ok(true)
}

fn has_time(ts: &str) -> bool {
    let b = ts.as_bytes();
    (1..b.len().saturating_sub(2)).any(|k| {
        b[k] == b':'
            && b[k - 1].is_ascii_digit()
            && b[k + 1].is_ascii_digit()
            && b[k + 2].is_ascii_digit()
    })
}

fn minutes_between(a: DateTime, b: DateTime) -> Option<i64> {
    let za = a.to_zoned(jiff::tz::TimeZone::UTC).ok()?;
    let zb = b.to_zoned(jiff::tz::TimeZone::UTC).ok()?;
    Some((zb.timestamp().as_second() - za.timestamp().as_second()).div_euclid(60))
}

/// `(org-at-timestamp-p 'agenda)` for the timestamp at `s`: on the
/// planning line of the headline at `h`, on a property line, or a
/// timestamp object.
fn agenda_timestamp(text: &str, root: &org_syntax::SyntaxNode, h: usize, s: usize) -> bool {
    let bol = text[..s].rfind('\n').map_or(0, |i| i + 1);
    if bol == next_line(text, h) && is_planning_line(text, bol) {
        return true;
    }
    root.descendants().any(|n| {
        let r = n.text_range();
        let inside = usize::from(r.start()) <= s && s < usize::from(r.end());
        inside
            && (n.kind() == SyntaxKind::TIMESTAMP
                || (n.kind() == SyntaxKind::NODE_PROPERTY && usize::from(r.start()) == bol))
    })
}

/// `org-remove-timestamp-with-keyword` for SCHEDULED when the entry's
/// scheduled date has no repeater.
fn remove_unrepeated_scheduled(buf: &mut Buf, h: usize) {
    let line2 = next_line(&buf.text, h);
    if !(line2 > h && buf.text.as_bytes()[line2 - 1] == b'\n' && is_planning_line(&buf.text, line2))
    {
        return;
    }
    let eol = line_end(&buf.text, line2);
    let Some((s, e)) = find_planning_ts(&buf.text, line2, eol, Some(Planning::Scheduled)) else {
        return;
    };
    if !repeat_matches(&buf.text, s, e).is_empty() {
        return;
    }
    // `\<SCHEDULED: +<[^>\n]+>[ \t]*`, last first, in the entry.
    let end = headings(&buf.text, None)
        .into_iter()
        .find(|(p, _)| *p > h)
        .map_or(buf.text.len(), |(p, _)| p);
    let mut found = Vec::new();
    for (i, _) in buf.text[h..end].match_indices("SCHEDULED:") {
        let at = h + i;
        if buf.text[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_alphanumeric)
        {
            continue;
        }
        let b = buf.text.as_bytes();
        let mut j = at + 10;
        let sp = b[j..].iter().take_while(|c| **c == b' ').count();
        if sp == 0 || b.get(j + sp) != Some(&b'<') {
            continue;
        }
        j += sp;
        let Some(k) = buf.text[j + 1..end].find(['>', '\n']) else {
            continue;
        };
        if k == 0 || b[j + 1 + k] != b'>' {
            continue;
        }
        let mut stop = j + 1 + k + 1;
        while matches!(b.get(stop), Some(b' ' | b'\t')) {
            stop += 1;
        }
        found.push((at, stop));
    }
    for (a, b) in found.into_iter().rev() {
        buf.delete(a, b);
        let bol = buf.text[..a].rfind('\n').map_or(0, |i| i + 1);
        let before = &buf.text[bol..a];
        if !before.trim().is_empty() && before.ends_with(' ') {
            buf.delete(a - 1, a);
        } else {
            let eol = line_end(&buf.text, bol);
            if buf.text[bol..eol].trim_matches([' ', '\t']).is_empty() {
                let stop = (eol + 1).min(buf.text.len());
                buf.delete(bol, stop);
            }
        }
    }
}

/// The end of the keyword (or of the stars) in `org-todo-line-regexp`.
fn keyword_line_end(text: &str, h: usize, eol: usize, kw: &Keywords<'_>) -> (usize, bool) {
    let stars = text[h..].bytes().take_while(|b| *b == b'*').count();
    let s = h + stars;
    let line = &text[s..eol];
    let spaces = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[spaces..];
    let k = kw
        .all
        .iter()
        .filter(|k| {
            spaces > 0
                && rest.strip_prefix(**k).is_some_and(|r| {
                    r.starts_with(' ') || r.trim_start_matches([' ', '\t']).is_empty()
                })
        })
        .max_by_key(|k| k.len());
    match k {
        Some(k) => (s + spaces + k.len(), true),
        None => (s, false),
    }
}

/// The keywords of planning lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Planning {
    /// `CLOSED:`
    Closed,
    /// `DEADLINE:`
    Deadline,
    /// `SCHEDULED:`
    Scheduled,
}

impl Planning {
    fn word(self) -> &'static str {
        match self {
            Planning::Closed => "CLOSED:",
            Planning::Deadline => "DEADLINE:",
            Planning::Scheduled => "SCHEDULED:",
        }
    }
}

/// A `KEYWORD: *[<[]...[]>]` timestamp (`org-closed-time-regexp` and
/// friends, `org-keyword-time-not-clock-regexp` when `which` is `None`)
/// in `from..to`, starting at a word boundary.
fn find_planning_ts(
    text: &str,
    from: usize,
    to: usize,
    which: Option<Planning>,
) -> Option<(usize, usize)> {
    let words: &[Planning] = match &which {
        Some(p) => std::slice::from_ref(p),
        None => &[Planning::Closed, Planning::Deadline, Planning::Scheduled],
    };
    let b = text.as_bytes();
    let mut best: Option<(usize, usize)> = None;
    for w in words {
        let word = w.word();
        for (i, _) in text[from..to].match_indices(word) {
            let at = from + i;
            if text[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
            {
                continue;
            }
            let mut j = at + word.len();
            while b.get(j) == Some(&b' ') {
                j += 1;
            }
            // CLOSED takes `[...]`, DEADLINE and SCHEDULED `<...>`; the
            // generic form takes either.
            let (open, close): (&[u8], &[u8]) = match which {
                Some(Planning::Closed) => (b"[", b"]"),
                Some(_) => (b"<", b">"),
                None => (b"[<", b"]>"),
            };
            if j >= to || !open.contains(&b[j]) {
                continue;
            }
            let stop: &[char] = if which.is_none() {
                &[']', '>']
            } else if close == b"]" {
                &[']']
            } else {
                &['>']
            };
            let Some(k) = text[j + 1..to].find(stop) else {
                continue;
            };
            if k == 0 || !close.contains(&b[j + 1 + k]) {
                continue;
            }
            let m = (at, j + 1 + k + 1);
            if best.is_none_or(|x| m.0 < x.0) {
                best = Some(m);
            }
            break;
        }
    }
    best
}

/// `org-add-planning-info`: removes the `remove` keywords (and the one
/// added) from the planning line of the headline at `h`, and adds `what`.
pub(crate) fn add_planning_info(
    buf: &mut Buf,
    h: usize,
    what: Option<(Planning, DateTime)>,
    remove: &[Planning],
    settings: &TodoSettings,
) {
    let line2 = next_line(&buf.text, h);
    let planning = line2 < buf.text.len()
        && line2 > h
        && buf.text.as_bytes()[line2 - 1] == b'\n'
        && is_planning_line(&buf.text, line2);
    let p;
    if planning {
        p = line2 + buf.text[line2..].len()
            - buf.text[line2..].trim_start_matches([' ', '\t']).len();
        let types: Vec<Planning> = what
            .iter()
            .map(|w| w.0)
            .chain(remove.iter().copied())
            .collect();
        for t in types {
            let eol = line_end(&buf.text, p);
            if let Some((s, e)) = find_planning_ts(&buf.text, p, eol, Some(t)) {
                let stop = find_planning_ts(&buf.text, e, eol, None).map_or(eol, |m| m.0);
                buf.delete(s, stop);
            }
        }
        let eol = line_end(&buf.text, p);
        if buf.text[p..eol].trim_matches([' ', '\t']).is_empty() && what.is_none() {
            // Remove the line and the line feed before it.
            let prev_eol = p - (p - bol_of(&buf.text, p)) - 1;
            buf.delete(prev_eol, eol);
            return;
        }
        let trimmed = buf.text[p..eol].trim_end_matches([' ', '\t']).len();
        if p + trimmed < eol {
            buf.delete(p + trimmed, eol);
        }
    } else if what.is_some() {
        let eol = line_end(&buf.text, h);
        let level = buf.text[h..].bytes().take_while(|b| *b == b'*').count();
        let indent = if settings.adapt_indentation {
            indentation(level + 1)
        } else {
            String::new()
        };
        buf.insert_before_point(eol, &format!("\n{indent}"));
        p = eol + 1 + indent.len();
    } else {
        return;
    }
    if let Some((kind, t)) = what {
        let ts = if kind == Planning::Closed {
            format!("[{}]", time::format(t, settings.log_done_with_time))
        } else {
            format!("<{}>", time::format(t, false))
        };
        let mut s = format!("{} {ts}", kind.word());
        if p != line_end(&buf.text, p) {
            s.push(' ');
        }
        buf.insert_before_point(p, &s);
    }
}

/// `org-update-parent-todo-statistics` for the headline at `h` with
/// `ltoggle` stars.
fn update_parent_todo_statistics(buf: &mut Buf, doc: &Document, h: usize, ltoggle: usize) {
    let ctx = doc.parse().context();
    let outline = doc.outline();
    // Parents are before `h`, where the text has not changed.
    let Some(current) = outline
        .entry_at(h)
        .filter(|e| usize::from(outline.get(*e).range.start()) == h)
    else {
        return;
    };
    let parent_of = |id: org_model::EntryId| outline.get(id).parent;
    let Some(first_parent) = parent_of(current) else {
        return;
    };
    let (prop, lim) = doc.entry_get_with_source(Some(first_parent), "COOKIE_DATA");
    let recursive = !doc.settings().hierarchical_todo_statistics
        || prop.as_deref().is_some_and(|p| has_word(p, "recursive"));
    let lim = if prop.is_some() { lim.unwrap_or(0) } else { 0 };
    let mut parent = Some(first_parent);
    let mut first = true;
    while let Some(pid) = parent {
        let pstart = usize::from(outline.get(pid).range.start());
        if !(recursive || first) || pstart < lim {
            break;
        }
        first = false;
        let data = doc
            .entry_get(Some(pid), "COOKIE_DATA", Inherit::No, false)
            .unwrap_or_default();
        if has_word(&data.to_lowercase(), "checkbox") {
            break;
        }
        let level = buf.text[pstart..]
            .bytes()
            .take_while(|b| *b == b'*')
            .count();
        // The cookies of the parent's heading line, from the parse.
        let peol = line_end(&text_of(doc), pstart);
        let cookies: Vec<(usize, usize)> = doc
            .parse()
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::STATISTICS_COOKIE)
            .map(|n| {
                let r = n.text_range();
                let t = n.text().to_string();
                let end = usize::from(r.start()) + t.trim_end_matches([' ', '\t']).len();
                (usize::from(r.start()), end)
            })
            .filter(|(s, _)| *s >= pstart && *s < peol)
            .collect();
        for (cs, ce) in cookies.into_iter().rev() {
            let (mut all, mut done) = (0, 0);
            let hs = headings(&buf.text, None);
            for &(b, l) in hs.iter().filter(|(b, _)| *b > pstart) {
                if l <= level {
                    break;
                }
                let eol = line_end(&buf.text, b);
                let kwd = (recursive || l == ltoggle)
                    .then(|| org_model::complex_heading_todo(&buf.text[b..eol], ctx))
                    .flatten();
                if let Some(k) = kwd {
                    all += 1;
                    if ctx.done_keywords.contains(&k) {
                        done += 1;
                    }
                }
            }
            let old = &buf.text[cs..ce];
            let new = if old.contains('%') {
                format!("[{}%]", (100 * done) / all.max(1))
            } else {
                format!("[{done}/{all}]")
            };
            if new != old {
                buf.insert_before_point(cs, &new);
                buf.delete(cs + new.len(), ce + new.len());
                align_tags(buf, pstart);
            }
        }
        parent = parent_of(pid);
    }
}

fn text_of(doc: &Document) -> String {
    doc.parse().syntax().to_string()
}

/// `\<word\>` in `s`.
fn has_word(s: &str, word: &str) -> bool {
    s.match_indices(word).any(|(i, _)| {
        let before = s[..i].chars().next_back();
        let after = s[i + word.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

/// `org-end-of-meta-data`: after the planning line and the property
/// drawer of the headline at `h`.
fn end_of_meta_data(text: &str, h: usize) -> usize {
    let mut p = next_line(text, h);
    if p > h && p <= text.len() && text.as_bytes()[p - 1] == b'\n' && is_planning_line(text, p) {
        p = next_line(text, p);
    }
    if let Some(end) = property_drawer_at(text, p) {
        p = next_line(text, end);
    }
    p
}

/// `org-log-beginning` with CREATE: where a new log entry of the headline
/// at `h` goes, creating the drawer if needed.
fn log_beginning(
    buf: &mut Buf,
    h: usize,
    drawer: Option<&str>,
    settings: &TodoSettings,
    ctx: &ParseContext,
) -> usize {
    let text = buf.text.clone();
    let meta = end_of_meta_data(&text, h);
    let finish = |t: &str, p: usize| {
        if p == 0 || t.as_bytes()[p - 1] == b'\n' {
            p
        } else {
            next_line(t, p)
        }
    };
    match drawer {
        Some(name) => {
            let at_heading = |p: usize| crate::headline::stars_at(&text, p).is_some();
            let end = if meta < text.len() && at_heading(meta) {
                meta
            } else {
                headings(&text, None)
                    .into_iter()
                    .find(|(s, _)| *s > meta)
                    .map_or(text.len(), |(s, _)| s)
            };
            // An existing drawer of that name.
            let parse = org_syntax::parse_with(&text, ctx);
            let mut l = meta;
            while l < end {
                let eol = line_end(&text, l);
                let line = text[l..eol].trim_matches([' ', '\t']);
                if line.len() == name.len() + 2
                    && line.starts_with(':')
                    && line.ends_with(':')
                    && line[1..line.len() - 1].eq_ignore_ascii_case(name)
                    && eol <= end
                {
                    let drawer_node = parse.syntax().descendants().find(|n| {
                        n.kind() == SyntaxKind::DRAWER && usize::from(n.text_range().start()) == l
                    });
                    if let Some(d) = drawer_node {
                        let mut p = eol;
                        if !settings.log_states_order_reversed
                            && let Some(r) = org_syntax::ast::contents_range(&d)
                        {
                            p = usize::from(r.end());
                        }
                        return finish(&text, p);
                    }
                }
                if eol >= text.len() {
                    break;
                }
                l = eol + 1;
            }
            // Create the drawer.
            let mut p = meta;
            if p < text.len() && at_heading(p) {
                p -= 1;
            }
            let t = &buf.text;
            let at_bol = p == 0 || t.as_bytes()[p - 1] == b'\n';
            let at_blank = at_bol && t[p..line_end(t, p)].trim_matches([' ', '\t']).is_empty();
            let at_nonblank_bol = at_bol && p != line_end(t, p);
            let mut ins = String::new();
            if !at_bol {
                ins.push('\n');
            }
            let indent = indentation(crate::property::sibling_indent(
                &text,
                Some(h),
                p,
                settings.adapt_indentation,
                ctx.inlinetask_min_level,
            ));
            ins.push_str(&format!("{indent}:{name}:\n{indent}:END:"));
            if at_blank || at_nonblank_bol {
                ins.push('\n');
            }
            buf.insert_before_point(p, &ins);
            // The line after `:NAME:`.
            let start = p + if at_bol { 0 } else { 1 };
            next_line(&buf.text, start)
        }
        None => {
            let endpos = meta;
            let t = &buf.text;
            let mut p = endpos + t[endpos..].len()
                - t[endpos..].trim_start_matches([' ', '\t', '\n']).len();
            p = bol_of(t, p);
            if !settings.log_states_order_reversed {
                p = skip_over_state_notes(t, p, ctx);
                p = t[..p].trim_end_matches([' ', '\t', '\n']).len();
                p = next_line(t, p);
            }
            if p < endpos {
                p = endpos;
            }
            finish(t, p)
        }
    }
}

/// `org-skip-over-state-notes` at `p`: past the items of the list at `p`
/// that are state notes.
fn skip_over_state_notes(text: &str, p: usize, ctx: &ParseContext) -> usize {
    let parse = org_syntax::parse_with(text, ctx);
    let Some(list) = parse
        .syntax()
        .descendants()
        .find(|n| n.kind() == SyntaxKind::PLAIN_LIST && usize::from(n.text_range().start()) == p)
    else {
        return p;
    };
    let mut pos = p;
    for item in list.children().filter(|c| c.kind() == SyntaxKind::ITEM) {
        let s = item.text().to_string();
        let line = s.lines().next().unwrap_or("").trim_start();
        let is_state = line
            .strip_prefix('-')
            .is_some_and(|r| r.trim_start().starts_with("State "));
        if !is_state {
            return usize::from(item.text_range().start());
        }
        pos = usize::from(item.text_range().end());
    }
    pos
}

/// The heading of a log entry (`org-log-note-headings`).
fn note_heading(note: &PendingNote) -> String {
    let t = format!("[{}]", time::format(note.time, true));
    let quote = |s: &Option<String>| s.as_ref().map_or(String::new(), |s| format!("\"{s}\""));
    match note.purpose {
        NotePurpose::Done => format!("CLOSING NOTE {t}"),
        NotePurpose::State => {
            format!(
                "State {:<12} from {:<12} {t}",
                quote(&note.state),
                quote(&note.previous_state)
            )
        }
    }
}

/// `org-store-log-note`: writes the log entry `note` with the user's
/// `content` into `text`.
pub fn store_log_note(
    doc: &Document,
    point: usize,
    note: &PendingNote,
    content: &str,
    settings: &TodoSettings,
) -> Transaction {
    let text = doc.parse().syntax().to_string();
    let mut buf = Buf::new(&text, point);
    let drawer = log_drawer(doc, note.heading, settings);
    store_note(
        &mut buf,
        note,
        Some(content),
        drawer.as_deref(),
        settings,
        doc.parse().context(),
    );
    buf.transaction("Store note")
}

fn store_note(
    buf: &mut Buf,
    note: &PendingNote,
    content: Option<&str>,
    drawer: Option<&str>,
    settings: &TodoSettings,
    ctx: &ParseContext,
) {
    let txt = content.unwrap_or("").trim_end();
    let mut lines: Vec<String> = if txt.is_empty() {
        Vec::new()
    } else {
        txt.split('\n').map(str::to_string).collect()
    };
    let mut heading = note_heading(note);
    if !lines.is_empty() {
        heading.push_str(" \\\\");
    }
    lines.insert(0, heading);
    let mut p = log_beginning(buf, note.heading, drawer, settings, ctx);
    let t = &buf.text;
    if !(p == 0 || t.as_bytes()[p - 1] == b'\n') {
        buf.insert_before_point(p, "\n");
        p += 1;
    } else if !t[p..line_end(t, p)].trim_matches([' ', '\t']).is_empty() {
        buf.insert_before_point(p, "\n");
    }
    // In a list, the item goes at the list's indentation; otherwise at the
    // indentation of the text.
    let indent = match list_indentation(&buf.text, p, ctx) {
        Some(i) => i,
        None => crate::property::sibling_indent(
            &buf.text,
            Some(note.heading),
            p,
            settings.adapt_indentation,
            ctx.inlinetask_min_level,
        ),
    };
    let eol = line_end(&buf.text, p);
    let mut s = indentation(indent);
    s.push_str("- ");
    s.push_str(&lines[0]);
    let body = indent + 2;
    for l in &lines[1..] {
        s.push('\n');
        if !l.is_empty() {
            s.push_str(&indentation(body));
            s.push_str(l);
        }
    }
    buf.replace(p, eol, &s);
}

/// `org-in-item-p` on the empty line at `p`: the indentation of the top
/// list when the line before belongs to a list item.
fn list_indentation(text: &str, p: usize, ctx: &ParseContext) -> Option<usize> {
    let before = text[..p].trim_end_matches([' ', '\t', '\n']).len();
    if before == 0 {
        return None;
    }
    let parse = org_syntax::parse_with(text, ctx);
    let root = parse.syntax();
    let item = root
        .descendants()
        .filter(|n| n.kind() == SyntaxKind::ITEM)
        .filter(|n| {
            usize::from(n.text_range().start()) < before
                && usize::from(n.text_range().end()) >= before
        })
        .last()?;
    let top = item
        .ancestors()
        .filter(|a| a.kind() == SyntaxKind::PLAIN_LIST)
        .last()?;
    let s = usize::from(top.text_range().start());
    Some(text[s..].len() - text[s..].trim_start_matches([' ', '\t']).len())
}

/// The action of `org-priority`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityAction {
    /// Set this priority (a letter, or a number with numeric priorities);
    /// a space removes it.
    Set(char),
    /// One higher.
    Up,
    /// One lower.
    Down,
    /// Remove the cookie.
    Remove,
}

/// `org-priority` on the headline at `point`. `repeated` tells whether
/// the previous command was the same one (`last-command`).
pub fn priority(
    doc: &Document,
    point: usize,
    action: PriorityAction,
    repeated: bool,
    settings: &TodoSettings,
) -> Result<Transaction, EditError> {
    let text = doc.parse().syntax().to_string();
    let ctx = doc.parse().context();
    let pr = &doc.info().priorities;
    let (highest, lowest, default) = (pr.highest as i64, pr.lowest as i64, pr.default as i64);
    let nump = lowest < 65;
    let Some(h) = org_back_to_heading(&text, point, ctx) else {
        return Err(EditError::new(&format!(
            "Before first headline at position {}",
            point + 1
        )));
    };
    let eol = line_end(&text, h);
    // `org-priority-regexp`, case-insensitively: the first `[#X] ?`.
    let line = &text[h..eol];
    let cookie = line.match_indices("[#").find_map(|(i, _)| {
        let n = line[i + 2..]
            .bytes()
            .take_while(|c| c.is_ascii_alphanumeric())
            .count();
        (n > 0 && line.as_bytes().get(i + 2 + n) == Some(&b']')).then(|| {
            let space = usize::from(line.as_bytes().get(i + 3 + n) == Some(&b' '));
            (h + i, h + i + 2, h + i + 2 + n, h + i + 3 + n + space)
        })
    });
    let value_of = |s: &str| -> i64 {
        if s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().unwrap_or(0)
        } else {
            s.chars().next().map_or(0, |c| c as i64)
        }
    };
    let current = cookie.map(|(_, vs, ve, _)| value_of(&text[vs..ve]));
    let upcase = |c: i64| {
        char::from_u32(c as u32).map_or(c, |ch| ch.to_uppercase().next().map_or(c, |u| u as i64))
    };
    let mut remove = false;
    let mut new: i64;
    match action {
        PriorityAction::Remove => {
            remove = true;
            new = ' ' as i64;
        }
        PriorityAction::Set(c) => {
            new = if nump {
                c.to_digit(10).map_or(c as i64, i64::from)
            } else {
                c as i64
            };
            if c == ' ' {
                new = ' ' as i64;
            }
            if upcase(highest) == highest && upcase(lowest) == lowest {
                new = upcase(new);
            }
            if new == ' ' as i64 {
                remove = true;
            } else if upcase(new) < highest || upcase(new) > lowest {
                let msg = if nump {
                    format!("Priority must be between `{highest}' and `{lowest}'")
                } else {
                    format!(
                        "Priority must be between `{}' and `{}'",
                        char::from_u32(highest as u32).unwrap_or('?'),
                        char::from_u32(lowest as u32).unwrap_or('?')
                    )
                };
                return Err(EditError::new(&msg));
            }
        }
        PriorityAction::Up | PriorityAction::Down => {
            let up = action == PriorityAction::Up;
            new = match current {
                Some(c) => {
                    if up {
                        c - 1
                    } else {
                        c + 1
                    }
                }
                None if repeated => {
                    if up {
                        lowest
                    } else {
                        highest
                    }
                }
                None if settings.priority_start_cycle_with_default => default,
                None => {
                    if up {
                        default - 1
                    } else {
                        default + 1
                    }
                }
            };
        }
    }
    if upcase(new) < highest || upcase(new) > lowest {
        if matches!(action, PriorityAction::Up | PriorityAction::Down)
            && current.is_none()
            && !repeated
        {
            return Err(EditError::new(
                "The default can not be set, see `org-priority-default' why",
            ));
        }
        remove = true;
    }
    let news = if new > 64 {
        char::from_u32(new as u32).map_or(String::new(), String::from)
    } else {
        new.to_string()
    };
    let mut buf = Buf::new(&text, point);
    match cookie {
        Some((s, vs, ve, e)) => {
            if remove {
                buf.replace(s, e, "");
            } else {
                buf.replace(vs, ve, &news);
            }
        }
        None if remove => return Err(EditError::new("No priority cookie found in line")),
        None => {
            let kw = Keywords::new(ctx);
            let (end, has_kw) = keyword_line_end(&text, h, eol, &kw);
            if has_kw {
                buf.insert_before_point(end, &format!(" [#{news}]"));
            } else {
                // Before the title.
                let t = &text[end..eol];
                let spaces = t.len() - t.trim_start_matches(' ').len();
                if spaces == 0 || t.trim_matches([' ', '\t']).is_empty() {
                    return Err(EditError::new(
                        "Wrong type argument: integer-or-marker-p, nil",
                    ));
                }
                buf.insert_before_point(end + spaces, &format!("[#{news}] "));
            }
        }
    }
    align_tags(&mut buf, h);
    Ok(buf.transaction("Priority"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_specs() {
        assert_eq!(
            log_spec("w@/!"),
            Some((Some(LogKind::Note), Some(LogKind::Time)))
        );
        assert_eq!(log_spec("!"), Some((Some(LogKind::Time), None)));
        assert_eq!(log_spec("t"), None);
        assert_eq!(log_spec("/@"), Some((None, Some(LogKind::Note))));
        assert_eq!(log_spec("xy"), None);
    }

    #[test]
    fn property_drawers() {
        let t = "* H\n:PROPERTIES:\n:A: 1\n:END:\nx\n";
        assert_eq!(property_drawer_at(t, 4), Some(28));
        assert_eq!(end_of_meta_data(t, 0), 29);
        assert_eq!(
            property_drawer_at("* H\n:PROPERTIES:\nbad\n:END:\n", 4),
            None
        );
    }
}
