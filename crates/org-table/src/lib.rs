//! Org mode tables: `#+TBLFM` formulas (design §8.2).
//!
//! Emacs evaluates a formula by substituting the referenced fields into
//! its text and handing the result to Calc. This crate follows the same
//! steps, with the part of Calc that table formulas use, so that a table
//! recalculated here reads as Emacs would leave it.

pub mod calc;
pub mod csv;
pub mod emacs;
pub mod formula;
pub mod recalc;
pub mod table;
pub mod tblfm;
