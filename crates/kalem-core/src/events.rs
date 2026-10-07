//! The event bus (design §11.3): events from the editor to plugins and to
//! the frontends' own listeners.
//!
//! Handlers run on the UI thread in the order they subscribed. Vetoable
//! events (`document:before-save`, `babel:*`, `export:*`) wait for every
//! handler, including ones that answer later (script promises), for at
//! most [`VETO_TIMEOUT`]; after that the event proceeds with a warning.
//! Waiting never blocks: [`EventBus::emit_vetoable`] returns a
//! [`PendingVeto`] that the frontend polls while its event loop, and the
//! script runtime on it, keep running. Events raised on other threads go
//! through an [`EventSender`] and are dispatched by
//! [`EventBus::dispatch_queued`]. A handler that panics three times is
//! removed.

use std::collections::BTreeMap;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::time::{Duration, Instant};

use org_edit::{Assoc, Transaction};

/// How long a vetoable event waits for its handlers.
pub const VETO_TIMEOUT: Duration = Duration::from_millis(500);

/// Failures after which a handler is removed.
const MAX_FAILURES: u32 = 3;

/// An open document, as the editor numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DocumentId(pub u64);

/// An event. Positions are byte offsets in the document's text.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// `app:ready`: startup is complete.
    AppReady,
    /// `document:open`.
    DocumentOpen {
        /// The document.
        doc: DocumentId,
        /// Its file, if any.
        path: Option<PathBuf>,
    },
    /// `document:close`.
    DocumentClose {
        /// The document.
        doc: DocumentId,
    },
    /// `document:before-save` (vetoable).
    DocumentBeforeSave {
        /// The document.
        doc: DocumentId,
        /// Where it is saved.
        path: PathBuf,
    },
    /// `document:after-save`.
    DocumentAfterSave {
        /// The document.
        doc: DocumentId,
        /// Where it was saved.
        path: PathBuf,
    },
    /// `document:changed`, debounced ([`ChangeDebouncer`]).
    DocumentChanged {
        /// The document.
        doc: DocumentId,
        /// The version of its text.
        version: u64,
        /// The changed ranges in that version, sorted and disjoint.
        ranges: Vec<Range<usize>>,
    },
    /// `selection:changed`.
    SelectionChanged {
        /// The document.
        doc: DocumentId,
        /// Where the selection started.
        anchor: usize,
        /// The cursor.
        head: usize,
    },
    /// `headline:todo-changed`.
    HeadlineTodoChanged {
        /// The document.
        doc: DocumentId,
        /// The start of the headline.
        headline: usize,
        /// The old keyword.
        from: Option<String>,
        /// The new keyword.
        to: Option<String>,
    },
    /// `headline:tags-changed`.
    HeadlineTagsChanged {
        /// The document.
        doc: DocumentId,
        /// The start of the headline.
        headline: usize,
        /// The new tags.
        tags: Vec<String>,
    },
    /// `headline:scheduled`.
    HeadlineScheduled {
        /// The document.
        doc: DocumentId,
        /// The start of the headline.
        headline: usize,
        /// The new timestamp, or `None` when it was removed.
        timestamp: Option<String>,
    },
    /// `table:before-recalc`.
    TableBeforeRecalc {
        /// The document.
        doc: DocumentId,
        /// The start of the table.
        table: usize,
    },
    /// `table:recalculated`.
    TableRecalculated {
        /// The document.
        doc: DocumentId,
        /// The start of the table.
        table: usize,
    },
    /// `babel:before-execute` (vetoable).
    BabelBeforeExecute {
        /// The document.
        doc: DocumentId,
        /// The start of the block.
        block: usize,
        /// The block's language.
        language: String,
    },
    /// `babel:after-execute` (vetoable: a veto drops the result).
    BabelAfterExecute {
        /// The document.
        doc: DocumentId,
        /// The start of the block.
        block: usize,
        /// The block's language.
        language: String,
        /// Whether it succeeded.
        success: bool,
    },
    /// `export:before` (vetoable).
    ExportBefore {
        /// The document.
        doc: DocumentId,
        /// The backend, such as `html`.
        backend: String,
    },
    /// `export:after` (vetoable: a veto discards the output).
    ExportAfter {
        /// The document.
        doc: DocumentId,
        /// The backend.
        backend: String,
        /// The file written, if any.
        output: Option<PathBuf>,
    },
    /// `workspace:file-changed`, from the file watcher.
    WorkspaceFileChanged {
        /// The file.
        path: PathBuf,
    },
}

