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

The desktop host implements the platform-independent `PrintBackend` contract. The existing
`NativePrintBackend` name remains as a compatibility alias. The WordCraft Print page is the
integrated printer-selection and options dialog on desktop; the generated PDF remains the common
print payload.

Linux and macOS use the system CUPS command interface: `lpstat` discovers queues,
`lpoptions -l` reports available queue options, `lp` submits the selected PDF, and `cancel` stops a
queued job. The app exposes discovered media and duplex settings, copies/collation, orientation and
fit-to-page requests. The printer's default media remains selected until the user picks a size.
This uses CUPS rather than an AppKit `NSPrintPanel`. Printer drivers can apply additional
device-specific transformations after preview; WordCraft cannot render those transformations in its
page thumbnails. Missing queues and rejected options produce an error and retain the PDF-viewer
fallback where available.

Windows uses the `winprint` adapter with the system `Windows.Data.Pdf` renderer and XPS
PrintTicket/PrintCapabilities pipeline. The compile-checked backend discovers installed printers,
offers the media and duplex options the driver reports, and passes copies, collation, orientation
and fit-to-page through a driver-validated PrintTicket. Invalid or unsupported choices return an
error before the PDF is submitted. The adapter currently waits for Windows' print pipeline to
complete the submission call; the UI can appear unresponsive during a slow driver operation. The
wrapper does not return a cancellable spool-job identifier, so WordCraft cannot cancel an active
Windows job; users can cancel it through the system print queue. Windows printer status, collation
and scaling remain subject to driver support.

The macOS AArch64 and Windows x86_64 GNU targets both passed `cargo check` from Linux. Those checks
verify compilation only: no native OS runtime, native print dialog, driver, or physical printer was
available. The Mac implementation is CUPS-based rather than AppKit-based. The web build delegates
to the browser's PDF print dialog and does not enumerate printers.

Tests cover one-based page ranges, selected-page PDF output, multi-page preview, shared option
validation, printer setting pass-through, cancellation, and CUPS option parsing and command
construction. Existing PDF and DOCX tests cover academic layouts, footnotes, endnotes, tables,
charts, drawings, recovery and save/reopen. This verifies internal layout reuse and document
preservation; there was no physical-printer test or independent Word print comparison. The
Windows-specific PrintTicket path was compile-checked but could not be exercised against a driver.
CUPS and Windows options vary by printer; WordCraft only applies settings that the driver accepts.
