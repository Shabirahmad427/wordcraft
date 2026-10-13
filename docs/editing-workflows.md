# Editing workflow regression coverage

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** Phase 7N added a cross-container rich-copy regression for tables.

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

## DOCX preservation boundary

For imported packages whose unsupported OOXML cannot be mapped safely, the package-preservation
guard rejects edits that would rewrite protected package parts. This phase does not widen that
guard or test arbitrary unsupported extensions. See [DOCX compatibility](docx-compatibility.md)
for the preservation contract and known limits.
