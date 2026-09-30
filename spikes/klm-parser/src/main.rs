//! `klm-parser-spike parse|fmt|html|check FILE`, `examples OUTDIR
//! CHAPTER.org…` (the suite's files from Part III's examples).

#![allow(clippy::print_stdout, clippy::print_stderr)]

use klm_parser_spike as klm;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{p}: {e}"));
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["parse", f] => {
            let doc = klm::parse(&read(f));
            println!(
                "{}",
                serde_json::to_string_pretty(&klm::model(&doc)).unwrap()
            );
        }
        ["fmt", f] => print!("{}", klm::fmt(&klm::parse(&read(f)))),
        ["html", f] => print!("{}", klm::html(&klm::parse(&read(f)))),
        ["check", files @ ..] => {
            let mut bad = false;
            for f in files {
                let text = read(f);
                let doc = klm::parse(&text);
                for d in &doc.diagnostics {
                    let line = text[..d.range.0].matches('\n').count() + 1;
                    println!("{f}:{line}: {}: {}", d.code, d.message);
                    bad = true;
                }
                let canonical = klm::fmt(&doc);
                if canonical != text {
                    println!("{f}: not in canonical form");
                }
            }
            if bad {
                return std::process::ExitCode::FAILURE;
            }
        }
        ["examples", out, sources @ ..] => {
            let _ = std::fs::remove_dir_all(out);
            std::fs::create_dir_all(out).unwrap();
            let mut all = Vec::new();
            for f in sources {
                let stem = std::path::Path::new(f)
                    .file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                let found = if f.ends_with(".org") {
                    klm::org_examples(&read(f))
                } else {
                    klm::examples(&read(f))
                };
                for (i, (_, src)) in found.into_iter().enumerate() {
                    all.push((format!("{stem}-{}", i + 1), src));
                }
            }
            for (base, src) in all {
                let name = format!("{out}/{base}");
                let doc = klm::parse(&src);
                std::fs::write(format!("{name}.klm"), &src).unwrap();
                std::fs::write(
                    format!("{name}.json"),
                    serde_json::to_string_pretty(&klm::model(&doc)).unwrap() + "\n",
                )
                .unwrap();
                std::fs::write(format!("{name}.canonical.klm"), klm::fmt(&doc)).unwrap();
                std::fs::write(format!("{name}.html"), klm::html(&doc)).unwrap();
            }
        }
        _ => {
            eprintln!(
                "usage: klm-parser-spike parse|fmt|html|check FILE… | examples OUTDIR CHAPTER.org…"
            );
            return std::process::ExitCode::from(2);
        }
    }
    std::process::ExitCode::SUCCESS
}
