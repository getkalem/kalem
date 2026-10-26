//! The BibTeX grid's commands: sorting the entries by a column, and the
//! file's order again. Registered with the other built-in commands
//! (`crate::builtin::commands`).

use serde_json::Value;

use crate::command::{Command, CommandError, EditorContext, Handler, arg_bool, cmd, scoped};

/// The commands of CSV documents (§2.6.2).
/// The commands of the BibTeX grid (T2.7h.19).
pub(crate) fn bib_commands() -> Vec<Command> {
    use crate::command::Scope;
    let c = |id: &str, title: &str, keys: &[&str], h: Handler| {
        scoped(
            cmd(id, title, "BibTeX", keys, None, h),
            Scope::only(&["bib"]),
        )
    };
    fn bib(ctx: &mut EditorContext<'_>) -> Result<(), CommandError> {
        if crate::bibtex::is_bib(ctx.doc()?) {
            Ok(())
        } else {
            Err(CommandError::new(crate::tr!("msg-not-bib")))
        }
    }
    vec![
        c(
            "bib.sortView",
            "Sort Entries by Column",
            &[],
            |ctx, args| {
                // The rows in the order of the column named, or of the field at
                // the cursor (again: descending); the file keeps its order.
                bib(ctx)?;
                let reverse = arg_bool(args, "reverse");
                let d = ctx.doc()?;
                let col = crate::bibtex::column(d, args.get("column").and_then(Value::as_str));
                let reverse = reverse || d.bib_sort() == Some((col, false));
                d.set_bib_sort(Some((col, reverse)));
                Ok(())
            },
        ),
        c("bib.unsortView", "File Order", &[], |ctx, _| {
            bib(ctx)?;
            ctx.doc()?.set_bib_sort(None);
            Ok(())
        }),
        c("bib.setField", "Set Field", &[], |ctx, args| {
            bib(ctx)?;
            let field = args
                .get("field")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let value = args.get("value").and_then(Value::as_str).unwrap_or("");
            if field.is_empty()
                || !field
                    .chars()
                    .all(|c| c.is_alphanumeric() || "-_:.".contains(c))
            {
                return Err(CommandError::new(crate::tr!("msg-bib-field-name")));
            }
            let now = ctx.now;
            let d = ctx.doc()?;
            let e = crate::bibtex::entry_at_cursor(d)
                .ok_or_else(|| CommandError::new(crate::tr!("msg-bib-no-entry")))?;
            let tx = crate::bibtex::set_field(d.text().as_str(), &e, &field, value);
            d.apply(&tx, org_edit::ChangeKind::Command, now);
            Ok(())
        }),
        c("bib.newEntry", "New Entry", &[], |ctx, args| {
            bib(ctx)?;
            let kind = args
                .get("type")
                .and_then(Value::as_str)
                .filter(|t| !t.trim().is_empty())
                .unwrap_or("article")
                .trim()
                .to_string();
            let key = args
                .get("key")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if key.is_empty() || key.contains([',', '{', '}', ' ']) {
                return Err(CommandError::new(crate::tr!("msg-bib-key")));
            }
            let now = ctx.now;
            let d = ctx.doc()?;
            let text = d.text().as_str();
            let g = crate::bibtex::grid(d);
            if g.entries.iter().any(|e| text[e.key.clone()] == key) {
                return Err(CommandError::new(crate::tr!(
                    "msg-bib-key-taken",
                    key = key
                )));
            }
            // After the entry at the cursor, else at the end.
            let pos = d.selection.head;
            let at = g
                .entries
                .iter()
                .find(|e| e.range.start <= pos && pos <= e.range.end)
                .map_or(text.len(), |e| e.range.end);
            let lead = if at == 0 || text[..at].ends_with("\n\n") {
                ""
            } else if text[..at].ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            let fields = ["author", "title", "year"];
            let body: String = fields.iter().map(|f| format!("  {f} = {{}},\n")).collect();
            let entry = format!("{lead}@{kind}{{{key},\n{body}}}\n");
            // The cursor in the first field's braces.
            let cursor = at + lead.len() + format!("@{kind}{{{key},\n  author = {{").len();
            let mut tx = org_edit::Transaction::new("New Entry");
            let _ = tx.insert(at, entry);
            tx.selection_after = Some(org_edit::Selection::caret(cursor));
            d.apply(&tx, org_edit::ChangeKind::Command, now);
            Ok(())
        }),
    ]
}
