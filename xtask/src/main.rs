//! Workspace tooling: `cargo xtask <command>`.
//!
//! Pure Rust (std + serde_json). External tools (`cargo`, `curl`, `tar`) are
//! invoked through `std::process::Command`.

mod assets;
mod ico;
mod interop;
mod layers;
mod parity;
mod perf;
mod stats;
mod version;
mod visual;
mod visual_diff;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  assets          check that every icon/image/font/asset is attributed in ATTRIBUTION.md
  layers          enforce the crate dependency layering (plan/architecture.md §3)
  wasm            cargo check --target wasm32-unknown-unknown for the wasm-safe crates
  fast [--base REV] [--plan]
                  fmt plus tests and focused Clippy for changed workspace packages
  standard        run the complete Linux Rust workspace CI gates (same as ci)
  full            dispatch the GitHub-hosted Linux/macOS/Windows and additional-platform suite
  ci              fmt --check, clippy -D warnings, test, assets, layers, wasm (stops at first failure)
  corpus [--download]
                  show where test corpora live; --download fetches PngSuite into corpus/pngsuite
  interop inspect <docx> <report.json>
                  fingerprint package parts and relationships
  interop compare <original.docx> <roundtrip.docx> <case.json> <report.json>
                  enforce a per-case allowlist of changed package parts
  visual compare <expected.png> <actual.png> <policy.json> <report.json>
                  compare page rasters; requires explicit tolerances, never updates baselines
  visual compare-pages <expected-dir> <actual-dir> <policy.json> <report.json> <diff-dir>
                  compare numbered page sets, check page counts and write changed-page diffs
  perf summarize <metrics.tsv> <summary.json>
                  summarize benchmark samples with median, MAD and peak RSS
  perf compare <baseline.json> <current.json> <report.json>
                  write advisory performance deltas; never fails on timing changes
  stats [--exact] count tests and lines per crate (--exact: ask the test harness via `-- --list`)
  parity          recompute the Word feature-parity summary in docs/parity-checklist.md
  visual-diff <ref.pdf> <test.pdf> <outdir> [--private] [--dpi N]
                  compare two PDFs of a document page by page (SSIM, line breaks, offsets);
                  needs poppler (pdftoppm, pdftotext)
  ico <out.ico> <in.png>...
                  pack square PNGs (<= 256 px) into a Windows .ico (see packaging/icons.sh)
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("assets") => assets::run(&root()),
        Some("layers") => cmd_layers(),
        Some("wasm") => cmd_wasm(),
        Some("ci") => cmd_ci(),
        Some("fast") => cmd_fast(&rest),
        Some("standard") => cmd_ci(),
        Some("full") => cmd_full(),
        Some("corpus") => cmd_corpus(rest.contains(&"--download")),
        Some("stats") => stats::run(&root(), rest.contains(&"--exact")),
        Some("parity") => parity::run(&root()),
        Some("ico") => ico::run(&rest),
        Some("interop") => interop::run(&args[1..]),
        Some("visual") => visual::run(&args[1..]),
        Some("perf") => perf::run(&args[1..]),
        Some("visual-diff") => visual_diff::run(&rest),
        Some("version") => version::run(&root(), &rest),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask has a parent dir").to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("{what}: exited with {status}")) }
}

fn git_names(args: &[&str]) -> Result<Vec<String>, String> {
    let out = Command::new("git").current_dir(root()).args(args).output().map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).lines().filter(|s| !s.is_empty()).map(str::to_owned).collect())
}

fn changed_paths(base: Option<&str>) -> Result<Vec<String>, String> {
    let mut changed = if let Some(base) = base.filter(|s| !s.chars().all(|c| c == '0')) {
        git_names(&["diff", "--name-only", &format!("{base}...HEAD")])?
    } else {
        Vec::new()
    };
    // Include staged, unstaged, and untracked local edits even when a base ref was supplied.
    changed.extend(git_names(&["diff", "--name-only", "HEAD"])?);
    changed.extend(git_names(&["ls-files", "--others", "--exclude-standard"])?);
    changed.sort();
    changed.dedup();
    Ok(changed)
}

