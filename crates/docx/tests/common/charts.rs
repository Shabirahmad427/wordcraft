//! Original synthetic fixtures from public OOXML syntax. No Word-produced assets.
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
use wordcraft_doc::para::NoteKind;
use wordcraft_doc::{Document, InlineObject, Paragraph, PartKind, StoryRef, para_block};
const OFFICE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
pub const CHART: &str = r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:future="urn:wordcraft:test:future"><c:lang val="en-US"/><c:chart><c:autoTitleDeleted val="1"/><c:plotArea><c:layout/><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:v>Scientific measurements</c:v></c:tx><c:cat><c:strLit><c:ptCount val="2"/><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strLit></c:cat><c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="2"/><c:pt idx="0"><c:v>42</c:v></c:pt><c:pt idx="1"><c:v>84</c:v></c:pt></c:numLit></c:val></c:ser><c:axId val="10"/><c:axId val="20"/></c:barChart><c:catAx><c:axId val="10"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="b"/><c:tickLblPos val="nextTo"/><c:crossAx val="20"/><c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/><c:lblOffset val="100"/></c:catAx><c:valAx><c:axId val="20"/><c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/><c:axPos val="l"/><c:numFmt formatCode="General" sourceLinked="1"/><c:tickLblPos val="nextTo"/><c:crossAx val="10"/><c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx></c:plotArea><c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart><c:externalData r:id="book"><c:autoUpdate val="0"/></c:externalData><c:extLst><c:ext uri="urn:wordcraft:test"><future:science value="αβ">Untouched future metadata</future:science></c:ext></c:extLst></c:chartSpace>"#;

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
    let mut files = BTreeMap::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        files.insert(entry.name().into(), bytes);
    }
    files
}
pub fn text(files: &BTreeMap<String, Vec<u8>>, name: &str) -> String {
    String::from_utf8(files.get(name).unwrap().clone()).unwrap()
}
pub fn replace(files: &mut BTreeMap<String, Vec<u8>>, name: &str, from: &str, to: &str) {
    files.insert(name.into(), text(files, name).replacen(from, to, 1).into_bytes());
}
pub fn drawing(id: &str, anchor: bool) -> String {
    let open = if anchor {
        r#"<wp:anchor distT="0" distB="0" distL="0" distR="0" simplePos="0" relativeHeight="0" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1"><wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset></wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset></wp:positionV>"#
    } else {
        "<wp:inline>"
    };
    let wrap = if anchor { "<wp:wrapNone/>" } else { "" };
    let tag = if anchor { "anchor" } else { "inline" };
    format!(
        r#"<w:r><w:drawing>{open}<wp:extent cx="3657600" cy="2438400"/>{wrap}<wp:docPr id="1" name="Scientific figure" descr="Original chart"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="{id}"/></a:graphicData></a:graphic></wp:{tag}></w:drawing></w:r>"#
    )
}

