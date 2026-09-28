//! Exports a file: `cargo run --example export_file -- FILE [html|md]`.

#![allow(clippy::print_stdout, clippy::print_stderr)]

fn main() {
    let mut args = std::env::args().skip(1);
    let file = args.next().expect("a file");
    let backend = args.next().unwrap_or_else(|| "html".into());
    let text = std::fs::read_to_string(&file).expect("readable");
    let b: &dyn org_export::Backend = match backend.as_str() {
        "md" => &org_export::Markdown,
        _ => &org_export::Html,
    };
    let t = std::time::Instant::now();
    let out = org_export::export(
        &text,
        b,
        &org_export::Settings {
            body_only: true,
            input_file: Some(file.into()),
            now: None,
            subtree: None,
        },
    );
    eprintln!("{:.2}s", t.elapsed().as_secs_f64());
    match out {
        Ok(s) => print!("{s}"),
        Err(e) => eprintln!("error: {e}"),
    }
}
