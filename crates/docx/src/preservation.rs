//! Conservative package-wide preservation. Unsupported XML is never regenerated.
//! A guarded import supports only proven, isolated plain-text paragraph changes.
use crate::{
    DocxError, Flavor,
    package::{ContentTypes, Package, zip_entries},
    preserved_xml::{self as xml, CT, Element, R, REL, RS, error, qualified},
};
use std::{collections::BTreeSet, sync::Arc};
use wordcraft_doc::{Block, Document};
const SOURCE: &str = "wordcraft:packageSource.v1";
const BASE: &str = "wordcraft:packageBaseline.v1";
const MODEL: &str = "wordcraft:packageModel.v1";
const MAX_DATA: usize = 64 * 1024 * 1024;
// A chart at the 100k-cell limit can serialize very large finite f64 values as long decimal
// strings. Keep the projection bounded while leaving enough room for that accepted chart size.
const MAX_MODEL: usize = 32 * 1024 * 1024;
const MAX_PARTS: usize = 512;
fn open(bytes: &[u8]) -> Result<Package, DocxError> {
    Package::open(bytes)
}
fn attr<'a>(node: &'a Element, name: &str) -> Option<&'a str> {
    node.attrs.get(&qualified("", name)).map(String::as_str)
}
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains(['\\', '#', '?'])
        && !path.split('/').next().is_some_and(|p| p.contains(':'))
        && path.split('/').all(|segment| !segment.is_empty() && segment != "." && segment != "..")
}
fn relationship_owner(path: &str) -> Result<String, DocxError> {
    if path == "_rels/.rels" {
        return Ok(String::new());
    }
    let (directory, filename) = path.rsplit_once('/').ok_or_else(|| error("invalid relationship part path"))?;
    let directory = directory.strip_suffix("_rels").ok_or_else(|| error("relationship part outside _rels directory"))?;
    let filename = filename.strip_suffix(".rels").ok_or_else(|| error("invalid relationship filename"))?;
    if filename.is_empty() {
        return Err(error("empty relationship owner"));
    }
    Ok(format!("{directory}{filename}"))
}
fn safe_target(owner: &str, target: &str) -> bool {
    if target.is_empty() || target.len() > 4096 || target.contains(['\\', '#', '?']) || target.split('/').next().is_some_and(|p| p.contains(':')) {
        return false;
    }
    let mut depth = if target.starts_with('/') { 0 } else { owner.rsplit_once('/').map_or(0, |(dir, _)| dir.split('/').count()) };
    for part in target.trim_start_matches('/').split('/') {
        match part {
            ".." => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            "" | "." => {}
            _ => depth += 1,
        }
    }
    true
}
/// Validate all graphs, including disconnected/orphaned parts, before the lenient model reader.
/// No package entry is executed and no external target is fetched.
pub(crate) fn validate(pkg: &Package) -> Result<(), DocxError> {
    let mut total = 0usize;
    let mut count = 0usize;
    for (path, bytes) in pkg.entries() {
        count += 1;
        total = total.checked_add(bytes.len()).ok_or_else(|| error("package size overflow"))?;
        if !safe_path(path) || count > MAX_PARTS || total > MAX_DATA {
            return Err(error("package preservation limit or invalid part path (512 parts / 64 MiB)"));
        }
    }
    let content_types = pkg.get("[Content_Types].xml").ok_or_else(|| error("missing package content types"))?;
    let nodes = xml::inspect(content_types)?;
    let root = nodes.first().filter(|n| n.is(CT, "Types")).ok_or_else(|| error("invalid content type root"))?;
    let mut definitions = BTreeSet::new();
    for index in &root.children {
        let node = nodes.get(*index).ok_or_else(|| error("content type structure"))?;
        let key = if node.is(CT, "Override") {
            let path = attr(node, "PartName").ok_or_else(|| error("content type override has no part name"))?;
            if !path.starts_with('/') || !safe_path(path.trim_start_matches('/')) {
                return Err(error("invalid content type part name"));
            }
            format!("override:{}", path.to_ascii_lowercase())
        } else if node.is(CT, "Default") {
            let extension = attr(node, "Extension")
                .filter(|s| !s.is_empty() && !s.contains(['/', '\\', ':', '.']))
                .ok_or_else(|| error("invalid content type extension"))?;
            format!("default:{}", extension.to_ascii_lowercase())
        } else {
            continue;
        };
        if attr(node, "ContentType").is_none_or(str::is_empty) || !definitions.insert(key) {
            return Err(error("empty or conflicting content type definition"));
        }
    }
    let types = ContentTypes::read(pkg);
    for (path, bytes) in pkg.entries() {
        if path == "[Content_Types].xml" {
            continue;
        }
        let content_type = types.of(path).ok_or_else(|| error(format!("missing content type for {path}")))?;
        let is_xml = path.ends_with(".xml")
            || path.ends_with(".rels")
            || content_type.ends_with("+xml")
            || matches!(content_type, "application/xml" | "text/xml");
        // Flat OPC permits an explicit empty `pkg:xmlData` part. It is not a well-formed XML
        // document, but it must survive unpacking as an empty part rather than making the entire
        // package unreadable. Relationship parts still fail below because they require a root.
        let nodes = if is_xml && !bytes.is_empty() {
            Some(xml::inspect(bytes).map_err(|e| match e {
                DocxError::Preservation(message) => error(format!("invalid XML in {path}: {message}")),
                other => other,
            })?)
        } else {
            None
        };
        if path.ends_with(".rels") {
            let owner = relationship_owner(path)?;
            if !owner.is_empty() && pkg.get(&owner).is_none() {
                return Err(error(format!("relationship owner {owner} is missing")));
            }
            let nodes = nodes.as_ref().ok_or_else(|| error("uninspected relationships"))?;
            let root = nodes.first().filter(|n| n.is(REL, "Relationships")).ok_or_else(|| error("invalid relationship namespace/root"))?;
            for index in &root.children {
                let node = nodes.get(*index).ok_or_else(|| error("relationship structure"))?;
                if !node.is(REL, "Relationship") {
                    continue;
                }
                let target = attr(node, "Target").ok_or_else(|| error("relationship has no target"))?;
                let kind = attr(node, "Type").unwrap_or("");
                let empty_hyperlink = target.is_empty() && kind.ends_with("/hyperlink");
                let external = match attr(node, "TargetMode") {
                    None => false,
                    Some(mode) if mode.eq_ignore_ascii_case("Internal") => false,
                    Some(mode) if mode.eq_ignore_ascii_case("External") => true,
                    _ => return Err(error("invalid relationship target mode")),
                };
                if !external && !empty_hyperlink && !safe_target(&owner, target) {
                    return Err(error(format!("unsafe relationship target in {path}")));
                }
            }
            for relationship in pkg.rels_checked(&owner)?.list {
                let optional_story = relationship.kind.ends_with("/header") || relationship.kind.ends_with("/footer");
                if !relationship.external && !relationship.target.is_empty() && !optional_story && pkg.get(&relationship.target).is_none() {
                    return Err(DocxError::MissingPart(relationship.target));
                }
            }
        } else if let Some(nodes) = nodes {
            let references: Vec<&str> = nodes
                .iter()
                .flat_map(|node| &node.attrs)
                .filter_map(|(key, value)| {
                    ([R, RS].iter().any(|ns| key.starts_with(&format!("{{{ns}}}")))
                        || key == &qualified("urn:schemas-microsoft-com:office:office", "relid"))
                        .then_some(value.as_str())
                })
                .collect();
            if !references.is_empty() {
                let rels = pkg.rels_checked(path)?;
                for id in references {
                    if rels.by_id(id).is_none() {
                        return Err(error(format!("{path}: undefined relationship ID {id}")));
                    }
                }
            }
        }
    }
    Ok(())
}
fn flavor(pkg: &Package, main: &str) -> Result<Flavor, DocxError> {
    let types = ContentTypes::read(pkg);
    let content_type = types.of(main).ok_or_else(|| error("missing main document content type"))?;
    [Flavor::Document, Flavor::Template, Flavor::MacroDocument, Flavor::MacroTemplate]
        .into_iter()
        .find(|f| f.main_content_type() == content_type)
        .ok_or_else(|| error("unsupported main document content type"))
}
fn sensitive(pkg: &Package) -> bool {
    pkg.entries().any(|(path, bytes)| {
        let path = path.to_ascii_lowercase();
        path.starts_with("_xmlsignatures/")
            || path.contains("/activex/")
            || path.contains("vbaproject")
            || (path.ends_with(".rels")
                && [b"vbaProject".as_slice(), b"activeXControl", b"digital-signature"]
                    .iter()
                    .any(|needle| bytes.windows(needle.len()).any(|part| part == *needle)))
    })
}
fn same_package(source: &Package, baseline: &Package) -> Result<bool, DocxError> {
    if source.entries().map(|(name, _)| name).collect::<BTreeSet<_>>() != baseline.entries().map(|(name, _)| name).collect::<BTreeSet<_>>() {
        return Ok(false);
    }
    for (path, source) in source.entries() {
        let Some(baseline) = baseline.get(path) else {
            return Ok(false);
        };
        if path.ends_with(".xml") || path.ends_with(".rels") {
            if !xml::equivalent(source, baseline)? {
                return Ok(false);
            }
        } else if source != baseline {
            return Ok(false);
        }
    }
    Ok(true)
}
fn has_custom_urn_markup(pkg: &Package) -> Result<bool, DocxError> {
    for (path, bytes) in pkg.entries().filter(|(path, _)| path.ends_with(".xml") || path.ends_with(".rels")) {
        if path == "[Content_Types].xml" || bytes.is_empty() {
            continue;
        }
        if xml::inspect(bytes)?.iter().any(|node| {
            node.name
                .strip_prefix('{')
                .and_then(|name| name.split_once('}'))
                .is_some_and(|(namespace, _)| namespace.starts_with("urn:") && !namespace.starts_with("urn:schemas-microsoft-com:"))
        }) {
            return Ok(true);
        }
    }
    Ok(false)
}
fn preserved_xml_tokens(source: &[u8], output: &[u8]) -> Result<bool, DocxError> {
    let source_bytes = xml::utf8(source)?;
    let output_bytes = xml::utf8(output)?;
    let source = std::str::from_utf8(&source_bytes).map_err(|e| error(e.to_string()))?;
    let output = std::str::from_utf8(&output_bytes).map_err(|e| error(e.to_string()))?;
    for (start, _) in source.match_indices("<!--") {
        let Some(end) = source.get(start..).and_then(|rest| rest.find("-->")) else { return Ok(false) };
        if !output.contains(source.get(start..start + end + 3).ok_or_else(|| error("XML comment range"))?) {
            return Ok(false);
        }
    }
    for (start, _) in source.match_indices("<?") {
        if source.get(start + 2..).is_some_and(|tail| tail.starts_with("xml ") || tail.starts_with("xml?>")) {
            continue;
        }
        let Some(end) = source.get(start..).and_then(|rest| rest.find("?>")) else { return Ok(false) };
        if !output.contains(source.get(start..start + end + 2).ok_or_else(|| error("XML processing instruction range"))?) {
            return Ok(false);
        }
    }
    for (start, _) in source.match_indices("xmlns:") {
        let tail = source.get(start + 6..).ok_or_else(|| error("namespace declaration range"))?;
        let Some(eq) = tail.find('=') else { return Ok(false) };
        let name = tail.get(..eq).ok_or_else(|| error("namespace declaration name"))?.trim();
        let value = tail.get(eq + 1..).ok_or_else(|| error("namespace declaration value"))?.trim_start();
        let Some(quote) = value.chars().next().filter(|c| *c == '\'' || *c == '"') else { return Ok(false) };
        let value = value
            .get(1..)
            .and_then(|value| value.find(quote).and_then(|end| value.get(..end)))
            .ok_or_else(|| error("namespace declaration terminator"))?;
        if !output.contains(&format!("xmlns:{name}=\"{value}\"")) && !output.contains(&format!("xmlns:{name}='{value}'")) {
            return Ok(false);
        }
    }
    Ok(true)
}
/// VML preset shapes that WordCraft maps to an equivalent DrawingML preset are an intentional
/// conversion, not unsupported source content. Permit that conversion when the main document is
/// the only package part that changes and every VML element belongs to the mapped preset subset.
fn modeled_vml_conversion(source: &Package, baseline: &Package, main: &str) -> Result<bool, DocxError> {
    for (path, bytes) in source.entries() {
        let main_rels = relationship_owner(path).is_ok_and(|owner| owner.eq_ignore_ascii_case(main));
        if !path.eq_ignore_ascii_case(main)
            && path != "[Content_Types].xml"
            && path != "_rels/.rels"
            && !main_rels
            && baseline.get(path) != Some(bytes)
        {
            return Ok(false);
        }
    }
    let source_root = source.rels_checked("")?;
    let generated_root = baseline.rels_checked("")?;
    let source_main_rels = source.rels_checked(main)?;
    let generated_main_rels = baseline.rels_checked(main)?;
    if source_root.list.iter().chain(&source_main_rels.list).any(|rel| {
        !generated_root
            .list
            .iter()
            .chain(&generated_main_rels.list)
            .any(|candidate| candidate.kind == rel.kind && candidate.target == rel.target && candidate.external == rel.external)
    }) {
        return Ok(false);
    }
    let source_types = ContentTypes::read(source);
    let generated_types = ContentTypes::read(baseline);
    if source.entries().any(|(path, _)| path != "[Content_Types].xml" && source_types.of(path) != generated_types.of(path)) {
        return Ok(false);
    }
    let Some(bytes) = source.get(main) else { return Ok(false) };
    let nodes = xml::inspect(bytes)?;
    let namespace = "urn:schemas-microsoft-com:vml";
    let vml: Vec<_> = nodes.iter().filter(|node| node.name.starts_with(&format!("{{{namespace}}}"))).collect();
    if vml.is_empty() {
        return Ok(false);
    }
    let allowed = ["shapetype", "shape", "stroke", "formulas", "f", "path", "line", "rect", "roundrect", "oval"];
    if vml.iter().any(|node| !allowed.contains(&node.name.trim_start_matches(&format!("{{{namespace}}}")))) {
        return Ok(false);
    }
    let office_spt = "{urn:schemas-microsoft-com:office:office}spt";
    let unqualified = |name: &str| qualified("", name);
    for shape in vml.iter().filter(|node| node.name == format!("{{{namespace}}}shape")) {
        let type_id = shape.attrs.get(&unqualified("type")).map(|value| value.trim().trim_start_matches('#'));
        let shape_type = type_id.and_then(|id| {
            vml.iter().find(|node| node.name == format!("{{{namespace}}}shapetype") && node.attrs.get(&unqualified("id")).is_some_and(|v| v == id))
        });
        let spt = shape
            .attrs
            .get(office_spt)
            .or_else(|| shape_type.and_then(|node| node.attrs.get(office_spt)))
            .and_then(|value| value.parse::<u32>().ok())
            .or_else(|| type_id.and_then(|id| id.strip_prefix("_x0000_t")).and_then(|value| value.parse::<u32>().ok()));
        if !["line", "rect", "roundrect", "oval"].iter().any(|kind| shape.name == format!("{{{namespace}}}{kind}"))
            && !spt.is_some_and(|value| crate::read::shape_style::vml_preset_kind(value).is_some())
        {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Allow the native writer to handle its modeled Word parts while retaining the package guard
/// for unsupported markup. Every unmodeled part must remain byte-identical, every original
/// relationship must survive, and each source XML element and attribute shape must still occur
/// in the generated part. This lets modeled comments, numbering, styles and privacy metadata use
/// the current writer without discarding unknown package content.
fn modeled_writer_rewrite(source: &Package, generated: &Package, main: &str) -> Result<bool, DocxError> {
    let editable = [
        main,
        "[Content_Types].xml",
        "_rels/.rels",
        "docProps/app.xml",
        "word/_rels/document.xml.rels",
        "word/styles.xml",
        "word/numbering.xml",
        "word/settings.xml",
        "word/comments.xml",
        "word/commentsExtended.xml",
        "word/commentsIds.xml",
        "word/commentsExtensible.xml",
    ];
    for (path, bytes) in source.entries() {
        let Some(output) = generated.get(path) else { return Ok(false) };
        if editable.iter().any(|candidate| path.eq_ignore_ascii_case(candidate)) {
            if path.ends_with(".rels") {
                let owner = relationship_owner(path)?;
                let input_rels = source.rels_checked(&owner)?;
                let output_rels = generated.rels_checked(&owner)?;
                if input_rels.list.iter().any(|rel| {
                    !output_rels
                        .list
                        .iter()
                        .any(|candidate| candidate.kind == rel.kind && candidate.target == rel.target && candidate.external == rel.external)
                }) {
                    return Ok(false);
                }
            } else if path != "[Content_Types].xml" && path != "docProps/app.xml" {
                let input = xml::inspect(bytes)?;
                let output = xml::inspect(output)?;
                if !preserved_xml_tokens(bytes, generated.get(path).ok_or_else(|| error("generated XML part missing"))?)? {
                    return Ok(false);
                }
                if input.iter().any(|node| {
                    node.name
                        .strip_prefix('{')
                        .and_then(|name| name.split_once('}'))
                        .is_some_and(|(namespace, _)| namespace.starts_with("urn:") && !namespace.starts_with("urn:schemas-microsoft-com:"))
                }) {
                    return Ok(false);
                }
                if input.iter().any(|node| {
                    !output.iter().any(|candidate| {
                        candidate.name == node.name
                            && node.attrs.keys().all(|name| candidate.attrs.contains_key(name))
                            // Section direction is an enum the model normalizes when it does not
                            // recognize the source value. Preserve the package in that case.
                            && (!node.word("textDirection")
                                || node.attrs.iter().all(|(name, value)| candidate.attrs.get(name) == Some(value)))
                            && (node.text.trim().is_empty() || candidate.text == node.text)
                    }) && !(path.eq_ignore_ascii_case("word/settings.xml")
                        && node.word("zoom")
                        && node.attrs.keys().all(|name| {
                            matches!(
                                name.as_str(),
                                "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}val"
                                    | "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}percent"
                            )
                        })
                        && output.iter().any(|candidate| {
                            candidate.name == node.name
                                && candidate.attrs.contains_key("{http://schemas.openxmlformats.org/wordprocessingml/2006/main}percent")
                        }))
                }) {
                    return Ok(false);
                }
            }
        } else if bytes != output {
            return Ok(false);
        }
    }
    let source_types = ContentTypes::read(source);
    let generated_types = ContentTypes::read(generated);
    for (path, _) in source.entries().filter(|(path, _)| *path != "[Content_Types].xml") {
        let writer_owned = editable.iter().any(|candidate| path.eq_ignore_ascii_case(candidate));
        if !writer_owned && source_types.of(path) != generated_types.of(path) {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Only imports whose complete package is demonstrably reproduced by the ordinary writer
/// avoid guarded mode. Unknown names/attributes, bindings and opaque parts force preservation.
pub(crate) fn capture(bytes: &[u8], pkg: &Package, main: &str, doc: &mut Document) -> Result<(), DocxError> {
    let baseline = crate::write::write_native(doc, flavor(pkg, main)?).unwrap_or_default();
    if !baseline.is_empty() && !sensitive(pkg) && !has_custom_urn_markup(pkg)? {
        let generated = open(&baseline)?;
        let same = same_package(pkg, &generated)?;
        let vml = modeled_vml_conversion(pkg, &generated, main)?;
        let styles_have_explicit_defaults =
            pkg.get("word/styles.xml").map(xml::inspect).transpose()?.is_none_or(|nodes| nodes.iter().any(|node| node.word("pPrDefault")));
        let modeled = styles_have_explicit_defaults && modeled_writer_rewrite(pkg, &generated, main)?;
        if same || vml || modeled {
            return Ok(());
        }
    }
    let model = serde_json::to_vec(doc).map_err(|e| error(e.to_string()))?;
    if bytes.len() > MAX_DATA || baseline.len() > MAX_DATA || model.len() > MAX_MODEL {
        return Err(error(format!(
            "preservation storage budget exceeded (source {} / {MAX_DATA} bytes, baseline {} / {MAX_DATA} bytes, model {} / {MAX_MODEL} bytes)",
            bytes.len(),
            baseline.len(),
            model.len()
        )));
    }
    doc.passthrough.insert(SOURCE.into(), Arc::new(bytes.to_vec()));
    doc.passthrough.insert(BASE.into(), Arc::new(baseline));
    doc.passthrough.insert(MODEL.into(), Arc::new(model));
    Ok(())
}
fn get<'a>(doc: &'a Document, key: &str, limit: usize) -> Result<&'a [u8], DocxError> {
    doc.passthrough
        .get(key)
        .filter(|b| b.len() <= limit)
        .map(|b| b.as_slice())
        .ok_or_else(|| error(format!("missing, corrupt or oversized preservation state {key}")))
}
struct TextEdit {
    index: usize,
    anchor: String,
    replacement: String,
}
#[derive(Clone, Copy)]
struct RunFormatEdit {
    index: usize,
    start: usize,
    end: usize,
    property: RunProperty,
    value: Option<bool>,
}
#[derive(Clone, Copy)]
enum RunProperty {
    Bold,
    Italic,
}
struct StyleEdit {
    index: usize,
    previous: Option<String>,
    style: String,
}
#[derive(Clone, Copy)]
struct GraphicSizeEdit {
    old_w: f32,
    old_h: f32,
    new_w: f32,
    new_h: f32,
}

/// Names document-model fields that differ after an edit is normalized against the imported
/// baseline. The diagnostic intentionally reports field paths only, never source text or values.
fn differing_model_fields(current: &Document, baseline: &Document) -> Vec<&'static str> {
    let mut fields = Vec::new();
    macro_rules! compare_field {
        ($field:ident) => {
            if current.$field != baseline.$field {
                fields.push(concat!("document.", stringify!($field)));
            }
        };
    }
    compare_field!(body);
    compare_field!(last_section);
    compare_field!(parts);
    compare_field!(styles);
    compare_field!(numbering);
    compare_field!(comments);
    compare_field!(revisions);
    let zoom_changed = current.settings.zoom != baseline.settings.zoom;
    if zoom_changed {
        fields.push("document.settings.zoom");
    }
    let mut current_settings = current.settings.clone();
    let baseline_settings = baseline.settings.clone();
    current_settings.zoom = baseline_settings.zoom;
    if current_settings != baseline_settings {
        fields.push("document.settings");
    }
    compare_field!(core);
    compare_field!(sources);
    compare_field!(custom_props);
    compare_field!(footnote_separators);
    compare_field!(endnote_separators);
    compare_field!(media);
    compare_field!(passthrough);
    fields
}

fn edits(doc: &Document, baseline: &Document) -> Result<(Vec<TextEdit>, Vec<RunFormatEdit>, Vec<StyleEdit>, Vec<GraphicSizeEdit>), DocxError> {
    let mut normalized = doc.clone();
    if normalized.body.len() != baseline.body.len() {
        return Err(error("structural editing of a guarded DOCX is unsupported; the original package is preserved"));
    }
    let mut edits = Vec::new();
    let mut format_edits = Vec::new();
    let mut style_edits = Vec::new();
    let mut graphic_sizes = Vec::new();
    for (index, (current, original)) in doc.body.iter().zip(&baseline.body).enumerate() {
        if current == original {
            continue;
        }
        let (Some(current), Some(original)) = (current.as_para(), original.as_para()) else {
            return Err(error("table/object editing cannot be preserved safely in this DOCX"));
        };
        if current.text == original.text && current.objects == original.objects && current.section == original.section {
            let mut paragraph = current.clone();
            let style = paragraph.props.style.take();
            let mut baseline_paragraph = original.clone();
            baseline_paragraph.props.style = None;
            if paragraph.props == baseline_paragraph.props {
                let changed_style = style != original.props.style;
                if changed_style {
                    let Some(style) = style else {
                        return Err(error("removing a paragraph style cannot be preserved safely"));
                    };
                    if !baseline.styles.styles.iter().any(|candidate| candidate.id == style) {
                        return Err(error("target paragraph style is not present in the source styles part"));
                    }
                    style_edits.push(StyleEdit { index, previous: original.props.style.clone(), style });
                }
                if current.mark != original.mark || current.runs.len() != original.runs.len() {
                    return Err(error("run formatting boundaries changed and cannot be mapped to source OOXML"));
                }
                let mut offset = 0usize;
                for (now, before) in current.runs.iter().zip(&original.runs) {
                    if now.len != before.len {
                        return Err(error("run formatting boundaries changed and cannot be mapped to source OOXML"));
                    }
                    if now.props != before.props {
                        let mut restored = now.props.clone();
                        restored.bold = before.props.bold;
                        let bold_only = restored == before.props;
                        let property = if bold_only {
                            restored = now.props.clone();
                            RunProperty::Bold
                        } else {
                            restored = now.props.clone();
                            restored.italic = before.props.italic;
                            if restored != before.props {
                                return Err(error("only direct bold or italic formatting on whole mapped runs can be preserved safely"));
                            }
                            RunProperty::Italic
                        };
                        let value = match property {
                            RunProperty::Bold => now.props.bold,
                            RunProperty::Italic => now.props.italic,
                        };
                        let end = offset.checked_add(now.len).ok_or_else(|| error("run offset overflow"))?;
                        format_edits.push(RunFormatEdit { index, start: offset, end, property, value });
                    }
                    offset = offset.checked_add(now.len).ok_or_else(|| error("run offset overflow"))?;
                }
                let changed_format = current.runs.iter().zip(&original.runs).any(|(now, before)| now.props != before.props);
                if changed_style || changed_format {
                    *normalized.body.get_mut(index).ok_or_else(|| error("paragraph index"))? = Arc::new(Block::Para(original.clone()));
                    continue;
                }
            }
        }
        let mut size_only = current.clone();
        let mut size_edits = Vec::new();
        if size_only.objects.len() == original.objects.len() {
            for (now, before) in size_only.objects.iter_mut().zip(&original.objects) {
                match (now, before) {
                    (wordcraft_doc::InlineObject::Graphic { w, h, .. }, wordcraft_doc::InlineObject::Graphic { w: old_w, h: old_h, .. })
                        if w != old_w || h != old_h =>
                    {
                        size_edits.push(GraphicSizeEdit { old_w: *old_w, old_h: *old_h, new_w: *w, new_h: *h });
                        *w = *old_w;
                        *h = *old_h;
                    }
                    _ => {}
                }
            }
        }
        if !size_edits.is_empty() && size_only == *original {
            graphic_sizes.extend(size_edits);
            *normalized.body.get_mut(index).ok_or_else(|| error("paragraph index"))? = Arc::new(Block::Para(original.clone()));
            continue;
        }
        let prefix = original.text.chars().zip(current.text.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
        let old_tail = original.text.get(prefix..).ok_or_else(|| error("text edit boundary"))?;
        let new_tail = current.text.get(prefix..).ok_or_else(|| error("text edit boundary"))?;
        let suffix = old_tail.chars().rev().zip(new_tail.chars().rev()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
        let old_end = original.text.len().saturating_sub(suffix);
        let new_end = current.text.len().saturating_sub(suffix);
        let added = current.text.get(prefix..new_end).ok_or_else(|| error("text edit range"))?;
        let removed = original.text.get(prefix..old_end).ok_or_else(|| error("text edit range"))?;
        if added.is_empty() && removed.is_empty() {
            return Err(error("formatting/state editing is unsupported in this guarded DOCX"));
        }
        if added.chars().chain(removed.chars()).any(|c| c.is_control() || c == '\u{fffc}') {
            return Err(error("editing fields, objects or text structure is unsupported in this guarded DOCX"));
        }
        let mut expected = original.clone();
        let props = original.props_of_char(prefix).clone();
        expected.delete(prefix, old_end).map_err(|e| error(e.to_string()))?;
        expected.insert_text(prefix, added, &props).map_err(|e| error(e.to_string()))?;
        if &expected != current {
            return Err(error("only isolated text edits with unchanged formatting and objects can be preserved safely"));
        }
        let text_end = original.text.char_indices().find(|(_, c)| c.is_control() || *c == '\u{fffc}').map_or(original.text.len(), |(i, _)| i);
        let text_end = text_end.min(original.runs.first().map_or(text_end, |run| run.len));
        let anchor = original.text.get(..text_end).ok_or_else(|| error("text anchor boundary"))?;
        if old_end > text_end {
            return Err(error("this edit crosses a protected field or formatting boundary"));
        }
        let mut replacement = anchor.to_string();
        replacement.replace_range(prefix..old_end, added);
        edits.push(TextEdit { index, anchor: anchor.to_string(), replacement });
        *normalized.body.get_mut(index).ok_or_else(|| error("paragraph index"))? = Arc::new(Block::Para(original.clone()));
    }
    // The preservation envelope is storage metadata, not an edit to the imported document.
    for key in [SOURCE, BASE, MODEL] {
        normalized.passthrough.remove(key);
    }
    if serde_json::to_vec(&normalized).map_err(|e| error(e.to_string()))? != serde_json::to_vec(baseline).map_err(|e| error(e.to_string()))? {
        let fields = differing_model_fields(&normalized, baseline);
        let fields = if fields.is_empty() { "serialized model state".to_string() } else { fields.join(", ") };
        return Err(error(format!("protected document state changed at {fields}; refusing save to preserve unsupported OOXML")));
    }
    Ok((edits, format_edits, style_edits, graphic_sizes))
}
struct SourceRun {
    start: usize,
    end: usize,
    run: usize,
}

/// Direct paragraphs made only of simple text runs have a provable source mapping. Wrappers,
/// fields, markers, multiple text nodes in a run, and non-text run children remain guarded.
fn source_runs(nodes: &[Element], para: &Element) -> Option<Vec<SourceRun>> {
    if !para.word("p") {
        return None;
    }
    let mut out = Vec::new();
    let mut offset = 0usize;
    for run_index in &para.children {
        let run = nodes.get(*run_index)?;
        if run.word("pPr") {
            continue;
        }
        if !run.word("r") {
            return (!out.is_empty()).then_some(out);
        }
        let mut text = None;
        for child_index in &run.children {
            let child = nodes.get(*child_index)?;
            if child.word("rPr") {
                continue;
            }
            if text.is_some() || !child.word("t") || child.empty || !child.children.is_empty() {
                return (!out.is_empty()).then_some(out);
            }
            if !child.attrs.keys().all(|key| key == &qualified("http://www.w3.org/XML/1998/namespace", "space")) {
                return None;
            }
            text = Some(*child_index);
        }
        let text_index = text?;
        let text_node = nodes.get(text_index)?;
        if text_node.text.is_empty() {
            return None;
        }
        let end = offset.checked_add(text_node.text.len())?;
        out.push(SourceRun { start: offset, end, run: *run_index });
        offset = end;
    }
    Some(out)
}
/// Only the first direct, simple text run has the legacy safe text-edit mapping.
fn text_slot<'a>(nodes: &'a [Element], para: &Element) -> Option<&'a Element> {
    if !para.word("p") {
        return None;
    }
    let run = para.children.iter().filter_map(|i| nodes.get(*i)).find(|node| !node.word("pPr"))?;
    if !run.word("r") {
        return None;
    }
    let texts: Vec<_> = run.children.iter().filter_map(|i| nodes.get(*i)).filter(|node| !node.word("rPr")).collect();
    let text = *texts.first()?;
    if texts.len() != 1
        || !text.word("t")
        || text.empty
        || !text.children.is_empty()
        || !text.attrs.keys().all(|key| key == &qualified("http://www.w3.org/XML/1998/namespace", "space"))
    {
        return None;
    }
    Some(text)
}
fn map_blocks(nodes: &[Element], parent: &Element, protected: bool, blocks: &mut Vec<Option<usize>>) -> Result<(), DocxError> {
    for index in &parent.children {
        let node = nodes.get(*index).ok_or_else(|| error("source block index"))?;
        if node.word("p") || node.word("tbl") {
            blocks.push((!protected).then_some(*index));
        } else if node.word("sdt") {
            if let Some(content) = node.children.iter().filter_map(|i| nodes.get(*i)).find(|n| n.word("sdtContent")) {
                map_blocks(nodes, content, true, blocks)?;
            }
        } else if ["customXml", "smartTag", "ins", "moveTo"].iter().any(|name| node.word(name)) {
            map_blocks(nodes, node, true, blocks)?;
        } else if node.is("http://schemas.openxmlformats.org/markup-compatibility/2006", "AlternateContent") {
            return Err(error("editing a story containing block alternate-content branches cannot be mapped safely"));
        }
    }
    Ok(())
}
#[derive(Clone)]
struct Splice {
    start: usize,
    end: usize,
    replacement: String,
}

fn run_property_name(property: RunProperty) -> &'static str {
    match property {
        RunProperty::Bold => "b",
        RunProperty::Italic => "i",
    }
}

fn run_property_rank(name: &str) -> Option<usize> {
    Some(match name {
        "rStyle" => 0,
        "rFonts" => 1,
        "b" => 2,
        "bCs" => 3,
        "i" => 4,
        "iCs" => 5,
        "caps" => 6,
        "smallCaps" => 7,
        "strike" => 8,
        "dstrike" => 9,
        "outline" => 10,
        "shadow" => 11,
        "emboss" => 12,
        "imprint" => 13,
        "noProof" => 14,
        "snapToGrid" => 15,
        "vanish" => 16,
        "webHidden" => 17,
        "color" => 18,
        "spacing" => 19,
        "w" => 20,
        "kern" => 21,
        "position" => 22,
        "sz" => 23,
        "szCs" => 24,
        "highlight" => 25,
        "u" => 26,
        "effect" => 27,
        "bdr" => 28,
        "shd" => 29,
        "fitText" => 30,
        "vertAlign" => 31,
        "rtl" => 32,
        "cs" => 33,
        "em" => 34,
        "lang" => 35,
        "eastAsianLayout" => 36,
        "specVanish" => 37,
        "oMath" => 38,
        "rPrChange" => 39,
        _ => return None,
    })
}

fn property_element(bytes: &[u8], node: &Element) -> Result<String, DocxError> {
    let raw =
        std::str::from_utf8(bytes.get(node.span.clone()).ok_or_else(|| error("run property source range"))?).map_err(|e| error(e.to_string()))?;
    let name = raw.strip_prefix('<').and_then(|tail| tail.split([' ', '/', '>']).next()).ok_or_else(|| error("run property source tag"))?;
    if name.split_once(':').map(|(_, local)| local) != Some(node.name.rsplit('}').next().unwrap_or_default()) {
        return Err(error("run property source name is ambiguous"));
    }
    Ok(name.to_string())
}

fn patch_run_property(bytes: &[u8], nodes: &[Element], run_index: usize, property: RunProperty, value: Option<bool>) -> Result<Splice, DocxError> {
    let run = nodes.get(run_index).ok_or_else(|| error("source run index"))?;
    let local = run_property_name(property);
    let word_name = qualified(xml::W, local);
    let rpr = run.children.iter().filter_map(|index| nodes.get(*index)).find(|child| child.word("rPr"));
    let prefix = property_element(bytes, run)?
        .split_once(':')
        .map(|(prefix, _)| prefix.to_string())
        .filter(|prefix| !prefix.is_empty())
        .ok_or_else(|| error("formatting edit needs an explicit source namespace prefix"))?;
    let tag = format!("{prefix}:{local}");
    let value_attr = format!("{prefix}:val");
    let markup = match value {
        Some(true) => format!("<{tag}/>"),
        Some(false) => format!("<{tag} {value_attr}=\"0\"/>"),
        None => String::new(),
    };
    if let Some(rpr) = rpr {
        let matches: Vec<_> = rpr.children.iter().filter_map(|index| nodes.get(*index)).filter(|child| child.name == word_name).collect();
        if matches.len() > 1 {
            return Err(error("duplicate direct run properties cannot be edited safely"));
        }
        if let Some(existing) = matches.first() {
            if !existing.children.is_empty() || existing.attrs.keys().any(|name| name != &qualified(xml::W, "val")) {
                return Err(error("run property carries unsupported markup or attributes"));
            }
            return Ok(Splice { start: existing.span.start, end: existing.span.end, replacement: markup });
        }
        if value.is_none() {
            return Err(error("source run formatting does not match the imported baseline"));
        }
        if rpr.empty {
            return Ok(Splice { start: rpr.span.start, end: rpr.span.end, replacement: format!("<{prefix}:rPr>{markup}</{prefix}:rPr>") });
        }
        let rank = run_property_rank(local).ok_or_else(|| error("unsupported run property rank"))?;
        let mut insertion = rpr.content.end;
        for child in rpr.children.iter().filter_map(|index| nodes.get(*index)) {
            let name = child.name.strip_prefix(&format!("{{{}}}", xml::W)).ok_or_else(|| error("run properties contain an unsupported namespace"))?;
            let child_rank = run_property_rank(name).ok_or_else(|| error("run property order cannot be preserved safely"))?;
            if child_rank > rank {
                insertion = child.span.start;
                break;
            }
        }
        Ok(Splice { start: insertion, end: insertion, replacement: markup })
    } else {
        if value.is_none() {
            return Err(error("source run formatting does not match the imported baseline"));
        }
        let text = run
            .children
            .iter()
            .filter_map(|index| nodes.get(*index))
            .find(|child| child.word("t"))
            .ok_or_else(|| error("source run has no text node"))?;
        Ok(Splice { start: text.span.start, end: text.span.start, replacement: format!("<{prefix}:rPr>{markup}</{prefix}:rPr>") })
    }
}

fn patch_paragraph_style(bytes: &[u8], nodes: &[Element], paragraph_index: usize, edit: &StyleEdit) -> Result<Splice, DocxError> {
    let paragraph = nodes.get(paragraph_index).ok_or_else(|| error("source paragraph index"))?;
    let ppr = paragraph
        .children
        .iter()
        .filter_map(|index| nodes.get(*index))
        .find(|child| child.word("pPr"))
        .ok_or_else(|| error("paragraph style has no mapped source properties"))?;
    let mut styles = ppr.children.iter().filter_map(|index| nodes.get(*index)).filter(|child| child.word("pStyle"));
    let style = styles.next().ok_or_else(|| error("paragraph style reference is absent from source OOXML"))?;
    if styles.next().is_some() || style.attrs.len() != 1 || !style.children.is_empty() {
        return Err(error("paragraph style source mapping is ambiguous"));
    }
    let value = style.attrs.get(&qualified(xml::W, "val")).ok_or_else(|| error("paragraph style value is absent"))?;
    if edit.previous.as_deref() != Some(value.as_str()) {
        return Err(error("paragraph style source does not match the imported baseline"));
    }
    let name = property_element(bytes, style)?;
    let prefix = name
        .split_once(':')
        .map(|(prefix, _)| prefix)
        .filter(|prefix| !prefix.is_empty())
        .ok_or_else(|| error("paragraph style edit needs an explicit source namespace prefix"))?;
    Ok(Splice {
        start: style.span.start,
        end: style.span.end,
        replacement: format!("<{prefix}:pStyle {prefix}:val=\"{}\"/>", crate::xml::esc(&edit.style)),
    })
}

fn patch_text(
    bytes: &[u8],
    edits: &[TextEdit],
    format_edits: &[RunFormatEdit],
    style_edits: &[StyleEdit],
    expected_blocks: usize,
) -> Result<Vec<u8>, DocxError> {
    if edits.is_empty() && format_edits.is_empty() && style_edits.is_empty() {
        return Ok(bytes.to_vec());
    }
    if !matches!(xml::utf8(bytes)?, std::borrow::Cow::Borrowed(_)) {
        return Err(error("editing preserved UTF-16 story XML is unsupported"));
    }
    let nodes = xml::inspect(bytes)?;
    let body = nodes
        .first()
        .and_then(|root| root.children.iter().filter_map(|i| nodes.get(*i)).find(|n| n.word("body")))
        .ok_or_else(|| error("missing document body"))?;
    let mut blocks = Vec::new();
    map_blocks(&nodes, body, false, &mut blocks)?;
    if blocks.len() != expected_blocks {
        return Err(error("source/model block mapping is ambiguous; this edit cannot be preserved safely"));
    }
    let mut replacements = Vec::new();
    for edit in edits {
        let para = blocks
            .get(edit.index)
            .and_then(|i| *i)
            .and_then(|i| nodes.get(i))
            .ok_or_else(|| error("edited text is inside a protected block structure"))?;
        let slot =
            text_slot(&nodes, para).filter(|text| text.text == edit.anchor).ok_or_else(|| error("edited text has no provable source mapping"))?;
        if slot.attrs.get(&qualified("http://www.w3.org/XML/1998/namespace", "space")).is_none_or(|s| s != "preserve")
            && edit.replacement.trim() != edit.replacement
        {
            return Err(error("this text edit requires a whitespace attribute change that cannot be preserved safely"));
        }
        replacements.push(Splice { start: slot.content.start, end: slot.content.end, replacement: crate::xml::esc(&edit.replacement) });
    }
    for edit in format_edits {
        let paragraph =
            blocks.get(edit.index).and_then(|index| *index).ok_or_else(|| error("formatted text is inside a protected block structure"))?;
        let para = nodes.get(paragraph).ok_or_else(|| error("source paragraph index"))?;
        let slot = source_runs(&nodes, para)
            .and_then(|runs| runs.into_iter().find(|run| run.start == edit.start && run.end == edit.end))
            .ok_or_else(|| error("formatted text has no whole-run source mapping"))?;
        replacements.push(patch_run_property(bytes, &nodes, slot.run, edit.property, edit.value)?);
    }
    for edit in style_edits {
        let paragraph =
            blocks.get(edit.index).and_then(|index| *index).ok_or_else(|| error("styled paragraph is inside a protected block structure"))?;
        replacements.push(patch_paragraph_style(bytes, &nodes, paragraph, edit)?);
    }
    replacements.sort_by_key(|splice| (splice.start, splice.end));
    let mut output = Vec::new();
    let mut position = 0usize;
    for splice in replacements {
        if splice.start < position || splice.end < splice.start {
            return Err(error("overlapping or invalid source edits"));
        }
        output.extend_from_slice(bytes.get(position..splice.start).ok_or_else(|| error("invalid text splice"))?);
        output.extend_from_slice(splice.replacement.as_bytes());
        if output.len() > MAX_DATA {
            return Err(error("edited package exceeds preservation budget"));
        }
        position = splice.end;
    }
    output.extend_from_slice(bytes.get(position..).ok_or_else(|| error("invalid text splice tail"))?);
    Ok(output)
}
/// Resize one parsed chart frame only when the source contains exactly its native and fallback
/// DrawingML extents and both still match the imported model dimensions.
fn patch_graphic_extents(bytes: &[u8], edits: &[GraphicSizeEdit]) -> Result<Vec<u8>, DocxError> {
    if edits.is_empty() {
        return Ok(bytes.to_vec());
    }
    if edits.len() != 1 {
        return Err(error("resizing multiple graphics in a guarded DOCX is unsupported"));
    }
    let edit = edits[0];
    let nodes = xml::inspect(bytes)?;
    let extents: Vec<_> =
        nodes.iter().filter(|node| node.is("http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing", "extent")).collect();
    let cx = crate::units::emu(edit.old_w);
    let cy = crate::units::emu(edit.old_h);
    if extents.len() != 2
        || extents.iter().any(|node| node.attrs.get(&qualified("", "cx")) != Some(&cx) || node.attrs.get(&qualified("", "cy")) != Some(&cy))
    {
        return Err(error("chart frame extents do not match the imported native and fallback graphics"));
    }
    let source = std::str::from_utf8(bytes).map_err(|e| error(e.to_string()))?;
    let mut replacements = Vec::new();
    for extent in extents {
        let opening_end = source
            .get(extent.span.start..extent.span.end)
            .and_then(|part| part.find('>').map(|offset| extent.span.start + offset + 1))
            .ok_or_else(|| error("chart extent tag boundary"))?;
        let mut opening = source.get(extent.span.start..opening_end).ok_or_else(|| error("chart extent source range"))?.to_owned();
        for (axis, old, new) in [("cx", cx.as_str(), crate::units::emu(edit.new_w)), ("cy", cy.as_str(), crate::units::emu(edit.new_h))] {
            let needle = format!("{axis}=\"{old}\"");
            if opening.matches(&needle).count() != 1 {
                return Err(error("chart frame extent source mapping is ambiguous"));
            }
            opening = opening.replacen(&needle, &format!("{axis}=\"{new}\""), 1);
        }
        replacements.push((extent.span.start, opening_end, opening));
    }
    let mut output = Vec::new();
    let mut position = 0;
    for (start, end, replacement) in replacements {
        output.extend_from_slice(source.get(position..start).ok_or_else(|| error("chart extent splice range"))?.as_bytes());
        output.extend_from_slice(replacement.as_bytes());
        position = end;
    }
    output.extend_from_slice(source.get(position..).ok_or_else(|| error("chart extent splice tail"))?.as_bytes());
    Ok(output)
}
fn signed(pkg: &Package) -> bool {
    pkg.entries().any(|(path, bytes)| {
        path.to_ascii_lowercase().starts_with("_xmlsignatures/")
            || (path.ends_with(".rels") && bytes.windows(b"digital-signature".len()).any(|b| b == b"digital-signature"))
    })
}
pub(crate) fn write(doc: &Document, requested: Flavor, generated: &[u8]) -> Result<Option<Vec<u8>>, DocxError> {
    if !is_guarded(doc) {
        return Ok(None);
    }
    let original = get(doc, SOURCE, MAX_DATA)?;
    let pkg = open(original)?;
    validate(&pkg)?;
    let baseline = open(get(doc, BASE, MAX_DATA)?)?;
    validate(&baseline)?;
    let current = open(generated)?;
    validate(&current)?;
    let model = crate::read::read_model(original)?;
    if serde_json::to_vec(&model).map_err(|e| error(e.to_string()))? != get(doc, MODEL, MAX_MODEL)? {
        return Err(error("preservation baseline does not match the original document projection"));
    }
    let (edits, format_edits, style_edits, graphic_sizes) = edits(doc, &model)?;
    let main = pkg
        .rels_checked("")?
        .by_type(crate::package::rt::OFFICE_DOC)
        .filter(|r| !r.external)
        .map(|r| r.target.clone())
        .unwrap_or_else(|| "word/document.xml".into());
    let original_flavor = flavor(&pkg, &main)?;
    let modeled_rewrite = modeled_writer_rewrite(&pkg, &current, &main)?;
    // The native writer can remove personal metadata from the model projection, but a guarded
    // package may contain unsupported annotation-bearing parts that it does not know about.
    // Returning the original bytes here would silently defeat the user's privacy settings.
    if edits.is_empty()
        && format_edits.is_empty()
        && style_edits.is_empty()
        && graphic_sizes.is_empty()
        && !modeled_rewrite
        && (doc.settings.remove_personal_info || doc.settings.remove_date_time)
    {
        return Err(error(
            "privacy settings cannot be applied safely to a guarded DOCX with unsupported package content; original file was not overwritten",
        ));
    }
    if signed(&pkg)
        && (!edits.is_empty()
            || !format_edits.is_empty()
            || !style_edits.is_empty()
            || !graphic_sizes.is_empty()
            || requested != original_flavor
            || modeled_rewrite)
    {
        return Err(error("editing this package would invalidate its digital signature; original content was preserved"));
    }
    if original_flavor.macros() && !requested.macros() {
        return Err(error("converting a guarded macro-enabled package would remove active content; choose DOCM/DOTM"));
    }
    // Native output checks detect changes to media and custom XML/opaque graphs, which are
    // skipped by Document's JSON serializer. They must not be hidden by the source overlay.
    for (path, bytes) in baseline.entries() {
        if !["word/document.xml", "[Content_Types].xml", "docProps/app.xml"].contains(&path) && current.get(path) != Some(bytes) {
            return Err(error(format!("editing protected package part {path} is unsupported")));
        }
    }
    if current.entries().any(|(path, _)| baseline.get(path).is_none()) {
        return Err(error("adding package relationships or parts is unsupported in a guarded DOCX"));
    }
    if edits.is_empty() && format_edits.is_empty() && style_edits.is_empty() && graphic_sizes.is_empty() && requested == original_flavor {
        return Ok(Some(if modeled_rewrite { generated.to_vec() } else { original.to_vec() }));
    }
    let mut entries: Vec<_> = pkg.entries().map(|(path, bytes)| (path.to_string(), bytes.to_vec())).collect();
    let main_data = pkg.get(&main).ok_or_else(|| error("missing source story"))?;
    let patched = patch_graphic_extents(&patch_text(main_data, &edits, &format_edits, &style_edits, model.body.len())?, &graphic_sizes)?;
    let (_, bytes) = entries.iter_mut().find(|(path, _)| path.eq_ignore_ascii_case(&main)).ok_or_else(|| error("missing output source story"))?;
    *bytes = patched;
    if requested != original_flavor {
        let types = entries.iter_mut().find(|(path, _)| path == "[Content_Types].xml").ok_or_else(|| error("missing content types"))?;
        let data = xml::utf8(&types.1)?;
        if !matches!(data, std::borrow::Cow::Borrowed(_)) {
            return Err(error("changing preserved UTF-16 content types is unsupported"));
        }
        let data = std::str::from_utf8(&data).map_err(|e| error(e.to_string()))?;
        let old = original_flavor.main_content_type();
        if data.matches(old).count() != 1 {
            return Err(error("ambiguous flavor content type mapping"));
        }
        types.1 = data.replacen(old, requested.main_content_type(), 1).into_bytes();
    }
    let output = zip_entries(&entries)?;
    let output_pkg = open(&output)?;
    validate(&output_pkg)?;
    // Re-reading is avoided here: the text splice leaves every relationship ID, namespace,
    // extension attribute and non-story part byte unchanged.
    Ok(Some(output))
}
pub(crate) fn is_guarded(doc: &Document) -> bool {
    [SOURCE, BASE, MODEL].iter().any(|key| doc.passthrough.contains_key(*key))
}
pub(crate) fn warnings(doc: &Document) -> Vec<String> {
    if doc.passthrough.contains_key(SOURCE) {
        vec!["This DOCX contains package content WordCraft cannot fully reproduce. Original parts, namespaces, content controls and active content are preserved without execution. Only isolated main-body text edits within one mapped run, direct bold/italic formatting on whole mapped runs, and changes to existing paragraph style references outside protected structures can be saved; unsafe edits return an error. Unsupported content may not be displayed or printed. Other export formats do not preserve the original package.".into()]
    } else {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_state_error_names_changed_field_without_values() {
        let baseline = Document::new();
        let mut current = baseline.clone();
        current.settings.zoom = Some(wordcraft_doc::Zoom { percent: 173, fit: None });
        let error = match edits(&current, &baseline) {
            Ok(_) => panic!("changed protected setting must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("document.settings.zoom"));
        assert!(!error.to_string().contains("173"));
    }
}
