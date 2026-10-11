//! The same synthetic chart corpus exercises editing, failure isolation and durable recovery.
#[path = "../../docx/tests/common/charts.rs"]
mod fixtures;
use fixtures::{archive, fixture, unpack};
use serde_json::json;
use std::collections::BTreeMap;
use wordcraft_engine::recovery::{Snapshot, Store, unique_id};
use wordcraft_engine::{
    Session,
    doc::{Document, InlineObject, StoryRef},
};

fn editable_chart_fixture() -> Vec<u8> {
    editable_chart_fixture_with_direction("col")
}

fn editable_chart_fixture_with_direction(direction: &str) -> Vec<u8> {
    let mut files = fixture("scientific");
    let chart = format!(
        r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><c:chart><c:plotArea><c:barChart><c:barDir val="{direction}"/><c:ser><c:idx val="0"/><c:order val="0"/><c:tx><c:strRef><c:f>'Scientific data'!$A$1</c:f><c:strCache><c:ptCount val="1"/><c:pt idx="0"><c:v>Measurements</c:v></c:pt></c:strCache></c:strRef></c:tx><c:cat><c:strRef><c:f>'Scientific data'!$A$2:$A$3</c:f><c:strCache><c:ptCount val="2"/><c:pt idx="0"><c:v>A</c:v></c:pt><c:pt idx="1"><c:v>B</c:v></c:pt></c:strCache></c:strRef></c:cat><c:val><c:numRef><c:f>'Scientific data'!$B$2:$B$3</c:f><c:numCache><c:formatCode>General</c:formatCode><c:ptCount val="2"/><c:pt idx="0"><c:v>42</c:v></c:pt><c:pt idx="1"><c:v>84</c:v></c:pt></c:numCache></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart><c:externalData r:id="book"/></c:chartSpace>"#
    );
    files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
    files.insert(
        "word/charts/_rels/chart1.xml.rels".into(),
        br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="book" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/package" Target="../embeddings/data.xlsx"/></Relationships>"#.to_vec(),
    );
    let book = BTreeMap::from([
        ("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#.to_vec()),
        ("_rels/.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="book" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec()),
        ("xl/workbook.xml".into(), br#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Scientific data" sheetId="1" r:id="sheet"/></sheets></workbook>"#.to_vec()),
        ("xl/_rels/workbook.xml.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="sheet" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#.to_vec()),
        ("xl/worksheets/sheet1.xml".into(), br#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Measurements</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>A</t></is></c><c r="B2"><v>42</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>B</t></is></c><c r="B3"><v>84</v></c></row></sheetData></worksheet>"#.to_vec()),
    ]);
    files.insert("word/embeddings/data.xlsx".into(), archive(&book));
    archive(&files)
}

fn editable_line_chart_fixture() -> Vec<u8> {
    let mut files = unpack(&editable_chart_fixture());
    let chart = String::from_utf8(files["word/charts/chart1.xml"].clone())
        .unwrap()
        .replace("<c:barChart>", "<c:lineChart>")
        .replace("</c:barChart>", "</c:lineChart>")
        .replace("<c:barDir val=\"col\"/>", "");
    files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
    archive(&files)
}

fn editable_area_chart_fixture() -> Vec<u8> {
    let mut files = unpack(&editable_chart_fixture());
    let chart = String::from_utf8(files["word/charts/chart1.xml"].clone())
        .unwrap()
        .replace("<c:barChart>", "<c:areaChart>")
        .replace("</c:barChart>", "</c:areaChart>")
        .replace("<c:barDir val=\"col\"/>", "");
    files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
    archive(&files)
}

fn editable_slice_chart_fixture(chart_type: &str) -> Vec<u8> {
    let local = match chart_type {
        "pie" => "pieChart",
        "doughnut" => "doughnutChart",
        _ => "unsupportedChart",
    };
    let mut files = unpack(&editable_chart_fixture());
    let chart = String::from_utf8(files["word/charts/chart1.xml"].clone())
        .unwrap()
        .replace("<c:barChart>", &format!("<c:{local}>"))
        .replace("</c:barChart>", &format!("</c:{local}>"))
        .replace("<c:barDir val=\"col\"/>", "");
    files.insert("word/charts/chart1.xml".into(), chart.into_bytes());
    archive(&files)
}
fn has_chart(doc: &Document) -> bool {
    doc.body
        .iter()
        .filter_map(|block| block.as_para())
        .flat_map(|para| &para.objects)
        .any(|object| matches!(object, InlineObject::Graphic { graphic, .. } if graphic.kind == wordcraft_engine::doc::graphic::GraphicKind::Chart))
}
#[test]
fn chart_save_reopen_undo_and_recovery_preserve_all_chart_bytes() {
    let directory = std::env::temp_dir().join(format!("wordcraft-chart-engine-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("source.docx");
    let destination = directory.join("edited.docx");
    let original = archive(&fixture("scientific"));
    std::fs::write(&source, &original).unwrap();
    let mut session = Session::new(Document::new());
    let opened = session.run("file.open", &json!({"path": source})).unwrap();
    assert!(!opened["compatibilityWarnings"].as_array().unwrap().is_empty());
    assert!(has_chart(&session.doc));
    session.run("text.insert", &json!({"text": "Edited science: "})).unwrap();
    let store = Store::new(directory.join("recovery")).unwrap();
    let id = store.save(&unique_id(), session.document_id(), &Snapshot::capture(&session)).unwrap();
    let mut recovered = Session::new(Document::new());
    store.load(&id).unwrap().restore(&mut recovered);
    assert_eq!(recovered.doc.passthrough, session.doc.passthrough);
    assert_eq!(*recovered.source_package.as_ref().unwrap().bytes, original);
    let saved = recovered.run("file.save", &json!({"path": destination})).unwrap();
    assert!(!saved["compatibilityWarnings"].as_array().unwrap().is_empty());
    let output = unpack(&std::fs::read(&destination).unwrap());
    let input = unpack(&original);
    for path in ["word/charts/chart1.xml", "word/charts/_rels/chart1.xml.rels", "word/charts/style1.xml", "word/embeddings/data.xlsx"] {
        assert_eq!(output.get(path), input.get(path));
    }
    assert_eq!(std::fs::read(&source).unwrap(), original);
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path": destination})).unwrap();
    assert!(has_chart(&reopened.doc));
    assert_eq!(reopened.doc.plain_text(StoryRef::Body), recovered.doc.plain_text(StoryRef::Body));
    let a = recovered.layout();
    let b = reopened.layout();
    assert!(a.pages.len() > 1);
    assert_eq!(a.pages.len(), b.pages.len());
    for (a, b) in a.pages.iter().zip(&b.pages) {
        let options = wordcraft_render::RenderOptions::default();
        assert_eq!(
            wordcraft_render::render_page(&recovered.doc, a, 1.0, &options).to_straight(),
            wordcraft_render::render_page(&reopened.doc, b, 1.0, &options).to_straight()
        );
    }
    session.run("edit.undo", &json!({})).unwrap();
    assert!(!session.doc.plain_text(StoryRef::Body).contains("Edited science: "));
    assert!(has_chart(&session.doc));
    assert_eq!(session.doc.passthrough, recovered.doc.passthrough);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn engine_chart_edit_is_undoable_recoverable_rendered_and_saved_as_one_package_change() {
    let directory = std::env::temp_dir().join(format!("wordcraft-chart-edit-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source_path = directory.join("source.docx");
    let saved_path = directory.join("edited.docx");
    let original = editable_chart_fixture();
    std::fs::write(&source_path, &original).unwrap();

    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": source_path})).unwrap();
    let document_id = session.run("file.info", &json!({})).unwrap()["documentId"].as_u64().unwrap();
    let before_inspection = serde_json::to_value(&session.doc).unwrap();
    let inspected = session.run("chart.inspectData", &json!({"documentId": document_id, "chartPart": "word/charts/chart1.xml"})).unwrap();
    assert_eq!(inspected["chartKind"], "column");
    assert_eq!(inspected["series"][0]["name"], "Measurements");
    assert_eq!(inspected["series"][0]["categories"], json!(["A", "B"]));
    assert_eq!(inspected["series"][0]["values"], json!([42.0, 84.0]));
    assert!(!session.dirty, "read-only inspection does not dirty the document");
    assert_eq!(serde_json::to_value(&session.doc).unwrap(), before_inspection, "inspection has no document side effects");
    let original_source = session.source_package.as_ref().unwrap().bytes.clone();
    session.run("text.insert", &json!({"text": "Unsaved manuscript edit: "})).unwrap();
    let original_doc = session.doc.clone();
    let transaction_input = wordcraft_docx::write(&session.doc).unwrap();
    let before_layout = session.layout();
    let before_raster = wordcraft_render::render_page(&session.doc, &before_layout.pages[0], 1.0, &Default::default()).to_straight();

    let result = session
        .run(
            "chart.editData",
            &json!({
                "documentId": document_id,
                "chartPart": "word/charts/chart1.xml",
                "series": [{"name": "Updated measurements", "categories": ["Trial X", "Trial Y"], "values": [120.0, 250.0]}]
            }),
        )
        .unwrap();
    assert_eq!(result["updated"], true);
    assert!(session.dirty);
    let first_edit_bytes = std::sync::Arc::new(wordcraft_docx::write(&session.doc).unwrap());
    assert_ne!(*first_edit_bytes, *original_source);
    let initial_parts = unpack(&transaction_input);
    let first_parts = unpack(&first_edit_bytes);
    for (part, bytes) in &initial_parts {
        if !matches!(part.as_str(), "word/charts/chart1.xml" | "word/embeddings/data.xlsx") {
            assert_eq!(first_parts.get(part), Some(bytes), "unrelated package part changed: {part}");
        }
    }
    let edited = unpack(&first_edit_bytes);
    let chart = String::from_utf8(edited.get("word/charts/chart1.xml").unwrap().clone()).unwrap();
    assert!(chart.contains("Updated measurements"));
    assert!(chart.contains("Trial X"));
    assert!(chart.contains(">120</c:v>"));
    let xlsx = unpack(edited.get("word/embeddings/data.xlsx").unwrap());
    let sheet = String::from_utf8(xlsx.get("xl/worksheets/sheet1.xml").unwrap().clone()).unwrap();
    assert!(sheet.contains("Updated measurements"));
    assert!(sheet.contains("Trial X"));
    assert!(sheet.contains(">120</v>"));

    let after_first_edit = serde_json::to_value(&session.doc).unwrap();
    let stale = session.run(
        "chart.editData",
        &json!({
            "documentId": document_id,
            "chartPart": "word/charts/chart1.xml",
            "expected": {"chartKind": "column", "series": [{"name": "Measurements", "categories": ["A", "B"], "values": [42.0, 84.0]}]},
            "series": [{"name": "Stale draft"}]
        }),
    );
    assert!(stale.is_err(), "a dialog snapshot cannot overwrite newer chart data");
    assert_eq!(serde_json::to_value(&session.doc).unwrap(), after_first_edit, "stale edits leave document and history state untouched");

    // A second edit to the same chart uses the newly committed package/mapping.
    session
        .run("chart.editData", &json!({"documentId": document_id, "chartPart": "word/charts/chart1.xml", "series": [{"name": "Final measurements"}]}))
        .unwrap();
    let edited_bytes = std::sync::Arc::new(wordcraft_docx::write(&session.doc).unwrap());
    let edited = unpack(&edited_bytes);
    let chart = String::from_utf8(edited.get("word/charts/chart1.xml").unwrap().clone()).unwrap();
    assert!(chart.contains("Final measurements"));
    assert!(chart.contains(">120</c:v>"));
    let xlsx = unpack(edited.get("word/embeddings/data.xlsx").unwrap());
    let sheet = String::from_utf8(xlsx.get("xl/worksheets/sheet1.xml").unwrap().clone()).unwrap();
    assert!(sheet.contains("Final measurements"));
    assert!(sheet.contains(">120</v>"));

    let after_layout = session.layout();
    let after_raster = wordcraft_render::render_page(&session.doc, &after_layout.pages[0], 1.0, &Default::default()).to_straight();
    assert_ne!(before_raster, after_raster, "updated chart caches should refresh chart rendering");

    let recovery_store = Store::new(directory.join("recovery")).unwrap();
    let recovery_id = recovery_store.save(&unique_id(), document_id, &Snapshot::capture(&session)).unwrap();
    let mut recovered = Session::new(Document::new());
    recovery_store.load(&recovery_id).unwrap().restore(&mut recovered);
    assert_eq!(*recovered.source_package.as_ref().unwrap().bytes, original);
    assert_eq!(recovered.doc.passthrough, session.doc.passthrough);
    let recovered_path = directory.join("recovered.docx");
    recovered.run("file.save", &json!({"path": recovered_path})).unwrap();
    wordcraft_docx::prepare_chart_edit(&std::fs::read(recovered_path).unwrap(), "word/charts/chart1.xml").unwrap();

    session.run("edit.undo", &json!({})).unwrap();
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc, original_doc);
    assert_eq!(*session.source_package.as_ref().unwrap().bytes, *original_source);
    session.run("edit.redo", &json!({})).unwrap();
    session.run("edit.redo", &json!({})).unwrap();
    assert_eq!(wordcraft_docx::write(&session.doc).unwrap(), *edited_bytes);

    session.run("file.save", &json!({"path": saved_path})).unwrap();
    let saved_package = unpack(&std::fs::read(&saved_path).unwrap());
    assert_eq!(String::from_utf8(saved_package.get("word/charts/chart1.xml").unwrap().clone()).unwrap(), chart);
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path": saved_path})).unwrap();
    let mapping = wordcraft_docx::prepare_chart_edit(&std::fs::read(saved_path).unwrap(), "word/charts/chart1.xml").unwrap();
    drop(mapping);
    assert!(has_chart(&reopened.doc));
    let reopen_layout = reopened.layout();
    let reopen_raster = wordcraft_render::render_page(&reopened.doc, &reopen_layout.pages[0], 1.0, &Default::default()).to_straight();
    assert_eq!(after_raster, reopen_raster);
    assert_eq!(std::fs::read(source_path).unwrap(), original, "the original DOCX is never overwritten");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn horizontal_bar_chart_edit_survives_render_undo_recovery_and_docx_reopen() {
    let directory = std::env::temp_dir().join(format!("wordcraft-horizontal-chart-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source_path = directory.join("horizontal.docx");
    let saved_path = directory.join("horizontal-edited.docx");
    let original = editable_chart_fixture_with_direction("bar");
    std::fs::write(&source_path, &original).unwrap();

    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": source_path})).unwrap();
    let document_id = session.document_id();
    let before = session.layout();
    let before_raster = wordcraft_render::render_page(&session.doc, &before.pages[0], 1.0, &Default::default()).to_straight();
    let inspected = session.run("chart.inspectData", &json!({"documentId": document_id, "chartPart": "word/charts/chart1.xml"})).unwrap();
    assert_eq!(inspected["chartKind"], "bar");

    session
        .run(
            "chart.editData",
            &json!({
                "documentId": document_id,
                "chartPart": "word/charts/chart1.xml",
                "series": [{"name": "Horizontal measurements", "categories": ["North", "South"], "values": [120.0, 250.0]}]
            }),
        )
        .unwrap();
    let edited_package = wordcraft_docx::write(&session.doc).unwrap();
    let edited_parts = unpack(&edited_package);
    let source_parts = unpack(&original);
    for (part, bytes) in &source_parts {
        if !matches!(part.as_str(), "word/charts/chart1.xml" | "word/embeddings/data.xlsx") {
            assert_eq!(edited_parts.get(part), Some(bytes), "unrelated DOCX part changed: {part}");
        }
    }
    let chart_xml = String::from_utf8(edited_parts["word/charts/chart1.xml"].clone()).unwrap();
    assert!(chart_xml.contains("<c:barDir val=\"bar\"/>"));
    assert!(chart_xml.contains("Horizontal measurements"));
    let xlsx = unpack(&edited_parts["word/embeddings/data.xlsx"]);
    let sheet = String::from_utf8(xlsx["xl/worksheets/sheet1.xml"].clone()).unwrap();
    assert!(sheet.contains("Horizontal measurements"));
    assert!(sheet.contains("North"));
    assert!(sheet.contains(">120</v>"));

    let after = session.layout();
    let after_raster = wordcraft_render::render_page(&session.doc, &after.pages[0], 1.0, &Default::default()).to_straight();
    assert_ne!(before_raster, after_raster, "cache edits refresh the horizontal chart rendering");
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(
        wordcraft_docx::inspect_chart_data(&wordcraft_docx::write(&session.doc).unwrap(), "word/charts/chart1.xml").unwrap().series[0].values,
        [42.0, 84.0]
    );
    session.run("edit.redo", &json!({})).unwrap();
    assert_eq!(wordcraft_docx::write(&session.doc).unwrap(), edited_package);

    let recovery_store = Store::new(directory.join("recovery")).unwrap();
    let recovery_id = recovery_store.save(&unique_id(), document_id, &Snapshot::capture(&session)).unwrap();
    let mut recovered = Session::new(Document::new());
    recovery_store.load(&recovery_id).unwrap().restore(&mut recovered);
    assert_eq!(wordcraft_docx::write(&recovered.doc).unwrap(), edited_package);
    recovered.run("file.save", &json!({"path": saved_path})).unwrap();
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path": saved_path})).unwrap();
    let reopened_package = std::fs::read(directory.join("horizontal-edited.docx")).unwrap();
    let snapshot = wordcraft_docx::inspect_chart_data(&reopened_package, "word/charts/chart1.xml").unwrap();
    assert_eq!(snapshot.chart_kind, "bar");
    assert_eq!(snapshot.series[0].categories, ["North", "South"]);
    assert_eq!(snapshot.series[0].values, [120.0, 250.0]);
    assert_eq!(std::fs::read(source_path).unwrap(), original, "source DOCX remains unchanged");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn line_chart_edit_survives_render_undo_recovery_and_docx_reopen() {
    let directory = std::env::temp_dir().join(format!("wordcraft-line-chart-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source_path = directory.join("line.docx");
    let saved_path = directory.join("line-edited.docx");
    let original = editable_line_chart_fixture();
    std::fs::write(&source_path, &original).unwrap();

    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": source_path})).unwrap();
    let document_id = session.document_id();
    let before = session.layout();
    let before_raster = wordcraft_render::render_page(&session.doc, &before.pages[0], 1.0, &Default::default()).to_straight();
    let inspected = session.run("chart.inspectData", &json!({"documentId": document_id, "chartPart": "word/charts/chart1.xml"})).unwrap();
    assert_eq!(inspected["chartKind"], "line");

    session
        .run(
            "chart.editData",
            &json!({
                "documentId": document_id,
                "chartPart": "word/charts/chart1.xml",
                "series": [{"name": "Line measurements", "categories": ["North", "South"], "values": [130.0, 260.0]}]
            }),
        )
        .unwrap();
    let edited_package = wordcraft_docx::write(&session.doc).unwrap();
    let edited_parts = unpack(&edited_package);
    let source_parts = unpack(&original);
    for (part, bytes) in &source_parts {
        if !matches!(part.as_str(), "word/charts/chart1.xml" | "word/embeddings/data.xlsx") {
            assert_eq!(edited_parts.get(part), Some(bytes), "unrelated DOCX part changed: {part}");
        }
    }
    let chart_xml = String::from_utf8(edited_parts["word/charts/chart1.xml"].clone()).unwrap();
    assert!(chart_xml.contains("<c:lineChart>"));
    assert!(chart_xml.contains("Line measurements"));
    let xlsx = unpack(&edited_parts["word/embeddings/data.xlsx"]);
    let sheet = String::from_utf8(xlsx["xl/worksheets/sheet1.xml"].clone()).unwrap();
    assert!(sheet.contains("North"));
    assert!(sheet.contains(">130</v>"));

    let after = session.layout();
    let after_raster = wordcraft_render::render_page(&session.doc, &after.pages[0], 1.0, &Default::default()).to_straight();
    assert_ne!(before_raster, after_raster, "cache edits refresh line chart rendering");
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(
        wordcraft_docx::inspect_chart_data(&wordcraft_docx::write(&session.doc).unwrap(), "word/charts/chart1.xml").unwrap().series[0].values,
        [42.0, 84.0]
    );
    session.run("edit.redo", &json!({})).unwrap();
    assert_eq!(wordcraft_docx::write(&session.doc).unwrap(), edited_package);

    let recovery_store = Store::new(directory.join("recovery")).unwrap();
    let recovery_id = recovery_store.save(&unique_id(), document_id, &Snapshot::capture(&session)).unwrap();
    let mut recovered = Session::new(Document::new());
    recovery_store.load(&recovery_id).unwrap().restore(&mut recovered);
    assert_eq!(wordcraft_docx::write(&recovered.doc).unwrap(), edited_package);
    recovered.run("file.save", &json!({"path": saved_path})).unwrap();
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path": directory.join("line-edited.docx")})).unwrap();
    let snapshot = wordcraft_docx::inspect_chart_data(&std::fs::read(directory.join("line-edited.docx")).unwrap(), "word/charts/chart1.xml").unwrap();
    assert_eq!(snapshot.chart_kind, "line");
    assert_eq!(snapshot.series[0].categories, ["North", "South"]);
    assert_eq!(snapshot.series[0].values, [130.0, 260.0]);
    assert_eq!(std::fs::read(source_path).unwrap(), original, "source DOCX remains unchanged");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn area_chart_edit_survives_render_undo_recovery_and_docx_reopen() {
    let directory = std::env::temp_dir().join(format!("wordcraft-area-chart-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source_path = directory.join("area.docx");
    let saved_path = directory.join("area-edited.docx");
    let original = editable_area_chart_fixture();
    std::fs::write(&source_path, &original).unwrap();

    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": source_path})).unwrap();
    let document_id = session.document_id();
    let before = session.layout();
    let before_raster = wordcraft_render::render_page(&session.doc, &before.pages[0], 1.0, &Default::default()).to_straight();
    let inspected = session.run("chart.inspectData", &json!({"documentId": document_id, "chartPart": "word/charts/chart1.xml"})).unwrap();
    assert_eq!(inspected["chartKind"], "area");

    session
        .run(
            "chart.editData",
            &json!({
                "documentId": document_id,
                "chartPart": "word/charts/chart1.xml",
                "series": [{"name": "Area measurements", "categories": ["North", "South"], "values": [140.0, 270.0]}]
            }),
        )
        .unwrap();
    let edited_package = wordcraft_docx::write(&session.doc).unwrap();
    let edited_parts = unpack(&edited_package);
    let source_parts = unpack(&original);
    for (part, bytes) in &source_parts {
        if !matches!(part.as_str(), "word/charts/chart1.xml" | "word/embeddings/data.xlsx") {
            assert_eq!(edited_parts.get(part), Some(bytes), "unrelated DOCX part changed: {part}");
        }
    }
    let chart_xml = String::from_utf8(edited_parts["word/charts/chart1.xml"].clone()).unwrap();
    assert!(chart_xml.contains("<c:areaChart>"));
    assert!(chart_xml.contains("Area measurements"));
    let xlsx = unpack(&edited_parts["word/embeddings/data.xlsx"]);
    let sheet = String::from_utf8(xlsx["xl/worksheets/sheet1.xml"].clone()).unwrap();
    assert!(sheet.contains("South"));
    assert!(sheet.contains(">140</v>"));

    let after = session.layout();
    let after_raster = wordcraft_render::render_page(&session.doc, &after.pages[0], 1.0, &Default::default()).to_straight();
    assert_ne!(before_raster, after_raster, "cache edits refresh area chart rendering");
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(
        wordcraft_docx::inspect_chart_data(&wordcraft_docx::write(&session.doc).unwrap(), "word/charts/chart1.xml").unwrap().series[0].values,
        [42.0, 84.0]
    );
    session.run("edit.redo", &json!({})).unwrap();
    assert_eq!(wordcraft_docx::write(&session.doc).unwrap(), edited_package);

    let recovery_store = Store::new(directory.join("recovery")).unwrap();
    let recovery_id = recovery_store.save(&unique_id(), document_id, &Snapshot::capture(&session)).unwrap();
    let mut recovered = Session::new(Document::new());
    recovery_store.load(&recovery_id).unwrap().restore(&mut recovered);
    assert_eq!(wordcraft_docx::write(&recovered.doc).unwrap(), edited_package);
    recovered.run("file.save", &json!({"path": saved_path})).unwrap();
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path": directory.join("area-edited.docx")})).unwrap();
    let snapshot = wordcraft_docx::inspect_chart_data(&std::fs::read(directory.join("area-edited.docx")).unwrap(), "word/charts/chart1.xml").unwrap();
    assert_eq!(snapshot.chart_kind, "area");
    assert_eq!(snapshot.series[0].categories, ["North", "South"]);
    assert_eq!(snapshot.series[0].values, [140.0, 270.0]);
    assert_eq!(std::fs::read(source_path).unwrap(), original, "source DOCX remains unchanged");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn pie_and_doughnut_edits_survive_render_undo_recovery_and_docx_reopen() {
    for chart_type in ["pie", "doughnut"] {
        let directory = std::env::temp_dir().join(format!("wordcraft-{chart_type}-chart-{}", unique_id()));
        std::fs::create_dir_all(&directory).unwrap();
        let source_path = directory.join("source.docx");
        let saved_path = directory.join("edited.docx");
        let original = editable_slice_chart_fixture(chart_type);
        std::fs::write(&source_path, &original).unwrap();

        let mut session = Session::new(Document::new());
        session.run("file.open", &json!({"path": source_path})).unwrap();
        let document_id = session.document_id();
        let before = session.layout();
        let before_raster = wordcraft_render::render_page(&session.doc, &before.pages[0], 1.0, &Default::default()).to_straight();
        let inspected = session.run("chart.inspectData", &json!({"documentId": document_id, "chartPart": "word/charts/chart1.xml"})).unwrap();
        assert_eq!(inspected["chartKind"], chart_type);

        session
            .run(
                "chart.editData",
                &json!({
                    "documentId": document_id,
                    "chartPart": "word/charts/chart1.xml",
                    "series": [{"name": "Revised series", "categories": ["Cyan", "Magenta"], "values": [150.0, 280.0]}]
                }),
            )
            .unwrap();
        let edited_package = wordcraft_docx::write(&session.doc).unwrap();
        let edited_parts = unpack(&edited_package);
        let source_parts = unpack(&original);
        for (part, bytes) in &source_parts {
            if !matches!(part.as_str(), "word/charts/chart1.xml" | "word/embeddings/data.xlsx") {
                assert_eq!(edited_parts.get(part), Some(bytes), "unrelated DOCX part changed: {part}");
            }
        }
        let chart_xml = String::from_utf8(edited_parts["word/charts/chart1.xml"].clone()).unwrap();
        assert!(chart_xml.contains("Revised series"));
        assert!(chart_xml.contains("Cyan"));
        let xlsx = unpack(&edited_parts["word/embeddings/data.xlsx"]);
        let sheet = String::from_utf8(xlsx["xl/worksheets/sheet1.xml"].clone()).unwrap();
        assert!(sheet.contains("Cyan"));
        assert!(sheet.contains(">150</v>"));

        let after = session.layout();
        let after_raster = wordcraft_render::render_page(&session.doc, &after.pages[0], 1.0, &Default::default()).to_straight();
        assert_ne!(before_raster, after_raster, "cache edits refresh {chart_type} rendering");
        session.run("edit.undo", &json!({})).unwrap();
        assert_eq!(
            wordcraft_docx::inspect_chart_data(&wordcraft_docx::write(&session.doc).unwrap(), "word/charts/chart1.xml").unwrap().series[0].values,
            [42.0, 84.0]
        );
        session.run("edit.redo", &json!({})).unwrap();
        assert_eq!(wordcraft_docx::write(&session.doc).unwrap(), edited_package);

        let recovery_store = Store::new(directory.join("recovery")).unwrap();
        let recovery_id = recovery_store.save(&unique_id(), document_id, &Snapshot::capture(&session)).unwrap();
        let mut recovered = Session::new(Document::new());
        recovery_store.load(&recovery_id).unwrap().restore(&mut recovered);
        assert_eq!(wordcraft_docx::write(&recovered.doc).unwrap(), edited_package);
        recovered.run("file.save", &json!({"path": saved_path})).unwrap();
        let mut reopened = Session::new(Document::new());
        reopened.run("file.open", &json!({"path": directory.join("edited.docx")})).unwrap();
        let snapshot = wordcraft_docx::inspect_chart_data(&std::fs::read(directory.join("edited.docx")).unwrap(), "word/charts/chart1.xml").unwrap();
        assert_eq!(snapshot.chart_kind, chart_type);
        assert_eq!(snapshot.series[0].categories, ["Cyan", "Magenta"]);
        assert_eq!(snapshot.series[0].values, [150.0, 280.0]);
        assert_eq!(std::fs::read(source_path).unwrap(), original, "source DOCX remains unchanged");
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn chart_engine_command_rejects_stale_document_and_unsupported_chart_without_mutation() {
    let directory = std::env::temp_dir().join(format!("wordcraft-chart-reject-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("unsupported.docx");
    let original = archive(&fixture("scientific"));
    std::fs::write(&path, &original).unwrap();
    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": path})).unwrap();
    let current_id = session.document_id();
    let before = serde_json::to_value(&session.doc).unwrap();
    let original_package = session.source_package.as_ref().unwrap().bytes.clone();
    assert!(session.run("chart.editData", &json!({"documentId": current_id + 1, "chartPart": "word/charts/chart1.xml", "series": []})).is_err());
    assert!(
        session
            .run("chart.editData", &json!({"documentId": current_id, "chartPart": "word/charts/chart1.xml", "series": [{"values": [1.0, 2.0]}]}),)
            .is_err()
    );
    assert_eq!(serde_json::to_value(&session.doc).unwrap(), before);
    assert_eq!(*session.source_package.as_ref().unwrap().bytes, *original_package);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::remove_dir_all(directory).unwrap();
}
#[test]
fn failed_chart_save_keeps_original_and_document_metadata_unchanged() {
    let directory = std::env::temp_dir().join(format!("wordcraft-chart-failed-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("original.docx");
    let original = archive(&fixture("scientific"));
    std::fs::write(&path, &original).unwrap();
    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": path})).unwrap();
    session.run("text.insert", &json!({"text": "Don't lose this edit: "})).unwrap();
    session.doc.passthrough.remove("word/embeddings/data.xlsx");
    let before = serde_json::to_value(&session.doc).unwrap();
    assert!(session.run("file.save", &json!({})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(serde_json::to_value(&session.doc).unwrap(), before);
    assert!(session.dirty);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failed_save_after_chart_edit_keeps_working_package_and_original_file_intact() {
    let directory = std::env::temp_dir().join(format!("wordcraft-chart-write-failure-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("source.docx");
    let original = editable_chart_fixture();
    std::fs::write(&path, &original).unwrap();
    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path": path})).unwrap();
    let document_id = session.document_id();
    session
        .run("chart.editData", &json!({"documentId": document_id, "chartPart": "word/charts/chart1.xml", "series": [{"values": [8.0, 9.0]}]}))
        .unwrap();
    let doc = serde_json::to_value(&session.doc).unwrap();
    let source = session.source_package.as_ref().unwrap().bytes.clone();
    let dirty = session.dirty;
    let impossible = directory.join("missing").join("failed.docx");
    assert!(session.run("file.save", &json!({"path": impossible})).is_err());
    assert_eq!(serde_json::to_value(&session.doc).unwrap(), doc);
    assert_eq!(*session.source_package.as_ref().unwrap().bytes, *source);
    assert_eq!(session.dirty, dirty);
    assert_eq!(std::fs::read(path).unwrap(), original);
    std::fs::remove_dir_all(directory).unwrap();
}
