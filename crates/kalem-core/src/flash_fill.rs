//! Flash Fill: a spreadsheet column's rest filled from a few examples
//! typed beside the data, the way the examples were made from it.
//!
//! An example is told by the cells of its row; what it is made of is
//! found among pieces of them: a cell whole, a word of it (counted from
//! its start or its end), a word's first letter, each as written, in
//! capitals, in small letters or capitalized, joined with text that is
//! the same in every example. The cheapest way of making every example
//! at once (fewest pieces, words before typed text) is the pattern.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Which part of a cell a piece takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    /// The whole text.
    Whole,
    /// A word (a run of letters and digits): from the start, or from the
    /// end when negative (-1 the last).
    Word(i32),
    /// A word's first letter.
    Initial(i32),
}

/// How a piece's letters are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Case {
    /// As the cell has them.
    Same,
    /// All capitals.
    Upper,
    /// All small letters.
    Lower,
    /// The first letter a capital, the rest small.
    Title,
}

/// A piece of the output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Text that is the same in every row.
    Text(String),
    /// Part of the cell in column `col` (of the row's inputs).
    Cell {
        /// Which input.
        col: usize,
        /// Which part of it.
        part: Part,
        /// In which case.
        case: Case,
    },
}

/// A pattern: its pieces, one after another.
pub type Pattern = Vec<Piece>;

fn words(s: &str) -> Vec<&str> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect()
}

fn recase(s: &str, case: Case) -> String {
    match case {
        Case::Same => s.to_owned(),
        Case::Upper => s.to_uppercase(),
        Case::Lower => s.to_lowercase(),
        Case::Title => {
            let mut c = s.chars();
            c.next().map_or_else(String::new, |f| {
                f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase()
            })
        }
    }
}

fn part(s: &str, part: Part) -> Option<String> {
    let pick = |i: i32| -> Option<&str> {
        let w = words(s);
        let k = if i >= 0 {
            i as usize
        } else {
            w.len().checked_sub(i.unsigned_abs() as usize)?
        };
        w.get(k).copied()
    };
    match part {
        Part::Whole => (!s.is_empty()).then(|| s.to_owned()),
        Part::Word(i) => pick(i).map(str::to_owned),
        Part::Initial(i) => pick(i).and_then(|w| w.chars().next()).map(String::from),
    }
}

/// A piece's text in a row, if the row has it.
fn piece(p: &Piece, inputs: &[String]) -> Option<String> {
    match p {
        Piece::Text(t) => Some(t.clone()),
        Piece::Cell {
            col,
            part: pt,
            case,
        } => part(inputs.get(*col)?, *pt).map(|v| recase(&v, *case)),
    }
}

/// A row's output by a pattern; `None` when the row lacks a piece of it.
pub fn apply(pattern: &[Piece], inputs: &[String]) -> Option<String> {
    pattern.iter().map(|p| piece(p, inputs)).collect()
}

/// A cost that orders a heap (smallest first).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cost(f64);

impl Eq for Cost {}

impl PartialOrd for Cost {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cost {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.total_cmp(&self.0)
    }
}

