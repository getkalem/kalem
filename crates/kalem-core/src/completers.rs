//! Completers (§11.12, T2.7a.8): one contract for everything that offers
//! completions at the cursor. A completer says where it applies (a
//! when-clause over the context), what opens its menu (trigger strings, a
//! word prefix, or a request), and gives items; the core runs the ones
//! that apply, merges and ranks their items, and shows one menu in both
//! frontends. Slow completers (a language server, later a model) run on
//! another thread with a copy of the text near the cursor and a budget;
//! their items arrive when they come, and a newer keystroke cancels them.
//!
//! The built-ins: Org's completions (`#+` keywords and blocks, `[[` link
//! targets, `[fn:` labels, tags; [`crate::input`]) and the words of the
//! document, for every text file.

use std::collections::VecDeque;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use org_edit::{Selection, Transaction};

use crate::DocumentState;
use crate::mode::DocumentMode;

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A word of the text.
    Word,
    /// A keyword or block (`#+title:`).
    Keyword,
    /// A link target.
    Link,
    /// A tag.
    Tag,
    /// A footnote label.
    Footnote,
    /// A symbol of a program.
    Symbol,
    /// A snippet with places to fill.
    Snippet,
}

impl Kind {
    /// The kind's name, for menus and `kalem complete`.
    pub fn name(self) -> &'static str {
        match self {
            Kind::Word => "word",
            Kind::Keyword => "keyword",
            Kind::Link => "link",
            Kind::Tag => "tag",
            Kind::Footnote => "footnote",
            Kind::Symbol => "symbol",
            Kind::Snippet => "snippet",
        }
    }
}

/// A completion to choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// What the menu shows.
    pub label: String,
    /// What replaces `range`.
    pub insert: String,
    /// The text the choice replaces (it ends at the cursor).
    pub range: Range<usize>,
    /// Where the cursor goes in `insert`.
    pub cursor: usize,
    /// What it is.
    pub kind: Kind,
    /// A line more about it (where a word comes from, a target's kind).
    pub detail: String,
    /// The completer's id.
    pub source: &'static str,
    /// An Org completion, applied as `crate::input::apply_completion`
    /// does (tags are aligned afterwards).
    org: Option<(crate::input::Completion, crate::input::CompletionItem)>,
}

impl Item {
    /// An item replacing `range` with `insert`, the cursor after it.
    pub fn new(
        label: impl Into<String>,
        insert: impl Into<String>,
        range: Range<usize>,
        kind: Kind,
    ) -> Item {
        let insert = insert.into();
        Item {
            label: label.into(),
            cursor: insert.len(),
            insert,
            range,
            kind,
            detail: String::new(),
            source: "",
            org: None,
        }
    }
}

/// What a completer sees.
#[derive(Debug, Clone)]
pub struct Context {
    /// The text near the cursor (all of it for small documents), and where
    /// it starts in the document.
    pub text: Arc<str>,
    /// The offset of `text` in the document.
    pub base: usize,
    /// The cursor, in the document.
    pub point: usize,
    /// The document's mode.
    pub mode: DocumentMode,
    /// The language at the cursor: a file's, a source block's, `org`.
    pub language: Option<String>,
    /// The document's file.
    pub path: Option<PathBuf>,
    /// The user asked (Ctrl+Space), rather than typing a trigger.
    pub requested: bool,
}

impl Context {
    /// The text of the document from `a` to `b` (within the copy).
    /// Ends inside a character move back to its start.
    pub fn slice(&self, r: Range<usize>) -> &str {
        let t = &*self.text;
        let back = |mut i: usize| {
            while !t.is_char_boundary(i) {
                i -= 1;
            }
            i
        };
        let a = back(r.start.saturating_sub(self.base).min(t.len()));
        let b = back(r.end.saturating_sub(self.base).min(t.len()));
        &t[a..b.max(a)]
    }

