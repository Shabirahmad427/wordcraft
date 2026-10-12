//! Bounded DOCX package fingerprinting and round-trip contract checks.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::Path;

const MAX_PARTS: usize = 512;
const MAX_PART_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_BYTES: u64 = 128 * 1024 * 1024;
const INTEROP_USAGE: &str =
    "usage: cargo xtask interop inspect <docx> <report.json> | interop compare <original.docx> <roundtrip.docx> <case.json> <report.json>";

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PartFingerprint {
    sha256: String,
    expanded_bytes: u64,
    compressed_bytes: u64,
    crc32: u32,
    compression: String,
    kind: &'static str,
    modified: String,
    unix_mode: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PackageFingerprint {
    sha256: String,
    expanded_bytes: u64,
    parts: BTreeMap<String, PartFingerprint>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PartDelta {
    part: String,
    status: &'static str,
    kind: &'static str,
    original_sha256: Option<String>,
    roundtrip_sha256: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CompareReport {
    schema: &'static str,
    case_id: String,
    source_kind: String,
    source_provenance: String,
    license: String,
    controlled_edits: Vec<String>,
    original: PackageFingerprint,
    roundtrip: PackageFingerprint,
    deltas: Vec<PartDelta>,
    allowed_changed_parts: Vec<String>,
    contract_passed: bool,
    violations: Vec<String>,
    note: &'static str,
}

fn digest(bytes: &[u8]) -> String {
    let hash = Sha256::digest(bytes);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn part_kind(name: &str) -> &'static str {
    if name == "[Content_Types].xml" {
        "content-types"
    } else if name.ends_with(".rels") {
        "relationships"
    } else if name.starts_with("word/charts/") {
        "chart"
    } else if name.starts_with("word/embeddings/") {
        "embedded"
    } else if name.ends_with(".xml") {
        "xml"
    } else {
        "binary"
    }
}

fn valid_part_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\\', ':', '\0'])
        && name.split('/').all(|component| !component.is_empty() && component != "." && component != "..")
}

fn fingerprint(path: &Path) -> Result<PackageFingerprint, String> {
    let archive_size = std::fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?.len();
    if archive_size > MAX_ARCHIVE_BYTES {
        return Err(format!("{}: compressed package exceeds the benchmark archive limit", path.display()));
    }
    let package_bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let package_sha256 = digest(&package_bytes);
    let mut archive = zip::ZipArchive::new(Cursor::new(package_bytes)).map_err(|error| format!("{}: invalid DOCX ZIP: {error}", path.display()))?;
    if archive.len() > MAX_PARTS {
        return Err(format!("{}: package has {} entries; benchmark limit is {MAX_PARTS}", path.display(), archive.len()));
    }
    let mut parts = BTreeMap::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| format!("{}: unreadable ZIP entry {index}: {error}", path.display()))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if !valid_part_name(&name) {
            return Err(format!("{}: unsafe package part name `{name}`", path.display()));
        }
        if entry.size() > MAX_PART_BYTES {
            return Err(format!("{}: part `{name}` exceeds the benchmark part limit", path.display()));
        }
        total = total.checked_add(entry.size()).ok_or_else(|| "package expanded-size overflow".to_string())?;
        if total > MAX_TOTAL_BYTES {
            return Err(format!("{}: package exceeds the benchmark expanded-size limit", path.display()));
        }
        let capacity = usize::try_from(entry.size()).map_err(|_| "part size does not fit memory".to_string())?;
        let mut bytes = Vec::with_capacity(capacity);
        entry
            .by_ref()
            .take(MAX_PART_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|error| format!("{}: cannot read `{name}`: {error}", path.display()))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_PART_BYTES {
            return Err(format!("{}: part `{name}` expanded beyond the benchmark part limit", path.display()));
        }
        if u64::try_from(bytes.len()).ok() != Some(entry.size()) {
            return Err(format!("{}: part `{name}` expanded to an unexpected size", path.display()));
        }
        let record = PartFingerprint {
            sha256: digest(&bytes),
            expanded_bytes: entry.size(),
            compressed_bytes: entry.compressed_size(),
            crc32: entry.crc32(),
            compression: format!("{:?}", entry.compression()),
            kind: part_kind(&name),
            modified: format!("{:?}", entry.last_modified()),
            unix_mode: entry.unix_mode(),
        };
        if parts.insert(name.clone(), record).is_some() {
            return Err(format!("{}: duplicate ZIP part name `{name}`", path.display()));
        }
    }
    Ok(PackageFingerprint { sha256: package_sha256, expanded_bytes: total, parts })
}

