//! The licences of the syntax definitions and themes the highlighter
//! embeds (through `two-face`), as Markdown, for
//! `tools/third-party-licenses.sh`.

#![allow(clippy::print_stdout)]

fn main() {
    print!("{}", two_face::acknowledgement::listing().to_md());
}