    /// The text before the cursor on its line.
    pub fn line_before(&self) -> &str {
        let before = self.slice(self.base..self.point);
        &before[before.rfind('\n').map_or(0, |i| i + 1)..]
    }

    /// The text before the cursor in its paragraph (since the last blank
    /// line).
    pub fn paragraph_before(&self) -> &str {
        let before = self.slice(self.base..self.point);
        &before[before.rfind("\n\n").map_or(0, |i| i + 2)..]
    }

    /// The word being typed before the cursor (letters, digits, `_` and
    /// `-` inside), and where it starts.
    pub fn word_prefix(&self) -> (usize, &str) {
        let line = self.line_before();
        let start = line
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_alphanumeric() || *c == '_')
            .last()
            .map_or(line.len(), |(i, _)| i);
        (self.point - (line.len() - start), &line[start..])
    }
}

/// What opens a completer's menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// Any of these strings before the cursor on the line.
    Strings(&'static [&'static str]),
    /// A word of at least this many letters being typed.
    WordPrefix(usize),
    /// Only a request (Ctrl+Space).
    Request,
}

/// Cancels a completer's work: a newer keystroke asks for other items.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    /// Whether the work should stop.
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// A source of completions.
pub trait Completer: Send + Sync {
    /// Its id (`org`, `words`).
    fn id(&self) -> &'static str;
    /// Its place among the others when items rank the same (higher first).
    fn priority(&self) -> i32 {
        0
    }
    /// Where it applies (its when-clause, over the context).
    fn applies(&self, ctx: &Context) -> bool;
    /// What opens its menu.
    fn trigger(&self) -> Trigger;
    /// Whether it takes long (a server, a model): run on another thread,
    /// within the budget.
    fn slow(&self) -> bool {
        false
    }
    /// Its items for `ctx`; long work checks `cancel`.
    fn complete(&self, ctx: &Context, doc: Option<&DocumentState>, cancel: &Cancel) -> Vec<Item>;
    /// Documentation for an item, fetched when it is chosen in the menu.
    fn resolve(&self, _item: &Item) -> Option<String> {
        None
    }
}

/// Whether the trigger of a completer fires in `ctx`.
fn fires(t: Trigger, ctx: &Context) -> bool {
    match t {
        Trigger::Strings(s) => ctx.requested || s.iter().any(|s| ctx.line_before().contains(s)),
        Trigger::WordPrefix(n) => {
            let (_, w) = ctx.word_prefix();
            w.chars().count() >= if ctx.requested { 1 } else { n }
        }
        Trigger::Request => ctx.requested,
    }
}

/// The completers there are.
#[derive(Clone)]
pub struct Registry {
    completers: Vec<Arc<dyn Completer>>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.completers.iter().map(|c| c.id()))
            .finish()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Registry::with_builtins()
    }
}

/// Items accepted lately, which rank first among equals.
static RECENT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Remembers that `item` was chosen.
pub fn accepted(item: &Item) {
    if let Ok(mut r) = RECENT.lock() {
        r.retain(|x| *x != item.insert);
        r.push_front(item.insert.clone());
        r.truncate(50);
    }
}

impl Registry {
    /// The built-in completers: Org's, and the document's words.
    pub fn with_builtins() -> Registry {
        Registry {
            completers: vec![
                Arc::new(OrgCompleter),
                Arc::new(crate::latex_complete::LatexCompleter),
                Arc::new(WordsCompleter),
            ],
        }
    }

    /// Adds a completer (a plugin's).
    pub fn register(&mut self, c: Arc<dyn Completer>) {
        self.completers.push(c);
    }

    /// The completers that apply in `ctx` and whose trigger fires.
    fn active(&self, ctx: &Context) -> Vec<Arc<dyn Completer>> {
        self.completers
            .iter()
            .filter(|c| c.applies(ctx) && fires(c.trigger(), ctx))
            .cloned()
            .collect()
    }