/// The cheapest pattern that makes every example's output from its row's
/// inputs; `None` when none does.
pub fn learn(examples: &[(Vec<String>, String)]) -> Option<Pattern> {
    if examples.is_empty() || examples.iter().any(|e| e.1.is_empty()) {
        return None;
    }
    let cols = examples.iter().map(|e| e.0.len()).max().unwrap_or(0);
    // Every piece a cell offers, and what it is in each example.
    let mut cells: Vec<(Piece, Vec<Option<String>>)> = Vec::new();
    let max_words = examples
        .iter()
        .flat_map(|e| e.0.iter().map(|s| words(s).len()))
        .max()
        .unwrap_or(0)
        .min(12) as i32;
    for col in 0..cols {
        let mut parts = vec![Part::Whole];
        for i in 0..max_words {
            parts.extend([
                Part::Word(i),
                Part::Word(-1 - i),
                Part::Initial(i),
                Part::Initial(-1 - i),
            ]);
        }
        for pt in parts {
            for case in [Case::Same, Case::Upper, Case::Lower, Case::Title] {
                let p = Piece::Cell {
                    col,
                    part: pt,
                    case,
                };
                let values: Vec<Option<String>> =
                    examples.iter().map(|e| piece(&p, &e.0)).collect();
                if values.iter().all(Option::is_some) && !cells.iter().any(|c| c.1 == values) {
                    cells.push((p, values));
                }
            }
        }
    }
    let outs: Vec<Vec<char>> = examples.iter().map(|e| e.1.chars().collect()).collect();
    let goal: Vec<usize> = outs.iter().map(Vec::len).collect();
    let start = vec![0usize; examples.len()];
    // The cheapest way to each state (a position in every output), and how
    // it was reached.
    let mut best: HashMap<Vec<usize>, f64> = HashMap::new();
    let mut from: HashMap<Vec<usize>, (Vec<usize>, Piece)> = HashMap::new();
    let mut heap = BinaryHeap::new();
    best.insert(start.clone(), 0.0);
    heap.push((Cost(0.0), start));
    let mut steps = 0;
    while let Some((Cost(cost), at)) = heap.pop() {
        if at == goal {
            // The pieces back from the goal, typed text run together.
            let mut pieces = Vec::new();
            let mut s = at;
            while let Some((prev, p)) = from.get(&s) {
                pieces.push(p.clone());
                s = prev.clone();
            }
            pieces.reverse();
            let mut out: Pattern = Vec::new();
            for p in pieces {
                match (out.last_mut(), p) {
                    (Some(Piece::Text(a)), Piece::Text(b)) => a.push_str(&b),
                    (_, p) => out.push(p),
                }
            }
            return Some(out);
        }
        if best.get(&at).is_some_and(|b| *b < cost) {
            continue;
        }
        steps += 1;
        if steps > 200_000 {
            return None;
        }
        let mut edges: Vec<(Vec<usize>, Piece, f64)> = Vec::new();
        // A cell's piece, where it comes next in every output.
        for (p, values) in &cells {
            let mut next = Vec::with_capacity(at.len());
            let ok = values.iter().zip(&outs).zip(&at).all(|((v, o), &i)| {
                let v: Vec<char> = v.as_deref().unwrap_or("").chars().collect();
                let fits = !v.is_empty() && o.len() >= i + v.len() && o[i..i + v.len()] == v[..];
                if fits {
                    next.push(i + v.len());
                }
                fits
            });
            if ok {
                let changed = matches!(p, Piece::Cell { case, .. } if *case != Case::Same);
                edges.push((next, p.clone(), if changed { 1.05 } else { 1.0 }));
            }
        }
        // Typed text: the same letter next in every output.
        if at.iter().zip(&outs).all(|(&i, o)| i < o.len()) {
            let c = outs[0][at[0]];
            if at.iter().zip(&outs).all(|(&i, o)| o[i] == c) {
                // Letters and digits cost more as typed text than marks do:
                // a pattern prefers to take words from the cells.
                let cost = if c.is_alphanumeric() { 1.5 } else { 0.3 };
                edges.push((
                    at.iter().map(|i| i + 1).collect(),
                    Piece::Text(c.to_string()),
                    cost,
                ));
            }
        }
        for (next, p, c) in edges {
            let nc = cost + c;
            if best.get(&next).is_none_or(|b| nc < *b) {
                best.insert(next.clone(), nc);
                from.insert(next.clone(), (at.clone(), p));
                heap.push((Cost(nc), next));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ex(inputs: &[&str], out: &str) -> (Vec<String>, String) {
        (
            inputs.iter().map(|s| s.to_string()).collect(),
            out.to_owned(),
        )
    }

    fn row(inputs: &[&str]) -> Vec<String> {
        inputs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn patterns_from_examples() {
        // An initial and the last name, from one example.
        let p = learn(&[ex(&["Ahmet Yılmaz"], "A. Yılmaz")]).unwrap();
        assert_eq!(
            apply(&p, &row(&["Zeynep Kaya"])).as_deref(),
            Some("Z. Kaya")
        );
        // Names out of an address, capitalized.
        let p = learn(&[ex(&["ahmet.yilmaz@firma.com"], "Ahmet Yilmaz")]).unwrap();
        assert_eq!(
            apply(&p, &row(&["zeynep.kaya@okul.edu"])).as_deref(),
            Some("Zeynep Kaya")
        );
        // A date's parts turned round.
        let p = learn(&[ex(&["2026-10-04"], "04/10/2026")]).unwrap();
        assert_eq!(
            apply(&p, &row(&["1999-01-31"])).as_deref(),
            Some("31/01/1999")
        );
        // Two columns joined.
        let p = learn(&[ex(&["Ahmet", "Yılmaz"], "Yılmaz, Ahmet")]).unwrap();
        assert_eq!(
            apply(&p, &row(&["Zeynep", "Kaya"])).as_deref(),
            Some("Kaya, Zeynep")
        );
        // Capitals.
        let p = learn(&[ex(&["istanbul"], "ISTANBUL")]).unwrap();
        assert_eq!(apply(&p, &row(&["ankara"])).as_deref(), Some("ANKARA"));
        // Two examples settle what one cannot: the second word, not the last.
        let p = learn(&[ex(&["Ali Veli Can"], "Veli"), ex(&["Ayşe Nur"], "Nur")]).unwrap();
        assert_eq!(apply(&p, &row(&["Ece Su Deniz"])).as_deref(), Some("Su"));
        // A row without the piece gets nothing.
        let p = learn(&[ex(&["Ahmet Yılmaz"], "Yılmaz")]).unwrap();
        assert!(apply(&p, &row(&[""])).is_none());
        // Typed text that is in no cell stays typed.
        let p = learn(&[ex(&["Kaya"], "Sn. Kaya")]).unwrap();
        assert_eq!(apply(&p, &row(&["Demir"])).as_deref(), Some("Sn. Demir"));
    }
}
