//! The least a host does: load the guest, call `spin`. Built once per
//! engine; the sizes against the build without one are what it adds.

fn main() {
    let path = std::env::args().nth(1).unwrap_or_default();
    let bytes = std::fs::read(&path).unwrap_or_default();
    println!("{}", run(&bytes));
}

#[cfg(any(feature = "cranelift", feature = "runtime", feature = "pulley-runtime"))]
fn run(bytes: &[u8]) -> u64 {
    use wasmtime::component::{Component, Linker};
    use wasmtime::{Config, Engine, Store};
    wasmtime::component::bindgen!({ world: "parser", path: "../wit" });
    let mut c = Config::new();
    if cfg!(feature = "pulley-runtime") {
        c.target("pulley64").unwrap();
    }
    let e = Engine::new(&c).unwrap();
    #[cfg(feature = "cranelift")]
    let component = Component::new(&e, bytes).unwrap();
    // With a second argument: the component precompiled, for the builds
    // that only load.
    #[cfg(feature = "cranelift")]
    if let Some(out) = std::env::args().nth(2) {
        std::fs::write(out, component.serialize().unwrap()).unwrap();
    }
    // SAFETY: a spike; the bytes are a component precompiled by this
    // version of wasmtime.
    #[cfg(not(feature = "cranelift"))]
    let component = unsafe { Component::deserialize(&e, bytes).unwrap() };
    let mut s = Store::new(&e, ());
    let p = Parser::instantiate(&mut s, &component, &Linker::new(&e)).unwrap();
    p.call_spin(&mut s, 10).unwrap()
}

#[cfg(feature = "wasmi")]
fn run(bytes: &[u8]) -> u64 {
    use wasm_component_layer::{Component, Engine, Linker, Store, Value};
    let e = Engine::new(wasmi_runtime_layer::Engine::default());
    let c = Component::new(&e, bytes).unwrap();
    let mut s = Store::new(&e, ());
    let i = Linker::default().instantiate(&mut s, &c).unwrap();
    let f = i.exports().root().func("spin").unwrap();
    let mut out = [Value::U64(0)];
    f.call(&mut s, &[Value::U64(10)], &mut out).unwrap();
    match out[0] {
        Value::U64(n) => n,
        _ => 0,
    }
}

#[cfg(not(any(
    feature = "cranelift",
    feature = "runtime",
    feature = "pulley-runtime",
    feature = "wasmi"
)))]
fn run(bytes: &[u8]) -> u64 {
    bytes.len() as u64
}
