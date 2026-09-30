//! Prints the syntaxes the highlighter knows and the extensions they
//! claim, as the Org table of the Book's chapter on plain text.

#![allow(clippy::print_stdout)]

fn main() {
    let set = two_face::syntax::extra_newlines();
    let mut rows: Vec<(String, String)> = set
        .syntaxes()
        .iter()
        .filter(|s| s.name != "Plain Text")
        .map(|s| {
            let exts: Vec<String> = s.file_extensions.iter().map(|e| format!("={e}=")).collect();
            (s.name.clone(), exts.join(" "))
        })
        .collect();
    rows.sort_by_key(|r| r.0.to_lowercase());
    println!("| Syntax | Extensions |\n|-|-|");
    for (name, exts) in rows {
        println!(
            "| {} | {} |",
            name.replace('|', "\\vert{}"),
            exts.replace('|', "\\vert{}")
        );
    }
}
