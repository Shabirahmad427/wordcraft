//! The same synthetic chart corpus exercises editing, failure isolation and durable recovery.
#[path = "../../docx/tests/common/charts.rs"]
mod fixtures;
use fixtures::{archive, fixture, unpack};
use serde_json::json;
use wordcraft_engine::recovery::{Snapshot, Store, unique_id};
use wordcraft_engine::{
    Session,
    doc::{Document, StoryRef},
};
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
    assert!(session.doc.plain_text(StoryRef::Body).contains("Chart preserved"));
    assert_eq!(session.doc.passthrough, recovered.doc.passthrough);
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
