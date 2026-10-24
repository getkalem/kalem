//! Each running plugin answers the editors' calls from their threads (its
//! layers' overlays, its completers' items, its formatters' text, its
//! outlines' headings) under a lock of its own, not the plugins' one: a
//! plugin whose call runs long keeps no other waiting, nor busy. Two fake
//! plugins, one slow; the fast one answers while the slow one's call
//! runs. In a test binary of its own, as the plugins are the process's.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use kalem_core::DocumentState;
use kalem_core::completers::{Cancel, Context, Item};
use kalem_core::events::Event;
use kalem_core::extensions::{self, Answer, Calls, Extensions, HttpResponse, PanelEvent};
use kalem_core::formatters::plugins::{self as formatters, PluginFormatterSpec};
use kalem_core::layers::{self, LayerSpec, LineEffect, Lines, Outcome, Overlays};
use kalem_core::outlines::{self, PluginOutlineSpec};
use kalem_core::view::BlockKind;

/// The plugins' own calls (commands, events): none here.
struct Nothing;

impl Extensions for Nothing {
    fn run(&mut self, _: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn event(&mut self, _: &Event) -> Option<String> {
        None
    }
    fn answer(&mut self, _: u64, _: Answer) {}
    fn panel_event(&mut self, _: &str, _: &str, _: &PanelEvent) {}
    fn settings_changed(&mut self, _: &[String]) {}
    fn respond(&mut self, _: u64, _: Result<HttpResponse, String>) {}
    fn process_done(&mut self, _: u64, _: Result<extensions::ProcessExit, String>) {}
}

/// Whether the slow plugin may end its call; it waits until then.
static OPEN: AtomicBool = AtomicBool::new(false);
/// The slow plugin's calls begun.
static SLOW: AtomicUsize = AtomicUsize::new(0);

/// A plugin hiding every `id::` line; the slow one waits for `OPEN`
/// first, far longer than any budget.
struct Plugin {
    slow: bool,
}

impl Plugin {
    fn wait(&self) {
        if !self.slow {
            return;
        }
        SLOW.fetch_add(1, Ordering::SeqCst);
        let end = Instant::now() + Duration::from_secs(20);
        while !OPEN.load(Ordering::SeqCst) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

impl Calls for Plugin {
    fn overlays(&self, _: &str, _: Option<&Path>, text: &str) -> Outcome {
        self.wait();
        let mut o = Overlays::default();
        let mut at = 0;
        for line in text.split_inclusive('\n') {
            if line.trim_start().starts_with("id::") {
                o.lines.push(Lines {
                    range: at..at + line.len(),
                    effect: LineEffect::Hidden,
                });
            }
            at += line.len();
        }
        Outcome::Done(o)
    }

    fn complete(
        &self,
        _: &kalem_core::completers::plugins::PluginCompleterSpec,
        _: &Context,
        _: &Cancel,
    ) -> Vec<Item> {
        self.wait();
        Vec::new()
    }

    fn format(
        &self,
        _: &PluginFormatterSpec,
        _: Option<&Path>,
        text: &str,
        _: &str,
    ) -> formatters::Answer {
        self.wait();
        formatters::Answer::Text(text.to_uppercase())
    }

    fn outline(
        &self,
        _: &PluginOutlineSpec,
        _: Option<&Path>,
        _: &str,
        _: &str,
    ) -> outlines::Answer {
        self.wait();
        outlines::Answer::Headings(Vec::new())
    }
}

/// The lines hidden, from their start to the next's.
fn hidden(d: &DocumentState) -> Vec<(usize, usize)> {
    kalem_core::mode_view::blocks(d)
        .iter()
        .filter(|b| b.kind == BlockKind::Hidden)
        .map(|b| (b.range.start, b.range.end))
        .collect()
}

#[test]
fn a_slow_plugin_keeps_no_other_waiting() {
    let dir = std::env::temp_dir().join(format!("kalem-plugin-calls-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut docs = Vec::new();
    extensions::install(Box::new(Nothing));
    for (plugin, slow) in [("slowpl", true), ("fastpl", false)] {
        let marker = format!("{plugin}.marker");
        std::fs::create_dir_all(dir.join(plugin)).unwrap();
        std::fs::write(dir.join(plugin).join(&marker), "").unwrap();
        layers::register(LayerSpec {
            plugin: plugin.into(),
            id: "ids".into(),
            markers: vec![marker],
            modes: vec!["markdown".into()],
            open: None,
        });
        formatters::register(PluginFormatterSpec {
            plugin: plugin.into(),
            id: "upper".into(),
            types: vec![format!("{plugin}lang")],
        });
        extensions::set_calls(plugin, Some(Arc::new(Plugin { slow })));
        let path = dir.join(plugin).join("a.md");
        let text = "- one\n  id:: 1\n- two\n";
        let meta = kalem_core::Metadata {
            path: Some(path.clone()),
            mode: kalem_core::DocumentMode::detect(Some(&path), text.as_bytes()),
            line_ending: kalem_core::LineEnding::Lf,
            bom: false,
            encoding: kalem_core::encoding_rs::UTF_8,
            lossy: false,
        };
        docs.push(DocumentState::new(
            text,
            meta,
            Arc::new(org_model::Settings::default()),
        ));
    }
    let (mut slow, mut fast) = (docs.remove(0), docs.remove(0));

    // The slow plugin's layer is asked, and its call runs.
    assert!(hidden(&slow).is_empty());
    let end = Instant::now() + Duration::from_secs(20);
    while SLOW.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < end, "never asked");
        std::thread::sleep(Duration::from_millis(1));
    }
    // Meanwhile the fast one answers: its document's layer…
    assert!(hidden(&fast).is_empty());
    let end = Instant::now() + Duration::from_secs(20);
    while !fast.poll() {
        assert!(Instant::now() < end, "the fast plugin never answered");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(hidden(&fast), [(6, 15)]);
    // …and its formatter, not busy.
    let spec = formatters::for_names(&["fastpllang".into()]).expect("its formatter");
    assert_eq!(
        formatters::format(&spec, &fast),
        formatters::Answer::Text("- ONE\n  ID:: 1\n- TWO\n".into())
    );
    // The slow one's call is still running.
    assert!(slow.waiting_layer() && !slow.poll());

    // Its call ends: its answer is drawn too.
    OPEN.store(true, Ordering::SeqCst);
    let end = Instant::now() + Duration::from_secs(20);
    while !slow.poll() {
        assert!(Instant::now() < end, "the slow plugin never answered");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(hidden(&slow), [(6, 15)]);

    // A plugin stopped: its calls are gone.
    extensions::set_calls("fastpl", None);
    layers::refresh();
    let _ = hidden(&fast);
    let end = Instant::now() + Duration::from_secs(20);
    while !fast.poll() {
        assert!(Instant::now() < end, "never answered");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(hidden(&fast).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
