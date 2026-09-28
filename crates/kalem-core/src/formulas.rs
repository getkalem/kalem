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