pub fn run(args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("{INTEROP_USAGE}");
        return Ok(());
    }
    match args.first().map(String::as_str) {
        Some("inspect") if args.len() == 3 => {
            let package = fingerprint(Path::new(&args[1]))?;
            write_json(Path::new(&args[2]), &package)?;
            println!("fingerprinted {} parts ({} expanded bytes)", package.parts.len(), package.expanded_bytes);
            Ok(())
        }
        Some("compare") if args.len() == 5 => compare(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3]), Path::new(&args[4])),
        _ => Err(INTEROP_USAGE.into()),
    }
}

fn compare(original_path: &Path, roundtrip_path: &Path, case_path: &Path, report_path: &Path) -> Result<(), String> {
    let case_bytes = std::fs::read(case_path).map_err(|error| format!("{}: {error}", case_path.display()))?;
    let case: Value = serde_json::from_slice(&case_bytes).map_err(|error| format!("{}: invalid JSON: {error}", case_path.display()))?;
    let string =
        |key: &str| case.get(key).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("case manifest requires string `{key}`"));
    let case_id = string("caseId")?;
    let source_kind = string("sourceKind")?;
    if !matches!(source_kind.as_str(), "synthetic" | "word365" | "other") {
        return Err("sourceKind must be synthetic, word365, or other".into());
    }
    let source_provenance = string("sourceProvenance")?;
    let license = string("license")?;
    let allowed_changed_parts = string_array(&case, "allowedChangedParts")?;
    let controlled_edits = case
        .get("controlledEdits")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    let original = fingerprint(original_path)?;
    let roundtrip = fingerprint(roundtrip_path)?;
    let allowed: BTreeSet<_> = allowed_changed_parts.iter().cloned().collect();
    let names: BTreeSet<_> = original.parts.keys().chain(roundtrip.parts.keys()).cloned().collect();
    let mut deltas = Vec::new();
    let mut violations = Vec::new();
    for name in names {
        let before = original.parts.get(&name);
        let after = roundtrip.parts.get(&name);
        let status = match (before, after) {
            (Some(a), Some(b)) if a.sha256 == b.sha256 => "unchanged",
            (Some(_), Some(_)) => "changed",
            (Some(_), None) => "removed",
            (None, Some(_)) => "added",
            (None, None) => continue,
        };
        if status != "unchanged" && !allowed.contains(&name) {
            violations.push(format!("unexpected {status} part `{name}`"));
        }
        if status != "unchanged" {
            deltas.push(PartDelta {
                kind: before.or(after).map(|part| part.kind).unwrap_or("unknown"),
                part: name,
                status,
                original_sha256: before.map(|part| part.sha256.clone()),
                roundtrip_sha256: after.map(|part| part.sha256.clone()),
            });
        }
    }
    let contract_passed = violations.is_empty();
    let report = CompareReport {
        schema: "wordcraft.docx-interop/v1",
        case_id,
        source_kind,
        source_provenance,
        license,
        controlled_edits,
        original,
        roundtrip,
        deltas,
        allowed_changed_parts,
        contract_passed,
        violations: violations.clone(),
        note: "Part equality is a package-integrity result, not proof of Microsoft Word rendering or editability.",
    };
    write_json(report_path, &report)?;
    if contract_passed {
        println!("package contract passed; see {}", report_path.display());
        Ok(())
    } else {
        Err(format!("package contract failed: {}", violations.join("; ")))
    }
}

