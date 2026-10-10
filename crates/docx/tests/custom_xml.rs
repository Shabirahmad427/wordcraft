//! Synthetic OPC fixtures, authored from ECMA-376; no files produced by Word.
use std::io::{Cursor, Read, Write};
use wordcraft_doc::StoryRef;

const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const BIB: &str = r#"<?xml version="1.0"?><b:Sources xmlns:b="http://schemas.openxmlformats.org/officeDocument/2006/bibliography"><b:Source><b:Tag>Rivera2024</b:Tag><b:SourceType>Book</b:SourceType><b:Title>Open Spaces 文献</b:Title><b:Year>2024</b:Year></b:Source></b:Sources>"#;
const PROPS: &str = r#"<ds:datastoreItem xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml" ds:itemID="{3C4242A1-2222-4444-8888-ABCDEF012345}"><ds:schemaRefs><ds:schemaRef ds:uri="http://schemas.openxmlformats.org/officeDocument/2006/bibliography"/></ds:schemaRefs></ds:datastoreItem>"#;

fn archive(entries: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        z.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}
fn entry(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    z.by_name(name).unwrap().read_to_end(&mut out).unwrap();
    out
}
fn fixture(root_target: &str, extra_rels: &str) -> Vec<u8> {
    let root = format!(
        r#"<Relationships xmlns="{REL_NS}"><Relationship Id="office" Type="{OFFICE}officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    let document = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Original body</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#;
    let rels = format!(
        r#"<Relationships xmlns="{REL_NS}"><Relationship Id="bibliography" Type="{OFFICE}customXml" Target="{root_target}"/></Relationships>"#
    );
    let item_rels = format!(
        r#"<Relationships xmlns="{REL_NS}"><Relationship Id="properties" Type="{OFFICE}customXmlProps" Target="itemProps1.xml"/>{extra_rels}</Relationships>"#
    );
    let types = r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="xml" ContentType="application/xml"/><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/></Types>"#;
    archive(&[
        ("_rels/.rels".into(), root.into_bytes()),
        ("word/document.xml".into(), document.as_bytes().to_vec()),
        ("word/_rels/document.xml.rels".into(), rels.into_bytes()),
        ("[Content_Types].xml".into(), types.as_bytes().to_vec()),
        ("customXml/item1.xml".into(), BIB.as_bytes().to_vec()),
        ("customXml/itemProps1.xml".into(), PROPS.as_bytes().to_vec()),
        ("customXml/_rels/item1.xml.rels".into(), item_rels.into_bytes()),
    ])
}
#[test]
fn bibliography_parts_survive_edits_and_repeated_round_trips() {
    let input = fixture("../customXml/item1.xml", "");
    let mut doc = wordcraft_docx::read(&input).unwrap();
    let path = wordcraft_doc::Pos::body(0, 0);
    doc.insert_text(&path, "Edited: ", &Default::default()).unwrap();
    for flavor in [wordcraft_docx::Flavor::Document, wordcraft_docx::Flavor::Template, wordcraft_docx::Flavor::MacroDocument] {
        let mut current = doc.clone();
        for _ in 0..3 {
            let out = wordcraft_docx::write_as(&current, flavor).unwrap();
            for name in ["customXml/item1.xml", "customXml/itemProps1.xml", "customXml/_rels/item1.xml.rels"] {
                assert_eq!(entry(&out, name), entry(&input, name), "changed {name}");
            }
            let rels = String::from_utf8(entry(&out, "word/_rels/document.xml.rels")).unwrap();
            assert!(rels.contains("/customXml/item1.xml") && rels.contains("/relationships/customXml"));
            let types = String::from_utf8(entry(&out, "[Content_Types].xml")).unwrap();
            assert!(types.contains("/customXml/itemProps1.xml") && types.contains("customXmlProperties+xml"));
            current = wordcraft_docx::read(&out).unwrap();
            assert!(current.plain_text(StoryRef::Body).starts_with("Edited: Original body"));
        }
    }
}

fn replace_entry(input: &[u8], name: &str, value: Vec<u8>) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(Cursor::new(input)).unwrap();
    let mut entries = Vec::new();
    for i in 0..z.len() {
        let mut file = z.by_index(i).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        entries.push((file.name().to_string(), if file.name() == name { value.clone() } else { bytes }));
    }
    if !entries.iter().any(|(n, _)| n == name) {
        entries.push((name.into(), value));
    }
    archive(&entries)
}