fn workbook() -> Vec<u8> {
    archive(&BTreeMap::from([
        ("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_vec()),
        ("_rels/.rels".into(), format!(r#"<Relationships xmlns="{REL_NS}"><Relationship Id="workbook" Type="{OFFICE}officeDocument" Target="xl/workbook.xml"/></Relationships>"#).into_bytes()),
        ("xl/workbook.xml".into(), br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Scientific data" sheetId="1" r:id="sheet"/></sheets></workbook>"#.to_vec()),
        ("xl/_rels/workbook.xml.rels".into(), format!(r#"<Relationships xmlns="{REL_NS}"><Relationship Id="sheet" Type="{OFFICE}worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#).into_bytes()),
        ("xl/worksheets/sheet1.xml".into(), br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Experiment</t></is></c><c r="B1"><v>42</v></c></row></sheetData></worksheet>"#.to_vec()),
    ]))
}
pub fn fixture(kind: &str) -> BTreeMap<String, Vec<u8>> {
    let mut doc = Document::from_text(&format!(
        "{kind} manuscript\n{}",
        (0..35).map(|i| format!("Research paragraph {i}: α β γ and independent synthetic scientific results.")).collect::<Vec<_>>().join("\n")
    ));
    doc.core.title = format!("Synthetic {kind}");
    doc.set_custom_prop("ZOTERO_PREF_1", r#"<data data-version="3"/>"#);
    let header = doc.add_part(PartKind::Header, vec![para_block(Paragraph::with_text("Research header", Default::default()))]);
    doc.last_section.headers.default = Some(header);
    let note = doc.add_part(PartKind::Footnote, vec![para_block(Paragraph::with_text("Original scientific footnote", Default::default()))]);
    doc.para_mut(StoryRef::Body, &wordcraft_doc::Path(vec![1]))
        .unwrap()
        .insert_object(0, InlineObject::NoteRef { kind: NoteKind::Footnote, id: note, custom: String::new() }, &Default::default())
        .unwrap();
    doc.para_mut(StoryRef::Body, &wordcraft_doc::Path(vec![2]))
        .unwrap()
        .insert_object(0, InlineObject::Equation { linear: "E=mc^2".into(), display: false, math: Default::default() }, &Default::default())
        .unwrap();
    let list = doc.numbering.add_list(wordcraft_doc::numbering::ListKind::Numbered);
    for (index, level) in [(3, 0), (4, 1)] {
        doc.para_mut(StoryRef::Body, &wordcraft_doc::Path(vec![index])).unwrap().props.numbering =
            Some(wordcraft_doc::props::NumRef { num: list, level });
    }
    let mut table = wordcraft_doc::table::Table::new(3, 2, 400.0);
    table.rows.get_mut(0).unwrap().props.header = true;
    table.rows.get_mut(0).unwrap().cells = vec![wordcraft_doc::table::Cell::with_text("Trial"), wordcraft_doc::table::Cell::with_text("Measurement")];
    table.rows.get_mut(1).unwrap().cells = vec![wordcraft_doc::table::Cell::with_text("A"), wordcraft_doc::table::Cell::with_text("42")];
    table.rows.get_mut(2).unwrap().cells = vec![wordcraft_doc::table::Cell::with_text("B"), wordcraft_doc::table::Cell::with_text("84")];
    doc.body.push(std::sync::Arc::new(wordcraft_doc::Block::Table(table)));
    let mut footer = Paragraph::with_text("Page ", Default::default());
    footer.insert_object(footer.len(), InlineObject::Field { instr: "PAGE".into(), result: "1".into(), locked: false }, &Default::default()).unwrap();
    doc.last_section.footers.default = Some(doc.add_part(PartKind::Footer, vec![para_block(footer)]));
    let mut files = unpack(&wordcraft_docx::write(&doc).unwrap());
    let drawing = drawing("originalChart", kind == "thesis");
    let field = r#"<w:bookmarkStart w:id="99" w:name="Figure1"/><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> ADDIN ZOTERO_ITEM CSL_CITATION {"citationID":"synthetic"} </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>(Rivera, 2024)</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:fldSimple w:instr=" REF Figure1 \h "><w:r><w:t>Figure 1</w:t></w:r></w:fldSimple><w:bookmarkEnd w:id="99"/>"#;
    replace(&mut files, "word/document.xml", "</w:p>", &format!("{drawing}{field}</w:p>"));
    if kind == "tracked report" {
        replace(
            &mut files,
            "word/document.xml",
            "</w:p>",
            r#"<w:ins w:id="42" w:author="Synthetic researcher" w:date="2026-01-01T00:00:00Z"><w:r><w:t>Tracked addition</w:t></w:r></w:ins></w:p>"#,
        );
    }
    replace(
        &mut files,
        "word/_rels/document.xml.rels",
        "</Relationships>",
        &format!(r#"<Relationship Id="originalChart" Type="{OFFICE}chart" Target="charts/chart1.xml"/></Relationships>"#),
    );
    files.insert("word/charts/chart1.xml".into(), CHART.as_bytes().to_vec());
    files.insert("word/charts/_rels/chart1.xml.rels".into(), format!(r#"<Relationships xmlns="{REL_NS}" xmlns:future="urn:wordcraft:test"><Relationship Id="book" Type="{OFFICE}package" Target="../embeddings/data.xlsx"/><Relationship Id="style" Type="urn:wordcraft:test/chartStyle" Target="style1.xml" future:keep="yes"/><Relationship Id="external" Type="{OFFICE}hyperlink" Target="https://example.invalid/data" TargetMode="External"/></Relationships>"#).into_bytes());
    files.insert("word/charts/style1.xml".into(), b"<style xmlns='urn:wordcraft:test'><color value='#123456'/></style>".to_vec());
    files.insert("word/embeddings/data.xlsx".into(), workbook());
    replace(
        &mut files,
        "[Content_Types].xml",
        "</Types>",
        r#"<Override PartName="/word/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/><Override PartName="/word/charts/style1.xml" ContentType="application/vnd.ms-office.chartstyle+xml"/><Override PartName="/word/embeddings/data.xlsx" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"/></Types>"#,
    );
    files
}
