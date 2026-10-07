//! The host on components written in the component text format, so the
//! tests need no WebAssembly toolchain: calls, the cache, the time budget,
//! the memory limit, grants, threads.

use std::time::{Duration, Instant, SystemTime};

use kalem_script::{Error, Host, Limits, Plugin};

/// A component of `add`, `spin` (an endless loop) and `grow` (grows its
/// memory by a number of 64 kB pages, trapping when refused).
const TOOLS: &str = r#"
(component
  (core module $m
    (memory (export "memory") 1)
    (func (export "add") (param i32 i32) (result i32)
      local.get 0 local.get 1 i32.add)
    (func (export "spin") (loop $l br $l))
    (func (export "grow") (param i32) (result i32)
      local.get 0 memory.grow
      i32.const -1 i32.eq
      if unreachable end
      i32.const 1))
  (core instance $i (instantiate $m))
  (func (export "add") (param "a" u32) (param "b" u32) (result u32)
    (canon lift (core func $i "add")))
  (func (export "spin") (canon lift (core func $i "spin")))
  (func (export "grow") (param "pages" u32) (result u32)
    (canon lift (core func $i "grow"))))
"#;

/// A component importing a host function `log` and calling it from `run`.
const LOGS: &str = r#"
(component
  (import "log" (func $log (param "x" u32)))
  (core func $log-lowered (canon lower (func $log)))
  (core module $m
    (import "host" "log" (func $l (param i32)))
    (func (export "run") (param i32) local.get 0 call $l))
  (core instance $i
    (instantiate $m (with "host" (instance (export "log" (func $log-lowered))))))
  (func (export "run") (param "x" u32) (canon lift (core func $i "run"))))
"#;

