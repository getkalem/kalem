//! Table formulas at the cursor (design §8.3): the formula of the field,
//! shown in the status bar and edited with Edit Formula, and the fields
//! it refers to, highlighted.

use org_edit::recalc::FormulaInfo;

use crate::document::DocumentState;

/// The formula at the cursor, computed again only when the text or the
/// cursor changes.
#[derive(Debug, Clone, Default)]
pub struct FormulaCache {
    key: Option<(u64, usize)>,
    info: Option<FormulaInfo>,
}

/// Whether the cursor's line is a table line.
fn in_table(doc: &DocumentState) -> bool {
    let text = doc.text().as_str();
    let head = doc.selection.head.min(text.len());
    let bol = text[..head].rfind('\n').map_or(0, |i| i + 1);
    text[bol..].trim_start_matches([' ', '\t']).starts_with('|')
}

impl FormulaCache {
    /// The formula of the field at the cursor, if the cursor is in a
    /// table row of an Org document.
    pub fn get(&mut self, doc: &mut DocumentState) -> Option<&FormulaInfo> {
        let key = (doc.version(), doc.selection.head);
        if self.key != Some(key) {
            self.key = Some(key);
            self.info = if in_table(doc) {
                let head = doc.selection.head;
                doc.model()
                    .and_then(|m| org_edit::recalc::formula_info(&m, head))
            } else {
                None
            };
        }
        self.info.as_ref()
    }
}

/// What the status bar says about a formula: `$3 = $1*$2`, with why the
/// field shows `#ERROR` or that an Emacs Lisp formula is not computed.
pub fn status(info: &FormulaInfo) -> Option<String> {
    if info.rhs.is_empty() {
        return None;
    }
    let mut s = format!("{} = {}", info.lhs, info.rhs);
    if info.lisp {
        s.push_str("   ");
        s.push_str(&crate::l10n::tr("status-formula-lisp"));
    } else if let Some(why) = &info.error {
        s.push_str("   ");
        s.push_str(&crate::tr!("status-formula-error", why = why));
    }
    Some(s)
}

/// The fields a formula refers to, in order, for highlighting.
pub fn references(info: &FormulaInfo) -> Vec<std::ops::Range<usize>> {
    let mut r = info.references.clone();
    r.sort_by_key(|x| (x.start, x.end));
    r
}

/// What Edit Formula starts with: `=` and the column formula, or `:=`
/// and the field formula, as typed in a field in Emacs.
pub fn prompt(info: Option<&FormulaInfo>) -> String {
    match info {
        Some(i) if i.field => format!(":={}", i.rhs),
        Some(i) => format!("={}", i.rhs),
        None => "=".into(),
    }
}

/// The fields of a table line: their ranges in the line, `None` for a
/// rule line.
fn fields(line: &str) -> Option<Vec<std::ops::Range<usize>>> {
    let t = line.trim_start_matches([' ', '\t']);
    if !t.starts_with('|') || t.starts_with("|-") {
        return None;
    }
    let bars: Vec<usize> = line.match_indices('|').map(|(i, _)| i).collect();
    let mut out: Vec<std::ops::Range<usize>> = bars.windows(2).map(|w| w[0] + 1..w[1]).collect();
    // A last field without its closing bar.
    if let Some(&last) = bars.last()
        && !line[last + 1..].trim().is_empty()
    {
        out.push(last + 1..line.len());
    }
    Some(out)
}

