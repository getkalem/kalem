//! Measures one WebAssembly engine (the build's feature) running the
//! spike's guest component: compile, load from a precompiled cache,
//! instantiate, a Markdown parse of 10 MB and of 10 kB across the
//! boundary against the same parse natively, the cost of a time budget
//! (fuel or epochs) and whether it stops a loop, a 64 MB memory limit, and
//! one instance per thread.
//!
//! `cargo run --release --features cranelift|pulley|wasmi -- GUEST.wasm`

use std::time::{Duration, Instant};

use anyhow::Result;

const MB: usize = 1 << 20;

/// The repository's Markdown files, repeated to `len` bytes.
fn corpus(len: usize) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut files = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if p.is_dir() && !name.starts_with('.') && name != "target" {
                stack.push(p);
            } else if name.ends_with(".md") {
                files.push(p);
            }
        }
    }
    files.sort();
    let mut text = String::new();
    while text.len() < len {
        for f in &files {
            text.push_str(&std::fs::read_to_string(f).unwrap_or_default());
            text.push('\n');
        }
    }
    let mut cut = len;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.truncate(cut);
    text
}

/// The parse natively, as the guest does it: the elements' count.
fn native(text: &str) -> usize {
    use pulldown_cmark::{Event, Options, Parser};
    Parser::new_ext(text, Options::all())
        .into_offset_iter()
        .filter(|(e, _)| matches!(e, Event::Start(_)))
        .count()
}

fn ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1e3)
}

/// The median of `n` runs of `f`.
fn median(n: usize, mut f: impl FnMut()) -> Duration {
    let mut t: Vec<Duration> = (0..n)
        .map(|_| {
            let s = Instant::now();
            f();
            s.elapsed()
        })
        .collect();
    t.sort();
    t[n / 2]
}

#[cfg(any(feature = "cranelift", feature = "pulley"))]
mod engine {
    use super::*;
    use wasmtime::component::{Component, Linker};
    use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

    wasmtime::component::bindgen!({ world: "parser", path: "../wit" });

    pub(crate) const NAME: &str = if cfg!(feature = "pulley") {
        "wasmtime 49, Pulley (interpreter)"
    } else {
        "wasmtime 49, Cranelift"
    };

    struct State {
        limits: StoreLimits,
    }

    #[derive(Clone, Copy)]
    enum Budget {
        None,
        Fuel,
        Epoch,
    }

    fn engine(budget: Budget) -> Result<Engine> {
        let mut c = Config::new();
        if cfg!(feature = "pulley") {
            c.target("pulley64")?;
        }
        match budget {
            Budget::None => {}
            Budget::Fuel => {
                c.consume_fuel(true);
            }
            Budget::Epoch => {
                c.epoch_interruption(true);
            }
        }
        Ok(Engine::new(&c)?)
    }

    fn store(engine: &Engine, budget: Budget, memory: usize) -> Store<State> {
        let mut s = Store::new(
            engine,
            State {
                limits: StoreLimitsBuilder::new().memory_size(memory).build(),
            },
        );
        s.limiter(|s| &mut s.limits);
        match budget {
            Budget::None => {}
            Budget::Fuel => s.set_fuel(u64::MAX).unwrap(),
            Budget::Epoch => s.set_epoch_deadline(u64::MAX / 2),
        }
        s
    }