fn string_array(value: &Value, field: &str) -> Result<Vec<String>, String> {
    let items = value.get(field).and_then(Value::as_array).ok_or_else(|| format!("case manifest requires array `{field}`"))?;
    items.iter().map(|item| item.as_str().map(str::to_string).ok_or_else(|| format!("`{field}` entries must be strings"))).collect()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| format!("cannot encode benchmark report: {error}"))?;
    std::fs::write(path, bytes).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        for (name, data) in entries {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).expect("start ZIP entry");
            writer.write_all(data).expect("write ZIP entry");
        }
        writer.finish().expect("finish ZIP").into_inner()
    }

    #[test]
    fn fingerprints_and_compares_parts_with_an_allowlist() {
        let directory = std::env::temp_dir().join(format!("wordcraft-interop-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let original = directory.join("original.docx");
        let edited = directory.join("edited.docx");
        std::fs::write(
            &original,
            archive(&[("[Content_Types].xml", b"types"), ("word/document.xml", b"before"), ("word/_rels/document.xml.rels", b"rels")]),
        )
        .expect("write original");
        std::fs::write(
            &edited,
            archive(&[("[Content_Types].xml", b"types"), ("word/document.xml", b"after"), ("word/_rels/document.xml.rels", b"rels")]),
        )
        .expect("write edited");
        let first = fingerprint(&original).expect("fingerprint original");
        assert_eq!(first.parts.len(), 3);
        assert_eq!(first.parts["word/_rels/document.xml.rels"].kind, "relationships");
        let case = directory.join("case.json");
        std::fs::write(&case, br#"{"caseId":"synthetic-one","sourceKind":"synthetic","sourceProvenance":"Rust fixture","license":"MIT","controlledEdits":["text.insert"],"allowedChangedParts":["word/document.xml"]}"#).expect("write case");
        let report_path = directory.join("report.json");
        compare(&original, &edited, &case, &report_path).expect("compare packages");
        let report: Value = serde_json::from_slice(&std::fs::read(report_path).expect("read report")).expect("parse report");
        assert_eq!(report["contractPassed"], true);
        assert_eq!(report["deltas"][0]["part"], "word/document.xml");
        std::fs::remove_dir_all(directory).expect("remove temp dir");
    }

    #[test]
    fn compare_fails_when_any_unlisted_part_changes() {
        let directory = std::env::temp_dir().join(format!("wordcraft-interop-reject-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let original = directory.join("original.docx");
        let edited = directory.join("edited.docx");
        std::fs::write(&original, archive(&[("word/document.xml", b"before"), ("word/customXml/item1.xml", b"binding")])).expect("write original");
        std::fs::write(&edited, archive(&[("word/document.xml", b"after"), ("word/customXml/item1.xml", b"changed")])).expect("write edited");
        let case = directory.join("case.json");
        std::fs::write(&case, br#"{"caseId":"guarded-edit","sourceKind":"synthetic","sourceProvenance":"Rust fixture","license":"MIT","allowedChangedParts":["word/document.xml"]}"#).expect("write case");
        let report_path = directory.join("report.json");
        assert!(compare(&original, &edited, &case, &report_path).is_err());
        let report: Value = serde_json::from_slice(&std::fs::read(report_path).expect("read report")).expect("parse report");
        assert_eq!(report["contractPassed"], false);
        assert_eq!(report["violations"][0], "unexpected changed part `word/customXml/item1.xml`");
        std::fs::remove_dir_all(directory).expect("remove temp dir");
    }

    #[test]
    fn package_fingerprint_rejects_unsafe_paths() {
        let directory = std::env::temp_dir().join(format!("wordcraft-interop-unsafe-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("create temp dir");
        let path = directory.join("unsafe.docx");
        std::fs::write(&path, archive(&[("../escape.xml", b"x")])).expect("write ZIP");
        assert!(fingerprint(&path).is_err());
        std::fs::remove_dir_all(directory).expect("remove temp dir");
    }
}