macro_rules! kinds {
    ($($kind:ident = $name:literal, $veto:literal;)*) => {
        /// The kinds of events, for subscribing.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum EventKind {
            $(
                #[doc = concat!("`", $name, "`")]
                $kind,
            )*
        }

        impl EventKind {
            /// Every kind.
            pub const ALL: &[EventKind] = &[$(EventKind::$kind),*];

            /// The name plugins use.
            pub fn name(self) -> &'static str {
                match self {
                    $(EventKind::$kind => $name,)*
                }
            }

            /// Whether handlers can veto events of this kind.
            pub fn vetoable(self) -> bool {
                match self {
                    $(EventKind::$kind => $veto,)*
                }
            }
        }

        impl Event {
            /// The event's kind.
            pub fn kind(&self) -> EventKind {
                match self {
                    $(Event::$kind { .. } => EventKind::$kind,)*
                }
            }
        }
    };
}

kinds! {
    AppReady = "app:ready", false;
    DocumentOpen = "document:open", false;
    DocumentClose = "document:close", false;
    DocumentBeforeSave = "document:before-save", true;
    DocumentAfterSave = "document:after-save", false;
    DocumentChanged = "document:changed", false;
    SelectionChanged = "selection:changed", false;
    HeadlineTodoChanged = "headline:todo-changed", false;
    HeadlineTagsChanged = "headline:tags-changed", false;
    HeadlineScheduled = "headline:scheduled", false;
    TableBeforeRecalc = "table:before-recalc", false;
    TableRecalculated = "table:recalculated", false;
    BabelBeforeExecute = "babel:before-execute", true;
    BabelAfterExecute = "babel:after-execute", true;
    ExportBefore = "export:before", true;
    ExportAfter = "export:after", true;
    WorkspaceFileChanged = "workspace:file-changed", false;
}

impl EventKind {
    /// The kind with this name.
    pub fn from_name(name: &str) -> Option<EventKind> {
        EventKind::ALL.iter().copied().find(|k| k.name() == name)
    }
}

/// A handler's answer.
#[derive(Debug)]
pub enum Reply {
    /// Go on.
    Continue,
    /// Stop a vetoable event, with a reason for the user.
    Veto(String),
    /// The answer comes later (a script's promise); it must be `Continue`
    /// or `Veto`.
    Later(Receiver<Reply>),
}

/// A subscription, for unsubscribing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SubscriptionId(u64);

type Handler = Box<dyn FnMut(&Event) -> Reply>;

struct Subscriber {
    id: SubscriptionId,
    kind: Option<EventKind>,
    handler: Handler,
    failures: u32,
}

/// Sends events to the bus from other threads.
#[derive(Debug, Clone)]
pub struct EventSender(Sender<Event>);

impl EventSender {
    /// Queues an event; `false` if the bus is gone.
    pub fn send(&self, event: Event) -> bool {
        self.0.send(event).is_ok()
    }
}

/// The result of a vetoable event.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VetoOutcome {
    /// The first veto: the handler and its reason.
    pub veto: Option<(SubscriptionId, String)>,
    /// Handlers that did not answer in time.
    pub timed_out: Vec<SubscriptionId>,
}

impl VetoOutcome {
    /// Whether the event proceeds.
    pub fn allowed(&self) -> bool {
        self.veto.is_none()
    }
}

/// A vetoable event waiting for late answers.
#[derive(Debug)]
pub struct PendingVeto {
    deadline: Instant,
    waiting: Vec<(SubscriptionId, Receiver<Reply>)>,
    outcome: VetoOutcome,
}

