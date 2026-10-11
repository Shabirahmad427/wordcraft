# Pagination audit and verification

## Audit from 207c442

The existing pure-Rust layout engine already shapes and wraps text, paginates sections and columns, applies keep/widow rules, places footnotes, splits table rows, repeats table headers, wraps floating objects and renders equations. Headers/footers, field evaluation, cache reuse, PDF and raster output already exist. This arc reuses them rather than adding a second layout implementation.

The five highest-impact deficiencies found by inspection are:

1. **Section page finalization used the incoming section setup.** This is corrected: outgoing pages now finalize with their own geometry/alignment, and odd/even section starts use physical pages before display-number restarts. Filler-page decoration behavior still needs comparison against Word.
2. **Long footnotes/endnotes.** Long footnotes now split at line boundaries, reserve a bounded page area, continue in note order and produce continuation pages after body text ends. Oversized first lines and indivisible objects advance whole. Endnotes remain laid out as boxes and do not yet continue reliably; notes with complex tables, objects or section transitions need more coverage. The internal 40% page-area cap has not been compared with Word.
3. **Multicolumn footnote reservation and balancing.** Footnotes reserve page-wide space before body placement. A bounded high-water fallback favors clearance if anchors move repeatedly, at the cost of possible extra whitespace. Simple final-page text balancing is now implemented; complex content retains stable flow and reports a skipped page.
4. **Header/footer clearance.** Corrected internally: body layout reserves the tallest applicable default, inherited, first-page and (when enabled) even-page header and footer stories. This avoids body overlap but can leave extra whitespace on pages whose selected story is shorter. Different section geometries sharing one physical page and Word's exact clearance rules still need independent comparison.
5. **Unsplittable content can overflow.** Tall table headers/non-splittable rows and unbreakable inline objects intentionally overflow an empty page to guarantee progress. Layout reports horizontal and vertical boundary crossings and text-box clipping by page, element type, severity and bounds. The desktop status bar shows a localized warning, identifies the first affected element/axis and jumps to its page. There is no automatic rescaling or movement. Complex merged/nested table geometry, note tables, and some drawing effect bounds remain limited.

These are implementation findings, not independently measured Microsoft Word discrepancies. The format vocabulary is described by [OOXML section break values](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.sectionmarkvalues?view=openxml-3.0.1). The physical-page rule used here is internally tested; its interaction with numbering restarts and filler-page decorations still needs independent Word comparison.

## Implemented corrections

### Section page finalization

A page is finalized exactly once before replacing the outgoing section setup. Existing vertical alignment and footnote placement now run with that page's own margins, width and alignment. Subsequent page creation cannot apply alignment a second time. Page creation also clears contextual paragraph-spacing state.

Odd/even section breaks choose the next physical page before applying an incoming page-number restart. When needed, a filler page uses the outgoing section's page dimensions and section identity. It does not consume the incoming section's first-page header or restarted number. Filler pages currently inherit outgoing section header/footer and border behavior; whether Word suppresses those decorations remains unverified. Continuous sections and web view retain their existing flow behavior.

Page-number increments saturate at `u32::MAX` instead of panicking on hostile imported section properties. Saturation prevents a crash; it does not promise meaningful numbering beyond the representable limit.

DOCX import/export and guarded editing restrictions are unchanged. Layout does not mutate the document or preserved package. Recovery, Undo, failed-save protection and atomic file replacement remain existing engine responsibilities. No dependency, UI command or native-print backend is added.

### Long footnote continuation

Footnote stories are laid out as fragments with a 40% page-body budget. Each page reserves the current fragment before body lines are accepted. Remaining lines retain their original order and move to the following page; pages are appended until the final continuation is emitted, including when the body ends first. Line fragments preserve the existing paragraph shaping and are split only at line boundaries. An oversized first line or unbreakable object is emitted whole to guarantee progress. DOCX state and serialization are unaffected. The cap is a WordCraft policy that needs comparative testing. Footnote tables and objects and mixed-section reflow remain limited.

Document-end endnotes now flow after the body through the final section's columns and pages. Notes follow reference-number order; paragraph text splits at line boundaries and continues with a separator without repeating its number. Pages use body-bottom clearance, including reserved footnote space. Tables and other indivisible content remain whole; an oversized object or first line in an otherwise empty column/page may extend beyond the body rectangle so pagination can progress without dropping content. A hard continuation bound emits any remainder intact. Internal tests cover line coverage/order, multiple pages, body clearance, Undo/redo, recovery restore, DOCX save/reopen raster equality and PDF export.

