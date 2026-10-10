//! Preserve document-attached custom XML data (ECMA-376 Part 1 §15.2.4–5), including
//! bibliography libraries, as opaque OPC parts. Never execute or fetch their contents.
use crate::DocxError;
use crate::package::{ContentTypes, Package, Rels, rel_is};
use crate::write::PartRels;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use wordcraft_doc::Document;

const CUSTOM_XML: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXml";
const REL_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";
/// Bookkeeping only, never emitted as a ZIP entry.
const MANIFEST: &str = "wordcraft:customXml.v1";
const MAX_PARTS: usize = 512;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_MANIFEST: usize = 128 * 1024;
const MAX_LABEL: usize = 4096;

#[derive(Serialize, Deserialize)]
struct Root {
    kind: String,
    target: String,
}
#[derive(Serialize, Deserialize)]
struct Part {
    name: String,
    content_type: String,
}
#[derive(Default, Serialize, Deserialize)]
struct Manifest {
    roots: Vec<Root>,
    parts: Vec<Part>,
}

fn limit() -> DocxError {
    DocxError::Limit("custom XML preservation budget (512 parts / 64 MiB / 128 KiB manifest)".into())
}
fn invalid(reason: &str) -> DocxError {
    DocxError::NotWord(format!("custom XML preservation: {reason}"))
}
fn valid_path(path: &str) -> bool {
    !path.is_empty() && path.len() <= 1024 && !path.contains(['\\', ':', '?', '#']) && path.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}
fn rel_path(part: &str) -> String {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    if dir.is_empty() { format!("_rels/{file}.rels") } else { format!("{dir}/_rels/{file}.rels") }
}
fn reserved(path: &str) -> bool {
    [
        "word/document.xml",
        "word/_rels/document.xml.rels",
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/core.xml",
        "docProps/app.xml",
        "docProps/custom.xml",
        "word/vbaProject.bin",
    ]
    .iter()
    .any(|p| p.eq_ignore_ascii_case(path))
}

pub(crate) fn read(pkg: &Package, rels: &Rels, main: &str, doc: &mut Document) -> Result<(), DocxError> {
    let mut manifest = Manifest::default();
    let mut queue = VecDeque::new();
    let mut queued = BTreeSet::new();
    for rel in rels.list.iter().filter(|r| r.kind.rsplit('/').next() == Some("customXml")) {
        if rel.kind.len() > MAX_LABEL {
            return Err(limit());
        }
        if rel.external {
            return Err(invalid("a custom XML root must be internal"));
        }
        if !valid_path(&rel.target) {
            return Err(invalid("invalid part name"));
        }
        if manifest.roots.len() >= MAX_PARTS {
            return Err(limit());
        }
        manifest.roots.push(Root { kind: rel.kind.clone(), target: rel.target.clone() });
        if queued.insert(rel.target.to_ascii_lowercase()) {
            queue.push_back(rel.target.clone());
        }
    }
    if manifest.roots.is_empty() {
        return Ok(());
    }
    let types = ContentTypes::read(pkg);
    let mut total = 0usize;
    while let Some(path) = queue.pop_front() {
        if reserved(&path) || path.eq_ignore_ascii_case(main) || path.to_ascii_lowercase().ends_with(".rels") {
            return Err(invalid("custom XML links to a reserved document/relationship part"));
        }
        preserve(pkg, &types, &path, &mut manifest, &mut total, doc)?;
        let rp = rel_path(&path);
        if pkg.get(&rp).is_some() {
            // Parse only relationships for graph traversal; preserve their bytes exactly, including
            // original IDs, prefixes, external links and extension attributes.
            let root = pkg.xml(&rp)?.ok_or_else(|| DocxError::MissingPart(rp.clone()))?;
            if root.local() != "Relationships" {
                return Err(invalid("invalid relationships root"));
            }
            preserve(pkg, &types, &rp, &mut manifest, &mut total, doc)?;
            for rel in pkg.rels(&path).list {
                if rel.external {
                    continue;
                }
                if !valid_path(&rel.target) {
                    return Err(invalid("invalid related part name"));
                }
                if queued.insert(rel.target.to_ascii_lowercase()) {
                    if queued.len() > MAX_PARTS {
                        return Err(limit());
                    }
                    queue.push_back(rel.target);
                }
            }
        }
    }
    let metadata = serde_json::to_vec(&manifest).map_err(|e| invalid(&e.to_string()))?;
    if metadata.len() > MAX_MANIFEST {
        return Err(limit());
    }
    doc.passthrough.insert(MANIFEST.into(), Arc::new(metadata));
    Ok(())
}

fn preserve(
    pkg: &Package,
    types: &ContentTypes,
    path: &str,
    manifest: &mut Manifest,
    total: &mut usize,
    doc: &mut Document,
) -> Result<(), DocxError> {
    let bytes = pkg.get(path).ok_or_else(|| DocxError::MissingPart(path.into()))?;
    *total = total.checked_add(bytes.len()).ok_or_else(limit)?;
    if *total > MAX_BYTES || manifest.parts.len() >= MAX_PARTS {
        return Err(limit());
    }
    let content_type = types.of(path).unwrap_or(if path.ends_with(".rels") { REL_TYPE } else { "application/xml" });
    // A single Default type can apply to hundreds of parts: cap its label before cloning.
    if content_type.len() > MAX_LABEL {
        return Err(limit());
    }
    manifest.parts.push(Part { name: path.into(), content_type: content_type.into() });
    doc.passthrough.insert(path.into(), Arc::new(bytes.to_vec()));
    Ok(())
}