    /// Every item for the cursor of `doc`, the slow completers waited for
    /// within `budget` (for `kalem complete` and tests).
    pub fn complete(
        &self,
        doc: &mut DocumentState,
        requested: bool,
        budget: Duration,
    ) -> Vec<Item> {
        let mut s = self.start(doc, requested);
        let end = Instant::now() + budget;
        while s.pending > 0 && Instant::now() < end {
            if !s.poll() {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        std::mem::take(&mut s.items)
    }

    /// Starts completing at the cursor of `doc`: the fast completers'
    /// items at once, the slow ones' through [`Session::poll`].
    pub fn start(&self, doc: &mut DocumentState, requested: bool) -> Session {
        let ctx = context(doc, requested);
        let cancel = Cancel::default();
        let mut session = Session {
            items: Vec::new(),
            prefix: ctx.word_prefix().1.to_string(),
            results: None,
            pending: 0,
            cancel: cancel.clone(),
        };
        let active = self.active(&ctx);
        if active.is_empty() {
            return session;
        }
        // The model, for completers of Org documents.
        let _ = doc.model();
        let mut fast = Vec::new();
        let (send, recv) = mpsc::channel();
        for c in active {
            if c.slow() {
                session.pending += 1;
                let (ctx, cancel, send, c) = (ctx.clone(), cancel.clone(), send.clone(), c.clone());
                std::thread::spawn(move || {
                    let items = c.complete(&ctx, None, &cancel);
                    if !cancel.cancelled() {
                        let _ = send.send(tag(items, c.id(), c.priority()));
                    }
                });
            } else {
                let items = c.complete(&ctx, Some(doc), &cancel);
                fast.extend(tag(items, c.id(), c.priority()));
            }
        }
        session.results = (session.pending > 0).then_some(recv);
        session.add(fast);
        session
    }
}

fn tag(items: Vec<Item>, id: &'static str, priority: i32) -> Vec<(Item, i32)> {
    items
        .into_iter()
        .map(|mut i| {
            i.source = id;
            (i, priority)
        })
        .collect()
}

/// The context for the cursor of `doc`: the whole text of documents up to
/// a megabyte, else the megabyte around the cursor.
pub fn context(doc: &DocumentState, requested: bool) -> Context {
    const WINDOW: usize = 1 << 20;
    let text = doc.text().as_str();
    let point = doc.selection.head.min(text.len());
    let (mut a, mut b) = if text.len() <= WINDOW {
        (0, text.len())
    } else {
        (
            point.saturating_sub(WINDOW / 2),
            (point + WINDOW / 2).min(text.len()),
        )
    };
    while !text.is_char_boundary(a) {
        a -= 1;
    }
    while !text.is_char_boundary(b) {
        b += 1;
    }
    Context {
        text: Arc::from(&text[a..b]),
        base: a,
        point,
        mode: doc.meta.mode.clone(),
        language: crate::code::language_at(doc),
        path: doc.meta.path.clone(),
        requested,
    }
}

/// A completion in progress: the items so far, merged and ranked.
#[derive(Debug)]
pub struct Session {
    /// The items, best first.
    pub items: Vec<Item>,
    prefix: String,
    results: Option<mpsc::Receiver<Vec<(Item, i32)>>>,
    pending: usize,
    cancel: Cancel,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl Session {
    /// Takes in the items slow completers have sent; `true` if the menu
    /// changed.
    pub fn poll(&mut self) -> bool {
        let Some(r) = &self.results else {
            return false;
        };
        let mut got = Vec::new();
        while let Ok(items) = r.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            got.extend(items);
        }
        if self.pending == 0 {
            self.results = None;
        }
        let changed = !got.is_empty();
        self.add(got);
        changed
    }

    /// Whether slow completers are still working.
    pub fn waiting(&self) -> bool {
        self.pending > 0
    }

    fn add(&mut self, new: Vec<(Item, i32)>) {
        let recent: Vec<String> = RECENT
            .lock()
            .map(|r| r.iter().cloned().collect())
            .unwrap_or_default();
        let mut all: Vec<(Item, i32)> = self.items.drain(..).map(|i| (i, 0)).collect();
        all.extend(new);
        // Duplicates: the same insertion once.
        let mut seen = std::collections::HashSet::new();
        all.retain(|(i, _)| seen.insert((i.insert.clone(), i.range.clone())));
        let prefix = self.prefix.clone();
        let rank = |(i, p): &(Item, i32)| {
            let exact = !prefix.is_empty() && i.label.starts_with(prefix.as_str());
            let recent = recent
                .iter()
                .position(|r| *r == i.insert)
                .unwrap_or(usize::MAX);
            (std::cmp::Reverse(exact), recent, std::cmp::Reverse(*p))
        };
        all.sort_by_key(rank);
        self.items = all.into_iter().map(|(i, _)| i).collect();
    }
}

/// Applies `item` to `doc` as one undo step.
pub fn apply(doc: &mut DocumentState, item: &Item, now: Instant) {
    accepted(item);
    if let Some((c, it)) = &item.org
        && let Some(m) = doc.model()
    {
        let head = doc.selection.head;
        let tx = crate::input::apply_completion(&m, head, c, it);
        doc.apply(&tx, org_edit::ChangeKind::Command, now);
        return;
    }
    let mut tx = Transaction::new("Complete");
    if tx.replace(item.range.clone(), item.insert.clone()).is_err() {
        return;
    }
    let tx = tx.select(Selection::caret(item.range.start + item.cursor));
    doc.apply(&tx, org_edit::ChangeKind::Command, now);
}

/// Org's completions: `#+` keywords and blocks, `[[` link targets, `[fn:`
/// labels and tags.
struct OrgCompleter;

impl Completer for OrgCompleter {
    fn id(&self) -> &'static str {
        "org"
    }

    fn priority(&self) -> i32 {
        10
    }

    fn applies(&self, ctx: &Context) -> bool {
        ctx.mode == DocumentMode::Org
    }

    fn trigger(&self) -> Trigger {
        Trigger::Strings(&["#+", "[[", "[fn:", ":"])
    }

    fn complete(&self, ctx: &Context, doc: Option<&DocumentState>, _cancel: &Cancel) -> Vec<Item> {
        if !crate::input::completion_trigger(ctx.line_before()) {
            return Vec::new();
        }
        let Some(model) = doc.and_then(DocumentState::cached_model) else {
            return Vec::new();
        };
        let Some(c) = crate::input::completion(&model, ctx.point) else {
            return Vec::new();
        };
        let kind = match c.kind {
            crate::input::CompletionKind::Keyword => Kind::Keyword,
            crate::input::CompletionKind::Link => Kind::Link,
            crate::input::CompletionKind::Footnote => Kind::Footnote,
            crate::input::CompletionKind::Tag => Kind::Tag,
        };
        c.items
            .iter()
            .map(|it| Item {
                label: it.label.clone(),
                insert: it.insert.clone(),
                range: c.start..ctx.point,
                cursor: it.cursor,
                kind,
                detail: String::new(),
                source: "org",
                org: Some((c.clone(), it.clone())),
            })
            .collect()
    }
}

/// The words of the document starting with the word being typed, the
/// nearest to the cursor first (dabbrev).
struct WordsCompleter;

impl Completer for WordsCompleter {
    fn id(&self) -> &'static str {
        "words"
    }