The model currently exposes a global endnote number format and assigns continuous numbers in body reading order. It has no endnote position, starting number or continuous/section/page restart setting, so only document-end placement and continuous numbering are active. Section-end placement, restart selection and custom continuation separator/notice markup need explicit model and DOCX round-trip support. Package-preservation guards retain unsupported source OOXML when editing is restricted; they do not activate those settings in layout. No Microsoft Word reference output was used.

### Cross-column footnote reflow and marker equivalence

In print mode, when a document with footnotes uses multiple columns, layout performs bounded passes. Each pass records the vertical footnote area needed on each page. The next pass reserves that area before flowing body text in every column on the page, so notes found in a later column can move earlier-column lines forward. If note anchors move between columns or pages, the reservation map grows to a high-water mark; in a pathological nonconverging case this can leave unused space. Passes are bounded to avoid pagination loops. Existing continuation splitting and final note ordering remain in place.

The DOCX raster discrepancy was traced to a synthetic in-memory note story without its own reference mark. The writer correctly inserts `w:footnoteRef` into a note story when the model omits it; import retains that mark as a leading note-reference object. The regression fixture now contains the same self-reference mark before its initial render, and save/reopen compares every page's pixels exactly. The marker remains visible as required by the note story; the test compares equivalent models instead of weakening rendering.

One layout regression verifies a later-column note reduces the earlier column's body extent, preserves every body/note byte once, and leaves body lines above the note separator. The engine regression also exercises Undo, recovery, DOCX save/reopen, exact raster equality and PDF export. The tests use synthetic documents and WordCraft rendering only.

### Safe multicolumn balancing

At a continuous section ending or document end, the engine can balance the current final page when its body consists only of complete top-level paragraphs. It redistributes whole paragraphs in reading order across two or more columns, including unequal widths. It preserves section column offsets/spacing and page furniture geometry, and checks the already-shaped text width before moving any paragraph. Complete paragraphs stay intact, so keep-lines and widow/orphan behavior do not change; keep-with-next, contextual spacing, page-dependent fields and explicit breaks trigger fallback.

The fallback also applies when the page contains a note anchor or reservation, a table, an image/shape/other object, partial paragraph fragments, prior-section body content, or any line too wide for its destination. In these cases the original pagination remains in place and `DocLayout::column_balance_skipped` reports the page; the desktop status bar surfaces the warning. This pass does not rebalance multiple pages, rebreak text for a narrower destination, or balance endnotes together with body text. Regression coverage checks two and three equal-width columns, unequal columns, continuous transitions, fallback with a footnote/table/shape, and Undo/recovery/DOCX reopen/raster/PDF for the supported case. Internal Linux tests only; no Microsoft Word reference output was available.

### Header/footer clearance, column section starts and oversized content

Multicolumn text flows through configured columns and explicit column breaks. A `NextColumn` section now starts at the next column in the current section's column sequence. When the outgoing section is already at its final column, it starts at the first column of the next physical page; a page-number restart applies only when this transition creates a page. The incoming section supplies the continuing column geometry and page furniture. Simple complete-paragraph pages balance at a continuous section ending or document end; complex content is left in stable flow and reported through `column_balance_skipped`. This does not rebalance multi-page sections or rebreak paragraphs for narrower columns. Complex geometry transitions and Word comparison remain unverified. Cross-column note reservations are described above; high-water fallback can leave unused space.

Header and footer stories are resolved for rendering with link-to-previous inheritance and first/even-page selection. Before pagination, the engine measures the tallest possible header variant and reserves the body top below it; it measures the tallest footer variant and reserves the body bottom above it. First-page stories contribute only to title pages, and even-page stories contribute when even/odd headers are enabled. This is deliberately conservative, so a short selected story may leave unused body space when another variant is taller. Story measurement uses the same layout engine; it is internally verified but has not been compared page-for-page with Word.