/// Validate everything before appending entries. Refuse collisions/missing data rather than
/// overwrite generated parts or save a package with lost custom data.
pub(crate) fn write(
    doc: &Document,
    entries: &mut Vec<(String, Vec<u8>)>,
    types: &mut Vec<(String, String)>,
    rels: &mut PartRels,
) -> Result<(), DocxError> {
    let Some(bytes) = doc.passthrough.get(MANIFEST) else { return Ok(()) };
    if bytes.len() > MAX_MANIFEST {
        return Err(limit());
    }
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|e| invalid(&e.to_string()))?;
    if manifest.parts.len() > MAX_PARTS || manifest.roots.len() > MAX_PARTS {
        return Err(limit());
    }
    let mut seen = BTreeSet::new();
    let mut total = 0usize;
    for part in &manifest.parts {
        if !valid_path(&part.name)
            || reserved(&part.name)
            || !seen.insert(part.name.to_ascii_lowercase())
            || entries.iter().any(|(name, _)| name.eq_ignore_ascii_case(&part.name))
        {
            return Err(invalid("preserved part name is invalid, duplicate or collides with generated content"));
        }
        let bytes = doc.passthrough.get(&part.name).ok_or_else(|| DocxError::MissingPart(part.name.clone()))?;
        total = total.checked_add(bytes.len()).ok_or_else(limit)?;
        if total > MAX_BYTES {
            return Err(limit());
        }
        if part.content_type.is_empty() {
            return Err(invalid("missing content type"));
        }
    }
    for root in &manifest.roots {
        if !rel_is(&root.kind, CUSTOM_XML) || !seen.contains(&root.target.to_ascii_lowercase()) || !valid_path(&root.target) {
            return Err(invalid("invalid or missing custom XML root"));
        }
    }
    for part in manifest.parts {
        let bytes = doc.passthrough.get(&part.name).ok_or_else(|| DocxError::MissingPart(part.name.clone()))?;
        types.push((format!("/{}", part.name), part.content_type));
        entries.push((part.name, bytes.to_vec()));
    }
    for root in manifest.roots {
        rels.add(&root.kind, &format!("/{}", root.target), false);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostile_manifest_cannot_overwrite_document_or_allocate_large_part_lists() {
        for name in ["word/document.xml", "docProps/core.xml", "../evil.xml", "x\\y.xml", "https://remote", ""] {
            let mut d = Document::new();
            let manifest = Manifest { roots: vec![], parts: vec![Part { name: name.into(), content_type: "application/xml".into() }] };
            d.passthrough.insert(MANIFEST.into(), Arc::new(serde_json::to_vec(&manifest).unwrap()));
            d.passthrough.insert(name.into(), Arc::new(b"evil".to_vec()));
            let mut entries = vec![];
            assert!(write(&d, &mut entries, &mut vec![], &mut PartRels::default()).is_err());
            assert!(entries.is_empty());
        }
        let mut d = Document::new();
        d.passthrough.insert(MANIFEST.into(), Arc::new(vec![b' '; MAX_MANIFEST + 1]));
        assert!(write(&d, &mut vec![], &mut vec![], &mut PartRels::default()).is_err());
    }
    #[test]
    fn collisions_and_missing_parts_fail_before_appending() {
        let mut d = Document::new();
        let manifest = Manifest {
            roots: vec![Root { kind: CUSTOM_XML.into(), target: "data/item.xml".into() }],
            parts: vec![Part { name: "data/item.xml".into(), content_type: "application/xml".into() }],
        };
        d.passthrough.insert(MANIFEST.into(), Arc::new(serde_json::to_vec(&manifest).unwrap()));
        assert!(write(&d, &mut vec![], &mut vec![], &mut PartRels::default()).is_err());
        d.passthrough.insert("data/item.xml".into(), Arc::new(vec![]));
        let mut entries = vec![("DATA/ITEM.XML".into(), b"generated".to_vec())];
        assert!(write(&d, &mut entries, &mut vec![], &mut PartRels::default()).is_err());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1, b"generated");
    }
    #[test]
    fn budgets_fail_before_copying_extra_data() {
        let bytes = crate::package::zip_entries(&[("data/item.xml".into(), b"abc".to_vec())]).unwrap();
        let pkg = Package::open(&bytes).unwrap();
        let mut doc = Document::new();
        let mut manifest = Manifest::default();
        let mut total = MAX_BYTES;
        assert!(matches!(preserve(&pkg, &ContentTypes::default(), "data/item.xml", &mut manifest, &mut total, &mut doc), Err(DocxError::Limit(_))));
        assert!(doc.passthrough.is_empty());
        manifest.parts = (0..MAX_PARTS).map(|n| Part { name: format!("data/item{n}.xml"), content_type: "application/xml".into() }).collect();
        let mut total = 0;
        assert!(matches!(preserve(&pkg, &ContentTypes::default(), "data/item.xml", &mut manifest, &mut total, &mut doc), Err(DocxError::Limit(_))));
        assert!(doc.passthrough.is_empty());
        manifest.parts.push(Part { name: "data/one-too-many.xml".into(), content_type: "application/xml".into() });
        doc.passthrough.insert(MANIFEST.into(), Arc::new(serde_json::to_vec(&manifest).unwrap()));
        assert!(matches!(write(&doc, &mut vec![], &mut vec![], &mut PartRels::default()), Err(DocxError::Limit(_))));
    }
}