    fn applies(&self, ctx: &Context) -> bool {
        ctx.mode != DocumentMode::Directory
            && !(ctx.mode == DocumentMode::Org
                && crate::input::completion_trigger(ctx.line_before()))
    }

    fn trigger(&self) -> Trigger {
        Trigger::WordPrefix(3)
    }

    fn complete(&self, ctx: &Context, _doc: Option<&DocumentState>, cancel: &Cancel) -> Vec<Item> {
        let (start, prefix) = ctx.word_prefix();
        if prefix.is_empty() && !ctx.requested {
            return Vec::new();
        }
        let lower = prefix.to_lowercase();
        let text = &*ctx.text;
        let at = ctx.point - ctx.base;
        // Words with their distance from the cursor.
        let mut found: std::collections::HashMap<&str, usize> = Default::default();
        let mut i = 0;
        let bytes = text.as_bytes();
        while i < bytes.len() {
            if i % 65536 == 0 && cancel.cancelled() {
                return Vec::new();
            }
            let c = text[i..].chars().next().expect("a char");
            if c.is_alphanumeric() || c == '_' {
                let s = i;
                let mut e = i;
                for (k, ch) in text[i..].char_indices() {
                    if ch.is_alphanumeric() || ch == '_' {
                        e = i + k + ch.len_utf8();
                    } else {
                        break;
                    }
                }
                let w = &text[s..e];
                // Not the word being typed itself.
                let here = ctx.base + e == ctx.point && ctx.base + s == start;
                if !here
                    && w.len() > prefix.len()
                    && w.chars().count() >= 3
                    && w.to_lowercase().starts_with(&lower)
                {
                    let d = s.abs_diff(at);
                    let entry = found.entry(w).or_insert(d);
                    *entry = (*entry).min(d);
                }
                i = e;
            } else {
                i += c.len_utf8();
            }
        }
        let mut words: Vec<(&str, usize)> = found.into_iter().collect();
        words.sort_by_key(|(w, d)| (*d, *w));
        words.truncate(50);
        words
            .into_iter()
            .map(|(w, _)| {
                let mut item = Item::new(w, w, start..ctx.point, Kind::Word);
                item.detail = "document".into();
                item
            })
            .collect()
    }
}

