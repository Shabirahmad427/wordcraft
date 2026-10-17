# Development CI modes

WordCraft uses three validation levels so routine changes get quick feedback while release
validation keeps its full platform coverage.

## FAST

```sh
cargo xtask fast
cargo xtask fast --base origin/main
cargo xtask fast --base origin/main --plan
```

FAST always checks formatting, then runs tests and focused Clippy with warnings denied for each
changed workspace package. The default compares the current working tree with `HEAD`; `--base`
also selects committed changes since that revision. `--plan` prints selected checks without
running them. Root `Cargo.toml` or `Cargo.lock` changes select every workspace package. Changes
outside a Rust package still get the formatting check. FAST runs automatically on non-documentation
pushes and pull requests using a warm Cargo cache. Linux is its hosted runner; the same command
works locally on Ubuntu and macOS. A passing FAST run is package-scoped evidence, not full workspace
or cross-platform validation.

## STANDARD

```sh
cargo xtask standard
# Hosted Linux runner: Actions → STANDARD → Run workflow
```

STANDARD is the existing `cargo xtask ci` gate set: formatting, workspace Clippy, full workspace
tests, asset attribution, dependency layering, and wasm checks. Workspace tests include the DOCX,
GUI, layout, and PDF regression suites that are not marked ignored. Hosted STANDARD is Linux-only
and can also be invoked from FULL. It does not imply a macOS or Windows pass. Allow up to 90 minutes
on a cold or drifted cache even though the warm-cache goal is below ten minutes.

## FULL

```sh
cargo xtask full
# Or: gh workflow run .github/workflows/full-validation.yml --ref main
```

FULL dispatches the `FULL validation` workflow for the current branch and needs an authenticated
GitHub CLI (`gh auth status`). It runs STANDARD plus Document regressions, Printing, Packaging lint,
Linux DEB/macOS DMG/Windows MSI installation and GUI checks, FreeBSD, and Windows ARM64. Every job
must pass; no platform is treated as optional. The release workflow calls FULL validation and waits
for it before starting release builds. A manual full run reports hosted results in GitHub Actions;
the command only dispatches the run.

The existing Printing, Document regressions, and Native packages workflows remain manually
dispatchable for focused diagnostics. Their ordinary PR/main triggers were removed to avoid
duplicating FAST with 10–37 minute full suites. Packaging lint remains path-triggered on PRs and
main because it usually completes in seconds. FreeBSD and Windows ARM64 also retain nightly runs.

At baseline `fe4d4024`, hosted runs took 12m18s for Document regressions, 22m49s for Native packages,
36m55s for Printing, 35m41s for FreeBSD, 14m27s for Windows ARM64, and 24s for Packaging lint. FAST
and STANDARD after this change are measured from their new run links in the Phase 7U.15 report;
time targets are goals, not CI timeouts. These samples are GitHub-hosted durations, not local build
times. Branch protection was not enabled on `main` when this workflow change was made, so there were
no required status-check names to rename. Release gating is enforced by the release workflow's
dependency on the FULL reusable workflow.
