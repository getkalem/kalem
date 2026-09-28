//! Org mode citations (`oc.el`): bibliographies read from BibTeX,
//! BibLaTeX and CSL-JSON files as Org's basic processor reads them.

pub mod bib;
pub mod csl;

pub use bib::{Bibliography, Entry};