fn changed_packages(root: &Path, paths: &[String], metadata: &serde_json::Value) -> Result<Vec<String>, String> {
    let packages = metadata["packages"].as_array().ok_or("cargo metadata omitted packages")?;
    let members: std::collections::BTreeSet<&str> = metadata["workspace_members"]
        .as_array()
        .ok_or("cargo metadata omitted workspace_members")?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let root_manifest = PathBuf::from(root).join("Cargo.toml");
    let workspace_change = paths.iter().any(|p| p == "Cargo.toml" || p == "Cargo.lock");
    let mut selected = std::collections::BTreeSet::new();
    for package in packages {
        let id = package["id"].as_str().ok_or("cargo metadata package omitted id")?;
        if !members.contains(id) {
            continue;
        }
        let name = package["name"].as_str().ok_or("cargo metadata package omitted name")?;
        let manifest = package["manifest_path"].as_str().ok_or("cargo metadata package omitted manifest_path")?;
        if workspace_change {
            selected.insert(name.to_owned());
            continue;
        }
        let manifest = PathBuf::from(manifest);
        let package_root = manifest.parent().ok_or("package manifest has no parent")?;
        if paths.iter().any(|path| {
            let candidate = root.join(path);
            candidate.starts_with(package_root) || (package_root == root && candidate == root_manifest)
        }) {
            selected.insert(name.to_owned());
        }
    }
    Ok(selected.into_iter().collect())
}

fn cmd_fast(args: &[&str]) -> Result<(), String> {
    let mut base = None;
    let mut plan = false;
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--base" => {
                i += 1;
                base = Some(*args.get(i).ok_or("usage: cargo xtask fast [--base REV] [--plan]")?);
            }
            "--plan" => plan = true,
            _ => return Err(format!("unknown fast option `{}`\nusage: cargo xtask fast [--base REV] [--plan]", args[i])),
        }
        i += 1;
    }
    let paths = changed_paths(base)?;
    let selected = changed_packages(&root(), &paths, &metadata()?)?;
    if plan {
        println!("FAST plan: fmt check, then tests and focused Clippy for {} changed package(s).", selected.len());
        for package in &selected {
            println!("  cargo test --locked -p {package}");
            println!("  cargo clippy --locked -p {package} --all-targets -- -D warnings");
        }
        if selected.is_empty() {
            println!("  no Rust package changes; no package build needed");
        }
        return Ok(());
    }
    let mut fmt = cargo();
    fmt.args(["fmt", "--all", "--", "--check"]);
    run(fmt, "cargo fmt --all -- --check")?;
    for package in &selected {
        let mut test = cargo();
        test.args(["test", "--locked", "-p", package]);
        run(test, &format!("cargo test --locked -p {package}"))?;
        let mut clippy = cargo();
        clippy.args(["clippy", "--locked", "-p", package, "--all-targets", "--", "-D", "warnings"]);
        run(clippy, &format!("cargo clippy --locked -p {package} --all-targets -- -D warnings"))?;
    }
    if selected.is_empty() {
        println!("FAST passed: formatting is clean; no changed workspace packages needed compilation.");
    } else {
        println!("FAST passed for {} changed workspace package(s).", selected.len());
    }
    Ok(())
}

fn cmd_full() -> Result<(), String> {
    let branch = git_names(&["branch", "--show-current"])?.into_iter().next().unwrap_or_default();
    if branch.is_empty() {
        return Err("FULL dispatch requires a named Git branch; check out the branch to validate".into());
    }
    let mut gh = Command::new("gh");
    gh.current_dir(root()).args(["workflow", "run", ".github/workflows/full-validation.yml", "--ref", &branch]);
    run(gh, &format!("gh workflow run .github/workflows/full-validation.yml --ref {branch}"))?;
    println!("FULL validation dispatched for {branch}. Track it with `gh run list --branch {branch}`.");
    Ok(())
}

pub fn metadata() -> Result<serde_json::Value, String> {
    let out = cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed:\n{}", String::from_utf8_lossy(&out.stderr)));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: bad JSON: {e}"))
}

fn cmd_layers() -> Result<(), String> {
    let crates = layers::from_metadata(&metadata()?)?;
    println!("Dependency layering (plan/architecture.md §3)\n");
    println!("{:<28} {:<14} workspace deps", "crate", "layer");
    for c in &crates {
        let ws: Vec<String> = c
            .deps
            .iter()
            .filter(|d| d.workspace)
            .map(|d| {
                let k = match d.kind {
                    layers::DepKind::Normal => "",
                    layers::DepKind::Dev => " (dev)",
                    layers::DepKind::Build => " (build)",
                };
                format!("{}{k}", layers::short_name(&d.name))
            })
            .collect();
        println!("{:<28} {:<14} {}", c.name, layers::describe(layers::classify(&c.name)), ws.join(", "));
    }
    let violations = layers::check(&crates);
    println!();
    if violations.is_empty() {
        println!("OK: {} crates, no layering violations.", crates.len());
        Ok(())
    } else {
        println!("{} violation(s):", violations.len());
        for v in &violations {
            println!("  - {v}");
        }
        Err(format!("{} layering violation(s)", violations.len()))
    }
}

