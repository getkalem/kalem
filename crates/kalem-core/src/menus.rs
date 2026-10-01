//! The menus of the graphical editor's menu bar (the system's on macOS,
//! the window's own elsewhere) and of the menu list both editors show on
//! F10: each menu's items, in the interface language, with the command
//! each runs.

use serde_json::json;

/// A menu.
#[derive(Debug, Clone)]
pub struct MenuSpec {
    /// Its title.
    pub name: String,
    /// Its items.
    pub entries: Vec<MenuEntry>,
}

/// An item of a menu.
#[derive(Debug, Clone)]
pub enum MenuEntry {
    /// A line between groups of items.
    Separator,
    /// A command of the registry, with its arguments.
    Command {
        /// The item's title.
        label: String,
        /// The command.
        id: &'static str,
        /// Its arguments, if any.
        args: Option<serde_json::Value>,
    },
    /// Open a file (a dialog in the graphical editor).
    Open(String),
    /// Add a folder to the projects (a dialog in the graphical editor).
    AddProjectFolder(String),
}

/// Every menu, in the interface language.
pub fn menus() -> Vec<MenuSpec> {
    use crate::l10n::{command_key, tr};
    // An item titled like its command, or with its own label.
    let item = |id: &'static str| MenuEntry::Command {
        label: tr(&command_key(id)),
        id,
        args: None,
    };
    let named = |label: String, id: &'static str| MenuEntry::Command {
        label,
        id,
        args: None,
    };
    let with = |label: String, id: &'static str, args: serde_json::Value| MenuEntry::Command {
        label,
        id,
        args: Some(args),
    };
    let heading = |level: u8| {
        with(
            crate::tr!("menu-heading", level = level.to_string()),
            "org.headline.setLevel",
            json!({ "level": level }),
        )
    };
    let section = |level: u8| {
        with(
            crate::tr!("menu-heading", level = level.to_string()),
            "latex.section.setLevel",
            json!({ "level": level }),
        )
    };
    vec![
        MenuSpec {
            name: "Kalem".to_string(),
            entries: vec![
                named(tr("menu-settings"), "app.settings"),
                MenuEntry::Separator,
                named(tr("menu-quit"), "app.quit"),
            ],
        },
        MenuSpec {
            name: tr("menu-file"),
            entries: vec![
                item("file.new"),
                item("file.newFromTemplate"),
                MenuEntry::Open(tr("menu-open")),
                item("file.recent"),
                MenuEntry::Separator,
                named(tr("menu-file-manager"), "dired.jump"),
                MenuEntry::Separator,
                item("export.dialog"),
                item("export.html"),
                item("export.markdown"),
                item("export.gfm"),
                item("export.latex"),
                item("export.pdf"),
                item("export.docx"),
                item("export.odt"),
                item("export.epub"),
                item("export.rtf"),
                item("export.text"),
                item("export.htmlSubtree"),
                item("export.markdownSubtree"),
                item("export.latexSubtree"),
                item("export.pdfSubtree"),
                item("file.import"),
                // LaTeX: the PDF, and the project through pandoc.
                item("latex.build"),
                item("latex.cancelBuild"),
                item("latex.export.html"),
                item("latex.export.markdown"),
                item("latex.export.docx"),
                item("latex.convertToOrg"),
                // CSV.
                item("csv.copyAsTsv"),
                item("csv.convertToOrg"),
                item("csv.openAsText"),
                MenuEntry::Separator,
                item("app.save"),
                named(tr("menu-save-as"), "app.saveAs"),
                item("app.revert"),
                item("file.reopenWithEncoding"),
                item("file.saveWithEncoding"),
                MenuEntry::Separator,
                item("file.print"),
                MenuEntry::Separator,
                item("file.close"),
            ],
        },
        MenuSpec {
            name: tr("menu-project"),
            entries: vec![
                item("project.switch"),
                item("project.findFile"),
                item("project.search"),
                item("project.recentFiles"),
                MenuEntry::Separator,
                named(tr("menu-projects-view"), "dired.projects"),
                item("dired.projectRoot"),
                MenuEntry::Separator,
                item("project.add"),
                MenuEntry::AddProjectFolder(tr("menu-add-project-folder")),
                item("project.remove"),
                item("project.rename"),
            ],
        },
        MenuSpec {
            name: tr("menu-edit"),
            entries: vec![
                item("edit.undo"),
                item("edit.redo"),
                MenuEntry::Separator,
                item("edit.cut"),
                item("edit.copy"),
                item("edit.copyRichText"),
                item("edit.copyHtml"),
                item("edit.paste"),
                item("edit.pastePlain"),
                item("edit.selectAll"),
                MenuEntry::Separator,
                // Code and plain text.
                item("edit.toggleComment"),
                item("edit.gotoBracket"),
                item("lines.moveUp"),
                item("lines.moveDown"),
                item("lines.duplicate"),
                item("lines.join"),
                item("lines.sort"),
                item("edit.trimTrailingWhitespace"),
                // LaTeX's diagnostics.
                item("latex.fix"),
                item("latex.problems"),
                item("latex.nextProblem"),
                item("latex.previousProblem"),
                MenuEntry::Separator,
                item("find.open"),
                item("find.replace"),
            ],
        },
        MenuSpec {
            name: tr("menu-format"),
            entries: vec![
                item("org.emphasis.bold"),
                item("org.emphasis.italic"),
                item("org.emphasis.underline"),
                item("org.emphasis.strikeThrough"),
                item("org.emphasis.code"),
                MenuEntry::Separator,
                heading(1),
                heading(2),
                heading(3),
                with(
                    tr("menu-body-text"),
                    "org.headline.setLevel",
                    json!({"level": 0}),
                ),
                MenuEntry::Separator,
                item("org.todo.cycle"),
                named(tr("menu-schedule"), "org.schedule"),
                named(tr("menu-deadline"), "org.deadline"),
                named(tr("menu-properties"), "org.property.edit"),
                named(tr("menu-refile"), "org.refile"),
                named(tr("menu-archive-sibling"), "org.archive.sibling"),
                named(tr("menu-archive-tag"), "org.archive.toggleTag"),
                item("list.toggleCheckbox"),
                MenuEntry::Separator,
                // LaTeX.
                item("latex.format.bold"),
                item("latex.format.italic"),
                item("latex.format.underline"),
                item("latex.format.code"),
                MenuEntry::Separator,
                section(1),
                section(2),
                section(3),
                with(
                    tr("menu-body-text"),
                    "latex.section.setLevel",
                    json!({"level": 0}),
                ),
                MenuEntry::Separator,
                item("latex.section.promote"),
                item("latex.section.demote"),
                item("latex.section.moveUp"),
                item("latex.section.moveDown"),
                MenuEntry::Separator,
                item("latex.math.toggleDisplay"),
                item("latex.math.toggleNumbering"),
            ],
        },
        MenuSpec {
            name: tr("menu-insert"),
            entries: vec![
                named(tr("menu-link"), "org.insert.link"),
                named(tr("menu-citation"), "org.cite.insert"),
                named(tr("menu-footnote"), "org.footnote.new"),
                named(tr("menu-drawer"), "org.insert.drawer"),
                named(tr("menu-reference"), "org.insert.reference"),
                named(tr("menu-caption"), "org.caption.set"),
                named(tr("menu-name"), "org.name.set"),
                with(
                    tr("menu-table"),
                    "table.create",
                    json!({"columns": 3, "rows": 2}),
                ),
                named(tr("menu-timestamp"), "org.insert.timestamp"),
                named(tr("menu-date"), "org.insert.date"),
                with(
                    tr("menu-source-block"),
                    "org.insert.block",
                    json!({"type": "src"}),
                ),
                named(tr("menu-horizontal-rule"), "org.insert.horizontalRule"),
                // LaTeX.
                named(tr("menu-citation"), "latex.insert.citation"),
                item("latex.insert.equation"),
                item("latex.insert.figure"),
                with(
                    tr("menu-table"),
                    "latex.insert.table",
                    json!({"columns": 3, "rows": 2}),
                ),
            ],
        },
        MenuSpec {
            // CSV files: rows and columns.
            name: tr("menu-table"),
            entries: vec![
                item("csv.insertRow"),
                item("csv.deleteRow"),
                item("csv.moveRowUp"),
                item("csv.moveRowDown"),
                MenuEntry::Separator,
                item("csv.insertColumn"),
                item("csv.deleteColumn"),
                item("csv.moveColumnLeft"),
                item("csv.moveColumnRight"),
                MenuEntry::Separator,
                item("csv.filter"),
                item("csv.clearFilter"),
                item("csv.sortView"),
                item("csv.unsortView"),
                item("csv.sortFile"),
                item("csv.sortFileBy"),
                MenuEntry::Separator,
                item("csv.fillDown"),
                item("csv.fillSeries"),
                item("csv.splitColumn"),
                item("csv.joinColumns"),
                item("csv.removeDuplicates"),
                item("csv.transpose"),
                MenuEntry::Separator,
                item("csv.sumColumn"),
                item("csv.cellCoordinates"),
                item("csv.goToCell"),
            ],
        },
        MenuSpec {
            // BibTeX files: the entries as a grid.
            name: tr("menu-bibtex"),
            entries: vec![
                item("bib.newEntry"),
                item("bib.setField"),
                MenuEntry::Separator,
                with(
                    tr("menu-sort-author"),
                    "bib.sortView",
                    json!({"column": "author"}),
                ),
                with(
                    tr("menu-sort-year"),
                    "bib.sortView",
                    json!({"column": "year"}),
                ),
                with(
                    tr("menu-sort-title"),
                    "bib.sortView",
                    json!({"column": "title"}),
                ),
                with(
                    tr("menu-sort-key"),
                    "bib.sortView",
                    json!({"column": "key"}),
                ),
                item("bib.unsortView"),
            ],
        },
        MenuSpec {
            name: tr("menu-view"),
            entries: vec![
                item("view.fold"),
                item("view.foldAll"),
                named(tr("menu-source-view"), "view.toggleSource"),
                item("view.split"),
                MenuEntry::Separator,
                item("view.openFiles"),
                item("file.switch"),
                item("file.next"),
                item("file.previous"),
                MenuEntry::Separator,
                item("view.outline"),
                item("stats.chapters"),
                item("edit.gotoLine"),
                item("view.palette"),
            ],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_is_a_command() {
        let reg = crate::command::CommandRegistry::with_builtins();
        for m in menus() {
            for e in m.entries {
                if let MenuEntry::Command { id, .. } = e {
                    assert!(reg.get(id).is_some(), "{} › {id}", m.name);
                }
            }
        }
    }

    #[test]
    fn the_file_menu_has_what_the_book_puts_there() {
        let file = &menus()[1];
        let ids: Vec<_> = file
            .entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Command { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        for id in [
            "file.print",
            "file.import",
            "file.newFromTemplate",
            "export.latex",
            "export.pdf",
            "export.text",
            "export.docx",
        ] {
            assert!(ids.contains(&id), "{id}");
        }
    }
}
