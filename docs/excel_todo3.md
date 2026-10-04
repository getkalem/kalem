# Excel: what is still missing

The first two lists are done: `docs/excel_todo.md` (E1–E20: formatting,
moving and selecting, Find and Replace, sheets, notes, AutoSum, Paste
Special, names, CSV) and `docs/excel_todo2.md` (E21–E36: typing and
editing formulas, sorting and filtering, tables, page setup, grouping,
protection, auditing, Go To Special, pictures and shapes, sparklines,
What-If analysis, threaded comments and sheet tabs, views, other
formats). This list is what is left between Kalem and Excel for an
everyday user, the most basic first: the gaps the earlier lists named as
limits, and the Excel features not yet begun. As before, a task is done
when it works in both editors, is written into the file as Excel writes
it, undoes in one step, and has its tests; every command is in the
palette and its keys can be bound again. A change to the grid contract
also goes into its interface (`grid.wit` and its three companions), and a
new release of the xlsx plugin follows when the contract moves.

## E37. The mouse in the grid

- [x] E37 Right-click menus in the graphical grid: on cells (cut, copy,
  paste, Paste Special, insert, delete, clear, sort, filter, format,
  note, comment, link), on row and column headings (insert, delete,
  hide, unhide, height, width) and on sheet tabs (insert, delete,
  rename, move or copy, tab color, hide); a selection's border dragged
  to move the cells (with Ctrl, to copy them); Ctrl+click and
  Ctrl+drag adding ranges to the selection.

## E38. Large edits at speed

- [x] E38 Pasting, filling, clearing, sorting and inserting rows over
  100,000 cells in well under a second: a many-cell edit writes the
  sheet once, not a parse per cell (36,000 cells now take minutes); a
  workbook of a million cells opens, scrolls and recalculates without
  freezing the editor, the long work in the background with progress.
  (Done but for the last part: at a million cells, opening and a full
  recalculation take about a second each, still in the editor's thread.)

## E39. New workbooks and copied sheets

- [x] E39 New Workbook (a blank `Book1.xlsx`, saved where asked) and New
  from a template (`.xltx`, `.xltm`); Move or Copy Sheet: a copy of the
  sheet in the same workbook (its formulas, names, tables, charts and
  pictures along), or moved or copied into another open workbook.

## E40. Calculation

- [x] E40 `NOW()` and `TODAY()` in the local time zone (the engine runs
  in UTC); Automatic, Automatic except Data Tables and Manual
  calculation (`calcPr`), data tables then recalculated with every
  change; circular references found and shown (the cells), or
  calculated iteratively with a maximum of iterations and change.

## E41. Finding and spelling

- [x] E41 Find All: every match listed with its sheet, cell and value,
  chosen to go there; Find in values, formulas, notes or comments,
  matching case or the whole cell, in the sheet or the whole workbook;
  Spelling (F7) over the sheet's text with the editor's dictionaries,
  each word replaced, ignored or added. (Kalem had no dictionaries:
  Hunspell's are used, the user's, the system's or LibreOffice's.)

## E42. Pasting and filling, the rest

- [x] E42 Paste Link (references to the copied cells); Paste Special's
  operations (add, subtract, multiply, divide) and Skip Blanks; Insert
  Copied Cells shifting the others down or right; the Series dialog
  (linear and growth steps, a stop value, dates by day, weekday, month
  or year); Fill Justify.

## E43. Sorting and filtering, the rest

- [x] E43 Sort by cell color or font color; filter by condition: Top 10
  (items or percent), above or below average, dates (today, this
  month, last quarter, a year), text and number rules joined by And or
  Or; Advanced Filter with a criteria range, the result in place or
  copied elsewhere, unique records only; Clear and Reapply for every
  filter of the sheet.

## E44. Formatting, the rest

- [x] E44 Rotated text drawn (it is kept, not shown); border styles
  (dashed, dotted, double, medium, colors per side); gradient and
  pattern fills; cell styles as named styles (Normal, Good, Bad,
  Neutral, Headings, Total) kept as styles, and new ones; the
  workbook's theme (its colors and fonts) chosen; Format Cells as one
  panel of the number, alignment, font, border, fill and protection.
  (gpui draws rotated text as slanted letters, not turned glyphs; the
  terminal shows it unrotated. Format Cells is one menu of every part,
  each leading to its command, not a dialog.)

## E45. Printing, the rest

- [x] E45 Charts, pictures and shapes printed; each sheet of a workbook
  with its own paper and orientation (the first one's now); manual
  column breaks; columns repeated at the left; gridlines and headings
  printed; scaled to a number of pages wide and tall or a percentage;
  print the selection or chosen sheets; pictures in headers and
  footers; printing to a printer through the system's dialog.
  (A drawing prints whole on the page of its first cell; LaTeX's own
  page breaks are not known to it, so one crossing such a break runs
  past it. Charts print as TikZ drawings of their kind, title, axes,
  legend and labels.)

## E46. Charts, the rest

- [ ] E46 Combo charts (columns and a line) with a secondary axis;
  trendlines (linear, exponential, moving average) with their equation
  and R²; error bars; data labels from cells; a chart moved to a sheet
  of its own (a chart sheet) and back; chart templates; stock, radar,
  bubble, histogram and waterfall charts drawn.

## E47. PivotTables, the rest

- [ ] E47 Grouping dates by months, quarters and years and numbers by
  steps; calculated fields and items; Show Values As (percent of the
  total, of the row or column, running total, difference from);
  sorting and filtering fields (top 10, labels, values); report
  layouts (compact, outline, tabular) and subtotals; PivotCharts;
  slicers for PivotTables and tables.

## E48. Dynamic arrays

- [ ] E48 Formulas whose result spills into the cells around them:
  `FILTER`, `SORT`, `SORTBY`, `UNIQUE`, `SEQUENCE`, `RANDARRAY`,
  `XLOOKUP` and array arithmetic, the spill range drawn and referred to
  as `A1#`, `#SPILL!` where it is blocked, written as Excel 365 writes
  them; `LET` and `LAMBDA` (the engine's work, in getkalem/ironcalc).

## E49. Views and windows, the rest

- [ ] E49 A split's left pane scrolled too and the cursor in any pane;
  New Window on the same workbook, two sheets side by side, scrolled
  together or apart; Zoom to Selection; custom views (the view, the
  print settings, hidden rows and columns) kept by name; full screen;
  the Watch Window kept in the file.

## E50. Other formats, the rest

- [ ] E50 Number formats and cell styles into and out of `.ods`; a `.xls`
  file's formulas kept (now only their results), and `.xlsb` read the
  same way; Save as CSV with a chosen delimiter and encoding, a sheet or
  each sheet to its own file; export as HTML; a workbook's PDF of the
  selection, a sheet or every sheet.

## E51. Analysis tools

- [ ] E51 Consolidate ranges of several sheets (sum, count, average,
  max, min) by position or by labels, linked or not; Solver: a cell
  maximized, minimized or brought to a value by changing cells under
  constraints (linear and smooth nonlinear); the Scenario Manager's
  summary report on a new sheet; the Analysis ToolPak's descriptive
  statistics, histogram and regression.

## E52. Links between workbooks

- [ ] E52 Formulas referring to other workbooks (`[Sales.xlsx]Q1!B2`)
  kept, their last values shown and updated from the other file (Edit
  Links: update, change source, break); 3D references
  (`Sheet1:Sheet3!A1`) computed; `INDIRECT` across sheets; a link to
  another workbook followed open.
