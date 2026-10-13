# Editing workflow regression coverage

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** Phase 7O added structured whole-cell clipboard fragments and round-trip checks.

## Phase 7O table selection and clipboard behavior

The shared engine's active `CellRange` now drives table Copy. A selected cell, row, column, or other rectangular cell selection produces a clipped `Table` fragment that retains selected cells in row order, table/row/cell properties, run and paragraph formatting, and nested block content. Blank cells pad grid gaps where a selected merged cell extends beyond the selected columns; complete-table copies keep their merge links, while clipped fragments clear vertical-merge markers whose partner rows may be absent. Content-control markers are balanced before the fragment is put on the internal clipboard. If the selection is an ordinary text range that happens to start or end inside a table, the existing text-only copy behavior remains; a whole table included between surrounding paragraphs still follows the Phase 7N structured-copy path.

The engine regressions cover one cell, a complete row, a column across multiple rows, clipped vertical merges, paste into a blank document and beside existing text, Undo/Redo, DOCX save/reopen, and PDF export. A run of the new single-cell/row regression against the former paragraph-copy route failed at the structural-fragment assertion (`cell selection was flattened`); it passes with the shared cell-fragment builder. An imported package guarded against unsafe OOXML rewriting remains protected by the existing save guard; this phase does not widen that contract.

WordCraft's internal clipboard keeps the structured fragment. The desktop UI publishes Copy as plain text through egui today; Paste can read plain text, HTML, or RTF when supplied by the native clipboard adapter. Structured table export to OS-specific clipboard formats is not implemented. Linux/macOS/Windows native clipboard integration is therefore not claimed for structured tables, and no Microsoft Word clipboard format interoperability is claimed.

Nested tables, images, and rich objects inside selected cells are retained through model cloning and the normal fragment adoption path, but do not yet have separate fixtures. Mixed selections that begin/end inside tables, custom unsupported cell extensions, and OS clipboard rich-table export remain documented limits. The full table-intersection behavior is specified by the current `CellRange`; tests are synthetic and establish WordCraft behavior only.

### Phase 7O platform validation

On Apple Silicon macOS, all six local `cargo xtask ci` gates passed; the optimized GUI/CLI build and both version smokes passed. Hosted [Document regressions run 38394565623](https://github.com/Shabirahmad427/wordcraft/actions/runs/38394565623) passed the DOCX/session suites and CLI smoke on Linux, macOS and Windows, plus the Linux GUI/CLI release build, bundled-font visual regression and advisory performance job. This is shared-core test and CLI evidence across all three operating systems. The Linux GUI target was compiled but not launched; there was no native macOS or Windows GUI runtime test in this phase.

The local `git fetch --no-write-fetch-head origin main` reproduced the ref error at `.git/refs/remotes/origin/main.lock` (`Operation not permitted`). The loose ref was a regular file owned by the current user (0644); `.git/refs/remotes/origin` and parent directories were also user-owned (0755), with no ACL. The environment's workspace policy denies writes under `.git`; the fast-forward push succeeded when run with Git-metadata write access and updated the local tracking ref. No permission or ownership changes were made.

## Verified behavior

The regression `copying_a_table_range_into_following_text_preserves_cells_through_edit_undo_docx_and_pdf`
in `crates/engine/src/tests.rs` starts a selection at the first cell of a table and extends it into
the following body paragraph. It copies and pastes through the registered engine commands, verifies
both cell contents and direct bold formatting, checks Undo and Redo, saves and reopens DOCX, and
confirms PDF export succeeds. The lower-level document test also verifies the copied fragment
retains a table node and its cell structure.

This exposed a bug in `Document::copy_blocks`: its cross-container branch emitted paragraphs from
the selection but omitted a table node even when the range covered the entire table. The shared
document layer now copies a complete table node when its selection boundaries prove the whole table
is included. A range ending or beginning inside a table retains the existing flattened behavior;
partial table selection copying remains a limitation.

The wider editing suite also covers typing, paragraph breaks, Unicode, list behavior, tracked
changes, clipboard text, Undo/Redo and document recovery in `crates/engine/src/tests.rs` and
`crates/engine/src/tests_typing.rs`. These synthetic tests establish WordCraft behavior only. No
claim of Microsoft Word 365 editing compatibility follows without genuine reference documents and
application-level comparison.

## Platform validation for Phase 7N

On native Apple Silicon macOS, all six `cargo xtask ci` gates, workspace tests, a release GUI/CLI
build, and CLI version smokes passed. Hosted [Document regressions run 38390805361](https://github.com/Shabirahmad427/wordcraft/actions/runs/38390805361)
passed the document/session suites and native CLI smoke on Linux, macOS and Windows, plus the Linux
GUI/CLI release build, visual regression and advisory performance jobs. The hosted Linux GUI target
was built but not launched. This phase has no native Linux or Windows GUI runtime evidence.

## DOCX preservation boundary

For imported packages whose unsupported OOXML cannot be mapped safely, the package-preservation
guard rejects edits that would rewrite protected package parts. This phase does not widen that
guard or test arbitrary unsupported extensions. See [DOCX compatibility](docx-compatibility.md)
for the preservation contract and known limits.
