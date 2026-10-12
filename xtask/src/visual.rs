//! Deterministic page-raster comparison. The command only reads inputs and writes a report;
//! expected images are never changed implicitly.

use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;

const MAX_PIXELS: u64 = 25_000_000;
const MAX_PNG_BYTES: u64 = 128 * 1024 * 1024;
const MAX_POLICY_BYTES: u64 = 4 * 1024;
const USAGE: &str = "usage: cargo xtask visual compare <expected.png> <actual.png> <policy.json> <report.json>\n       cargo xtask visual compare-pages <expected-dir> <actual-dir> <policy.json> <report.json> <diff-dir>";

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Policy {
    max_mean_absolute_channel_error: f64,
    max_changed_pixel_percent: f64,
    changed_channel_threshold: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema: &'static str,
    width: u32,
    height: u32,
    mean_absolute_channel_error: f64,
    changed_pixel_percent: f64,
    max_channel_error: u8,
    changed_bounds: Option<Bounds>,
    policy_passed: bool,
    note: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Bounds {
    left: u32,
    top: u32,
    right: u32,
    bottom: u32,
}

fn read_png(path: &Path) -> Result<image::RgbaImage, String> {
    let bytes = read_bounded(path, MAX_PNG_BYTES, "raster")?;
    let reader =
        image::ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format().map_err(|error| format!("{}: {error}", path.display()))?;
    if reader.format() != Some(image::ImageFormat::Png) {
        return Err(format!("{}: expected a PNG page raster", path.display()));
    }
    let (width, height) = reader.into_dimensions().map_err(|error| format!("{}: {error}", path.display()))?;
    let pixels = u64::from(width).checked_mul(u64::from(height)).ok_or("image dimensions overflow")?;
    if width == 0 || height == 0 || pixels > MAX_PIXELS {
        return Err(format!("{}: image dimensions {width}x{height} are empty or exceed {MAX_PIXELS} pixels", path.display()));
    }
    let image = image::load_from_memory(&bytes).map_err(|error| format!("{}: {error}", path.display()))?.to_rgba8();
    if image.dimensions() != (width, height) {
        return Err(format!("{}: image dimensions changed while reading", path.display()));
    }
    Ok(image)
}

fn read_bounded(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
        return Err(format!("{}: {label} exceeds the {limit}-byte input limit", path.display()));
    }
    Ok(bytes)
}

fn compare(expected: &image::RgbaImage, actual: &image::RgbaImage, policy: Policy) -> Result<Report, String> {
    if expected.dimensions() != actual.dimensions() {
        return Err(format!("raster dimensions differ: expected {:?}, actual {:?}", expected.dimensions(), actual.dimensions()));
    }
    if !policy.max_mean_absolute_channel_error.is_finite()
        || !(0.0..=255.0).contains(&policy.max_mean_absolute_channel_error)
        || !policy.max_changed_pixel_percent.is_finite()
        || !(0.0..=100.0).contains(&policy.max_changed_pixel_percent)
    {
        return Err("visual thresholds must be finite and within their documented ranges".into());
    }
    let pixel_count = u64::from(expected.width()) * u64::from(expected.height());
    let mut absolute_error = 0u64;
    let mut changed_pixels = 0u64;
    let mut max_channel_error = 0u8;
    let mut bounds: Option<Bounds> = None;
    for (index, (before, after)) in expected.pixels().zip(actual.pixels()).enumerate() {
        let mut changed = false;
        for channel in 0..4 {
            let error = before[channel].abs_diff(after[channel]);
            absolute_error += u64::from(error);
            max_channel_error = max_channel_error.max(error);
            changed |= error > policy.changed_channel_threshold;
        }
        if changed {
            changed_pixels += 1;
            let x = u32::try_from(index % expected.width() as usize).map_err(|_| "pixel x coordinate overflow")?;
            let y = u32::try_from(index / expected.width() as usize).map_err(|_| "pixel y coordinate overflow")?;
            bounds = Some(match bounds {
                Some(old) => Bounds { left: old.left.min(x), top: old.top.min(y), right: old.right.max(x), bottom: old.bottom.max(y) },
                None => Bounds { left: x, top: y, right: x, bottom: y },
            });
        }
    }
    let mae = absolute_error as f64 / (pixel_count as f64 * 4.0);
    let changed_percent = changed_pixels as f64 * 100.0 / pixel_count as f64;
    Ok(Report {
        schema: "wordcraft.visual-raster/v1",
        width: expected.width(),
        height: expected.height(),
        mean_absolute_channel_error: mae,
        changed_pixel_percent: changed_percent,
        max_channel_error,
        changed_bounds: bounds,
        policy_passed: mae <= policy.max_mean_absolute_channel_error && changed_percent <= policy.max_changed_pixel_percent,
        note: "Comparison only; no reference raster or baseline was modified.",
    })
}