impl PendingVeto {
    fn answer(&mut self, id: SubscriptionId, reply: Reply) {
        if let Reply::Veto(reason) = reply
            && self.outcome.veto.is_none()
        {
            self.outcome.veto = Some((id, reason));
        }
    }

    fn done(&self) -> bool {
        self.outcome.veto.is_some() || self.waiting.is_empty()
    }

    /// The outcome once there is a veto, every handler has answered, or
    /// the deadline has passed.
    pub fn poll(&mut self, now: Instant) -> Option<VetoOutcome> {
        let mut still = Vec::new();
        for (id, rx) in std::mem::take(&mut self.waiting) {
            match rx.try_recv() {
                Ok(reply) => self.answer(id, reply),
                Err(TryRecvError::Empty) => still.push((id, rx)),
                // A dropped promise counts as no objection.
                Err(TryRecvError::Disconnected) => {}
            }
        }
        self.waiting = still;
        if self.done() {
            return Some(std::mem::take(&mut self.outcome));
        }
        if now >= self.deadline {
            self.outcome.timed_out = self.waiting.drain(..).map(|(id, _)| id).collect();
            return Some(std::mem::take(&mut self.outcome));
        }
        None
    }

    /// Blocks until [`PendingVeto::poll`] would give the outcome, for
    /// callers without an event loop (the CLI).
    pub fn wait(mut self) -> VetoOutcome {
        while !self.done() {
            let (id, rx) = self.waiting.remove(0);
            let left = self.deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(reply) => self.answer(id, reply),
                Err(RecvTimeoutError::Disconnected) => {}
                Err(RecvTimeoutError::Timeout) => {
                    self.waiting.insert(0, (id, rx));
                    break;
                }
            }
        }
        // At the deadline `poll` always gives the outcome.
        self.poll(self.deadline.max(Instant::now()))
            .unwrap_or_else(|| std::mem::take(&mut self.outcome))
    }
}

/// The event bus.
pub struct EventBus {
    subscribers: Vec<Subscriber>,
    next: u64,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    warnings: Vec<String>,
}

impl std::fmt::Debug for EventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBus")
            .field("subscribers", &self.subscribers.len())
            .finish()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        EventBus::new()
    }
}

impl EventBus {
    /// A bus without subscribers.
    pub fn new() -> EventBus {
        let (tx, rx) = mpsc::channel();
        EventBus {
            subscribers: Vec::new(),
            next: 0,
            tx,
            rx,
            warnings: Vec::new(),
        }
    }

    /// Calls `handler` for events of `kind`, or for every event.
    pub fn subscribe(
        &mut self,
        kind: Option<EventKind>,
        handler: impl FnMut(&Event) -> Reply + 'static,
    ) -> SubscriptionId {
        self.next += 1;
        let id = SubscriptionId(self.next);
        self.subscribers.push(Subscriber {
            id,
            kind,
            handler: Box::new(handler),
            failures: 0,
        });
        id
    }

    /// Removes a subscription; `false` if it was not there.
    pub fn unsubscribe(&mut self, id: SubscriptionId) -> bool {
        let n = self.subscribers.len();
        self.subscribers.retain(|s| s.id != id);
        self.subscribers.len() < n
    }

    /// Calls the handlers of `event`, and collects the answers that came
    /// at once.
    fn call(&mut self, event: &Event) -> Vec<(SubscriptionId, Reply)> {
        let kind = event.kind();
        let mut replies = Vec::new();
        let mut removed = Vec::new();
        for s in &mut self.subscribers {
            if s.kind.is_some_and(|k| k != kind) {
                continue;
            }
            match catch_unwind(AssertUnwindSafe(|| (s.handler)(event))) {
                Ok(r) => replies.push((s.id, r)),
                Err(_) => {
                    s.failures += 1;
                    tracing::warn!(
                        event = kind.name(),
                        failures = s.failures,
                        "event handler failed"
                    );
                    self.warnings
                        .push(format!("A `{}` handler failed", kind.name()));
                    if s.failures >= MAX_FAILURES {
                        removed.push(s.id);
                    }
                }
            }
        }
        for id in removed {
            self.unsubscribe(id);
            self.warnings.push(format!(
                "A `{}` handler failed {MAX_FAILURES} times and was removed",
                kind.name()
            ));
        }
        replies
    }