/// The completion menu of a frontend: the items and the chosen one.
#[derive(Debug)]
pub struct Menu {
    /// The completion.
    pub session: Session,
    /// The chosen item.
    pub chosen: usize,
    /// Opened on request (Alt+/): it stays open for shorter prefixes.
    pub requested: bool,
}

impl Menu {
    /// A menu for the cursor of `doc`, if any completer has items there;
    /// `keep` is the item chosen in the menu before.
    pub fn open(
        registry: &Registry,
        doc: &mut DocumentState,
        requested: bool,
        keep: usize,
    ) -> Option<Menu> {
        let session = registry.start(doc, requested);
        if session.items.is_empty() && !session.waiting() {
            return None;
        }
        let chosen = keep.min(session.items.len().saturating_sub(1));
        Some(Menu {
            session,
            chosen,
            requested,
        })
    }

    /// The items.
    pub fn items(&self) -> &[Item] {
        &self.session.items
    }

    /// One item down (or up), round.
    pub fn step(&mut self, down: bool) {
        let n = self.session.items.len();
        if n == 0 {
            return;
        }
        self.chosen = if down {
            (self.chosen + 1) % n
        } else {
            (self.chosen + n - 1) % n
        };
    }

    /// The chosen item.
    pub fn current(&self) -> Option<&Item> {
        self.session.items.get(self.chosen)
    }

    /// The menu after an edit: opened (`requested`), updated or closed.
    pub fn update(
        menu: Option<Menu>,
        registry: &Registry,
        doc: &mut DocumentState,
        requested: bool,
    ) -> Option<Menu> {
        let keep = menu.as_ref().map_or(0, |m| m.chosen);
        let requested = requested || menu.as_ref().is_some_and(|m| m.requested);
        drop(menu);
        Menu::open(registry, doc, requested, keep)
    }