/// The text as the host takes it (wasmtime reads the text format).
fn wasm(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("kalem-script-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn tools(host: &Host) -> Plugin {
    host.load(&wasm(TOOLS)).unwrap()
}

#[test]
fn a_call_crosses_the_boundary() {
    let host = Host::new(None).unwrap();
    let p = tools(&host);
    assert_eq!(p.exports(&host), ["add", "spin", "grow"]);
    assert!(p.imports(&host).is_empty());
    let mut i = p
        .instantiate(&host, &host.linker::<()>(), (), Limits::default())
        .unwrap();
    let (sum,): (u32,) = i.call("add", (40u32, 2u32)).unwrap();
    assert_eq!(sum, 42);
    assert!(matches!(
        i.call::<(u32,), (u32,)>("nothing", (1,)),
        Err(Error::Invalid(_))
    ));
}

#[test]
fn a_component_compiles_once() {
    let dir = temp("cache");
    let host = Host::new(Some(dir.clone())).unwrap();
    let bytes = wasm(TOOLS);
    assert!(!host.load(&bytes).unwrap().cached());
    let files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(files.len(), 1, "one compiled file");
    let p = host.load(&bytes).unwrap();
    assert!(p.cached(), "the second load reads the cache");
    let mut i = p
        .instantiate(&host, &host.linker::<()>(), (), Limits::default())
        .unwrap();
    assert_eq!(i.call::<(u32, u32), (u32,)>("add", (1, 2)).unwrap(), (3,));
    // A damaged file is compiled again, and replaced.
    std::fs::write(files[0].path(), b"not a compiled component").unwrap();
    assert!(!host.load(&bytes).unwrap().cached());
    assert!(host.load(&bytes).unwrap().cached());
    let _ = std::fs::remove_dir_all(dir);
}

/// Sets `file`'s modification time, the cache's time of last use, `days`
/// back.
fn used_ago(file: &std::path::Path, days: u64) {
    std::fs::File::options()
        .append(true)
        .open(file)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
        .unwrap();
}

#[test]
fn the_cache_keeps_what_is_used() {
    let dir = temp("used");
    std::fs::create_dir_all(&dir).unwrap();
    // Left by an engine of long ago, and by a write that never finished.
    let old = dir.join("an-old-engine.cwasm");
    let half = dir.join("a-write.tmp");
    for (f, days) in [(&old, 31), (&half, 2)] {
        std::fs::write(f, b"").unwrap();
        used_ago(f, days);
    }
    let host = Host::new(Some(dir.clone())).unwrap();
    let bytes = wasm(TOOLS);
    assert!(!host.load(&bytes).unwrap().cached());
    // Pruned as the new one was written (or by the host's start).
    let files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert_eq!(files[0].extension().unwrap(), "cwasm");
    // Loaded from the cache, a file is marked used now: pruned last.
    used_ago(&files[0], 10);
    assert!(host.load(&bytes).unwrap().cached());
    let at = std::fs::metadata(&files[0]).unwrap().modified().unwrap();
    assert!(at > SystemTime::now() - Duration::from_secs(60), "{at:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_endless_loop_is_stopped() {
    let host = Host::new(None).unwrap();
    let limits = Limits {
        time: Duration::from_millis(50),
        ..Limits::default()
    };
    let mut i = tools(&host)
        .instantiate(&host, &host.linker::<()>(), (), limits)
        .unwrap();
    let t = Instant::now();
    let out = i.call::<(), ()>("spin", ());
    let took = t.elapsed();
    assert!(matches!(out, Err(Error::Timeout(_))), "{out:?}");
    assert!(took >= Duration::from_millis(40), "{took:?}");
    assert!(took < Duration::from_secs(2), "{took:?}");
}

#[test]
fn an_instance_outliving_its_host_is_still_stopped() {
    let host = Host::new(None).unwrap();
    let limits = Limits {
        time: Duration::from_millis(50),
        ..Limits::default()
    };
    let mut i = tools(&host)
        .instantiate(&host, &host.linker::<()>(), (), limits)
        .unwrap();
    drop(host);
    let out = i.call::<(), ()>("spin", ());
    assert!(matches!(out, Err(Error::Timeout(_))), "{out:?}");
}

#[test]
fn memory_past_the_limit_is_refused() {
    let host = Host::new(None).unwrap();
    let p = tools(&host);
    let limits = Limits::default();
    let mut i = p
        .instantiate(&host, &host.linker::<()>(), (), limits)
        .unwrap();
    // 160 pages are 10 MB: allowed.
    assert_eq!(i.call::<(u32,), (u32,)>("grow", (160,)).unwrap(), (1,));
    // 1,600 more are 100 MB: past 64 MB.
    let out = i.call::<(u32,), (u32,)>("grow", (1600,));
    assert!(
        matches!(out, Err(Error::Memory(m)) if m == 64 << 20),
        "{out:?}"
    );
}

#[test]
fn only_what_is_granted_is_reachable() {
    let host = Host::new(None).unwrap();
    let p = host.load(&wasm(LOGS)).unwrap();
    assert_eq!(p.imports(&host), ["log"]);
    // Nothing granted: refused, naming what it asked for.
    let out = p.instantiate(
        &host,
        &host.linker::<Vec<u32>>(),
        Vec::new(),
        Limits::default(),
    );
    match out {
        Err(Error::NotGranted(names)) => assert_eq!(names, ["log"]),
        other => panic!("{other:?}"),
    }
    // Granted: the host's function runs with the host's data.
    let mut linker = host.linker::<Vec<u32>>();
    linker
        .root()
        .func_wrap("log", |mut cx, (x,): (u32,)| {
            cx.data_mut().user.push(x);
            Ok(())
        })
        .unwrap();
    let mut i = p
        .instantiate(&host, &linker, Vec::new(), Limits::default())
        .unwrap();
    i.call::<(u32,), ()>("run", (7,)).unwrap();
    i.call::<(u32,), ()>("run", (9,)).unwrap();
    assert_eq!(i.data(), &[7, 9]);
}

/// A component importing `log` and `logger`, one name inside the other.
const TWO: &str = r#"
(component
  (import "log" (func $log (param "x" u32)))
  (import "logger" (func $logger (param "x" u32)))
  (core func $a (canon lower (func $log)))
  (core func $b (canon lower (func $logger)))
  (core module $m
    (import "host" "log" (func (param i32)))
    (import "host" "logger" (func (param i32))))
  (core instance $i (instantiate $m (with "host" (instance
    (export "log" (func $a)) (export "logger" (func $b)))))))
"#;

#[test]
fn the_import_not_granted_is_named_exactly() {
    let host = Host::new(None).unwrap();
    let p = host.load(&wasm(TWO)).unwrap();
    let mut linker = host.linker::<()>();
    linker
        .root()
        .func_wrap("log", |_, (_,): (u32,)| Ok(()))
        .unwrap();
    match p.instantiate(&host, &linker, (), Limits::default()) {
        Err(Error::NotGranted(names)) => assert_eq!(names, ["logger"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn threads_share_a_plugin_each_with_an_instance() {
    let host = Host::new(None).unwrap();
    let p = tools(&host);
    let sums: Vec<u32> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..4u32)
            .map(|n| {
                let (host, p) = (&host, &p);
                s.spawn(move || {
                    let mut i = p
                        .instantiate(host, &host.linker::<()>(), (), Limits::default())
                        .unwrap();
                    i.call::<(u32, u32), (u32,)>("add", (n, 100)).unwrap().0
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert_eq!(sums, [100, 101, 102, 103]);
}

#[test]
fn not_a_component_is_refused() {
    let host = Host::new(None).unwrap();
    assert!(matches!(host.load(b"\0asm junk"), Err(Error::Invalid(_))));
}