Oversized inline objects and floating shapes stay intact and still allow later body blocks to paginate. The existing table paginator splits rows at line boundaries when possible, while unsplittable rows and oversized repeated headers are placed whole on an empty page to guarantee progress. `DocLayout::overflows` reports horizontal and vertical crossings for body text, tables, cells and objects, plus content clipped inside text boxes. `DocLayout::overflow_pages` remains the zero-based list of pages with vertical overflow or clipping. The diagnostic does not change placement or source document data.

### Horizontal overflow and text-box clipping diagnostics

The final layout pass compares paragraph line bounds with their actual column, table bounds with the body area, and cell text bounds with the allocated cell rectangle. Inline objects use the text area; positioned floating objects use the physical page edge, so page-anchored full-bleed art is accepted while an object cut by the paper edge is reported. Rotated object bounds are included. For text boxes, layout inspects the complete story before its fit pass; horizontal overflow is reported as a warning, while vertical content removed by the existing fit pass is marked clipped.

Each `OverflowDiagnostic` carries a zero-based page, element type (`BodyText`, `Table`, `TableCell`, `Image`, `FloatingObject`, or `TextBox`), axis, severity and page-coordinate bounds. The status-bar warning is localized and clickable; it describes the first issue and navigates to that page. Existing object sizes/positions and DOCX package data are unchanged. Normal-document, oversized text, wide-table, exact-cell, off-page float, full-bleed image and two-axis text-box regressions cover the checks. An engine integration test confirms diagnostics through Undo/redo, recovery, DOCX save/reopen and PDF export, with identical raster output before and after reopening.

Known limits: detection uses layout geometry rather than pixels. Shape stroke/shadow expansion is not fully represented; complex vertically merged/nested tables, note tables and unsupported drawing effects need more coverage. A boundary crossing may be intentional in source documents; positioned objects inside the physical page are therefore not flagged for crossing body margins. Text boxes continue to render only content that fits vertically; clipping is now explicitly reported. No independent Microsoft Word output was used.

### Regression corpus

Eighteen regression tests since the pagination baseline cover:

- Long-note line coverage/order/bounds, body separation, continuation after body end, and Undo/recovery/DOCX reopen with pixel comparison.
- Old-page footnote coordinates against a standalone section; all nine combinations of old/new vertical alignment.
- Odd/even starts with four page-number restarts each, filler ownership, incoming first-page headers, continuous/web flow and maximum page-number overflow.
- A five-page synthetic academic document with centered introduction, footnote, landscape results, odd-page transition, restarted numbering and a multipage table with repeated header rows and split cells. Every paragraph's text clusters must be represented and placed lines must remain inside the page body.
- Oversized inline content, floating shapes and unsplittable table rows remain intact, later text stays reachable, affected pages are reported, and ordinary empty documents report no overflow.
- Edit → Undo → Redo → durable recovery → atomic DOCX save → reopen, with unchanged tested pagination and exact raster self-comparison across every page.
- The preceding page's raster against its standalone-section rendering, independently of save/reopen.
- Guarded packages remain byte-identical after layout/no-op saving; changing section orientation still fails safely at save and Undo restores the original model.

Generate original synthetic page images locally:

```sh
WORDCRAFT_LAYOUT_CORPUS_DIR=/tmp/wordcraft-section-corpus cargo test -p wordcraft-engine --test section_pagination
cargo test -p wordcraft-layout tests_sections
cargo xtask ci
cargo build --release -p wordcraft -p wordcraft-cli
```

All six CI gates passed on the Phase 4B changes with 865 workspace tests across 47 suites. GUI and CLI release builds passed. The WASM check reports existing target-specific dead-code warnings in speech and file-dialog code; these do not fail CI. Native Clippy passes with warnings denied.

The generated images are WordCraft's own output, with original text and installed fonts; no Microsoft assets or Word-produced files are committed. The portrait introduction and landscape table images were visually inspected on Linux. Header/footer clearance regressions assert body/header/footer geometry across default, first-page and even-page stories; NextColumn regressions assert same-page column movement and final-column rollover. The mixed-section save/reopen test preserves page count, numbering, column placement and body/note text, exports a PDF and compares every reopened page pixel-for-pixel after the source model includes the OOXML footnote self-reference mark. Recovery preserves exact page pixels. A dedicated layout test exercises page-wide cross-column reflow. No Microsoft Word reference output was available or used. Installed system fonts and font substitution can differ across operating systems. Native printing and Windows/macOS visual verification remain outstanding. There is no Microsoft Word 365 pagination parity claim.
