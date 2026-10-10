# Local WordCraft bug audit — 2026-10-10

Base: upstream `004286e`. Integration target: `Shabirahmad427/wordcraft` main.
Validated fixes are pushed to the fork main; upstream issues and pull requests remain open.

## Checked individually

| Issue | Commit | Result and evidence |
|---|---|---|
| #121, #59: font picker freezes / Thai fallback performance | `2e5a5b7` (PR #233) | Installed. Visible-row virtualization and bounded fallback caching regression tests pass. The original Windows freeze was not reproduced on this Linux host. |
| #219: missing font weights | `4a74649` (PR #239) | Installed. Installed-family legacy-name resolution, variable-instance discovery, and font tests pass. |
| #67: zoom starts from stale manual scale | `8fd6143` (PR #232) | Installed. Fit-mode UI regression and engine zoom stepping tests pass. |
| #69: View Gridlines does nothing | `3567484`, `5074b23` (PR #231) | Installed. Separate page/table switches and DOCX drawing-grid round-trip tests pass; page grid and Zoom In/Out visually checked offscreen. |
| #240: Select Recipients has no data dialog | `dfa82eb` | Fixed locally. Empty user invocation opens a CSV entry dialog. Submit uses the existing engine command; invalid input stays open; scripts still require data. Actual dialog-button regression passes. |
| #229: Reject All leaves tracked paragraph breaks | `cd6f141` | Reproduced with the installed CLI: two paragraphs remain after Reject All. Fixed locally: join on rejection, include breaks in change reporting, preserve mark revisions in DOCX. Tests cover start/middle/end splits, Unicode, Undo/Redo, DOCX reopening, individual Accept/Reject, formatting, and table cell boundaries. |
| #217: mouse cannot resize tables | Current table resizing commit | Fixed locally for unmerged column borders and rows that are not split across pages. Drag sends one existing engine command on release; Escape cancels; stale document/revision cancels; protected documents cannot start drags. Regression checks row/column model values, rendered geometry, and one-step Undo. Row height remains a minimum, so text can require more room. |
| #195: Close loses unsaved work | Upstream `802ba30` | Existing fix verified by `close_asks_before_quitting` and `window_close_asks_first` in the full passing suite. |
| #187: Open replaces unsaved work | Upstream `802ba30` | Existing fix verified by `open_asks_before_discarding_unsaved_work`, cancellation, and save-prompt tests. |
| #168: lossy exports clear dirty state | Upstream `802ba30` | Existing fix verified by `web_downloads_that_drop_content_leave_the_document_unsaved` and save-prompt tests. Browser download delivery was not manually tested. |
| #94: synchronous Linux file dialogs freeze the UI | PR #246 plus lifecycle guard | Native dialogs now complete asynchronously. Regression tests cover delayed saves, cancellation, single-dialog enforcement, deferred Close, and stale responses after document replacement. Live desktop portal behavior remains unverified. |
| #241: missing Chinese UI glyphs in Windows release | PR #248 | Updated release font pin, required Japanese/Chinese manifest coverage, bounded installed-font fallback and coverage tests. Windows release behavior remains unverified on this Linux host. |

## Validation

Full `cargo xtask ci` passed separately for the recipient dialog, paragraph-break review, and table resizing: formatting, Clippy with warnings denied, workspace tests, attribution, dependency layering, and WASM checks. Logs: `/tmp/wordcraft-issue-{240,229,217}-ci.log`.

Open issue status alone does not establish that a bug is present in the current build. Reports not covered above remain unverified; platform-specific reports require their respective operating systems.
