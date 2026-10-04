# Excel: the next basics still missing

The first list (`docs/excel_todo.md`, E1–E20) is done: formatting, moving
and selecting, Find and Replace, sheets, hidden rows and columns, frozen
panes, notes, AutoSum, Paste Special, inserting and deleting cells,
Format Painter, Remove Duplicates, Text to Columns, hyperlinks, names,
Show Formulas and CSV. This list is what an everyday Excel user reaches
for next, the most basic first. As before, a task is done when it works in
both editors, is written into the file as Excel writes it, undoes in one
step, and has its tests; every command is in the palette and its keys can
be bound again.

## E21. Typing into cells

- [x] E21 Alt+Enter for a line break in a cell; Ctrl+Enter enters the
  same text into every selected cell; Ctrl+; today's date, Ctrl+Shift+;
  the time; Ctrl+' the formula of the cell above, Ctrl+Shift+" its value;
  AutoComplete of text from entries already in the column.

## E22. Editing formulas

- [x] E22 While a formula is typed: F4 cycles the reference at the cursor
  through `$A$1`, `A$1`, `$A1`, `A1`; arrow keys (and the mouse in the
  graphical editor) point at cells and ranges to put their reference in;
  function names and defined names completed, with the function's
  arguments shown as they are typed.

## E23. Sorting and filtering more

- [x] E23 Custom Sort by several columns (levels), each ascending or
  descending, by value or by a custom list; AutoFilter's Text and Number
  Filters (begins with, contains, greater than, between, top 10, above
  average) and filtering by a cell's color; Reapply.

## E24. Tables

- [x] E24 Format as Table (Ctrl+T): a table part with a name, header row,
  banded rows and a style; a Total Row with its functions; the table
  growing as rows are typed under it; structured references
  (`Table1[Amount]`) in formulas.

## E25. Page setup and printing

- [x] E25 Page Layout: orientation, paper size, margins, Fit to one page
  wide, Print Area, Print Titles (rows repeated on each page), headers
  and footers, page breaks; Print Preview and Export to PDF of the sheet,
  the selection or the workbook.

## E26. Grouping and subtotals

- [x] E26 Group and Ungroup rows or columns (Alt+Shift+Right and Left)
  as outline levels, collapsed and expanded from their buttons; Subtotal
  of a sorted table at each change in a column.

## E27. Cell styles and more alignment

- [x] E27 The built-in cell styles (Normal, Good, Bad, Neutral, Heading 1
  to 4, Title, Total, Currency, Percent); indent (increase and decrease),
  text rotated or vertical, Shrink to Fit, Center Across Selection.

## E28. Protection

- [x] E28 Lock and unlock cells (Format Cells' Protection); Protect Sheet
  with what stays allowed, and a password; Protect Workbook structure;
  the grid refusing edits to locked cells of a protected sheet.

## E29. Formula auditing

- [x] E29 Trace Precedents and Trace Dependents drawn as arrows, Remove
  Arrows; Evaluate Formula step by step; Error Checking that goes to each
  error and says what is wrong; Watch Window.

## E30. Go To Special and sheet-wide selection

- [x] E30 Go To Special: blanks, constants, formulas (by kind of result),
  errors, visible cells only (Alt+;), the last cell, cells with notes,
  conditional formats or data validation; the cells found selected
  together.

## E31. Pictures and shapes

- [x] E31 Insert a picture from a file onto the sheet, move and size it,
  delete it; text boxes and simple shapes; written as the sheet's drawing
  with its relationships.

## E32. Sparklines

- [x] E32 Line, column and win/loss sparklines in a cell from a row or
  column of data, with high and low points marked; written as Excel's
  sparkline groups.

## E33. What-If analysis

- [x] E33 Goal Seek (set a cell to a value by changing another); Data
  Tables of one and two variables; Scenario Manager.

## E34. Comments and sheet tabs

- [ ] E34 Threaded comments with replies (beside the notes of E11),
  resolved and deleted; a sheet tab's color; the sheet list shown and
  chosen from (right-click on the tab arrows).

## E35. Views

- [ ] E35 Zoom of the sheet (Ctrl+wheel, a percentage) kept in the file;
  Split the window into panes that scroll apart; Page Break Preview;
  gridlines and headings shown or hidden, kept in the sheet view.

## E36. Opening and saving other formats

- [ ] E36 Open a CSV or text file as a workbook through an import step
  (delimiter, encoding, column types); Save As another workbook name and
  as `.xlsx` from a legacy `.xls`; open and save `.ods`.
