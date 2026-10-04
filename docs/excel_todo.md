# Excel: the basics still missing

The workbook view (the xlsx plugin of getkalem/plugins with Kalem's grid)
already opens, edits and saves `.xlsx` files as themselves, with
formulas, undo, macros, sort and filter, conditional formatting, data
validation, pivot tables, charts, the fill handle and Flash Fill. This
list is what someone who uses Excel every day reaches for first and does
not find yet, the most basic first. The tasks are done in this order;
each is done when it works in both editors, is written into the file as
Excel writes it, undoes in one step, and has its tests.

Keys follow Excel where the grid has them free; every one is a command
in the palette and can be bound again.

## E1. Font formatting

- [x] E1 Bold, italic, underline and strikethrough of the selection
  (Ctrl+B, Ctrl+I, Ctrl+U, Ctrl+5), its font color, fill color, font size
  and typeface: Format Cells' Font and Fill, written as a cell style
  (`<xf>` with a `<font>` and `<fill>`), every cell of the selection.

## E2. Alignment

- [x] E2 Horizontal alignment (left, center, right, general) and
  vertical alignment (top, middle, bottom) of the selection, written in
  the cells' `<alignment>`, drawn so in both editors.

## E3. Borders

- [x] E3 Borders of the selection: all, outside, bottom, top, thick
  outside, none, in a color; written as `<border>` styles, drawn in the
  graphical editor and as box lines in the terminal.

## E4. Number formats

- [x] E4 The selection's number format: General, Number (two
  decimals, thousands), Currency (₺), Percent, Short Date, Long Date,
  Time, Text, Scientific, or a typed code; Increase and Decrease
  Decimal. Written as `numFmt` styles, shown through them.

## E5. Moving and selecting with the keyboard

- [x] E5 Ctrl+arrow to the edge of the data, Ctrl+Shift+arrow to select
  to it; Shift+Space selects the row, Ctrl+Space the column, Ctrl+A the
  data around the cursor (again, the sheet); Go To (F5; Ctrl+G stays the
  command palette) a cell
  or range by its reference.

## E6. The selection's sum

- [x] E6 The status line shows the selection's Sum, Average and Count
  (of numbers, of values) when it spans more than one cell.

## E7. Find and Replace

- [x] E7 Find (Ctrl+F) and Replace (Ctrl+H) in the sheet: next and
  previous match, whole cell or part, case, in values or formulas;
  Replace All in one undo step.

## E8. Sheets

- [x] E8 Insert a sheet, delete one (asked first), rename, move left or
  right, hide and unhide; written in the workbook part with every
  reference to the sheet kept right.

## E9. Hidden rows and columns

- [x] E9 Hide and unhide the selection's rows or columns (Ctrl+9,
  Ctrl+Shift+9, Ctrl+0), written as Excel writes them.

## E10. Frozen panes

- [x] E10 Freeze the top row, the first column, or at the cursor;
  unfreeze; written as the sheet view's pane.

## E11. Notes

- [x] E11 Add, edit and delete a cell's note (Shift+F2), written as
  Excel's comments part with its drawing.

## E12. AutoSum and functions

- [ ] E12 AutoSum (Alt+=) puts `=SUM()` of the numbers above or to the
  left; Insert Function lists the functions the engine knows with their
  arguments and enters the one chosen.

## E13. Paste Special

- [ ] E13 Paste values only, formats only, formulas only, and
  transposed, from cells copied in the workbook.

## E14. Inserting and deleting cells

- [ ] E14 Insert and delete as many rows or columns as are selected;
  insert and delete cells shifting the rest down or right, up or left
  (Ctrl++ and Ctrl+-).

## E15. Clear formats and Format Painter

- [ ] E15 Clear Formats (the style taken away, values kept), Clear All;
  Format Painter copies the cursor's format onto a selection.

## E16. Remove Duplicates and Text to Columns

- [ ] E16 Remove Duplicates from the table at the cursor by chosen
  columns; Text to Columns splits a column at a delimiter.

## E17. Hyperlinks

- [ ] E17 Insert, open and remove a cell's hyperlink (Ctrl+K), written
  as the sheet's hyperlinks and their relationships.

## E18. Named ranges

- [ ] E18 Define a name for the selection, list the workbook's names,
  go to one, delete one; written as defined names.

## E19. Formulas shown and recalculation

- [ ] E19 Show Formulas (Ctrl+`) in the cells instead of their values;
  Calculate Now (F9).

## E20. Saving a sheet as CSV

- [ ] E20 Save the sheet shown as CSV (values as shown, quoted as
  needed), the workbook left as it is.