    /// Sends an event that cannot be vetoed. Answers are ignored.
    pub fn emit(&mut self, event: &Event) {
        debug_assert!(!event.kind().vetoable(), "use emit_vetoable");
        self.call(event);
    }

    /// Sends a vetoable event. The first veto stops it; late answers are
    /// awaited until `now + VETO_TIMEOUT`.
    pub fn emit_vetoable(&mut self, event: &Event, now: Instant) -> PendingVeto {
        let mut pending = PendingVeto {
            deadline: now + VETO_TIMEOUT,
            waiting: Vec::new(),
            outcome: VetoOutcome::default(),
        };
        for (id, reply) in self.call(event) {
            match reply {
                Reply::Later(rx) => pending.waiting.push((id, rx)),
                r => pending.answer(id, r),
            }
        }
        pending
    }

    /// Records the outcome of a vetoable event: a warning for handlers
    /// that timed out.
    pub fn settle(&mut self, event: &Event, outcome: &VetoOutcome) {
        if let Some((_, reason)) = &outcome.veto {
            tracing::info!(event = event.kind().name(), reason, "event vetoed");
        }
        if !outcome.timed_out.is_empty() {
            tracing::warn!(
                event = event.kind().name(),
                handlers = outcome.timed_out.len(),
                "vetoable event handlers timed out"
            );
            self.warnings.push(format!(
                "{} `{}` handler(s) did not answer within {} ms; the event proceeded",
                outcome.timed_out.len(),
                event.kind().name(),
                VETO_TIMEOUT.as_millis()
            ));
        }
    }

    /// A sender for other threads.
    pub fn sender(&self) -> EventSender {
        EventSender(self.tx.clone())
    }

    /// Sends the events queued by other threads; returns how many.
    /// Vetoable events cannot be queued: they are dropped with a warning.
    pub fn dispatch_queued(&mut self) -> usize {
        let mut n = 0;
        while let Ok(e) = self.rx.try_recv() {
            if e.kind().vetoable() {
                tracing::warn!(
                    event = e.kind().name(),
                    "vetoable event sent from another thread"
                );
                self.warnings.push(format!(
                    "`{}` cannot be sent from another thread",
                    e.kind().name()
                ));
                continue;
            }
            self.call(&e);
            n += 1;
        }
        n
    }

    /// Warnings since the last call (failed handlers, timeouts), for the
    /// log and the plugin console.
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }
}

/// Ranges `ranges` (sorted, apart) moved through `tx`, as
/// `Transaction::map` moves each end, in one pass over its edits: an edit
/// on every line of a large file (a CSV column inserted) and its undo,
/// each ranges through every edit, took seconds.
fn map_ranges(tx: &Transaction, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let (mut i, mut shift) = (0usize, 0isize);
    let mut map = |pos: usize, assoc: Assoc| -> usize {
        while let Some(e) = tx.edits.get(i) {
            let (s, t) = (e.range.start, e.range.end);
            if pos < s {
                break;
            }
            if s == t {
                if pos == s && assoc == Assoc::Before {
                    break;
                }
                shift += e.insert.len() as isize;
                i += 1;
                continue;
            }
            if pos < t {
                // Inside a replaced range: not past it, a later end may be.
                let ns = (s as isize + shift) as usize;
                return match assoc {
                    Assoc::Before => ns,
                    Assoc::After => ns + e.insert.len(),
                };
            }
            shift += e.insert.len() as isize - (t - s) as isize;
            i += 1;
        }
        (pos as isize + shift) as usize
    };
    ranges
        .iter()
        .map(|r| {
            let start = map(r.start, Assoc::Before);
            start..map(r.end, Assoc::After)
        })
        .collect()
}

/// Collects a document's edits into `document:changed` events, sent once
/// no edit has come for `delay`. Ranges of earlier edits are moved
/// through later ones, and overlapping or touching ranges are merged.
#[derive(Debug, Clone)]
pub struct ChangeDebouncer {
    delay: Duration,
    pending: BTreeMap<DocumentId, (u64, Vec<Range<usize>>, Instant)>,
}