/// Workspace packages that must build for wasm32: all L0–L5 crates plus
/// the egui shell.
fn wasm_set() -> Result<Vec<String>, String> {
    let crates = layers::from_metadata(&metadata()?)?;
    Ok(crates
        .into_iter()
        .filter(|c| match layers::classify(&c.name) {
            Some(layers::Class::Layer(l)) => l <= 5 || layers::short_name(&c.name) == "ui-egui",
            Some(layers::Class::Standalone) => true,
            _ => false,
        })
        .map(|c| c.name)
        .collect())
}

fn cmd_wasm() -> Result<(), String> {
    let set = wasm_set()?;
    let mut results = Vec::new();
    for pkg in &set {
        let mut c = cargo();
        c.args(["check", "--target", "wasm32-unknown-unknown", "-p", pkg]);
        let ok = run(c, &format!("cargo check --target wasm32-unknown-unknown -p {pkg}")).is_ok();
        results.push((pkg.clone(), ok));
    }
    println!("\nwasm32-unknown-unknown check:");
    for (p, ok) in &results {
        println!("  {:<6} {p}", if *ok { "ok" } else { "FAIL" });
    }
    let failed = results.iter().filter(|r| !r.1).count();
    if failed == 0 { Ok(()) } else { Err(format!("{failed} crate(s) failed the wasm check")) }
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "fmt",
            Box::new(|| {
                let mut c = cargo();
                c.args(["fmt", "--all", "--", "--check"]);
                run(c, "cargo fmt --all -- --check")
            }),
        ),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets -- -D warnings")
            }),
        ),
        (
            "test",
            Box::new(|| {
                let mut c = cargo();
                c.args(["test", "--workspace"]);
                run(c, "cargo test --workspace")
            }),
        ),
        ("assets", Box::new(|| assets::run(&root()))),
        ("layers", Box::new(cmd_layers)),
        ("wasm", Box::new(cmd_wasm)),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            for (n, _) in steps.iter().skip(done.len() + 1) {
                println!("  skip  {n}");
            }
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}

const PNGSUITE_URL: &str = "http://www.schaik.com/pngsuite/PngSuite-2017jul19.tgz";

fn cmd_corpus(download: bool) -> Result<(), String> {
    let corpus = root().join("corpus");
    println!(
        "Test corpora live under {} (git-ignored, never committed).
Tests that use a corpus skip cleanly when it is absent.

  corpus/pngsuite/   PngSuite (public domain) — wordcraft-codecs compares every file
                     against the `image` crate. Fetch: cargo xtask corpus --download
  corpus/psd/        PSD samples from MIT/BSD projects (ag-psd, psd-tools test data).
                     Copy files in manually; licences must be MIT/BSD/CC0.
  corpus/tiff/       libtiff pics (optional)
  corpus/exr/        OpenEXR sample images (optional)
  corpus/raw/        raw.pixls.us samples, CC0 (optional)
",
        corpus.display()
    );
    if !download {
        return Ok(());
    }
    let dest = corpus.join("pngsuite");
    std::fs::create_dir_all(&dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let tgz = corpus.join("PngSuite-2017jul19.tgz");
    let mut curl = Command::new("curl");
    curl.args(["-fsSL", "-o"]).arg(&tgz).arg(PNGSUITE_URL);
    run(curl, &format!("curl {PNGSUITE_URL}"))?;
    let mut tar = Command::new("tar");
    tar.arg("-xzf").arg(&tgz).arg("-C").arg(&dest);
    run(tar, "tar -xzf PngSuite-2017jul19.tgz")?;
    let _ = std::fs::remove_file(&tgz);
    let n = std::fs::read_dir(&dest).map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "png")).count()).unwrap_or(0);
    println!("PngSuite: {n} PNG files in {}", dest.display());
    if n == 0 { Err("no PNG files extracted".into()) } else { Ok(()) }
}

#[cfg(test)]
mod fast_tests {
    use super::changed_packages;
    use std::path::Path;

    fn metadata() -> serde_json::Value {
        serde_json::json!({
            "workspace_members": ["a-id", "b-id"],
            "packages": [
                {"id":"a-id", "name":"a", "manifest_path":"/repo/crates/a/Cargo.toml"},
                {"id":"b-id", "name":"b", "manifest_path":"/repo/apps/b/Cargo.toml"}
            ]
        })
    }

    #[test]
    fn fast_selects_only_packages_containing_changed_files() {
        let selected = changed_packages(Path::new("/repo"), &["crates/a/src/lib.rs".into()], &metadata()).unwrap();
        assert_eq!(selected, ["a"]);
    }

    #[test]
    fn workspace_manifest_and_lock_changes_select_every_package() {
        for path in ["Cargo.toml", "Cargo.lock"] {
            let selected = changed_packages(Path::new("/repo"), &[path.into()], &metadata()).unwrap();
            assert_eq!(selected, ["a", "b"]);
        }
    }
}
