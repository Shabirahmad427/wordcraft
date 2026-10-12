# Cross-platform layout and PDF regressions

> **Last reviewed:** 2026-10-12 · **Last updated:** 2026-10-12 · **Status:** synthetic cross-platform suite; six local gates and Apple Silicon release build pass, hosted Linux/Windows pending for this revision

The fixtures are generated in Rust and written to DOCX before import. No Word-created file or proprietary asset is committed. The [Document regressions workflow](../.github/workflows/document-regressions.yml) runs the DOCX, engine, layout and PDF tests on native Linux, macOS and Windows runners, plus a CLI version smoke on each. Linux also builds the GUI and CLI in release mode and runs the reviewed two-page raster baseline. These checks establish internal consistency and catch export loss; they do not measure Microsoft Word 365 pagination.

| Content | Synthetic regression | Independent assertion |
|---|---|---|
| Long paragraphs, explicit page breaks, headers, footers, PAGE fields, inline/floating pictures | `pdf::tests::synthetic_docx_page_geometry_and_pdf_content`; `engine::section_pagination` | Authored 432 × 648 pt paper, 54 pt side margins, two pages, per-page extracted markers and numbers, parsed PDF media boxes, embedded image/font resources; edited DOCX save/reopen remains extractable |
| Multi-page tables, endnotes, equations, mixed font/Unicode | `pdf::tests::synthetic_docx_table_notes_columns_math_and_vertical_box`; `engine::section_pagination` | Table cells inside paper, no blank pages or overflow, first/last row and note text extracted from PDF, PDF page count equals authored layout count |
| Columns, column break, vertical text box | `pdf::tests::synthetic_docx_table_notes_columns_math_and_vertical_box`; `layout::tests_textbox` | Second column's caret moves right on the same page; box lines have downward text direction after DOCX import |
| Footnotes, note continuation, pagination | `pdf::tests::synthetic_docx_page_geometry_and_pdf_content`; `layout::tests_pages`; `engine::section_pagination` | Note text survives PDF export; line bounds and story coverage tests detect missing content |
| Charts and scientific error bars | `engine::opaque_charts::read_only_error_bars_survive_undo_recovery_docx_reopen_and_pdf_export`; `pdf::tests::inline_chart_is_a_figure_with_its_text_inside_it` | OOXML chart/error-bar package bytes and rendered primitive counts survive edit/reopen; PDF figure resource is present |
| Off-page header image | `layout::tests::off_page_header_picture_reports_clipping` | A page-anchored image at x = 600 pt, width = 80 pt, on 612 pt paper must report horizontal **Clipped** overflow. This failed before the shared layout fix. |

The PDF reader used for page boxes and text extraction is `hayro-syntax`/`hayro-interpret`, independent of the `krilla` writer. Text extraction can interleave footnote and table glyphs, so tests use stable markers and page-specific assertions instead of requiring one contiguous extraction order. Geometry is in points with a small tolerance; glyph pixels and binary PDF bytes are not compared across operating systems. `WORDCRAFT_FONT_DIRS` can isolate the bundled open-font visual baseline on Linux. Installed fonts, font substitution, shaping and raster backends can alter line breaks and pixels; native results must be reported separately. Windows ARM64 and macOS Intel runtime remain unverified.

Run locally with `cargo test --locked -p wordcraft-docx -p wordcraft-engine -p wordcraft-layout -p wordcraft-pdf --tests --lib`. Run all six gates with `cargo xtask ci`. The suite does not yet have independent Word-created reference geometry, full PDF image-raster comparison or every chart glyph's extracted text; those remain coverage gaps.

On 2026-10-12, native Apple Silicon `cargo xtask ci` passed all six gates (workspace test gate: 2,101 passed, 2 ignored across 61 suites). `cargo build --release --locked -p wordcraft -p wordcraft-cli` passed; both binaries answered `--version` as 0.6.1. This is a version smoke, not a full interactive window test. Linux release and Windows native results for this revision will be taken from the hosted matrix after push.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-12 | initial | DOCX-to-layout-to-PDF corpus, cross-platform workflow gate and reproduced header-image clipping diagnostic |
