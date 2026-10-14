# Native installation and package validation

> **Last reviewed:** 2026-10-14 · **Last updated:** 2026-10-14 · **Status:** Phase 7T in progress · **Target:** Linux, macOS and Windows packages

WordCraft uses one Rust desktop application. The release scripts in `packaging/` make Linux
AppImage, DEB, RPM, tarball and Flatpak outputs, a macOS DMG containing `WordCraft.app`, and
Windows MSI and portable ZIP outputs. The release workflow builds additional architectures.
The [native package workflow](../.github/workflows/native-packages.yml) installs generated DEB,
DMG and x64 MSI packages on disposable hosted runners and runs the same native editing smoke used
for source builds. A release is published only by the separate `release` branch workflow.
The Linux job also launches the installed app against headless Weston with native Wayland selected;
because that compositor has no input devices, this checks startup, rendering, recovery and quit,
while the X11 job checks UI input.
This fork has no published release yet; the upstream download links in the README are upstream
artifacts and do not include this fork's Phase 7T changes. Successful native package jobs upload
short-lived test artifacts. The macOS test artifact is ad-hoc signed and unnotarized; the Windows
test MSI is unsigned when signing secrets are unavailable.

## Install, update, roll back and remove

| System | Install and update | Roll back | Remove without deleting documents |
|---|---|---|---|
| Linux DEB/RPM | Use the distribution package manager on a downloaded release file. The package owns `/usr/bin/wordcraft`, `/usr/bin/wordcraft-cli` and `/usr/share` desktop, MIME and icon files. Install a newer package through the same manager. | Install the earlier release package if the package manager permits downgrades. Keep a copy of the earlier package before updating. | Remove the `wordcraft` package through the package manager. The post-remove script only refreshes desktop, MIME and icon caches; it does not address home directories. |
| Linux AppImage/tarball | Run or unpack in a user-chosen location. AppImage metadata advertises zsync updates when a matching `.zsync` is published. | Run or unpack the earlier artifact. | Remove only the downloaded artifact or unpacked application tree. |
| Linux Flatpak | `flatpak install --user <bundle>` and install a newer bundle the same way. | Install an earlier bundle. | `flatpak uninstall --user ai.storyteller.wordcraft`; do not request data deletion when preserving application data. |
| macOS | Open the DMG and copy `WordCraft.app` to Applications after quitting the old app. Finder's Open With menu can open a DOCX in WordCraft; the bundle declares document types without taking over the user's defaults. | Quit WordCraft and restore an earlier app bundle from its DMG. | Move only `WordCraft.app` to Trash. Leave `~/Library/Application Support/WordCraft` intact. |
| Windows MSI | Run the x64 installer (or the ARM64 installer on a Windows ARM PC). MSI major upgrades replace the application in Program Files and register Open With, Default apps and a Start Menu shortcut without forcing a DOCX default. | Uninstall the newer MSI and install an earlier one; the MSI blocks a direct downgrade. | Use Installed apps or `msiexec /x`. The package does not include `%APPDATA%\WordCraft` or user documents. |
| Windows portable ZIP | Extract to a new folder and run the executable; replace the application folder for updates. | Run the earlier extracted folder. | Remove only that extracted application folder. |

Do not put documents, preferences or recovery files inside an application install folder. The
default application profile is separate from package-owned files. Before replacing an app bundle
manually, quit WordCraft so the executable in use and any unsaved writing are not disrupted.
Package-manager rollback is separate from document-format compatibility; keep document backups
when trying older application versions.

## Package identity and signing

- The package scripts set `WORDCRAFT_BUILD_SHA` from the checkout. Installed
  `wordcraft-cli --version` prints the full commit; the GUI `--version` prints its short commit.
  The macOS `Info.plist` stores the full commit, and Windows VERSIONINFO has a `BuildCommit` field.
  A local build from a dirty checkout appends `-dirty` to its base commit, so it cannot be
  mistaken for an exact source snapshot; published artifacts come from clean GitHub Actions
  checkouts.
- The local Apple Silicon DMG in this phase is **ad-hoc signed and not notarized**. The release
  workflow uses a configured signing identity and notarization credentials when available. No
  local notarization claim is made.
- The macOS package script verifies the signed app before imaging and again from the final DMG.
  HFS `makehybrid` otherwise adds `com.apple.FinderInfo` to signed bundle files, which makes a
  copied app fail strict code-signature verification. The script removes those attributes in a
  writable image before compression and checks the Applications link and final signature.

## Validation evidence and limits

The local Apple Silicon run built a real arm64 DMG and CLI ZIP from the Phase 7T working tree.
The original DMG's mounted and installed apps failed `codesign --verify --strict --deep` on an
icon's FinderInfo attribute. After the packaging correction, both passed. An app copied from the
corrected DMG launched through LaunchServices with a fresh profile; the native smoke edited text,
formatted it bold, inserted a table, exercised Undo/Redo, saved and reopened DOCX, exported PDF,
delivered a DOCX via LaunchServices Open With, then quit through `app.quit`. Both isolated native
smokes passed, including a recovery checkpoint in the preferences-enabled test profile. The
no-preferences test left no preferences, recovery directory, logs or user fonts in its profile.
The app's exit callback ran, its control key was removed, and no process from the test bundle
remained. These tests use injected control events, not physical keyboard or mouse input.

A locally built universal DMG contained both arm64 and x86_64 GUI/CLI slices and its copied app
passed strict signature verification. The x86_64 GUI was launched with LaunchServices
`open --arch x86_64` under Rosetta and passed both isolated native smokes; the x86_64 CLI also
ran through `arch -x86_64`. No Intel Mac hardware was used.

The earlier local `kLSNoExecutableErr` from a different native launch has no established cause.
This successful packaged launch does not explain that older error. Finder default selection,
Spotlight indexing after an `/Applications` install, macOS Intel hardware, Linux Wayland, physical
printing, and Windows ARM64 GUI runtime need separate evidence. The existing ARM64 MSI job checks
native installation and CLI execution, not the GUI. The Flatpak bundle script installs and runs
the CLI, not a sandboxed GUI edit workflow. In hosted package run 38479505495, the installed DEB
and x64 MSI both passed their native edit/save/export/quit smokes and uninstall preservation
checks. The macOS DMG mounted, copied and passed strict signature verification, but its job stopped
at an incorrectly ordered `lipo -verify_arch` check before the packaged GUI smoke. The corrected
package run 38502639179 passed on Linux, macOS and Windows, including headless Weston startup,
recovery and quit on Linux. The Windows GUI step failed once before passing on a retry of the
same job: its first attempt timed out reading the control response immediately after queuing text,
and the harness force-terminated the still-running child after the assertion failure. The retry
passed both native smoke tests with normal shutdown. The one-off Windows timeout has no identified
cause and is not considered fixed; Phase 7T remains in progress. Weston headless has no input
devices, so its Wayland result covers startup/recovery/quit, not Wayland input.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-13 | major | Audited existing packages, corrected the macOS DMG signature defect, added installed-package native smoke paths and documented current runtime limits |
| 2026-10-14 | minor | Recorded passing Linux/macOS/Windows installed-package run, headless Wayland startup and one unexplained Windows control timeout followed by a passing retry |