    /// The rows the menu shows around the chosen item: labels, their
    /// kinds and whether chosen.
    pub fn rows(&self, n: usize) -> Vec<(String, &'static str, bool)> {
        let first = self.chosen.saturating_sub(n.saturating_sub(1));
        self.session
            .items
            .iter()
            .enumerate()
            .skip(first)
            .take(n)
            .map(|(i, it)| (it.label.clone(), it.kind.name(), i == self.chosen))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{LineEnding, Metadata};

    #[test]
    fn slices_end_at_characters() {
        let ctx = Context {
            text: Arc::from("aç b"),
            base: 0,
            point: 1,
            mode: DocumentMode::Latex,
            language: None,
            path: None,
            requested: false,
        };
        // One byte after `a` is inside `ç`: the slice stops before it.
        assert_eq!(ctx.slice(1..2), "");
        assert_eq!(ctx.slice(0..3), "aç");
    }

    fn doc(text: &str, mode: DocumentMode, point: usize) -> DocumentState {
        let mut d = DocumentState::new(
            text,
            Metadata {
                path: None,
                mode,
                line_ending: LineEnding::Lf,
                bom: false,
                encoding: encoding_rs::UTF_8,
            },
            Arc::new(org_model::Settings::default()),
        );
        d.move_cursor(point, false);
        d
    }

    #[test]
    fn document_words() {
        let t = "The quantum qualities of quartz.\nqua";
        let mut d = doc(t, DocumentMode::Text { language: None }, t.len());
        let items = Registry::with_builtins().complete(&mut d, false, Duration::ZERO);
        let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
        // The nearest first.
        assert_eq!(labels, ["quartz", "qualities", "quantum"]);
        apply(&mut d, &items[1], Instant::now());
        assert!(d.text().as_str().ends_with("\nqualities"));
        // Two letters open nothing, unless asked.
        let t = "alpha beta\nal";
        let mut d = doc(t, DocumentMode::Text { language: None }, t.len());
        assert!(
            Registry::with_builtins()
                .complete(&mut d, false, Duration::ZERO)
                .is_empty()
        );
        assert_eq!(
            Registry::with_builtins().complete(&mut d, true, Duration::ZERO)[0].label,
            "alpha"
        );
    }

    #[test]
    fn org_completions() {
        let t = "* Intro\n#+ti";
        let mut d = doc(t, DocumentMode::Org, t.len());
        let items = Registry::with_builtins().complete(&mut d, false, Duration::ZERO);
        assert_eq!(items[0].insert, "#+title: ");
        assert_eq!(items[0].kind, Kind::Keyword);
        apply(&mut d, &items[0], Instant::now());
        assert_eq!(d.text().as_str(), "* Intro\n#+title: ");
    }

    struct Slow;

    impl Completer for Slow {
        fn id(&self) -> &'static str {
            "slow"
        }
        fn applies(&self, _: &Context) -> bool {
            true
        }
        fn trigger(&self) -> Trigger {
            Trigger::Request
        }
        fn slow(&self) -> bool {
            true
        }
        fn complete(&self, ctx: &Context, _: Option<&DocumentState>, _: &Cancel) -> Vec<Item> {
            std::thread::sleep(Duration::from_millis(30));
            vec![Item::new(
                "from a server",
                "server",
                ctx.point..ctx.point,
                Kind::Symbol,
            )]
        }
    }

    #[test]
    fn slow_completers_arrive_later() {
        let t = "alpha\nal";
        let mut d = doc(t, DocumentMode::Text { language: None }, t.len());
        let mut r = Registry::with_builtins();
        r.register(Arc::new(Slow));
        let mut s = r.start(&mut d, true);
        assert!(s.waiting());
        assert!(s.items.iter().all(|i| i.source != "slow"));
        let end = Instant::now() + Duration::from_secs(2);
        while s.waiting() && Instant::now() < end {
            s.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(s.items.iter().any(|i| i.source == "slow"));
        // Exact prefix first.
        assert_eq!(s.items[0].label, "alpha");
    }
}
