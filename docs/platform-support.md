# Native desktop platform support

> **Last reviewed:** 2026-10-13 · **Last updated:** 2026-10-13 · **Change:** Phase 7T local macOS DMG validation and hosted package workflow added · **Target:** Linux, macOS and Windows desktop builds

This matrix distinguishes source/build configuration, hosted checks, a running native window and
physical desktop interaction. A passing shared-core test or cross-target build does not establish
that a desktop application launches or works on that target.

## Requested OS and architecture matrix

| Target | Build and packaging support | Hosted checks | Native GUI launch and document workflow | Remaining runtime gaps |
|---|---|---|---|---|
| Linux x86_64, X11 | eframe/winit X11 support; Linux package script emits x86_64 packages. Main CI builds the Linux GUI/CLI. | Printing run 38476284508 passed native startup, focus, Unicode/bold editing, Insert › Table, cell text, Undo/Redo, DOCX/PDF workflow and normal `app.quit` under Xvfb/Openbox/software GL. The discovered 2×2 target center `(166, 200)` resolved the prior fixed point `(166, 190)`, which landed between rows. | Local physical Linux desktop not available. | No physical X11 desktop, Wayland compositor, desktop-entry install, open-with or printer workflow was run locally. |
| Linux x86_64, Wayland | eframe enables Wayland. Linux prefers X11 when `DISPLAY` is set for drag/drop; set `WORDCRAFT_WAYLAND=1` to select native Wayland. Clipboard has Wayland data-control support with X11 fallback. | No Wayland compositor job exists in current CI. | Not run. The Xvfb check is X11 and does not establish Wayland behavior. | Needs a compositor-backed job (for example, a controlled Weston session) and desktop-level drag/drop/dialog checks. |
| Linux aarch64 | Linux release workflow builds/packages on an ARM64 runner; Flatpak and other package outputs are configured for ARM64. | Release workflow only; current document regression GUI build is x86_64. | Not run in this phase. | Native ARM64 GUI launch and editing remain unverified. |
| macOS Apple Silicon (arm64) | Native Rust target and macOS app bundle script; local release GUI/CLI build. | Printing run 38476284508 passed both native-window smokes on `macos-latest` and reported `macos-aarch64`. | Hosted LaunchServices smokes use fresh profiles; editing, DOCX/PDF workflow, `app.quit`, eframe exit callback, recovery snapshot and focus readiness passed. `open -W` waiters returned 0 and control keys were removed. A raw-executable launch from a non-interactive test launcher stalls in AppKit before winit resume; the same occurs in a minimal eframe probe with wgpu and glow. | The underlying macOS condition behind the raw-launch stall is not narrowed to a specific environment setting. The separate earlier release launch's reported exit 143 has no identified sender. Physical input, native panels, Finder association, printer and Word 365 comparison remain untested. |
| macOS Intel (x86_64) | Release packaging script requests a universal app and compiles both Apple Silicon and Intel slices. | Printing run 38476284508 logged `macos-aarch64`; it did not exercise Intel. | Not run. | No Intel Mac runtime evidence. |
| Windows x64 | Windows packaging supports x64 MSI/portable outputs; eframe Windows and Win32/WinRT adapters are configured. | Printing run 38476284508 passed native focus, Unicode/bold editing, Insert-tab activation, table insertion, cell text, Undo/Redo, DOCX/PDF workflow and normal `app.quit`. It reported `windows-x86_64`; the table-cell center was discovered from live widget bounds. | No local Windows machine. | Physical user input, native panels, file association, printer selection and Word 365 comparison remain untested. |
| Windows ARM64 | Packaging cross-compiles the ARM64 executable and MSI from x64. | Scheduled Windows 11 ARM job installs/checks/uninstalls packages and runs the CLI natively. It does not launch the GUI. | Not run locally; Phase 7S window smoke is not yet configured on the ARM64 runner. | No GUI launch or interactive workflow evidence; do not infer it from package installation or CLI execution. |

## Shared and native integration boundaries

- Document editing, layout, DOCX I/O, PDF export, and the egui UI are shared Rust code. The desktop
  host supplies windowing through eframe/winit and small OS adapters.
- X11/Wayland selection and drag/drop constraints live in the desktop startup adapter. Clipboard
  serialization is shared; OS transport is in the desktop clipboard adapter and uses arboard plus
  platform-specific RTF/format enumeration where needed.
- macOS Finder document-open events use the `fmv-macos-events` adapter. The packaged app declares
  bundle metadata and document types. Neither this metadata nor a successful command-line path open
  proves Finder association behavior.
- Linux and macOS printing use the CUPS command interface; Windows uses Windows printer APIs and
  the XPS pipeline. PDF export is shared. No physical printer has been used in this phase.
- Fonts are discovered from the host system and mapped by the shared font crate. Substitution can
  differ with installed fonts; identical raster pixels are not expected across systems.
