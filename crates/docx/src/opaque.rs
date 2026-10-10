//! Preserve chart drawing references and their OPC dependency graphs without rendering them.
use crate::{
    DocxError, custom_xml,
    package::{Package, Rels},
    write::PartRels,
    xml::{self, El, Node},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, sync::Arc};
use wordcraft_doc::{Document, InlineObject};
const KEY: &str = "wordcraft:opaqueCharts.v1";
pub(crate) const FORMAT: &str = "docx-chart";
const MAX_MANIFEST: usize = 128 * 1024;
const MAX_ROOTS: usize = 512;
const MAX_XML: usize = 1024 * 1024;
fn invalid(reason: &str) -> DocxError {
    DocxError::Preservation(format!("chart preservation: {reason}"))
}
// Check before cloning/escaping input. A huge descriptor or extension tree must not
// allocate another unrestricted copy while trying to preserve one small chart reference.
fn check_drawing_size(root: &El) -> Result<(), DocxError> {
    let mut queue = vec![root];
    let mut nodes = 0usize;
    let mut size = 4096usize;
    let escaped_size = |s: &str| {
        s.chars()
            .map(|c| match c {
                '&' => 5,
                '<' | '>' => 4,
                '\'' | '"' => 6,
                _ => c.len_utf8(),
            })
            .sum::<usize>()
    };
    while let Some(node) = queue.pop() {
        nodes += 1;
        size = size.saturating_add(node.name.len().saturating_mul(2)).saturating_add(5);
        for (key, value) in &node.attrs {
            size = size.saturating_add(key.len()).saturating_add(escaped_size(value)).saturating_add(4);
        }
        for child in &node.kids {
            match child {
                Node::Text(t) => size = size.saturating_add(escaped_size(t)),
                Node::El(e) => {
                    if queue.len() >= 16_384 {
                        return Err(invalid("drawing structure limit"));
                    }
                    queue.push(e);
                }
            }
        }
        if nodes > 16_384 || size > MAX_XML {
            return Err(invalid("drawing exceeds preservation size/structure limits"));
        }
    }
    Ok(())
}
#[derive(Clone, Serialize, Deserialize)]
struct Root {
    id: String,
    kind: String,
    target: String,
    external: bool,
}
#[derive(Default, Serialize, Deserialize)]
pub(crate) struct Preservation {
    roots: Vec<Root>,
    parts: Vec<custom_xml::Part>,
}
impl Preservation {
    pub fn capture(&mut self, drawing: &El, rels: &Rels) -> Result<InlineObject, DocxError> {
        check_drawing_size(drawing)?;
        let chart = drawing.find("c:chart").ok_or_else(|| invalid("unsupported drawing type; opening it would discard content"))?;
        let id = chart.attr("r:id").ok_or_else(|| invalid("chart has no relationship ID"))?;
        let rel = rels.by_id(id).ok_or_else(|| invalid("missing chart relationship"))?;
        if rel.external || rel.kind.rsplit('/').next() != Some("chart") {
            return Err(invalid("invalid chart relationship"));
        }
        let mut drawing = drawing.clone();
        self.rebind(&mut drawing, rels)?;
        drawing.attrs.extend(xml::body_ns().into_iter().filter(|(name, _)| name.starts_with("xmlns:")));
        let xml = drawing.to_xml();
        if xml.len() > MAX_XML {
            return Err(invalid("drawing exceeds 1 MiB preservation limit"));
        }
        Ok(InlineObject::Opaque { format: FORMAT.into(), xml, text: "[Chart preserved — rendering and editing unavailable]".into() })
    }
    fn rebind(&mut self, element: &mut El, rels: &Rels) -> Result<(), DocxError> {
        if element.name.starts_with("?:") || element.attrs.iter().any(|(k, _)| k.starts_with("?:")) {
            return Err(invalid("unknown drawing namespace cannot be preserved safely"));
        }
        // Namespace-valued extension attributes cannot be canonicalized without a schema.
        if element.attrs.iter().any(|(k, _)| k.starts_with("mc:")) {
            return Err(invalid("drawing markup-compatibility attributes require unsupported namespace preservation"));
        }
        for (name, value) in &mut element.attrs {
            if !matches!(name.as_str(), "r:id" | "r:embed" | "r:link") {
                continue;
            }
            let rel = rels.by_id(value).ok_or_else(|| invalid("missing drawing relationship"))?;
            let index =
                if let Some(index) = self.roots.iter().position(|r| r.kind == rel.kind && r.target == rel.target && r.external == rel.external) {
                    index
                } else {
                    if self.roots.len() >= MAX_ROOTS || rel.kind.len() > 4096 || rel.target.len() > 4096 {
                        return Err(invalid("relationship preservation limit"));
                    }
                    let index = self.roots.len();
                    self.roots.push(Root {
                        id: format!("wcChart{}", index + 1),
                        kind: rel.kind.clone(),
                        target: rel.target.clone(),
                        external: rel.external,
                    });
                    index
                };
            *value = self.roots.get(index).ok_or_else(|| invalid("missing preserved relationship"))?.id.clone();
        }
        for child in &mut element.kids {
            if let Node::El(child) = child {
                self.rebind(child, rels)?;
            }
        }
        Ok(())
    }
    pub fn preserve(&mut self, pkg: &Package, main: &str, doc: &mut Document) -> Result<(), DocxError> {
        if self.roots.is_empty() {
            return Ok(());
        }
        self.parts = custom_xml::preserve_graph(pkg, main, self.roots.iter().filter(|r| !r.external).map(|r| r.target.as_str()), doc)?;
        let types = crate::package::ContentTypes::read(pkg);
        if self.parts.iter().any(|p| !p.name.ends_with(".rels") && types.of(&p.name).is_none()) {
            return Err(invalid("missing content type for a chart dependency"));
        }
        let bytes = serde_json::to_vec(self).map_err(|e| invalid(&e.to_string()))?;
        if bytes.len() > MAX_MANIFEST {
            return Err(invalid("manifest exceeds 128 KiB"));
        }
        doc.passthrough.insert(KEY.into(), Arc::new(bytes));
        Ok(())
    }
    pub fn from_doc(doc: &Document) -> Result<Self, DocxError> {
        let Some(bytes) = doc.passthrough.get(KEY) else { return Ok(Self::default()) };
        if bytes.len() > MAX_MANIFEST {
            return Err(invalid("manifest exceeds 128 KiB"));
        }
        let manifest: Self = serde_json::from_slice(bytes).map_err(|e| invalid(&e.to_string()))?;
        if manifest.roots.len() > MAX_ROOTS || manifest.parts.len() > MAX_ROOTS {
            return Err(invalid("manifest part limit"));
        }
        let mut ids = BTreeSet::new();
        for root in &manifest.roots {
            if !root.id.strip_prefix("wcChart").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                || root.id.len() > 32
                || !ids.insert(&root.id)
                || root.kind.len() > 4096
                || root.target.len() > 4096
            {
                return Err(invalid("invalid manifest relationship"));
            }
        }
        Ok(manifest)
    }
    /// Register explicit IDs in the story actually containing each opaque drawing. IDs cannot
    /// collide with generated rIdN links, and targets are absolute within the OPC package.
    pub fn emit(&self, fragment: &str, rels: &mut PartRels, docpr: &mut u32) -> Result<String, DocxError> {
        if fragment.len() > MAX_XML {
            return Err(invalid("drawing exceeds 1 MiB"));
        }
        let mut root = xml::parse(fragment.as_bytes())?;
        if root.name != "w:drawing" || root.find("c:chart").is_none() {
            return Err(invalid("invalid opaque chart drawing"));
        }
        let mut queue = vec![&mut root];
        while let Some(node) = queue.pop() {
            if node.name.starts_with("?:") || node.attrs.iter().any(|(k, _)| k.starts_with("?:")) {
                return Err(invalid("unsupported namespace in stored drawing"));
            }
            if node.name == "c:chart" {
                let id = node.attr("r:id").ok_or_else(|| invalid("stored chart has no relationship ID"))?;
                if !self.roots.iter().any(|r| r.id == id && !r.external && r.kind.rsplit('/').next() == Some("chart")) {
                    return Err(invalid("stored chart has no internal chart relationship"));
                }
            }
            if node.name == "wp:docPr" {
                *docpr = docpr.checked_add(1).ok_or_else(|| invalid("drawing ID overflow"))?;
                if let Some((_, value)) = node.attrs.iter_mut().find(|(name, _)| name == "id") {
                    *value = docpr.to_string();
                } else {
                    node.attrs.push(("id".into(), docpr.to_string()));
                }
            }
            for (name, value) in &node.attrs {
                if matches!(name.as_str(), "r:id" | "r:embed" | "r:link") {
                    let rel = self.roots.iter().find(|r| &r.id == value).ok_or_else(|| invalid("drawing relationship not in manifest"))?;
                    rels.preserve_id(&rel.id, &rel.kind, &if rel.external { rel.target.clone() } else { format!("/{}", rel.target) }, rel.external)?;
                }
            }
            queue.extend(node.kids.iter_mut().filter_map(|n| if let Node::El(e) = n { Some(e) } else { None }));
        }
        root.attrs.extend(xml::body_ns().into_iter().filter(|(name, _)| name.starts_with("xmlns:")));
        let output = root.to_xml();
        if output.len() > MAX_XML {
            return Err(invalid("drawing exceeds preservation size limit"));
        }
        Ok(output)
    }
    pub fn append(&self, doc: &Document, entries: &mut Vec<(String, Vec<u8>)>, types: &mut Vec<(String, String)>) -> Result<(), DocxError> {
        let mut names = BTreeSet::new();
        let mut total = 0usize;
        for part in &self.parts {
            let name = &part.name;
            if name.is_empty()
                || name.len() > 1024
                || name.contains(['\\', ':', '#', '?'])
                || name.split('/').any(|p| p.is_empty() || p == "." || p == "..")
                || !names.insert(name.to_ascii_lowercase())
                || entries.iter().any(|(n, _)| n.eq_ignore_ascii_case(name))
                || [
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
                .any(|p| p.eq_ignore_ascii_case(name))
                || part.content_type.is_empty()
                || part.content_type.len() > 4096
            {
                return Err(invalid("preserved part collides with generated content or has invalid metadata"));
            }
            let bytes = doc.passthrough.get(name).ok_or_else(|| DocxError::MissingPart(name.clone()))?;
            total = total.checked_add(bytes.len()).ok_or_else(|| invalid("part size overflow"))?;
            if total > 64 * 1024 * 1024 {
                return Err(invalid("preserved parts exceed 64 MiB"));
            }
        }
        for root in self.roots.iter().filter(|r| !r.external) {
            if !names.contains(&root.target.to_ascii_lowercase()) {
                return Err(invalid("missing root part"));
            }
        }
        for part in &self.parts {
            let bytes = doc.passthrough.get(&part.name).ok_or_else(|| DocxError::MissingPart(part.name.clone()))?;
            entries.push((part.name.clone(), bytes.to_vec()));
            types.push((format!("/{}", part.name), part.content_type.clone()));
        }
        Ok(())
    }
}

/// Explicitly disclose preservation-only charts to desktop and automation callers.
pub(crate) fn warnings(doc: &Document) -> Vec<String> {
    if doc.passthrough.contains_key(KEY) {
        vec!["Charts are preserved in DOCX/DOTX/DOCM/DOTM, but WordCraft shows placeholders and cannot render, print or edit them. Other export formats may lose chart data.".into()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_drawing_and_hostile_manifests_fail_before_preservation() {
        let mut drawing = El { name: "w:drawing".into(), attrs: vec![("descr".into(), "&".repeat(MAX_XML))], kids: vec![] };
        assert!(check_drawing_size(&drawing).is_err());
        drawing.attrs.clear();
        drawing.kids = (0..16_385).map(|_| Node::El(El::default())).collect();
        assert!(check_drawing_size(&drawing).is_err());
        for bytes in [
            b"{bad".to_vec(),
            vec![b' '; MAX_MANIFEST + 1],
            br#"{"roots":[{"id":"rId1","kind":"chart","target":"../original","external":false}],"parts":[]}"#.to_vec(),
        ] {
            let mut doc = Document::new();
            doc.passthrough.insert(KEY.into(), Arc::new(bytes));
            assert!(Preservation::from_doc(&doc).is_err());
        }
    }
}
