//! Original synthetic OPC corpus; no active code is executed or proprietary assets used.
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
use wordcraft_doc::{Document, Path, StoryRef};
pub const OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
pub const REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub fn archive(files: &BTreeMap<String, Vec<u8>>) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
pub fn unpack(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut part = zip.by_index(index).unwrap();
            let mut bytes = Vec::new();
            part.read_to_end(&mut bytes).unwrap();
            (part.name().to_string(), bytes)
        })
        .collect()
}
pub fn replace(files: &mut BTreeMap<String, Vec<u8>>, path: &str, before: &str, after: &str) {
    let text = String::from_utf8(files.get(path).unwrap().clone()).unwrap();
    assert!(text.contains(before), "fixture replacement {path}: {before}");
    files.insert(path.into(), text.replacen(before, after, 1).into_bytes());
}
pub fn fixture() -> BTreeMap<String, Vec<u8>> {
    let mut document = Document::from_text("Editable introduction α & β\nSecond editable paragraph\nShared text");
    let list = document.numbering.add_list(wordcraft_doc::numbering::ListKind::Numbered);
    document.para_mut(StoryRef::Body, &Path::top(2)).unwrap().props.numbering = Some(wordcraft_doc::props::NumRef { num: list, level: 0 });
    let mut files = unpack(&wordcraft_docx::write(&document).unwrap());
    replace(
        &mut files,
        "word/document.xml",
        "<w:document ",
        "<w:document xmlns:future=\"urn:wordcraft:future\" xmlns:dgm=\"http://schemas.openxmlformats.org/drawingml/2006/diagram\" future:extension=\"keep\" ",
    );
    let unsupported = r#"<!--keep original comment--><w:sdt future:control="keep"><w:sdtPr><w:id w:val="91"/><w:alias w:val="Bound scientific title"/><w:tag w:val="manuscript-title"/><w:lock w:val="sdtContentLocked"/><w:dataBinding w:storeItemID="{11111111-1111-1111-1111-111111111111}" w:xpath="/science/title" w:prefixMappings="xmlns:s='urn:science'"/><future:controlMetadata mode="preserve"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t xml:space="preserve">Shared text</w:t></w:r></w:p></w:sdtContent></w:sdt><w:p><w:r><w:drawing><wp:inline><wp:extent cx="1270000" cy="635000"/><wp:docPr id="17" name="Original synthetic SmartArt"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/diagram"><dgm:relIds r:dm="diagramData" r:lo="diagramLayout" r:qs="diagramStyle" r:cs="diagramColors" future:retained="yes"/></a:graphicData></a:graphic></wp:inline></w:drawing><w:object><o:OLEObject Type="Embed" ProgID="Synthetic.Inert" r:id="oleData"/></w:object><w:control r:id="controlData" w:name="Inert synthetic control"/><future:extension r:id="unknownData"/></w:r></w:p>"#;
    replace(&mut files, "word/document.xml", "<w:sectPr>", &format!("{unsupported}<w:sectPr>"));
    let relationships = format!(
        r#"<Relationship Id="diagramData" Type="{OFFICE}diagramData" Target="diagrams/data1.xml"/><Relationship Id="diagramLayout" Type="{OFFICE}diagramLayout" Target="diagrams/layout1.xml"/><Relationship Id="diagramStyle" Type="{OFFICE}diagramQuickStyle" Target="diagrams/style1.xml"/><Relationship Id="diagramColors" Type="{OFFICE}diagramColors" Target="diagrams/colors1.xml"/><Relationship Id="oleData" Type="{OFFICE}oleObject" Target="embeddings/object1.bin"/><Relationship Id="controlData" Type="{OFFICE}control" Target="activeX/activeX1.xml"/><Relationship Id="customScience" Type="{OFFICE}customXml" Target="../customXml/item1.xml"/><Relationship Id="unknownData" Type="urn:wordcraft:future:relationship" Target="../vendor/data.bin" future:extension="keep"/><Relationship Id="externalActive" Type="urn:wordcraft:future:active" Target="https://example.invalid/never-execute" TargetMode="External"/>"#
    );
    replace(&mut files, "word/_rels/document.xml.rels", "<Relationships ", "<Relationships xmlns:future=\"urn:wordcraft:future\" ");
    replace(&mut files, "word/_rels/document.xml.rels", "</Relationships>", &format!("{relationships}</Relationships>"));
    for (path,bytes) in [
        ("word/diagrams/data1.xml",br#"<dgm:dataModel xmlns:dgm="http://schemas.openxmlformats.org/drawingml/2006/diagram" xmlns:future="urn:wordcraft:future"><dgm:ptLst/><dgm:cxnLst/><future:metadata keep="exact"/></dgm:dataModel>"#.as_slice()),
        ("word/diagrams/layout1.xml",br#"<layout xmlns="urn:wordcraft:synthetic:diagram"/>"#),
        ("word/diagrams/style1.xml",br#"<style xmlns="urn:wordcraft:synthetic:diagram"/>"#),
        ("word/diagrams/colors1.xml",br#"<colors xmlns="urn:wordcraft:synthetic:diagram"/>"#),
        ("word/embeddings/object1.bin",b"Original inert OLE payload; this is not executable code."),
        ("word/activeX/activeX1.xml",br#"<ax:ocx xmlns:ax="http://schemas.microsoft.com/office/2006/activeX" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" r:id="binaryControl" ax:persistence="persistStorage"><ax:ocxPr ax:name="Original metadata" ax:value="preserve"/></ax:ocx>"#),
        ("word/activeX/activeX1.bin",b"Inert original ActiveX-related test bytes."),
        ("customXml/item1.xml",br#"<science xmlns:s="urn:science"><title>Shared text</title></science>"#),
        ("customXml/itemProps1.xml",br#"<ds:datastoreItem xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml" ds:itemID="{11111111-1111-1111-1111-111111111111}"><ds:schemaRefs/></ds:datastoreItem>"#),
        ("vendor/data.bin",b"Unmodeled vendor payload"),
        ("vendor/nested.bin",b"Nested opaque payload"),
        ("vendor/orphan.bin",b"Unreferenced package data is also retained"),
    ] { files.insert(path.into(),bytes.to_vec()); }
    for (path,relationships) in [
        ("word/activeX/_rels/activeX1.xml.rels",format!(r#"<Relationship Id="binaryControl" Type="{OFFICE}activeXControlBinary" Target="activeX1.bin"/>"#)),
        ("customXml/_rels/item1.xml.rels",format!(r#"<Relationship Id="properties" Type="{OFFICE}customXmlProps" Target="itemProps1.xml"/>"#)),
        ("vendor/_rels/data.bin.rels",r#"<Relationship Id="nested" Type="urn:wordcraft:synthetic:nested" Target="nested.bin"/>"#.into()),
        ("vendor/_rels/nested.bin.rels",r#"<Relationship Id="cycle" Type="urn:wordcraft:synthetic:cycle" Target="data.bin"/><Relationship Id="external" Type="urn:wordcraft:synthetic:external" Target="https://example.invalid/never-fetch" TargetMode="External"/>"#.into()),
    ] { files.insert(path.into(),format!(r#"<Relationships xmlns="{REL}">{relationships}</Relationships>"#).into_bytes()); }
    replace(&mut files, "[Content_Types].xml", "<Types ", "<Types xmlns:future=\"urn:wordcraft:future\" future:metadata=\"preserve\" ");
    let overrides = r#"<Default Extension="bin" ContentType="application/octet-stream"/><Override PartName="/word/diagrams/data1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.diagramData+xml"/><Override PartName="/word/diagrams/layout1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.diagramLayout+xml"/><Override PartName="/word/diagrams/style1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.diagramStyle+xml"/><Override PartName="/word/diagrams/colors1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.diagramColors+xml"/><Override PartName="/word/activeX/activeX1.xml" ContentType="application/vnd.ms-office.activeX+xml"/><Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/>"#;
    replace(&mut files, "[Content_Types].xml", "</Types>", &format!("{overrides}</Types>"));
    for (path, end, extension) in [
        (
            "word/styles.xml",
            "</w:styles>",
            r#"<w:style w:type="paragraph" w:styleId="FutureStyle" xmlns:future="urn:wordcraft:future" future:style="exact"><w:name w:val="Future academic style"/><w:pPr><w:contextualSpacing/><future:unknownStyleProperty keep="yes"/></w:pPr></w:style>"#,
        ),
        ("word/numbering.xml", "</w:numbering>", r#"<future:numberingProperty xmlns:future="urn:wordcraft:future" keep="yes"/>"#),
        ("word/settings.xml", "</w:settings>", r#"<future:setting xmlns:future="urn:wordcraft:future" value="keep"/>"#),
        ("docProps/core.xml", "</cp:coreProperties>", r#"<future:metadata xmlns:future="urn:wordcraft:future" value="keep"/>"#),
    ] {
        replace(&mut files, path, end, &format!("{extension}{end}"));
    }
    // A numbering part not recognized by the imported body is still retained exactly.
    files.insert("word/numbering-extra.xml".into(),br#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:future="urn:wordcraft:future"><w:abstractNum w:abstractNumId="999" future:extension="keep"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/><future:counter value="keep"/></w:lvl></w:abstractNum></w:numbering>"#.to_vec());
    files
}
