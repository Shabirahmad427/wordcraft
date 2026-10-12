#!/usr/bin/env bash
# Generate the original synthetic visual fixture and raster every page into a fresh directory.
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <wordcraft-cli> <new-output-directory>" >&2
  exit 2
fi
cli=$1
out=$2
if [[ ! -x "$cli" ]]; then echo "CLI is not executable: $cli" >&2; exit 2; fi
if [[ -e "$out" ]] && [[ -n "$(find "$out" -mindepth 1 -maxdepth 1 -print -quit)" ]]; then
  echo "output directory must be absent or empty: $out" >&2
  exit 2
fi
mkdir -p "$out/pages"
mkdir -p "$out/fontset"
export WORDCRAFT_FONT_DIRS="$out/fontset"

body='Scientific pagination fixture: deterministic paragraph spacing, equations, figures, tables, charts and notes.'
table_text='Table content verifies row geometry and stable text placement.'
commands=(--template blank
  --cmd 'design.themeFonts={"heading":"Source Sans 3","body":"Source Sans 3"}'
  --cmd "text.insert={\"text\":\"$body\"}"
  --cmd 'caret.docEnd'
  --cmd 'insert.equation={"linear":"E=mc^2","edit":false}'
  --cmd 'caret.docEnd'
  --cmd 'insert.table={"rows":3,"cols":3}'
  --cmd 'caret.docEnd'
  --cmd 'insert.chart={"type":"column","title":"Synthetic results","data":{"categories":["A","B","C"],"series":[{"name":"Yield","values":[2,4,3]}]}}'
  --cmd 'caret.docEnd'
  --cmd 'insert.shape={"kind":"ellipse","width":84,"height":48,"fill":"2878B5"}'
  --cmd 'caret.docEnd'
  --cmd "insert.picture={\"path\":\"$(cd "$(dirname "$0")/.." && pwd)/assets/app-icon/wordcraft-1024.png\",\"width\":48}"
  --cmd 'caret.docEnd'
  --cmd "text.insert={\"text\":\"\\n\\n$table_text\"}"
  --cmd 'insert.pageBreak'
  --cmd 'caret.docEnd'
  --cmd 'text.insert={"text":"Second-page continuation for deterministic multipage comparison.\n\nThis source-generated academic-style fixture is not a Microsoft Word reference document. It checks rendering stability only."}'
  --cmd 'caret.docEnd'
  --cmd 'insert.equation={"linear":"\\int_0^1 x^2 dx=1/3","edit":false}'
  --cmd 'caret.docEnd'
  --cmd 'references.footnote={"text":"A synthetic footnote used to exercise note placement and page clearance."}'
  --cmd 'caret.docEnd'
  --cmd 'insert.textBox={"text":"VERTICAL LABEL","width":72,"height":108}'
  --cmd 'shape.textDirection={"dir":"vert"}'
  --save "$out/visual-fixture.docx")

"$cli" run "${commands[@]}"
document_text=$("$cli" text "$out/visual-fixture.docx")
for expected in 'Scientific pagination fixture' 'Table content verifies row geometry' 'Second-page continuation for deterministic multipage comparison'; do
  if [[ "$document_text" != *"$expected"* ]]; then
    echo "visual fixture lost expected body text: $expected" >&2
    exit 1
  fi
done
page_count=$("$cli" info "$out/visual-fixture.docx" | sed -n 's/^[[:space:]]*"pages": \([0-9][0-9]*\),$/\1/p' | head -n 1)
if [[ ! "$page_count" =~ ^[1-9][0-9]*$ ]]; then echo "could not determine fixture page count" >&2; exit 1; fi
for ((page=1; page<=page_count; page++)); do
  "$cli" render "$out/visual-fixture.docx" "$out/pages/page-$page.png" --page "$page" --scale 1 >/dev/null
done
printf 'fixture\tsynthetic-academic-visual.docx\npages\t%s\n' "$page_count" > "$out/manifest.tsv"
printf 'Rendered %s synthetic page(s) under %s/pages\n' "$page_count" "$out"
