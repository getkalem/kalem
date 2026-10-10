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
    /// Where it shows, a plugin's menu: a when-clause on the document
    /// (`vcs == git`); everywhere when none.
    pub when: Option<crate::when::WhenClause>,
}

impl MenuSpec {
    /// Whether it shows for a document of context `ctx`.
    pub fn shows(&self, ctx: &crate::when::Context) -> bool {
        self.when.as_ref().is_none_or(|w| w.eval(ctx))
    }
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
        id: String,
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
        id: id.to_string(),
        args: None,
    };
    let named = |label: String, id: &'static str| MenuEntry::Command {
        label,
        id: id.to_string(),
        args: None,
    };
    let with = |label: String, id: &'static str, args: serde_json::Value| MenuEntry::Command {
        label,
        id: id.to_string(),
        args: Some(args),
    };
    let heading = |level: u8| {
        with(
            crate::tr!("menu-heading", level = level.to_string()),
            "org.headline.setLevel",
            json!({ "level": level }),
        )
    };
    // A workbook's commands (`viewer.grid.*`), titled as in the palette;
    // they show only for a document of cells, as every item shows only
    // where its command serves.
    let grid = |ids: &[&'static str]| -> Vec<MenuEntry> {
        ids.iter()
            .map(|id| match *id {
                "-" => MenuEntry::Separator,
                id => item(id),
            })
            .collect()
    };
    let section = |level: u8| {
        with(
            crate::tr!("menu-heading", level = level.to_string()),
            "latex.section.setLevel",
            json!({ "level": level }),
        )
    };
    let mut menus = vec![
        MenuSpec {
            when: None,
            name: "Kalem".to_string(),
            entries: vec![
                named(tr("menu-settings"), "app.settings"),
                MenuEntry::Separator,
                item("plugin.browse"),
                item("plugin.install"),
                item("plugin.installGitHub"),
                item("plugin.list"),
                item("plugin.sources"),
                MenuEntry::Separator,
                named(tr("menu-quit"), "app.quit"),
            ],
        },
        MenuSpec {
            when: None,
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
                item("latex.showInPdf"),
                item("latex.cancelBuild"),
                item("latex.export.html"),
                item("latex.export.markdown"),
                item("latex.export.docx"),
                item("latex.convertToOrg"),
                // CSV.
                item("csv.copyAsTsv"),
                item("csv.convertToOrg"),
                item("csv.openAsText"),
                // Markdown.
                item("markdown.convertToOrg"),
                // Workbooks.
                item("viewer.grid.saveSheetAsCsv"),
                item("viewer.grid.exportPdf"),
                MenuEntry::Separator,
                item("app.save"),
                named(tr("menu-save-as"), "app.saveAs"),
                item("app.revert"),
                item("file.reopenWithEncoding"),
                item("file.saveWithEncoding"),
                MenuEntry::Separator,
                item("file.print"),
                item("viewer.grid.pageSetup"),
                item("viewer.grid.printPreview"),
                item("viewer.grid.print"),
                MenuEntry::Separator,
                item("file.close"),
            ],
        },
        MenuSpec {
            when: None,
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
                MenuEntry::Separator,
                item("project.toggleAutoAdd"),
            ],
        },
        MenuSpec {
            when: None,
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
                item("viewer.grid.pasteSpecial"),
                item("viewer.grid.insertCopiedCells"),
                item("edit.selectAll"),
                item("viewer.grid.selectAll"),
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
                // A workbook's cells: filled, cleared and deleted.
                item("viewer.grid.fillDown"),
                item("viewer.grid.fillRight"),
                item("viewer.grid.series"),
                item("viewer.grid.fillJustify"),
                item("viewer.grid.flashFill"),
                MenuEntry::Separator,
                item("viewer.grid.clear"),
                item("viewer.grid.clearFormats"),
                item("viewer.grid.clearAll"),
                MenuEntry::Separator,
                item("viewer.grid.deleteCells"),
                item("viewer.grid.deleteRow"),
                item("viewer.grid.deleteColumn"),
                item("viewer.grid.deleteSheet"),
                MenuEntry::Separator,
                item("find.open"),
                item("find.replace"),
                item("viewer.grid.find"),
                item("viewer.grid.replace"),
                item("viewer.grid.findAll"),
                item("viewer.grid.findNext"),
                item("viewer.grid.findPrevious"),
                MenuEntry::Separator,
                item("viewer.grid.goTo"),
                item("viewer.grid.goToSpecial"),
                item("viewer.grid.selectVisible"),
            ],
        },
        MenuSpec {
            when: None,
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
                MenuEntry::Separator,
                // Documents of flowing text (a Word document).
                item("flow.format.bold"),
                item("flow.format.italic"),
                item("flow.format.underline"),
                item("flow.format.strikeThrough"),
                item("flow.format.superscript"),
                item("flow.format.subscript"),
                MenuEntry::Separator,
                item("flow.format.font"),
                item("flow.format.fontSize"),
                item("flow.format.color"),
                item("flow.format.highlight"),
                MenuEntry::Separator,
                item("flow.format.style"),
                item("flow.format.clear"),
                MenuEntry::Separator,
                item("flow.paragraph.alignLeft"),
                item("flow.paragraph.alignCenter"),
                item("flow.paragraph.alignRight"),
                item("flow.paragraph.justify"),
                MenuEntry::Separator,
                item("flow.list.bullets"),
                item("flow.list.numbering"),
                item("flow.list.numberingStyle"),
                item("flow.paragraph.increaseIndent"),
                item("flow.paragraph.decreaseIndent"),
                MenuEntry::Separator,
                item("flow.paragraph.lineSpacing"),
                item("flow.paragraph.spaceBefore"),
                item("flow.paragraph.spaceAfter"),
                item("flow.paragraph.clear"),
            ]
            .into_iter()
            // A workbook's cells, rows, columns and sheets, as Excel's Home
            // tab has them.
            .chain(grid(&[
                "-",
                "viewer.grid.formatCells",
                "-",
                "viewer.grid.bold",
                "viewer.grid.italic",
                "viewer.grid.underline",
                "viewer.grid.strikethrough",
                "viewer.grid.fontFace",
                "viewer.grid.fontSize",
                "viewer.grid.fontColor",
                "viewer.grid.fillColor",
                "viewer.grid.fillEffect",
                "-",
                "viewer.grid.alignLeft",
                "viewer.grid.alignCenter",
                "viewer.grid.alignRight",
                "viewer.grid.alignTop",
                "viewer.grid.alignMiddle",
                "viewer.grid.alignBottom",
                "viewer.grid.increaseIndent",
                "viewer.grid.decreaseIndent",
                "viewer.grid.textRotation",
                "viewer.grid.wrapText",
                "viewer.grid.shrinkToFit",
                "viewer.grid.mergeCenter",
                "viewer.grid.merge",
                "viewer.grid.unmerge",
                "viewer.grid.centerAcrossSelection",
                "-",
                "viewer.grid.borders",
                "viewer.grid.borderLine",
                "viewer.grid.borderColor",
                "-",
                "viewer.grid.numberFormat",
                "viewer.grid.increaseDecimal",
                "viewer.grid.decreaseDecimal",
                "-",
                "viewer.grid.cellStyle",
                "viewer.grid.formatAsTable",
                "viewer.grid.conditionalFormat",
                "viewer.grid.clearConditionalFormats",
                "viewer.grid.clearSheetConditionalFormats",
                "viewer.grid.formatPainter",
                "-",
                "viewer.grid.rowHeight",
                "viewer.grid.fitRowHeight",
                "viewer.grid.tallerRow",
                "viewer.grid.shorterRow",
                "viewer.grid.columnWidth",
                "viewer.grid.autofitColumn",
                "viewer.grid.autofitColumns",
                "viewer.grid.widenColumn",
                "viewer.grid.narrowColumn",
                "-",
                "viewer.grid.hideRows",
                "viewer.grid.unhideRows",
                "viewer.grid.hideColumns",
                "viewer.grid.unhideColumns",
                "-",
                "viewer.grid.renameSheet",
                "viewer.grid.moveOrCopySheet",
                "viewer.grid.moveSheetLeft",
                "viewer.grid.moveSheetRight",
                "viewer.grid.tabColor",
                "viewer.grid.hideSheet",
                "viewer.grid.unhideSheet",
                "-",
                "viewer.grid.theme",
                "viewer.grid.lockCells",
            ]))
            .collect(),
        },
        // Comments and tracked changes, of a document that has them.
        MenuSpec {
            when: None,
            name: tr("menu-review"),
            entries: vec![
                item("flow.comment.new"),
                item("flow.comment.reply"),
                item("flow.comment.edit"),
                item("flow.comment.resolve"),
                item("flow.comment.delete"),
                MenuEntry::Separator,
                item("flow.comment.previous"),
                item("flow.comment.next"),
                MenuEntry::Separator,
                item("flow.change.accept"),
                item("flow.change.reject"),
                item("flow.change.acceptAll"),
                item("flow.change.rejectAll"),
                item("flow.change.previous"),
                item("flow.change.next"),
                MenuEntry::Separator,
                item("flow.trackChanges"),
            ]
            .into_iter()
            // A workbook's spelling, comments, notes and protection.
            .chain(grid(&[
                "viewer.grid.spelling",
                "-",
                "viewer.grid.newComment",
                "viewer.grid.comments",
                "viewer.grid.editNote",
                "viewer.grid.deleteNote",
                "-",
                "viewer.grid.protectSheet",
                "viewer.grid.protectWorkbook",
            ]))
            .collect(),
        },
        MenuSpec {
            when: None,
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
            ]
            .into_iter()
            // A workbook's cells, sheets, functions and objects.
            .chain(grid(&[
                "viewer.grid.insertCells",
                "viewer.grid.insertRow",
                "viewer.grid.insertColumn",
                "viewer.grid.insertSheet",
                "-",
                "viewer.grid.insertFunction",
                "viewer.grid.autoSum",
                "-",
                "viewer.grid.insertChart",
                "viewer.grid.insertPivot",
                "viewer.grid.insertSparklines",
                "viewer.grid.insertSlicer",
                "viewer.grid.insertPicture",
                "viewer.grid.insertShape",
                "-",
                "viewer.grid.insertLink",
                "viewer.grid.newComment",
                "viewer.grid.editNote",
                "-",
                "viewer.grid.insertDate",
                "viewer.grid.insertTime",
            ]))
            .collect(),
        },
        MenuSpec {
            // CSV files: rows and columns.
            when: None,
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
                MenuEntry::Separator,
                item("csv.toggleAlignment"),
                item("csv.toggleRainbow"),
                item("csv.toggleCoordinates"),
                item("csv.toggleFrozen"),
                MenuEntry::Separator,
                item("csv.hideColumn"),
                item("csv.showColumns"),
                item("csv.autosizeColumn"),
                item("csv.autosizeColumns"),
                item("csv.widenColumn"),
                item("csv.narrowColumn"),
                item("csv.resetWidths"),
                MenuEntry::Separator,
                item("csv.copyCells"),
                item("csv.cutCells"),
                item("csv.pasteBlock"),
                item("csv.recordView"),
                item("csv.frequencies"),
                item("csv.histogram"),
            ],
        },
        MenuSpec {
            // Workbooks: Excel's Data tab.
            when: None,
            name: tr("menu-data"),
            entries: grid(&[
                "viewer.grid.sortAscending",
                "viewer.grid.sortDescending",
                "viewer.grid.customSort",
                "viewer.grid.sortByColor",
                "-",
                "viewer.grid.toggleFilter",
                "viewer.grid.filterCondition",
                "viewer.grid.filterByColor",
                "viewer.grid.clearFilters",
                "viewer.grid.reapplyFilter",
                "viewer.grid.advancedFilter",
                "-",
                "viewer.grid.textToColumns",
                "viewer.grid.removeDuplicates",
                "viewer.grid.dataValidation",
                "viewer.grid.circleInvalid",
                "viewer.grid.clearValidation",
                "-",
                "viewer.grid.group",
                "viewer.grid.ungroup",
                "viewer.grid.showDetail",
                "viewer.grid.hideDetail",
                "viewer.grid.subtotal",
                "-",
                "viewer.grid.goalSeek",
                "viewer.grid.dataTable",
                "viewer.grid.scenarios",
                "-",
                "viewer.grid.refreshPivots",
                "viewer.grid.pivotOptions",
                "viewer.grid.slicer",
                "viewer.grid.totalRow",
                "viewer.grid.convertToRange",
                "-",
                "viewer.grid.customLists",
            ]),
        },
        MenuSpec {
            // Workbooks: Excel's Formulas tab.
            when: None,
            name: tr("menu-formulas"),
            entries: grid(&[
                "viewer.grid.insertFunction",
                "viewer.grid.autoSum",
                "-",
                "viewer.grid.defineName",
                "viewer.grid.nameManager",
                "viewer.grid.deleteName",
                "-",
                "viewer.grid.tracePrecedents",
                "viewer.grid.traceDependents",
                "viewer.grid.removeArrows",
                "viewer.grid.showFormulas",
                "viewer.grid.errorChecking",
                "viewer.grid.evaluateFormula",
                "viewer.grid.watchWindow",
                "viewer.grid.circularReferences",
                "-",
                "viewer.grid.calculateNow",
                "viewer.grid.calculationOptions",
            ]),
        },
        MenuSpec {
            // Workbooks: the chart at the cursor, Excel's Chart Design and
            // Format tabs.
            when: None,
            name: tr("menu-chart"),
            entries: grid(&[
                "viewer.grid.insertChart",
                "viewer.grid.chartKind",
                "viewer.grid.seriesKind",
                "viewer.grid.applyChartTemplate",
                "viewer.grid.saveChartTemplate",
                "-",
                "viewer.grid.chartTitle",
                "viewer.grid.horizontalAxisTitle",
                "viewer.grid.verticalAxisTitle",
                "viewer.grid.chartLegend",
                "viewer.grid.dataLabels",
                "viewer.grid.labelsFromCells",
                "viewer.grid.trendline",
                "viewer.grid.errorBars",
                "viewer.grid.gridlines",
                "-",
                "viewer.grid.axisScale",
                "viewer.grid.axisFormat",
                "viewer.grid.axisFont",
                "viewer.grid.titleFont",
                "viewer.grid.legendFont",
                "-",
                "viewer.grid.seriesColor",
                "viewer.grid.pointColor",
                "viewer.grid.explodeSlice",
                "viewer.grid.chartArea",
                "-",
                "viewer.grid.moveChart",
                "viewer.grid.chartWider",
                "viewer.grid.chartNarrower",
                "viewer.grid.chartTaller",
                "viewer.grid.chartShorter",
                "-",
                "viewer.grid.deleteChart",
            ]),
        },
        MenuSpec {
            // BibTeX files: the entries as a grid.
            when: None,
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
            when: None,
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
            ]
            .into_iter()
            // A workbook's zoom, panes, gridlines, sheets and macros.
            .chain(grid(&[
                "-",
                "viewer.grid.zoomIn",
                "viewer.grid.zoomOut",
                "viewer.grid.zoom100",
                "viewer.grid.zoom",
                "-",
                "viewer.grid.freezePanes",
                "viewer.grid.freezeTopRow",
                "viewer.grid.freezeFirstColumn",
                "viewer.grid.unfreezePanes",
                "viewer.grid.split",
                "-",
                "viewer.grid.toggleGridlines",
                "viewer.grid.toggleHeadings",
                "viewer.grid.pageBreakPreview",
                "-",
                "viewer.grid.nextSheet",
                "viewer.grid.previousSheet",
                "viewer.grid.sheetList",
                "-",
                "viewer.grid.runMacro",
            ]))
            .collect(),
        },
    ];
    // The plugins' menus (the git plugin's Git menu), after Kalem's.
    for m in crate::extensions::menus() {
        let entries = m
            .items
            .iter()
            .map(|i| match i.as_str() {
                "-" => MenuEntry::Separator,
                id => MenuEntry::Command {
                    label: crate::extensions::menu_label(id),
                    id: id.to_string(),
                    args: None,
                },
            })
            .collect();
        menus.push(MenuSpec {
            name: m.title,
            entries,
            when: m.when,
        });
    }
    menus
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
                    assert!(reg.get(&id).is_some(), "{} › {id}", m.name);
                }
            }
        }
    }

    #[test]
    fn a_plugins_menu_shows_where_its_when_clause_holds() {
        use crate::command::{Command, CommandHandler, CommandSource, Scope};
        crate::extensions::add_command(Command {
            id: "menutest.status".into(),
            title: "Menutest: Status".into(),
            category: "Menutest".into(),
            default_keys: Vec::new(),
            when: None,
            handler: CommandHandler::Plugin("menutest".into()),
            args_schema: None,
            source: CommandSource::Plugin("menutest".into()),
            scope: Some(Scope::all()),
        })
        .unwrap();
        crate::extensions::add_menu(crate::extensions::PluginMenu {
            plugin: "menutest".into(),
            title: "Menutest".into(),
            when: Some(crate::when::WhenClause::parse("vcs == git").unwrap()),
            items: vec!["menutest.status".into(), "-".into()],
        });
        let m = menus().into_iter().find(|m| m.name == "Menutest").unwrap();
        // Titled as the command, its category's prefix left out.
        assert!(matches!(
            &m.entries[0],
            MenuEntry::Command { label, id, .. } if label == "Status" && id == "menutest.status"
        ));
        let mut ctx = crate::when::Context::default();
        assert!(!m.shows(&ctx));
        ctx.set("vcs", crate::when::Value::Str("git".into()));
        assert!(m.shows(&ctx));
        crate::extensions::remove_menus("menutest");
        crate::extensions::remove_command("menutest.status");
        assert!(!menus().iter().any(|m| m.name == "Menutest"));
    }

    /// The menus a viewer's document shows, with the items its commands
    /// serve: a workbook's (a grid) or a picture's.
    fn viewer_menus(grid: bool) -> Vec<(String, Vec<(String, String)>)> {
        use crate::when::{Context, Value};
        let reg = crate::command::CommandRegistry::with_builtins();
        let mut ctx = Context::default();
        ctx.set("editorMode", Value::Str("viewer".into()));
        ctx.set("textType", Value::Str("viewer".into()));
        ctx.flag("viewerGrid", grid);
        ctx.flag("hasComments", false);
        menus()
            .into_iter()
            .filter(|m| m.shows(&ctx))
            .map(|m| {
                let items = m
                    .entries
                    .into_iter()
                    .filter_map(|e| match e {
                        MenuEntry::Command { label, id, .. } if reg.offered(&id, &ctx) => {
                            Some((label, id))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                (m.name, items)
            })
            .filter(|(_, items)| !items.is_empty())
            .collect()
    }

    #[test]
    fn a_workbook_finds_its_commands_where_excel_has_them() {
        use crate::l10n::tr;
        let book = viewer_menus(true);
        let has = |menu: &str, id: &str| {
            book.iter()
                .any(|(n, items)| n == menu && items.iter().any(|(_, i)| i == id))
        };
        for (menu, id) in [
            ("menu-file", "viewer.grid.exportPdf"),
            ("menu-file", "viewer.grid.pageSetup"),
            ("menu-edit", "viewer.grid.pasteSpecial"),
            ("menu-edit", "viewer.grid.find"),
            ("menu-edit", "viewer.grid.goToSpecial"),
            ("menu-format", "viewer.grid.bold"),
            ("menu-format", "viewer.grid.numberFormat"),
            ("menu-format", "viewer.grid.conditionalFormat"),
            ("menu-review", "viewer.grid.protectSheet"),
            ("menu-insert", "viewer.grid.insertChart"),
            ("menu-data", "viewer.grid.customSort"),
            ("menu-data", "viewer.grid.goalSeek"),
            ("menu-formulas", "viewer.grid.tracePrecedents"),
            ("menu-chart", "viewer.grid.trendline"),
            ("menu-view", "viewer.grid.freezePanes"),
            ("menu-view", "viewer.grid.runMacro"),
        ] {
            assert!(has(&tr(menu), id), "{menu} › {id}");
        }
        // The grid's Find and Select All, not the text's; no menu names
        // an item twice.
        for (name, items) in &book {
            for (_, id) in items {
                assert!(
                    ![
                        "find.open",
                        "find.replace",
                        "edit.selectAll",
                        "edit.toggleComment"
                    ]
                    .contains(&id.as_str()),
                    "{name} › {id}"
                );
            }
            let mut labels: Vec<&String> = items.iter().map(|(l, _)| l).collect();
            labels.sort();
            let n = labels.len();
            labels.dedup();
            assert_eq!(labels.len(), n, "{name}: {items:?}");
        }
        // A picture's or a PDF's menus carry none of a workbook's.
        let picture = viewer_menus(false);
        for (name, items) in &picture {
            assert!(
                !items.iter().any(|(_, id)| id.starts_with("viewer.grid.")),
                "{name}: {items:?}"
            );
            assert!(
                ![tr("menu-data"), tr("menu-formulas"), tr("menu-chart")].contains(name),
                "{name}"
            );
        }
        assert!(
            picture
                .iter()
                .any(|(_, items)| items.iter().any(|(_, id)| id == "find.open"))
        );
    }

    #[test]
    fn the_file_menu_has_what_the_book_puts_there() {
        let file = &menus()[1];
        let ids: Vec<_> = file
            .entries
            .iter()
            .filter_map(|e| match e {
                MenuEntry::Command { id, .. } => Some(id.as_str()),
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
