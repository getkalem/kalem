#!/bin/sh
# Regenerates the LibreOffice fixtures of tests/csv from their spreadsheets
# (flat ODS): needs LibreOffice Calc (`soffice`). The filter options are
# field separator, text delimiter, character set (76 = UTF-8), first line,
# cell format, language, "quote all text cells", "detect special numbers"
# and "save cell contents as shown".
set -e
cd "$(dirname "$0")/../tests/csv"
out=$(mktemp -d)
soffice --headless --norestore \
  --convert-to 'csv:Text - txt - csv (StarCalc):44,34,76,1,,0,true,true,true' \
  --outdir "$out" libreoffice.fods
soffice --headless --norestore \
  --convert-to 'csv:Text - txt - csv (StarCalc):59,34,76,1,,1055,false,true,true' \
  --outdir "$out" libreoffice-tr.fods
cp "$out/libreoffice.csv" "$out/libreoffice-tr.csv" .
rm -r "$out"
