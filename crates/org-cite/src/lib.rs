//! Org mode citations (`oc.el`): bibliographies read from BibTeX,
//! BibLaTeX and CSL-JSON files as Org's basic processor reads them.

pub mod bib;

pub use bib::{Bibliography, Entry};