/// A number as a table shows it: `3`, `2.5`, `-0.125`.
pub(crate) fn number(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        let s = format!("{x:.6}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// What the status bar says about the fields of a table the selection
/// spans, as a spreadsheet does (the rectangle from the anchor's field to
/// the cursor's): how many are filled, and the sum, average, smallest and
/// largest of the numbers among them. `None` without such a selection.
pub fn selection_stats(doc: &DocumentState) -> Option<String> {
    // CSV: the numbers of the column at the cursor.
    if doc.meta.mode == crate::DocumentMode::Csv {
        return crate::csv::status(doc);
    }
    // BibTeX: how many entries, and the sort.
    if crate::bibtex::is_bib(doc) {
        return crate::bibtex::status(doc);
    }
    // A language pack's diagnostics (T2.7a.7).
    if let Some(s) = crate::packs::status(doc) {
        return Some(s);
    }
    // A language server's: the problem on the cursor's line, its work in
    // progress, or the counts (D57).
    if let Some(p) = &doc.meta.path
        && let Some(s) = crate::lsp::status(p, doc.selection.head)
    {
        return Some(s);
    }
    let sel = doc.selection;
    if sel.anchor == sel.head {
        return None;
    }
    let text = doc.text();
    let (la, lh) = (text.line_of(sel.anchor), text.line_of(sel.head));
    let (l1, l2) = (la.min(lh), la.max(lh));
    // Rows of one table, rules skipped.
    let line = |l: usize| &text.as_str()[text.line_range(l)];
    let is_table = |l: usize| line(l).trim_start_matches([' ', '\t']).starts_with('|');
    if !(l1..=l2).all(is_table) {
        return None;
    }
    let column = |l: usize, pos: usize| -> Option<usize> {
        let at = pos - text.line_range(l).start;
        fields(line(l))?
            .iter()
            .position(|r| r.start <= at && at <= r.end)
    };
    let (ca, ch) = (column(la, sel.anchor)?, column(lh, sel.head)?);
    let (c1, c2) = (ca.min(ch), ca.max(ch));
    let mut count = 0usize;
    let mut nums: Vec<f64> = Vec::new();
    for l in l1..=l2 {
        let Some(fs) = fields(line(l)) else { continue };
        for r in fs.iter().skip(c1).take(c2 - c1 + 1) {
            let v = line(l)[r.clone()].trim();
            if v.is_empty() {
                continue;
            }
            count += 1;
            if let Ok(x) = v.parse::<f64>()
                && x.is_finite()
            {
                nums.push(x);
            }
        }
    }
    if count < 2 {
        return None;
    }
    let mut s = crate::tr!("status-table-count", count = count);
    if !nums.is_empty() {
        let sum: f64 = nums.iter().sum();
        let min = nums.iter().copied().fold(f64::INFINITY, f64::min);
        let max = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        s.push_str("   ");
        s.push_str(&crate::tr!(
            "status-table-numbers",
            sum = number(sum),
            average = number(sum / nums.len() as f64),
            min = number(min),
            max = number(max)
        ));
    }
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DocumentMode, LineEnding, Metadata};

    fn stats(text: &str, from: &str, to: &str) -> Option<String> {
        crate::l10n::set_language("en");
        let meta = Metadata {
            path: None,
            mode: DocumentMode::Org,
            line_ending: LineEnding::Lf,
            bom: false,
            encoding: encoding_rs::UTF_8,
            lossy: false,
        };
        let mut d = DocumentState::new(text, meta, std::sync::Arc::default());
        d.selection = org_edit::Selection {
            anchor: text.find(from).unwrap(),
            head: text.find(to).unwrap(),
        };
        selection_stats(&d)
    }

    #[test]
    fn statistics_of_selected_fields() {
        let t = "| item | n | price |\n|------+---+-------|\n| a    | 2 |   1.5 |\n| b    | 4 |       |\n| c    | x |     3 |\n";
        // The column n from row a to row c: 2, 4 and x.
        assert_eq!(
            stats(t, "2 |", "x |").as_deref(),
            Some("Count: 3   Sum: 6   Average: 3   Min: 2   Max: 4")
        );
        // A rectangle over two columns, across the empty field.
        assert_eq!(
            stats(t, "2 |", "3 |").as_deref(),
            Some("Count: 5   Sum: 10.5   Average: 2.625   Min: 1.5   Max: 4")
        );
        // Text only: a count.
        assert_eq!(stats(t, "a  ", "c  ").as_deref(), Some("Count: 3"));
        // One field, or outside a table: nothing.
        assert_eq!(stats(t, "2 |", " 2 |"), None);
        assert_eq!(stats("x y\n| 1 |\n", "x", "1"), None);
    }
}