- Window scale, geometry, rendering backend, portals, system dialogs and file associations are
  host responsibilities. This phase does not establish parity for every desktop environment.

## Phase 7S automated native-window check

`apps/wordcraft/tests/native_window.rs` contains opt-in integration tests. On macOS the harness
starts an isolated app bundle through LaunchServices; Linux and Windows start the GUI executable.
The editing smoke uses a fresh home/config profile, `WORDCRAFT_NO_PREFS=1`, and a random available
loopback port. It opens a blank synthetic document, drives pointer and keyboard events through the
running window's authenticated control channel, inserts a table from the ribbon, immediately types
in its first cell, checks Undo/Redo, saves and reopens a DOCX, exports a PDF and validates a window
PNG. It asserts no preferences, recovery store, logs or user fonts were created. A second smoke
enables preferences only under another fresh temporary profile, types unsaved text, quits through
`app.quit`, and checks the final recovery snapshot. Both require successful launcher completion,
WordCraft's `on_exit` marker and control-key cleanup. On macOS, `open -W` is the waited process;
the app's normal shutdown is confirmed independently through its callback, cleanup and disappearance.
They print the OS and architecture recorded by the runner.

Run only with a graphical session available:

```sh
WORDCRAFT_NATIVE_WINDOW_SMOKE=1 cargo test -p wordcraft --test native_window -- isolated_native_ --ignored --nocapture --test-threads=1
```

The `native_startup_stage_matrix` is a separate diagnostic-only test for raw-executable launches.
It compares normal/control startup and preference modes with temporary data, redirects Cocoa's
fixed home and font search for its existing-HOME case, and uses bounded observations. Earlier
raw-launch children stalled before the first resume and were force-terminated for diagnostic
cleanup; none counts as a successful launch. The app-bundle smoke runs separately from an
interactive Terminal locally; the Printing workflow is configured to run Linux under Xvfb/software
GL and macOS/Windows on their native hosted runners. Printing run
[38467901780](https://github.com/Shabirahmad427/wordcraft/actions/runs/38467901780) passed both
macOS smokes. Printing run
[38476284508](https://github.com/Shabirahmad427/wordcraft/actions/runs/38476284508) passed native
editing on Linux, macOS and Windows, including table text, Undo/Redo, DOCX/PDF checks and normal
quit. Linux ran with Xvfb/Openbox/software GL; Windows used its DirectX 12 software adapter; macOS
used LaunchServices on Apple Silicon. Native results are distinct from headless tests and offscreen
rendering. The profiles were isolated, and the recovery shutdown checks used only their own
temporary test data.
Control-channel pointer/key/text events exercise the actual app window and UI code, but they are
injected events, not physical mouse or keyboard input. The harness records child status and stderr;
timeout cleanup never counts as success. The smoke does not use the system clipboard or open a
native file picker; the separate clipboard adapter test covers host transport on the hosted runners.
Hosted completion and per-OS results are recorded in
[desktop GUI validation](desktop-testing.md).

No platform is considered fully validated by this matrix. Linux Wayland, macOS Intel, Windows
ARM64 GUI, OS-level file association/dialog behavior, physical printer selection and Word 365
comparisons remain open runtime checks.

## Phase 7T installed-package checks

The [native installation guide](native-installation.md) lists package formats, update and
rollback behavior, and the separate profile locations. The
[native package workflow](../.github/workflows/native-packages.yml) builds and installs a Linux
DEB, macOS DMG app and Windows x64 MSI, then points the existing native-window smoke at the
installed application. The Linux job also checks desktop and DOCX MIME metadata; the Windows job
checks Start Menu and Open With registry entries; the macOS job checks bundle metadata, signing
and a LaunchServices DOCX Open With event. These hosted jobs are pending at this revision; the
workflow definition alone is not runtime evidence.

Locally on Apple Silicon, the corrected arm64 DMG was mounted and copied into a disposable
Applications directory. Its copied app passed strict code-signature verification and both
isolated native smokes, including editing, DOCX/PDF output, Finder-style Open With and graceful
quit. The first DMG failed verification because its HFS image introduced FinderInfo attributes
after signing. The local corrected artifact uses an ad-hoc signature and has not been notarized.
The earlier `kLSNoExecutableErr` remains unexplained; this successful packaged launch is a
separate observation. Intel macOS, Windows ARM64 GUI, Linux Wayland and physical device behavior
still need runtime checks.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-13 | minor | Recorded hosted Linux startup success but text-input failure, and Windows tab-boundary click root cause plus local macOS verification of the corrected coordinate |
| 2026-10-13 | minor | Printing run 38476284508 passed all three native smoke jobs with editing, table insertion, Undo/Redo and normal quit; raw native input and broader desktop integration remain separate gaps |
| 2026-10-13 | major | Began Phase 7T with a verified macOS DMG signature correction, installed-app native smoke and a pending three-OS package workflow |