    pub(crate) fn run(bytes: &[u8], big: &str, small: &str) -> Result<()> {
        let e = engine(Budget::None)?;
        let t = Instant::now();
        let component = Component::new(&e, bytes)?;
        println!("compile: {}", ms(t.elapsed()));
        let cwasm = component.serialize()?;
        let t = Instant::now();
        // SAFETY: the bytes were serialized just now by this engine.
        let component = unsafe { Component::deserialize(&e, &cwasm)? };
        println!(
            "load precompiled ({} kB): {}",
            cwasm.len() / 1024,
            ms(t.elapsed())
        );
        let imports: Vec<String> = component
            .component_type()
            .imports(&e)
            .map(|(n, _)| n.to_string())
            .collect();
        println!("imports: {imports:?}");
        let linker: Linker<State> = Linker::new(&e);
        let mut s = store(&e, Budget::None, 512 * MB);
        let t = Instant::now();
        let p = Parser::instantiate(&mut s, &component, &linker)?;
        println!("instantiate (first): {}", ms(t.elapsed()));
        let pre = linker.instantiate_pre(&component)?;
        let d = median(20, || {
            let mut s = store(&e, Budget::None, 512 * MB);
            pre.instantiate(&mut s).unwrap();
        });
        println!("instantiate (pre-linked, median): {}", ms(d));

        let n = p.call_parse(&mut s, big)?.len();
        let d = median(5, || {
            p.call_parse(&mut s, big).unwrap();
        });
        let dn = median(5, || {
            native(big);
        });
        println!(
            "parse 10 MB: {} ({} elements); native {} ({})",
            ms(d),
            n,
            ms(dn),
            native(big)
        );
        let d = median(200, || {
            p.call_parse(&mut s, small).unwrap();
        });
        let dn = median(200, || {
            native(small);
        });
        println!("parse 10 kB (a keystroke): {}; native {}", ms(d), ms(dn));

        // The budget's cost on the parse, and whether it stops a loop.
        for (name, b) in [("fuel", Budget::Fuel), ("epochs", Budget::Epoch)] {
            let e = engine(b)?;
            let component = Component::new(&e, bytes)?;
            let linker: Linker<State> = Linker::new(&e);
            let mut s = store(&e, b, 512 * MB);
            let p = Parser::instantiate(&mut s, &component, &linker)?;
            let d = median(5, || {
                if let Budget::Fuel = b {
                    s.set_fuel(u64::MAX).unwrap();
                }
                p.call_parse(&mut s, big).unwrap();
            });
            let mut s = store(&e, b, 512 * MB);
            let p = Parser::instantiate(&mut s, &component, &linker)?;
            let t = Instant::now();
            let stopped = match b {
                Budget::Fuel => {
                    // About 100 ms of work at a few hundred million
                    // instructions a second.
                    s.set_fuel(50_000_000)?;
                    p.call_spin(&mut s, u64::MAX).is_err()
                }
                Budget::Epoch => {
                    s.set_epoch_deadline(1);
                    let ticker = e.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(100));
                        ticker.increment_epoch();
                    });
                    p.call_spin(&mut s, u64::MAX).is_err()
                }
                Budget::None => false,
            };
            println!(
                "{name}: parse 10 MB {}; an endless loop stopped: {stopped} after {}",
                ms(d),
                ms(t.elapsed())
            );
        }

        // 64 MB: 100 MB asked for fails, 10 MB does not.
        let mut s = store(&e, Budget::None, 64 * MB);
        let p = Parser::instantiate(&mut s, &component, &linker)?;
        let over = p.call_grow(&mut s, 100);
        let mut s = store(&e, Budget::None, 64 * MB);
        let p = Parser::instantiate(&mut s, &component, &linker)?;
        let under = p.call_grow(&mut s, 10);
        println!(
            "memory 64 MB: 100 MB {}, 10 MB {}",
            if over.is_err() { "refused" } else { "ALLOWED" },
            if under.is_ok() { "allowed" } else { "REFUSED" }
        );

        // One instance per thread, four threads at once.
        let t = Instant::now();
        std::thread::scope(|sc| {
            for _ in 0..4 {
                let (e, component, linker) = (&e, &component, &linker);
                sc.spawn(move || {
                    let mut s = store(e, Budget::None, 512 * MB);
                    let p = Parser::instantiate(&mut s, component, linker).unwrap();
                    p.call_parse(&mut s, big).unwrap();
                });
            }
        });
        println!("4 threads, an instance each, 10 MB each: {}", ms(t.elapsed()));
        Ok(())
    }
}

#[cfg(feature = "wasmi")]
mod engine {
    use super::*;
    use wasm_component_layer::{Component, Engine, Linker, Store, Value};

    pub(crate) const NAME: &str = "wasmi 0.40 through wasm_component_layer 0.1.18";

    fn parse(
        store: &mut Store<(), wasmi_runtime_layer::Engine>,
        f: &wasm_component_layer::Func,
        text: &str,
    ) -> usize {
        let mut out = [Value::Bool(false)];
        f.call(&mut *store, &[Value::String(text.into())], &mut out)
            .unwrap();
        match &out[0] {
            Value::List(l) => l.len(),
            _ => 0,
        }
    }

    pub(crate) fn run(bytes: &[u8], big: &str, small: &str) -> Result<()> {
        let engine = Engine::new(wasmi_runtime_layer::Engine::default());
        let t = Instant::now();
        let component = Component::new(&engine, bytes)?;
        println!("compile (validate and translate): {}", ms(t.elapsed()));
        println!("load precompiled: not offered (wasmi translates lazily)");
        let linker = Linker::default();
        let mut store = Store::new(&engine, ());
        let t = Instant::now();
        let instance = linker.instantiate(&mut store, &component)?;
        println!("instantiate (first): {}", ms(t.elapsed()));
        let d = median(20, || {
            let mut s = Store::new(&engine, ());
            linker.instantiate(&mut s, &component).unwrap();
        });
        println!("instantiate (median): {}", ms(d));
        let f = instance.exports().root().func("parse").expect("parse");
        let n = parse(&mut store, &f, big);
        let d = median(5, || {
            parse(&mut store, &f, big);
        });
        let dn = median(5, || {
            native(big);
        });
        println!(
            "parse 10 MB: {} ({} elements); native {} ({})",
            ms(d),
            n,
            ms(dn),
            native(big)
        );
        let d = median(200, || {
            parse(&mut store, &f, small);
        });
        let dn = median(200, || {
            native(small);
        });
        println!("parse 10 kB (a keystroke): {}; native {}", ms(d), ms(dn));
        println!(
            "fuel, epochs and memory limits: not reachable through the component layer's API"
        );
        let t = Instant::now();
        std::thread::scope(|sc| {
            for _ in 0..4 {
                let (engine, component, linker) = (&engine, &component, &linker);
                sc.spawn(move || {
                    let mut s = Store::new(engine, ());
                    let i = linker.instantiate(&mut s, component).unwrap();
                    let f = i.exports().root().func("parse").unwrap();
                    parse(&mut s, &f, big);
                });
            }
        });
        println!("4 threads, an instance each, 10 MB each: {}", ms(t.elapsed()));
        Ok(())
    }
}

fn main() -> Result<()> {
    let guest = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "../guest/target/wasm32-wasip2/release/spike_guest.wasm".into());
    let bytes = std::fs::read(&guest)?;
    let big = corpus(10 * MB);
    let small = corpus(10 * 1024);
    println!("{} ({} kB guest)", engine::NAME, bytes.len() / 1024);
    engine::run(&bytes, &big, &small)
}
