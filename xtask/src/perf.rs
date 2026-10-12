//! Statistical summaries for the synthetic CLI benchmark. Timings are advisory, never CI gates.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

const MAX_INPUT: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Metric {
    stage: String,
    repetition: u32,
    wall_seconds: f64,
    peak_rss_kib: Option<f64>,
    status: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Summary {
    schema: String,
    context: BTreeMap<String, String>,
    observations: BTreeMap<String, Stage>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Stage {
    samples: usize,
    median_wall_seconds: f64,
    median_absolute_deviation_seconds: f64,
    min_wall_seconds: f64,
    max_wall_seconds: f64,
    median_peak_rss_kib: Option<f64>,
    max_peak_rss_kib: Option<f64>,
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) { (values[middle - 1] + values[middle]) / 2.0 } else { values[middle] })
}

fn summarize_metrics(metrics: &[Metric], context: BTreeMap<String, String>) -> Result<Summary, String> {
    if metrics.is_empty() {
        return Err("metrics file contains no observations".into());
    }
    let mut groups: BTreeMap<&str, Vec<&Metric>> = BTreeMap::new();
    for metric in metrics {
        if metric.stage.is_empty() || metric.repetition == 0 || metric.status != 0 || !metric.wall_seconds.is_finite() || metric.wall_seconds < 0.0 {
            return Err(format!("invalid or unsuccessful metric for stage {:?}, repetition {}", metric.stage, metric.repetition));
        }
        if metric.peak_rss_kib.is_some_and(|rss| !rss.is_finite() || rss < 0.0) {
            return Err(format!("invalid RSS metric for stage {:?}", metric.stage));
        }
        groups.entry(&metric.stage).or_default().push(metric);
    }
    let mut observations = BTreeMap::new();
    for (name, rows) in groups {
        let mut wall: Vec<f64> = rows.iter().map(|row| row.wall_seconds).collect();
        let Some(center) = median(&mut wall) else {
            return Err(format!("stage {name:?} has no timings"));
        };
        let mut deviations: Vec<f64> = rows.iter().map(|row| (row.wall_seconds - center).abs()).collect();
        let mut rss: Vec<f64> = rows.iter().filter_map(|row| row.peak_rss_kib).collect();
        let max_rss = rss.iter().copied().max_by(f64::total_cmp);
        observations.insert(
            name.to_owned(),
            Stage {
                samples: rows.len(),
                median_wall_seconds: center,
                median_absolute_deviation_seconds: median(&mut deviations).unwrap_or(0.0),
                min_wall_seconds: wall[0],
                max_wall_seconds: *wall.last().unwrap_or(&wall[0]),
                median_peak_rss_kib: median(&mut rss),
                max_peak_rss_kib: max_rss,
            },
        );
    }
    Ok(Summary { schema: "wordcraft.performance-summary/v1".into(), context, observations })
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    use std::io::Read;
    file.take(MAX_INPUT + 1).read_to_end(&mut bytes).map_err(|error| format!("{}: {error}", path.display()))?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(format!("{}: exceeds the {MAX_INPUT}-byte limit", path.display()));
    }
    Ok(bytes)
}

fn parse_tsv(bytes: &[u8]) -> Result<Vec<Metric>, String> {
    let text = std::str::from_utf8(bytes).map_err(|error| format!("metrics are not UTF-8: {error}"))?;
    let mut lines = text.lines();
    if lines.next() != Some("stage\trepetition\twall_seconds\tpeak_rss_kib\tstatus") {
        return Err("metrics TSV header is invalid".into());
    }
    let mut rows = Vec::new();
    for (index, line) in lines.enumerate() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() != 5 {
            return Err(format!("metrics line {} must have five tab-separated columns", index + 2));
        }
        rows.push(Metric {
            stage: cols[0].to_owned(),
            repetition: cols[1].parse().map_err(|_| format!("metrics line {} has invalid repetition", index + 2))?,
            wall_seconds: cols[2].parse().map_err(|_| format!("metrics line {} has invalid wall time", index + 2))?,
            peak_rss_kib: if cols[3] == "NA" {
                None
            } else {
                Some(cols[3].parse().map_err(|_| format!("metrics line {} has invalid RSS", index + 2))?)
            },
            status: cols[4].parse().map_err(|_| format!("metrics line {} has invalid status", index + 2))?,
        });
    }
    Ok(rows)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    use std::io::Write;
    file.write_all(bytes).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_manifest(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let text = String::from_utf8(read_bounded(path)?).map_err(|error| format!("{}: invalid UTF-8: {error}", path.display()))?;
    let mut result = BTreeMap::new();
    for (line_number, line) in text.lines().enumerate() {
        let Some((key, value)) = line.split_once('\t') else {
            return Err(format!("{}:{}: malformed manifest row", path.display(), line_number + 1));
        };
        if key.is_empty() || result.insert(key.to_owned(), value.to_owned()).is_some() {
            return Err(format!("{}:{}: empty or duplicate manifest key", path.display(), line_number + 1));
        }
    }
    Ok(result)
}

