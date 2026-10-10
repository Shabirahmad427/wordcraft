# Pagination audit and verification

## Audit from 207c442

The existing pure-Rust layout engine already shapes and wraps text, paginates sections and columns, applies keep/widow rules, places footnotes, splits table rows, repeats table headers, wraps floating objects and renders equations. Headers/footers, field evaluation, cache reuse, PDF and raster output already exist. This arc reuses them rather than adding a second layout implementation.

The five highest-impact deficiencies found by inspection are:

1. **Section page finalization uses the incoming section setup.** When the next section changes left margins or vertical alignment, outstanding footnotes and the preceding page's body can move. Odd/even section starts also use restarted display numbers rather than the physical page sequence. Selected first because these bugs affect short, otherwise supported documents and have isolated, reproducible corrections.
2. **Long footnotes/endnotes do not continue.** Notes are laid out as an unpaginated box. A note taller than the available page can overlap body text or extend outside the page. Splitting requires a continuation queue, reference-line reservation and bounded progress for unbreakable content; not implemented in this arc.
3. **Multicolumn footnote reservation is not retroactive.** A note discovered in a later column reduces the current body bottom but does not repaginate already placed earlier columns. Earlier text can overlap that note region. Column balancing and continuous-section transitions also need a section-wide pagination pass. `NextColumn` section starts currently behave as page starts.
4. **Header/footer clearance is incomplete.** Body clearance measures explicit default/first headers; inherited and even headers are selected later. Tall inherited/even headers can overlap the body, and footer height does not reserve body space. A page-specific resolved header/footer plan is needed before pagination.
5. **Unsplittable content can overflow.** Tall table headers/non-splittable rows and unbreakable inline objects intentionally overflow an empty page. Table splitting has a fixed 1000-iteration bound; huge rows need explicit overflow reporting rather than an oversized remainder. Text boxes clip excess text; long display equations have no line-breaking implementation. Floats, notes inside tables and vertically merged/nested tables require further auditing.

These are implementation findings, not independently measured Microsoft Word discrepancies. The format vocabulary is described by [OOXML section break values](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.wordprocessing.sectionmarkvalues?view=openxml-3.0.1). The physical-page rule used here is internally tested; its interaction with numbering restarts and filler-page decorations still needs independent Word comparison.

## Implemented correction

A page is finalized exactly once before replacing the outgoing section setup. Existing vertical alignment and footnote placement now run with that page's own margins, width and alignment. Subsequent page creation cannot apply alignment a second time. Page creation also clears contextual paragraph-spacing state.

Odd/even section breaks choose the next physical page before applying an incoming page-number restart. When needed, a filler page uses the outgoing section's page dimensions and section identity. It does not consume the incoming section's first-page header or restarted number. Filler pages currently inherit outgoing section header/footer and border behavior; whether Word suppresses those decorations remains unverified. Continuous sections and web view retain their existing flow behavior.

Page-number increments saturate at `u32::MAX` instead of panicking on hostile imported section properties. Saturation prevents a crash; it does not promise meaningful numbering beyond the representable limit.

DOCX import/export and guarded editing restrictions are unchanged. Layout does not mutate the document or preserved package. Recovery, Undo, failed-save protection and atomic file replacement remain existing engine responsibilities. No dependency, UI command or native-print backend is added.

## Regression corpus

Nine new tests cover:

- Old-page footnote coordinates against a standalone section; all nine combinations of old/new vertical alignment.
- Odd/even starts with four page-number restarts each, filler ownership, incoming first-page headers, continuous/web flow and maximum page-number overflow.
- A five-page synthetic academic document with centered introduction, footnote, landscape results, odd-page transition, restarted numbering and a multipage table with repeated header rows and split cells. Every paragraph's text clusters must be represented and placed lines must remain inside the page body.
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

All six `cargo xtask ci` gates passed with **859 tests** (nine new), and GUI/CLI release builds succeeded. Native Clippy passed with warnings denied; the WASM gate retains pre-existing dead-code warnings in speech and file-dialog code.

The generated images are WordCraft's own output, with original text and installed fonts; no Microsoft assets or Word-produced files are committed. The portrait introduction and landscape table images were visually inspected on Linux. Visual tests compare WordCraft with itself and with an isolated subsection, not with Microsoft Word reference output. Installed system fonts and font substitution can differ across operating systems. Native printing and Windows/macOS visual verification remain outstanding. There is no Microsoft Word 365 pagination parity claim.
