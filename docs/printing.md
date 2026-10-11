# Printing and print preview

File › Print now renders page thumbnails from `Session::export_layout`, the same print-mode
`DocLayout` passed to `wordcraft_pdf::export_layout`. Page-range selection filters that layout's
zero-based page indexes before PDF generation, so the previewed selection and submitted PDF have
the same page count and pagination. The document itself is never edited by printing. Tracked
deletions are omitted from this final-text output, matching ordinary PDF export.

The preview lists pages with horizontal or vertical overflow diagnostics before submission. These
warnings report the existing layout result; they do not move, resize, hide or truncate content.
Page range input is one-based (`1-3,5`), rejects invalid, repeated or out-of-document pages, and an
empty range selects every page.

On Linux, the desktop host uses the CUPS command interface. It lists configured queues with
`lpstat`, reads printer options using `lpoptions -l`, submits the generated PDF with `lp`, and
cancels queued jobs with `cancel`. The dialog offers advertised media sizes and duplex modes,
copies, collation, landscape orientation and printer fit-to-page scaling. The printer's default
media is left selected until the user chooses a size. Queue defaults may also apply additional
printer-side transformations; the thumbnail preview shows WordCraft's document pages before those
device-specific transformations. If CUPS, a queue or an option is unavailable, WordCraft reports
the error and keeps the PDF-viewer route available where the host provides it. Closing the preview
before submitting cancels the pending action.

macOS uses the same CUPS command interface in the native host, but this implementation has not
been compiled or tested on macOS and no printer was available in the verification environment.
Windows keeps the existing behavior of opening the PDF in the system viewer; printer enumeration,
page/copy controls, duplex and job cancellation are not implemented in WordCraft on Windows. The
web build delegates to the browser's PDF print dialog and does not enumerate printers.

Tests cover one-based page ranges, selected-page PDF output, multi-page preview, pass-through of
printer settings, cancellation, CUPS option parsing and command argument construction. Existing PDF
and DOCX tests cover academic layouts, footnotes, endnotes, tables, charts, drawings, recovery and
save/reopen. This verifies internal layout reuse and document preservation; there was no physical
printer test or independent Word print comparison. CUPS options are defined by the installed queue
and vary by printer; WordCraft only exposes options it discovers.