fn read_policy(path: &Path) -> Result<Policy, String> {
    let bytes = read_bounded(path, MAX_POLICY_BYTES, "visual policy")?;
    serde_json::from_slice(&bytes).map_err(|error| format!("{}: invalid policy JSON: {error}", path.display()))
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    use std::io::Write;
    output.write_all(bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn run_compare(expected: &Path, actual: &Path, policy_path: &Path, report_path: &Path) -> Result<(), String> {
    if report_path.exists() {
        return Err(format!("{}: report output already exists; choose a new path", report_path.display()));
    }
    let policy = read_policy(policy_path)?;
    let expected = read_png(expected)?;
    let actual = read_png(actual)?;
    let report = compare(&expected, &actual, policy)?;
    let encoded = serde_json::to_vec_pretty(&report).map_err(|error| format!("cannot encode visual report: {error}"))?;
    write_new(report_path, &encoded)?;
    println!(
        "{}x{}: MAE {:.4}, changed pixels {:.4}%, max channel delta {}; {}",
        report.width,
        report.height,
        report.mean_absolute_channel_error,
        report.changed_pixel_percent,
        report.max_channel_error,
        if report.policy_passed { "within policy" } else { "outside policy" }
    );
    if report.policy_passed { Ok(()) } else { Err("raster difference exceeds explicit policy; report was written".into()) }
}

fn page_files(dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let entries = std::fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let mut pages = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
        if let Some(number) = name.strip_prefix("page-").and_then(|name| name.strip_suffix(".png")).and_then(|n| n.parse::<usize>().ok()) {
            if number == 0 {
                return Err(format!("{}: page numbers start at 1", path.display()));
            }
            pages.push((number, path));
        }
    }
    pages.sort_by_key(|(number, _)| *number);
    for (offset, (number, _)) in pages.iter().enumerate() {
        if *number != offset + 1 {
            return Err(format!("{}: missing page-{:03}.png in page sequence", dir.display(), offset + 1));
        }
    }
    if pages.is_empty() {
        return Err(format!("{}: no page-N.png rasters found", dir.display()));
    }
    Ok(pages.into_iter().map(|(_, path)| path).collect())
}

fn diff_image(expected: &image::RgbaImage, actual: &image::RgbaImage, threshold: u8) -> image::RgbaImage {
    let mut diff = image::RgbaImage::new(expected.width(), expected.height());
    for (x, y, pixel) in diff.enumerate_pixels_mut() {
        let before = expected.get_pixel(x, y);
        let after = actual.get_pixel(x, y);
        let changed = (0..4).any(|channel| before[channel].abs_diff(after[channel]) > threshold);
        *pixel = if changed { image::Rgba([255, 32, 32, 255]) } else { image::Rgba([before[0] / 2, before[1] / 2, before[2] / 2, 255]) };
    }
    diff
}

fn run_compare_pages(expected_dir: &Path, actual_dir: &Path, policy_path: &Path, report_path: &Path, diff_dir: &Path) -> Result<(), String> {
    if report_path.exists() {
        return Err(format!("{}: report output already exists; choose a new path", report_path.display()));
    }
    if diff_dir.exists() {
        return Err(format!("{}: diff output already exists; choose a new path", diff_dir.display()));
    }
    let policy = read_policy(policy_path)?;
    let expected = page_files(expected_dir)?;
    let actual = page_files(actual_dir)?;
    let count_matches = expected.len() == actual.len();
    let mut page_reports = Vec::new();
    let mut all_passed = count_matches;
    for index in 0..expected.len().max(actual.len()) {
        let (Some(expected_path), Some(actual_path)) = (expected.get(index), actual.get(index)) else {
            page_reports.push(serde_json::json!({"page": index + 1, "passed": false, "error": "page missing from one raster set"}));
            continue;
        };
        let before = read_png(expected_path)?;
        let after = read_png(actual_path)?;
        match compare(&before, &after, policy) {
            Ok(report) => {
                let passed = report.policy_passed;
                if !passed {
                    if !diff_dir.exists() {
                        std::fs::create_dir(diff_dir).map_err(|error| format!("{}: {error}", diff_dir.display()))?;
                    }
                    let mut encoded = Cursor::new(Vec::new());
                    image::DynamicImage::ImageRgba8(diff_image(&before, &after, policy.changed_channel_threshold))
                        .write_to(&mut encoded, image::ImageFormat::Png)
                        .map_err(|error| format!("cannot encode page diff: {error}"))?;
                    write_new(&diff_dir.join(format!("page-{:03}.png", index + 1)), &encoded.into_inner())?;
                    all_passed = false;
                }
                page_reports.push(serde_json::to_value(report).map_err(|error| format!("cannot encode page report: {error}"))?);
            }
            Err(error) => {
                all_passed = false;
                page_reports.push(serde_json::json!({"page": index + 1, "passed": false, "error": error}));
            }
        }
    }
    let report = serde_json::json!({
        "schema": "wordcraft.visual-pages/v1", "expectedPages": expected.len(), "actualPages": actual.len(),
        "pageCountPassed": count_matches, "policyPassed": all_passed, "pages": page_reports,
        "diffDirectory": if diff_dir.exists() { Some(diff_dir.display().to_string()) } else { None },
        "note": "Synthetic WordCraft rendering comparison only; input baselines are never modified."
    });
    write_new(report_path, &serde_json::to_vec_pretty(&report).map_err(|error| format!("cannot encode visual report: {error}"))?)?;
    println!(
        "pages: expected {}, actual {}; {}",
        expected.len(),
        actual.len(),
        if all_passed { "within policy" } else { "regression; report and changed-page diffs written" }
    );
    if all_passed { Ok(()) } else { Err("page count or raster comparison exceeded explicit policy".into()) }
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("compare") if args.len() == 5 => run_compare(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]), Path::new(&args[4])),
        Some("compare-pages") if args.len() == 6 => {
            run_compare_pages(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]), Path::new(&args[4]), Path::new(&args[5]))
        }
        _ => Err(USAGE.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy { max_mean_absolute_channel_error: 0.0, max_changed_pixel_percent: 0.0, changed_channel_threshold: 0 }
    }

    #[test]
    fn identical_page_rasters_pass_and_changed_pixels_are_counted() {
        let before = image::RgbaImage::from_pixel(10, 8, image::Rgba([255, 255, 255, 255]));
        let same = compare(&before, &before, policy()).expect("same dimensions");
        assert!(same.policy_passed);
        let mut after = before.clone();
        after.put_pixel(4, 3, image::Rgba([0, 0, 0, 255]));
        let report = compare(&before, &after, policy()).expect("compare");
        assert!(!report.policy_passed);
        assert_eq!(report.changed_pixel_percent, 1.25);
        assert_eq!(report.max_channel_error, 255);
        assert!(report.mean_absolute_channel_error > 0.0);
    }

    #[test]
    fn raster_dimensions_and_invalid_thresholds_are_rejected() {
        let before = image::RgbaImage::new(2, 2);
        let after = image::RgbaImage::new(3, 2);
        assert!(compare(&before, &after, policy()).is_err());
        let mut invalid = policy();
        invalid.max_changed_pixel_percent = f64::NAN;
        assert!(compare(&before, &before, invalid).is_err());
    }

    #[test]
    fn changed_bounds_identify_clipped_or_displaced_ink_region() {
        let before = image::RgbaImage::from_pixel(20, 10, image::Rgba([255, 255, 255, 255]));
        let mut after = before.clone();
        after.put_pixel(7, 3, image::Rgba([0, 0, 0, 255]));
        after.put_pixel(8, 5, image::Rgba([0, 0, 0, 255]));
        let report = compare(&before, &after, policy()).expect("same dimensions");
        assert_eq!(report.changed_bounds, Some(Bounds { left: 7, top: 3, right: 8, bottom: 5 }));
    }

    #[test]
    fn page_set_rejects_gaps_and_diff_images_highlight_changed_pixels() {
        let dir = std::env::temp_dir().join(format!("wordcraft-visual-pages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test dir");
        let expected = dir.join("expected");
        std::fs::create_dir(&expected).expect("expected dir");
        let mut png = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255])))
            .write_to(&mut png, image::ImageFormat::Png)
            .expect("encode png");
        std::fs::write(expected.join("page-002.png"), png.into_inner()).expect("write gap page");
        assert!(page_files(&expected).is_err());

        let before = image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 255, 255, 255]));
        let mut after = before.clone();
        after.put_pixel(1, 0, image::Rgba([0, 0, 0, 255]));
        let diff = diff_image(&before, &after, 0);
        assert_eq!(*diff.get_pixel(1, 0), image::Rgba([255, 32, 32, 255]));
        assert_ne!(*diff.get_pixel(0, 0), image::Rgba([255, 32, 32, 255]));
        std::fs::remove_dir_all(dir).expect("remove test dir");
    }

    #[test]
    fn failed_page_comparison_writes_new_diagnostics_without_touching_expected_png() {
        let dir = std::env::temp_dir().join(format!("wordcraft-visual-diagnostics-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let expected_dir = dir.join("expected");
        let actual_dir = dir.join("actual");
        std::fs::create_dir_all(&expected_dir).expect("expected directory");
        std::fs::create_dir_all(&actual_dir).expect("actual directory");
        let expected = image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 255, 255, 255]));
        let mut actual = expected.clone();
        actual.put_pixel(2, 1, image::Rgba([0, 0, 0, 255]));
        for (path, raster) in [(expected_dir.join("page-1.png"), expected), (actual_dir.join("page-1.png"), actual)] {
            let mut bytes = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(raster).write_to(&mut bytes, image::ImageFormat::Png).expect("encode raster");
            std::fs::write(path, bytes.into_inner()).expect("write raster");
        }
        let expected_path = expected_dir.join("page-1.png");
        let original = std::fs::read(&expected_path).expect("read expected raster");
        let policy_path = dir.join("policy.json");
        std::fs::write(&policy_path, br#"{"maxMeanAbsoluteChannelError":0,"maxChangedPixelPercent":0,"changedChannelThreshold":0}"#)
            .expect("write policy");
        let report = dir.join("report.json");
        let diffs = dir.join("diffs");
        assert!(run_compare_pages(&expected_dir, &actual_dir, &policy_path, &report, &diffs).is_err());
        assert_eq!(std::fs::read(expected_path).expect("baseline unchanged"), original);
        assert!(report.exists());
        assert!(diffs.join("page-001.png").exists());
        assert!(run_compare_pages(&expected_dir, &actual_dir, &policy_path, &report, &diffs).is_err());
        let mut extra = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 255])))
            .write_to(&mut extra, image::ImageFormat::Png)
            .expect("encode extra page");
        std::fs::write(actual_dir.join("page-2.png"), extra.into_inner()).expect("write extra page");
        let count_report_path = dir.join("count-report.json");
        assert!(run_compare_pages(&expected_dir, &actual_dir, &policy_path, &count_report_path, &dir.join("count-diffs")).is_err());
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(count_report_path).expect("read count report")).expect("parse count report");
        assert_eq!(report["pageCountPassed"], false);
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }

    #[test]
    fn report_path_cannot_overwrite_a_reference_or_existing_file() {
        let dir = std::env::temp_dir().join(format!("wordcraft-visual-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).expect("create isolated test directory");
        let reference = dir.join("expected.png");
        let policy_path = dir.join("policy.json");
        let pixels = image::RgbaImage::from_pixel(1, 1, image::Rgba([1, 2, 3, 255]));
        let mut encoded = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(pixels).write_to(&mut encoded, image::ImageFormat::Png).expect("encode test raster");
        std::fs::write(&reference, encoded.into_inner()).expect("write test raster");
        std::fs::write(&policy_path, br#"{"maxMeanAbsoluteChannelError":0,"maxChangedPixelPercent":0,"changedChannelThreshold":0}"#)
            .expect("write policy");
        let original = std::fs::read(&reference).expect("read reference");
        assert!(run_compare(&reference, &reference, &policy_path, &reference).is_err());
        assert_eq!(std::fs::read(&reference).expect("reference remains"), original);
        assert!(run_compare(&reference, &reference, &policy_path, &dir.join("report.json")).is_ok());
        assert!(run_compare(&reference, &reference, &policy_path, &dir.join("report.json")).is_err());
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }
}