fn summarize_file(metrics_path: &Path, summary_path: &Path, manifest_path: Option<&Path>) -> Result<(), String> {
    if summary_path.exists() {
        return Err(format!("{}: output exists; choose a new path", summary_path.display()));
    }
    let metrics = parse_tsv(&read_bounded(metrics_path)?)?;
    let context = manifest_path.map(read_manifest).transpose()?.unwrap_or_default();
    let summary = summarize_metrics(&metrics, context)?;
    let bytes = serde_json::to_vec_pretty(&summary).map_err(|error| format!("encode summary: {error}"))?;
    write_new(summary_path, &bytes)?;
    for (name, stage) in summary.observations {
        println!(
            "{name}: n={}, median {:.3}s (MAD {:.3}s), median RSS {} KiB",
            stage.samples,
            stage.median_wall_seconds,
            stage.median_absolute_deviation_seconds,
            stage.median_peak_rss_kib.map(|n| format!("{n:.0}")).unwrap_or_else(|| "NA".into())
        );
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Comparison<'a> {
    schema: &'static str,
    advisory_only: bool,
    comparison_allowed: bool,
    context_differences: Vec<String>,
    baseline: &'a Summary,
    current: &'a Summary,
    stages: BTreeMap<String, serde_json::Value>,
}

fn compare_files(baseline_path: &Path, current_path: &Path, report_path: &Path) -> Result<(), String> {
    if report_path.exists() {
        return Err(format!("{}: output exists; choose a new path", report_path.display()));
    }
    let baseline: Summary = serde_json::from_slice(&read_bounded(baseline_path)?).map_err(|error| format!("baseline JSON: {error}"))?;
    let current: Summary = serde_json::from_slice(&read_bounded(current_path)?).map_err(|error| format!("current JSON: {error}"))?;
    let context_differences = context_differences(&baseline, &current);
    let comparison_allowed = context_differences.is_empty();
    let mut stages = BTreeMap::new();
    for (name, now) in &current.observations {
        if let Some(before) = baseline.observations.get(name) {
            let wall_delta =
                if before.median_wall_seconds > 0.0 { (now.median_wall_seconds / before.median_wall_seconds - 1.0) * 100.0 } else { 0.0 };
            let rss_delta = match (before.median_peak_rss_kib, now.median_peak_rss_kib) {
                (Some(a), Some(b)) if a > 0.0 => Some((b / a - 1.0) * 100.0),
                _ => None,
            };
            stages.insert(
                name.clone(),
                serde_json::json!({"wallChangePercent": wall_delta, "rssChangePercent": rss_delta,
                "largeChangeAdvisory": wall_delta >= 25.0 || rss_delta.is_some_and(|delta| delta >= 25.0),
                "baselineSamples": before.samples, "currentSamples": now.samples}),
            );
        }
    }
    if stages.is_empty() {
        return Err("summaries have no matching stages".into());
    }
    let comparison = Comparison {
        schema: "wordcraft.performance-comparison/v1",
        advisory_only: true,
        comparison_allowed,
        context_differences,
        baseline: &baseline,
        current: &current,
        stages,
    };
    write_new(report_path, &serde_json::to_vec_pretty(&comparison).map_err(|error| format!("encode comparison: {error}"))?)?;
    println!("performance deltas written as advisory report; no timing threshold can fail CI");
    Ok(())
}

fn context_differences(baseline: &Summary, current: &Summary) -> Vec<String> {
    ["scale", "repeats", "profile", "os", "rustc", "processors", "cpu_model", "font_sans", "font_serif", "font_math"]
        .into_iter()
        .filter_map(|key| match (baseline.context.get(key), current.context.get(key)) {
            (Some(before), Some(now)) if before == now => None,
            (Some(before), Some(now)) => Some(format!("{key}: baseline {before:?}, current {now:?}")),
            _ => Some(format!("{key}: missing from baseline or current manifest")),
        })
        .collect()
}

pub fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("summarize") if args.len() == 3 || args.len() == 4 => {
            let manifest_path = args.get(3).map(String::as_str).map(Path::new);
            summarize_file(Path::new(&args[1]), Path::new(&args[2]), manifest_path)
        }
        Some("compare") if args.len() == 4 => compare_files(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3])),
        _ => Err("usage: cargo xtask perf summarize <metrics.tsv> <summary.json> [manifest.tsv]\n       cargo xtask perf compare <baseline.json> <current.json> <report.json>".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_uses_median_and_median_absolute_deviation() {
        let rows =
            [1.0, 1.2, 100.0].map(|wall_seconds| Metric { stage: "load".into(), repetition: 1, wall_seconds, peak_rss_kib: Some(1024.0), status: 0 });
        let summary = summarize_metrics(&rows, BTreeMap::new()).expect("valid samples");
        let load = &summary.observations["load"];
        assert_eq!(load.median_wall_seconds, 1.2);
        assert!((load.median_absolute_deviation_seconds - 0.2).abs() < 1e-12);
        assert_eq!(load.max_wall_seconds, 100.0);
    }

    #[test]
    fn failed_and_non_finite_samples_are_rejected() {
        let invalid = Metric { stage: "load".into(), repetition: 1, wall_seconds: f64::NAN, peak_rss_kib: None, status: 0 };
        assert!(summarize_metrics(&[invalid], BTreeMap::new()).is_err());
    }

    #[test]
    fn environment_mismatch_marks_performance_comparison_non_comparable() {
        let baseline = Summary {
            schema: "wordcraft.performance-summary/v1".into(),
            context: ["scale", "repeats", "profile", "os", "rustc", "processors", "cpu_model", "font_sans", "font_serif", "font_math"]
                .into_iter()
                .map(|key| (key.to_owned(), "same".to_owned()))
                .collect(),
            observations: BTreeMap::new(),
        };
        let mut current = baseline.clone();
        current.context.insert("processors".into(), "different host".into());
        let differences = context_differences(&baseline, &current);
        assert_eq!(differences.len(), 1);
        assert!(differences[0].contains("processors"));
    }

    #[test]
    fn large_performance_changes_are_reported_without_failing_comparison() {
        let dir = std::env::temp_dir().join(format!("wordcraft-perf-advisory-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create test directory");
        let context: BTreeMap<String, String> =
            ["scale", "repeats", "profile", "os", "rustc", "processors", "cpu_model", "font_sans", "font_serif", "font_math"]
                .into_iter()
                .map(|key| (key.to_owned(), "same".to_owned()))
                .collect();
        let stage = |median_wall_seconds| Stage {
            samples: 3,
            median_wall_seconds,
            median_absolute_deviation_seconds: 0.01,
            min_wall_seconds: median_wall_seconds,
            max_wall_seconds: median_wall_seconds,
            median_peak_rss_kib: Some(100.0),
            max_peak_rss_kib: Some(100.0),
        };
        let summary = |median_wall_seconds| Summary {
            schema: "wordcraft.performance-summary/v1".into(),
            context: context.clone(),
            observations: [("rasterize-pages".to_owned(), stage(median_wall_seconds))].into_iter().collect(),
        };
        let baseline_path = dir.join("baseline.json");
        let current_path = dir.join("current.json");
        let report_path = dir.join("report.json");
        std::fs::write(&baseline_path, serde_json::to_vec(&summary(1.0)).expect("encode baseline")).expect("write baseline");
        std::fs::write(&current_path, serde_json::to_vec(&summary(1.3)).expect("encode current")).expect("write current");

        assert!(compare_files(&baseline_path, &current_path, &report_path).is_ok());
        let report: serde_json::Value = serde_json::from_slice(&std::fs::read(report_path).expect("read report")).expect("parse report");
        assert_eq!(report["advisoryOnly"], true);
        assert_eq!(report["comparisonAllowed"], true);
        assert_eq!(report["stages"]["rasterize-pages"]["largeChangeAdvisory"], true);
        assert!((report["stages"]["rasterize-pages"]["wallChangePercent"].as_f64().unwrap_or_default() - 30.0).abs() < 1e-9);
        std::fs::remove_dir_all(dir).expect("remove test directory");
    }
}