impl ChangeDebouncer {
    /// A debouncer that waits `delay` after the last edit.
    pub fn new(delay: Duration) -> ChangeDebouncer {
        ChangeDebouncer {
            delay,
            pending: BTreeMap::new(),
        }
    }

    /// Records `tx`, which made `version` of `doc`.
    pub fn record(&mut self, doc: DocumentId, version: u64, tx: &Transaction, now: Instant) {
        let entry = self
            .pending
            .entry(doc)
            .or_insert((version, Vec::new(), now));
        let mut ranges = map_ranges(tx, &entry.1);
        let mut shift: isize = 0;
        for e in &tx.edits {
            let start = (e.range.start as isize + shift) as usize;
            ranges.push(start..start + e.insert.len());
            shift += e.insert.len() as isize - e.range.len() as isize;
        }
        ranges.sort_by_key(|r| r.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for r in ranges {
            match merged.last_mut() {
                Some(last) if r.start <= last.end => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        *entry = (version, merged, now);
    }

    /// The events whose documents have been quiet for the delay.
    pub fn due(&mut self, now: Instant) -> Vec<Event> {
        let ready: Vec<DocumentId> = self
            .pending
            .iter()
            .filter(|(_, (_, _, last))| now.duration_since(*last) >= self.delay)
            .map(|(d, _)| *d)
            .collect();
        ready
            .into_iter()
            .filter_map(|doc| {
                self.pending
                    .remove(&doc)
                    .map(|(version, ranges, _)| Event::DocumentChanged {
                        doc,
                        version,
                        ranges,
                    })
            })
            .collect()
    }

    /// When [`ChangeDebouncer::due`] next has something, for the event
    /// loop's timer.
    pub fn next_due(&self) -> Option<Instant> {
        self.pending
            .values()
            .map(|(_, _, last)| *last + self.delay)
            .min()
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    fn save() -> Event {
        Event::DocumentBeforeSave {
            doc: DocumentId(1),
            path: "a.org".into(),
        }
    }

    #[test]
    fn names() {
        for k in EventKind::ALL {
            assert_eq!(EventKind::from_name(k.name()), Some(*k));
        }
        assert!(EventKind::DocumentBeforeSave.vetoable());
        assert!(!EventKind::DocumentChanged.vetoable());
        assert_eq!(save().kind().name(), "document:before-save");
    }

    #[test]
    fn subscriptions() {
        let mut bus = EventBus::new();
        let seen = Rc::new(RefCell::new(Vec::new()));
        let s = seen.clone();
        let all = bus.subscribe(None, move |e| {
            s.borrow_mut().push(e.kind().name());
            Reply::Continue
        });
        let s = seen.clone();
        bus.subscribe(Some(EventKind::AppReady), move |_| {
            s.borrow_mut().push("ready!");
            Reply::Continue
        });
        bus.emit(&Event::AppReady);
        bus.emit(&Event::DocumentClose { doc: DocumentId(1) });
        assert!(bus.unsubscribe(all));
        assert!(!bus.unsubscribe(all));
        bus.emit(&Event::AppReady);
        assert_eq!(
            *seen.borrow(),
            ["app:ready", "ready!", "document:close", "ready!"]
        );
        // From another thread.
        let tx = bus.sender();
        std::thread::spawn(move || tx.send(Event::AppReady))
            .join()
            .unwrap();
        assert_eq!(bus.dispatch_queued(), 1);
        assert_eq!(seen.borrow().len(), 5);
    }

    #[test]
    fn vetoes() {
        let now = Instant::now();
        let mut bus = EventBus::new();
        bus.subscribe(None, |_| Reply::Continue);
        assert!(bus.emit_vetoable(&save(), now).poll(now).unwrap().allowed());
        let veto = bus.subscribe(Some(EventKind::DocumentBeforeSave), |_| {
            Reply::Veto("read-only".into())
        });
        let out = bus.emit_vetoable(&save(), now).poll(now).unwrap();
        assert_eq!(out.veto, Some((veto, "read-only".into())));
        bus.unsubscribe(veto);
        // A late answer.
        let (tx, rx) = mpsc::channel();
        let rx = RefCell::new(Some(rx));
        let late = bus.subscribe(None, move |_| Reply::Later(rx.borrow_mut().take().unwrap()));
        let mut p = bus.emit_vetoable(&save(), now);
        assert!(p.poll(now).is_none());
        tx.send(Reply::Veto("no".into())).unwrap();
        assert_eq!(p.poll(now).unwrap().veto, Some((late, "no".into())));
        bus.unsubscribe(late);
        // No answer in time: the event proceeds.
        let (tx, rx) = mpsc::channel::<Reply>();
        let rx = RefCell::new(Some(rx));
        let slow = bus.subscribe(None, move |_| Reply::Later(rx.borrow_mut().take().unwrap()));
        let mut p = bus.emit_vetoable(&save(), now);
        assert!(p.poll(now + Duration::from_millis(100)).is_none());
        let out = p.poll(now + VETO_TIMEOUT).unwrap();
        assert!(out.allowed() && out.timed_out == [slow]);
        bus.settle(&save(), &out);
        assert_eq!(bus.take_warnings().len(), 1);
        drop(tx);
    }

    #[test]
    fn blocking_wait() {
        let mut bus = EventBus::new();
        let (tx, rx) = mpsc::channel();
        let rx = RefCell::new(Some(rx));
        bus.subscribe(None, move |_| Reply::Later(rx.borrow_mut().take().unwrap()));
        let p = bus.emit_vetoable(&save(), Instant::now());
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            tx.send(Reply::Continue).unwrap();
        });
        let out = p.wait();
        assert!(out.allowed() && out.timed_out.is_empty());
    }

    #[test]
    fn failing_handlers_are_removed() {
        let mut bus = EventBus::new();
        bus.subscribe(None, |_| panic!("plugin bug"));
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        for _ in 0..4 {
            bus.emit(&Event::AppReady);
        }
        std::panic::set_hook(prev);
        let w = bus.take_warnings();
        assert_eq!(w.len(), 4, "{w:?}");
        assert!(w[3].contains("removed"));
    }

    #[test]
    fn debouncing() {
        let t = Instant::now();
        let ms = |n| t + Duration::from_millis(n);
        let mut d = ChangeDebouncer::new(Duration::from_millis(300));
        let doc = DocumentId(7);
        let mut tx = Transaction::new("a");
        tx.insert(10, "abc").unwrap();
        d.record(doc, 1, &tx, t);
        // Typing before the first edit moves its range.
        let mut tx = Transaction::new("b");
        tx.insert(2, "xy").unwrap();
        d.record(doc, 2, &tx, ms(100));
        assert!(d.due(ms(350)).is_empty());
        assert_eq!(d.next_due(), Some(ms(400)));
        let mut tx = Transaction::new("c");
        tx.replace(13..16, "Q").unwrap();
        d.record(doc, 3, &tx, ms(200));
        assert_eq!(
            d.due(ms(500)),
            [Event::DocumentChanged {
                doc,
                version: 3,
                ranges: vec![2..4, 12..14],
            }]
        );
        assert!(d.due(ms(900)).is_empty());
    }

    #[test]
    fn ranges_move_as_each_end_would() {
        // Insertions, deletions and replacements, before, inside, at and
        // after the ranges' ends.
        let mut tx = Transaction::new("t");
        tx.insert(0, "ab").unwrap();
        tx.replace(3..5, "").unwrap();
        tx.insert(8, "x").unwrap();
        tx.replace(9..12, "QQ").unwrap();
        tx.insert(20, "yyy").unwrap();
        tx.replace(21..30, "z").unwrap();
        let ranges = vec![0..0, 2..4, 6..8, 10..11, 14..20, 25..40];
        let one_by_one: Vec<Range<usize>> = ranges
            .iter()
            .map(|r| tx.map(r.start, Assoc::Before)..tx.map(r.end, Assoc::After))
            .collect();
        assert_eq!(map_ranges(&tx, &ranges), one_by_one);
    }
}