#[test]
fn shared_roots_cycles_and_external_relationships_are_preserved_without_fetching() {
    let extra = r#"<Relationship Id="cycle" Type="urn:original:related" Target="item1.xml"/><Relationship Id="external" Type="urn:original:related" Target="https://example.invalid/never-fetch" TargetMode="External"/>"#;
    let input = fixture("/customXml/item1.xml", extra);
    let roots = format!(
        r#"<Relationships xmlns="{REL_NS}"><Relationship Id="one" Type="{OFFICE}customXml" Target="../customXml/item1.xml"/><Relationship Id="two" Type="{OFFICE}customXml" Target="../customXml/item2.xml"/></Relationships>"#
    );
    let input = replace_entry(&input, "word/_rels/document.xml.rels", roots.into_bytes());
    let input = replace_entry(&input, "customXml/item2.xml", b"<original xmlns='urn:original'>second</original>".to_vec());
    let props_rel = format!(
        r#"<Relationships xmlns="{REL_NS}"><Relationship Id="shared" Type="{OFFICE}customXmlProps" Target="itemProps1.xml"/></Relationships>"#
    );
    let input = replace_entry(&input, "customXml/_rels/item2.xml.rels", props_rel.into_bytes());
    let doc = wordcraft_docx::read(&input).unwrap();
    let out = wordcraft_docx::write(&doc).unwrap();
    for name in
        ["customXml/item1.xml", "customXml/item2.xml", "customXml/itemProps1.xml", "customXml/_rels/item1.xml.rels", "customXml/_rels/item2.xml.rels"]
    {
        assert_eq!(entry(&out, name), entry(&input, name));
    }
    let mut z = zip::ZipArchive::new(Cursor::new(&out)).unwrap();
    let names: Vec<_> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_ascii_lowercase()).collect();
    assert_eq!(names.len(), names.iter().collect::<std::collections::BTreeSet<_>>().len());
}

#[test]
fn strict_relationships_and_nonstandard_part_locations_survive() {
    let input = fixture("../customXml/item1.xml", "");
    let roots = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="strict" Type="http://purl.oclc.org/ooxml/officeDocument/relationships/customXml" Target="../academic/data.xml"/></Relationships>"#;
    let input = replace_entry(&input, "word/_rels/document.xml.rels", roots.as_bytes().to_vec());
    let input = replace_entry(&input, "academic/data.xml", BIB.as_bytes().to_vec());
    let doc = wordcraft_docx::read(&input).unwrap();
    let out = wordcraft_docx::write(&doc).unwrap();
    assert_eq!(entry(&out, "academic/data.xml"), BIB.as_bytes());
    let roots = String::from_utf8(entry(&out, "word/_rels/document.xml.rels")).unwrap();
    assert!(roots.contains("http://purl.oclc.org/ooxml/officeDocument/relationships/customXml"));
    assert!(wordcraft_docx::read(&out).is_ok());
}

#[test]
fn missing_reserved_and_malformed_relationship_parts_are_not_silently_dropped() {
    for target in ["../customXml/missing.xml", "document.xml", "../docProps/core.xml", "../customXml/_rels/item1.xml.rels"] {
        assert!(wordcraft_docx::read(&fixture(target, "")).is_err(), "{target}");
    }
    let input = fixture("../customXml/item1.xml", "");
    let bad = replace_entry(&input, "customXml/_rels/item1.xml.rels", b"<broken".to_vec());
    assert!(wordcraft_docx::read(&bad).is_err());
    let external = format!(
        r#"<Relationships xmlns="{REL_NS}"><Relationship Id="remote" Type="{OFFICE}customXml" Target="https://example.invalid/item.xml" TargetMode="External"/></Relationships>"#
    );
    let bad = replace_entry(&input, "word/_rels/document.xml.rels", external.into_bytes());
    assert!(wordcraft_docx::read(&bad).is_err());
}
