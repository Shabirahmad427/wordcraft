# Desktop GUI validation

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** Phase 7R reproduction/fix for table-grid focus; native macOS editing stopped safely at the existing recovery chooser · **Target:** WordCraft desktop on macOS, Linux and Windows

This document separates a live desktop window, egui event-driven tests, offscreen rendering and
hosted builds. A CI job that compiles or tests the GUI is not evidence that a person used a desktop
window on that platform.

## Phase 7R evidence

The checkout began at `0ff86c8`. On the local macOS host, the release app launched with its real
window and localhost control channel. It opened the normal profile's recovery chooser, which showed
existing recovery entries. I did not open, discard or modify those entries, so I stopped before
editing. This means there is no native-window save/reopen/PDF workflow result for this run. The
window screenshot was not inspected because it included the recovery chooser.

An isolated offscreen `ui_shot` run used a fresh blank synthetic document at 1440×900 logical points
and 2× rendering. Home, Insert, Layout, References, Review and View were captured and inspected;
the ribbon and Home style samples were visible. The screenshots were saved under `/private/tmp`
and are not repository fixtures. The existing `home_style_gallery_preview_contains_rendered_sample_ink`
pixel test separately exercises built-in and custom styles, missing-font fallback, light/dark
preview backgrounds and 1×/2× raster scale.

The egui event path (pointer, key and text events delivered to the running UI harness) exercised
Unicode text, Bold, Undo/Redo, the Insert tab, the Table menu, a 2×2 grid selection and cell typing.
This is not OS-level physical mouse/keyboard input. Before the correction, the table was inserted
but all four cells remained empty when text was typed immediately. The picker called `app.run`
directly and skipped the ribbon helper that requests canvas focus. The picker now calls
`run_from_ribbon`; `dialogs::tests::table_grid_selection_requests_document_focus` failed before
the change and passes after it. The same isolated UI event script then produced
`[["Alpha","Beta"],["Gamma","Delta"]]`; Cmd+Z removed `Delta` and Cmd+Shift+Z restored it.

## Platform evidence matrix

| Platform | Live desktop GUI | UI/render tests | Hosted/build evidence | Limits |
|---|---|---|---|---|
| macOS | App window launched on the local Mac; recovery chooser prevented editing, and existing entries were left untouched | 2× offscreen window screenshots; egui pointer/key/text workflow; style preview pixels at 1×/2× and light/dark | `cargo xtask ci` passed all six gates; native arm64 GUI/CLI release build and CLI 0.6.1 version smoke passed | No complete native editing workflow, native screenshot review of document content, physical input session, or Word 365 comparison |
| Linux | No native desktop session used | No local native-window run | Hosted jobs exercise tests/clipboard under Xvfb and compile the GUI/CLI; see the linked run records in `ROADMAP.md` | Xvfb clipboard tests and GUI compilation are not a Linux desktop editing session |
| Windows | No native desktop session used | No local native-window run | Hosted Windows jobs exercise app/UI tests and clipboard adapter; see the linked run records in `ROADMAP.md` | Hosted tests are not an interactive Windows desktop run |

### Area audit

| Ribbon area | Evidence | Result |
|---|---|---|
| Home: clipboard, fonts, paragraphs, styles | Offscreen screenshot and style-preview pixel regressions | Controls rendered; style samples were visible. No new defect observed in the screenshot. |
| Insert: tables, pictures, equations, charts | Offscreen screenshot; actual pointer path through Table menu and 2×2 grid | Menu and grid rendered. Table-grid focus defect reproduced and fixed. Picture/equation/chart insertion was not exercised in a live document. |
| Layout: margins, columns, breaks, page settings | Offscreen screenshot | Controls rendered; action behavior was not exhaustively tested in this phase. |
| References: citations, captions, footnotes | Offscreen screenshot | Controls rendered; action behavior was not exhaustively tested in this phase. |
| Review: editing and document tools | Offscreen screenshot | Controls rendered; action behavior was not exhaustively tested in this phase. |
| View: zoom, navigation, page display | Offscreen screenshot | Controls rendered; resize, scrollbar, zoom and navigation behavior were not exhaustively tested in a native window. |

No complete tooltip, keyboard-shortcut, 1×/2× desktop-window, resize, scrollbar, or screen-reader
audit was performed here. A 2× offscreen screenshot and a pixel test are not substitutes for those
checks. The existing ribbon group-fit regression remains part of the UI suite.

## Synthetic Word 365 comparison protocol (not yet performed)

Use only documents written from scratch for this comparison. Do not open a personal document. Word
may be observed as a black box; do not inspect its bundle or commit any file Word produces.

1. In WordCraft, start a blank document and type a fixed synthetic sample: a title, one heading,
   two paragraphs with bold/italic and `naïve café — 東京`, plus a 2×2 table with short labels.
2. Save it to a temporary path such as `/private/tmp/wordcraft-wordcraft-reference.docx`.
3. Open that file in Microsoft Word 365. Tile the WordCraft and Word windows left/right using the
   macOS window controls. Compare page size, margins, line breaks, heading/body appearance, table
   dimensions and cell text. Record app versions, macOS version, display scale and installed fonts.
4. In Word, create the same synthetic content from scratch and save it to a separate temporary
   file. Open that file in WordCraft. Compare the same properties, edit text and a table cell in
   each app, then save/reopen and export a PDF from WordCraft.
5. Record differences and whether they are layout, font substitution, editing or package-round-trip
   differences. Do not use byte-identical PDF comparison as the pass criterion.
6. If screenshots are needed, capture each application window separately by window ID. Word
   screenshots belong only under the ignored `plan/word/screenshots/` directory. Do not capture
   the whole desktop, and do not commit the temporary DOCX/PDF files or files saved by Word.

This protocol has not been run. No Word 365 compatibility result is claimed.
