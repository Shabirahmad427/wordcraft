//! Deterministic page-raster comparison. The command only reads inputs and writes a report;
//! expected images are never changed implicitly.

use serde::{Deserialize, Serialize};
use std::io::{Cursor, Read};
use std::path::Path;

const MAX_PIXELS: u64 = 25_000_000;
const MAX_PNG_BYTES: u64 = 128 * 1024 * 1024;
const MAX_POLICY_BYTES: u64 = 4 * 1024;
const USAGE: &str = "usage: cargo xtask visual compare <expected.png> <actual.png> <policy.json> <report.json>";

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
    policy_passed: bool,
    note: &'static str,
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
    for (before, after) in expected.pixels().zip(actual.pixels()) {
        let mut changed = false;
        for channel in 0..4 {
            let error = before[channel].abs_diff(after[channel]);
            absolute_error += u64::from(error);
            max_channel_error = max_channel_error.max(error);
            changed |= error > policy.changed_channel_threshold;
        }
        changed_pixels += u64::from(changed);
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
        policy_passed: mae <= policy.max_mean_absolute_channel_error && changed_percent <= policy.max_changed_pixel_percent,
        note: "Comparison only; no reference raster or baseline was modified.",
    })
}

fn run_compare(expected: &Path, actual: &Path, policy_path: &Path, report_path: &Path) -> Result<(), String> {
    if report_path.exists() {
        return Err(format!("{}: report output already exists; choose a new path", report_path.display()));
    }
    let policy_bytes = read_bounded(policy_path, MAX_POLICY_BYTES, "visual policy")?;
    let policy: Policy = serde_json::from_slice(&policy_bytes).map_err(|error| format!("{}: invalid policy JSON: {error}", policy_path.display()))?;
    let expected = read_png(expected)?;
    let actual = read_png(actual)?;
    let report = compare(&expected, &actual, policy)?;
    let encoded = serde_json::to_vec_pretty(&report).map_err(|error| format!("cannot encode visual report: {error}"))?;
    let mut output =
        std::fs::OpenOptions::new().write(true).create_new(true).open(report_path).map_err(|error| format!("{}: {error}", report_path.display()))?;
    use std::io::Write;
    output.write_all(&encoded).map_err(|error| format!("{}: {error}", report_path.display()))?;
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

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("compare") if args.len() == 5 => run_compare(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]), Path::new(&args[4])),
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
