//! Synthetic pagination and raster regressions; these are not Microsoft Word references.
#[path = "../../docx/tests/common/preservation.rs"]
mod fixtures;
use serde_json::json;
use wordcraft_engine::{
    Session,
    doc::{
        Document, Paragraph, PartKind, Path, Pos, StoryRef, Table,
        para::{InlineObject, NoteKind},
        props::VAlign,
        section::{SectionProps, SectionStart},
    },
    recovery::{Snapshot, Store, unique_id},
};
use wordcraft_layout::{DocLayout, Placed};

fn academic_document() -> (Document, u32) {
    let mut doc = Document::from_text("Academic introduction\nResults and measurements");
    let old = SectionProps { valign: VAlign::Center, ..Default::default() };
    doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().section = Some(Box::new(old));
    doc.last_section = SectionProps {
        start: SectionStart::OddPage,
        page_w: 792.0,
        page_h: 612.0,
        margin_top: 36.0,
        margin_bottom: 36.0,
        margin_left: 100.0,
        page_num_start: Some(1),
        ..Default::default()
    };
    let note = doc.add_part(
        PartKind::Footnote,
        vec![wordcraft_engine::doc::para_block(Paragraph::with_text("Source and methodology note", Default::default()))],
    );
    doc.insert_object(
        &Pos { story: StoryRef::Part(note), path: Path::top(0), off: 0 },
        InlineObject::NoteRef { kind: NoteKind::Footnote, id: note, custom: String::new() },
        &Default::default(),
    )
    .unwrap();
    doc.insert_object(&Pos::body(0, 8), InlineObject::NoteRef { kind: NoteKind::Footnote, id: note, custom: String::new() }, &Default::default())
        .unwrap();
    let mut table = Table::new(14, 2, doc.last_section.text_width());
    table.rows[0].props.header = true;
    for (row, cells) in table.rows.iter_mut().enumerate() {
        for (cell, column) in cells.cells.iter_mut().enumerate() {
            column.blocks = vec![wordcraft_engine::doc::para_block(Paragraph::with_text(
                &format!("Measurement {row}/{cell}: {}", "Original experimental observation. ".repeat(6)),
                Default::default(),
            ))];
        }
    }
    doc.body.push(std::sync::Arc::new(wordcraft_engine::doc::Block::Table(table)));
    doc.body.push(wordcraft_engine::doc::para_block(Paragraph::with_text("End of measurements", Default::default())));
    (doc, note)
}

fn assert_complete(doc: &Document, layout: &DocLayout) {
    // Every paragraph character is represented; repeated header rows are allowed.
    let mut placed = std::collections::HashMap::<(StoryRef, Path), Vec<(usize, usize)>>::new();
    for page in &layout.pages {
        for item in &page.items {
            if let Placed::Lines { story, path, para, l0, l1, y, .. } = item {
                placed.entry((*story, path.clone())).or_default();
                let first = &para.lines[*l0];
                for line in &para.lines[*l0..*l1] {
                    assert!(*y + line.top - first.top >= page.body.y - 0.5);
                    assert!(*y + line.top - first.top + line.height <= page.body.bottom() + 0.5);
                    for cluster in &para.clusters[line.c0..line.c1] {
                        placed.entry((*story, path.clone())).or_default().push((cluster.start, cluster.end));
                    }
                }
            }
        }
    }
    for story in std::iter::once(StoryRef::Body).chain(doc.parts.keys().copied().map(StoryRef::Part)) {
        for path in doc.para_paths(story) {
            let paragraph = doc.para(story, &path).unwrap();
            let ranges =
                placed.get(&(story, path.clone())).unwrap_or_else(|| panic!("paragraph disappeared: {story:?}/{path:?} text {:?}", paragraph.text));
            let mut covered: Vec<bool> = vec![false; paragraph.text.len()];
            for (a, b) in ranges {
                for byte in covered.iter_mut().take(*b).skip(*a) {
                    *byte = true;
                }
            }
            assert!(covered.iter().all(|byte| *byte), "missing text in {story:?}/{path:?}");
        }
    }
}

#[test]
fn academic_sections_roundtrip_undo_and_recovery_keep_pagination_and_pixels() {
    let directory = std::env::temp_dir().join(format!("wordcraft-section-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let (doc, note) = academic_document();
    let mut session = Session::new(doc);
    let initial = session.doc.clone();
    session.run("text.insert", &json!({"text":"Verified: "})).unwrap();
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc, initial);
    session.run("edit.redo", &json!({})).unwrap();
    let before = session.layout();
    assert!(before.pages.len() > 3);
    assert_eq!(before.caret(&Pos::body(1, 0)).unwrap().page, 2);
    assert_eq!(before.pages[2].number, 1);
    assert_complete(&session.doc, &before);
    let note_pos = Pos { story: StoryRef::Part(note), path: Path::top(0), off: 0 };
    let nc = before.caret(&note_pos).unwrap();
    assert_eq!(nc.page, 0);
    assert_eq!(nc.x, 72.0);
    let store = Store::new(directory.join("recovery")).unwrap();
    let id = store.save(&unique_id(), session.document_id(), &Snapshot::capture(&session)).unwrap();
    let mut restored = Session::new(Document::new());
    store.load(&id).unwrap().restore(&mut restored);
    let output = directory.join("academic.docx");
    restored.run("file.save", &json!({"path":output})).unwrap();
    let pdf = directory.join("academic.pdf");
    restored.run("file.exportPdf", &json!({"path":pdf})).unwrap();
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path":output})).unwrap();
    let after = reopened.layout();
    assert_eq!(before.pages.len(), after.pages.len());
    assert_complete(&reopened.doc, &after);
    for (index, (a, b)) in before.pages.iter().zip(&after.pages).enumerate() {
        assert_eq!((a.w, a.h, a.number, a.section), (b.w, b.h, b.number, b.section));
        let options = wordcraft_render::RenderOptions::default();
        let original = wordcraft_render::render_page(&session.doc, a, 1.0, &options);
        assert!(
            original.to_straight() == wordcraft_render::render_page(&reopened.doc, b, 1.0, &options).to_straight(),
            "pixels changed on page {index}"
        );
        if let Ok(path) = std::env::var("WORDCRAFT_LAYOUT_CORPUS_DIR") {
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(std::path::Path::new(&path).join(format!("section-{index}.png")), original.to_png()).unwrap();
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn layout_does_not_mutate_guarded_packages_or_enable_unsafe_section_edits() {
    let directory = std::env::temp_dir().join(format!("wordcraft-section-guard-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("guarded.docx");
    let bytes = fixtures::archive(&fixtures::fixture());
    std::fs::write(&path, &bytes).unwrap();
    let mut session = Session::new(Document::new());
    session.run("file.open", &json!({"path":path})).unwrap();
    let original = session.doc.clone();
    let _ = session.layout();
    assert_eq!(session.doc, original);
    session.run("file.save", &json!({})).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    session.run("layout.orientation", &json!({"value":"landscape"})).unwrap();
    assert!(session.run("file.save", &json!({})).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(session.dirty);
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc, original);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn preceding_section_raster_matches_its_standalone_page() {
    let (doc, _) = academic_document();
    let mut reference = doc.clone();
    reference.body.truncate(1);
    reference.last_section = *reference.para_mut(StoryRef::Body, &Path::top(0)).unwrap().section.take().unwrap();
    let mut combined = Session::new(doc);
    let mut standalone = Session::new(reference);
    let a = combined.layout();
    let b = standalone.layout();
    let options = wordcraft_render::RenderOptions::default();
    assert!(
        wordcraft_render::render_page(&combined.doc, &a.pages[0], 1.0, &options).to_straight()
            == wordcraft_render::render_page(&standalone.doc, &b.pages[0], 1.0, &options).to_straight(),
        "the following section changed the previous page's pixels"
    );
}

#[test]
fn next_column_with_inherited_page_furniture_survives_undo_recovery_docx_and_pdf() {
    let directory = std::env::temp_dir().join(format!("wordcraft-next-column-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut doc = Document::from_text("Introduction\nMethods and evidence\nConclusion");
    let header = doc.add_part(
        PartKind::Header,
        (0..8).map(|_| wordcraft_engine::doc::para_block(Paragraph::with_text("Inherited running header", Default::default()))).collect(),
    );
    let footer = doc.add_part(
        PartKind::Footer,
        (0..8).map(|_| wordcraft_engine::doc::para_block(Paragraph::with_text("Inherited footer", Default::default()))).collect(),
    );
    let mut first = SectionProps::default();
    first.columns.count = 2;
    first.headers.default = Some(header);
    first.footers.default = Some(footer);
    doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().section = Some(Box::new(first));
    doc.last_section.start = SectionStart::NextColumn;
    doc.last_section.columns.count = 2;
    let note = doc.add_part(
        PartKind::Footnote,
        vec![wordcraft_engine::doc::para_block(Paragraph::with_text("A supporting academic note.", Default::default()))],
    );
    doc.insert_object(&Pos::body(1, 6), InlineObject::NoteRef { kind: NoteKind::Footnote, id: note, custom: String::new() }, &Default::default())
        .unwrap();
    let mut table = Table::new(4, 2, doc.last_section.text_width());
    for (row, cells) in table.rows.iter_mut().enumerate() {
        for (column, cell) in cells.cells.iter_mut().enumerate() {
            cell.blocks = vec![wordcraft_engine::doc::para_block(Paragraph::with_text(
                &format!("Result {row}/{column}: measured value and interpretation."),
                Default::default(),
            ))];
        }
    }
    doc.body.push(std::sync::Arc::new(wordcraft_engine::doc::Block::Table(table)));
    let mut session = Session::new(doc);
    let original = session.doc.clone();
    session.run("text.insert", &json!({"text":"temporary "})).unwrap();
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc, original);
    let before = session.layout();
    let a = before.caret(&Pos::body(0, 0)).unwrap();
    let b = before.caret(&Pos::body(1, 0)).unwrap();
    assert_eq!(a.page, b.page);
    assert!(b.x > a.x + 100.0);
    let page = &before.pages[b.page];
    assert_eq!(page.header_story, Some(header));
    assert_eq!(page.footer_story, Some(footer));
    let header_bottom = page
        .header
        .iter()
        .filter_map(|it| {
            if let Placed::Lines { para, l1, y, .. } = it {
                para.lines.get(l1.saturating_sub(1)).map(|line| *y + line.top + line.height)
            } else {
                None
            }
        })
        .fold(0.0, f32::max);
    assert!(page.body.y >= header_bottom - 0.5);
    let footer_top = page
        .footer
        .iter()
        .filter_map(|it| if let Placed::Lines { y, para, l0, .. } = it { para.lines.get(*l0).map(|line| *y + line.top) } else { None })
        .fold(f32::MAX, f32::min);
    assert!(page.body.bottom() <= footer_top + 0.5);
    let expected_pixels: Vec<_> = before
        .pages
        .iter()
        .map(|page| wordcraft_render::render_page(&session.doc, page, 1.0, &wordcraft_render::RenderOptions::default()).to_straight())
        .collect();
    let store = Store::new(directory.join("recovery")).unwrap();
    let id = store.save(&unique_id(), session.document_id(), &Snapshot::capture(&session)).unwrap();
    let mut recovered = Session::new(Document::new());
    store.load(&id).unwrap().restore(&mut recovered);
    assert_eq!(recovered.doc.plain_text(StoryRef::Body).trim_end(), original.plain_text(StoryRef::Body).trim_end());
    assert_eq!(recovered.doc.parts.len(), original.parts.len());
    let before = recovered.layout();
    for (index, (page, expected)) in before.pages.iter().zip(&expected_pixels).enumerate() {
        assert!(
            wordcraft_render::render_page(&recovered.doc, page, 1.0, &wordcraft_render::RenderOptions::default()).to_straight() == *expected,
            "recovery changed page {index} pixels"
        );
    }
    let docx = directory.join("sections.docx");
    recovered.run("file.save", &json!({"path":docx})).unwrap();
    let pdf = directory.join("sections.pdf");
    recovered.run("file.exportPdf", &json!({"path":pdf})).unwrap();
    assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-"));
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path":docx})).unwrap();
    let after = reopened.layout();
    assert_eq!(before.pages.len(), after.pages.len());
    for (index, (a, b)) in before.pages.iter().zip(&after.pages).enumerate() {
        assert_eq!((a.w, a.h, a.number, a.section), (b.w, b.h, b.number, b.section));
        let original_pos = before.caret(&Pos::body(1, 0)).unwrap();
        let reopened_pos = after.caret(&Pos::body(1, 0)).unwrap();
        assert_eq!(original_pos.page, reopened_pos.page, "section flow changed after DOCX reopen on page {index}");
        assert!((original_pos.x - reopened_pos.x).abs() < 1.0, "column position changed after DOCX reopen: {original_pos:?} -> {reopened_pos:?}");
        let rendered = wordcraft_render::render_page(&reopened.doc, b, 1.0, &wordcraft_render::RenderOptions::default());
        assert_eq!((rendered.width, rendered.height), (b.w.ceil() as u32, b.h.ceil() as u32));
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn long_note_keeps_undo_recovery_and_docx_roundtrip_content() {
    let directory = std::env::temp_dir().join(format!("wordcraft-long-note-{}", unique_id()));
    std::fs::create_dir_all(&directory).unwrap();
    let note_text = "Footnote continuation evidence must remain visible, ordered, and editable. ".repeat(110);
    let mut doc = Document::from_text("Academic claim with a long source note.\nClosing body text.");
    let note = doc.add_part(PartKind::Footnote, vec![wordcraft_engine::doc::para_block(Paragraph::with_text(&note_text, Default::default()))]);
    doc.insert_object(
        &Pos { story: StoryRef::Part(note), path: Path::top(0), off: 0 },
        InlineObject::NoteRef { kind: NoteKind::Footnote, id: note, custom: String::new() },
        &Default::default(),
    )
    .unwrap();
    doc.insert_object(&Pos::body(0, 21), InlineObject::NoteRef { kind: NoteKind::Footnote, id: note, custom: String::new() }, &Default::default())
        .unwrap();
    let mut session = Session::new(doc);
    let original = session.doc.clone();
    session.run("text.insert", &json!({"text":"Reviewed: "})).unwrap();
    let edited = session.doc.clone();
    session.run("edit.undo", &json!({})).unwrap();
    assert_eq!(session.doc, original);
    session.run("edit.redo", &json!({})).unwrap();
    let before = session.layout();
    let note_pages: std::collections::BTreeSet<_> = before
        .pages
        .iter()
        .enumerate()
        .filter(|(_, page)| page.items.iter().any(|item| matches!(item, Placed::Lines { story: StoryRef::Part(id), .. } if *id == note)))
        .map(|(index, _)| index)
        .collect();
    assert!(note_pages.len() >= 2, "long footnote should continue across pages: {note_pages:?}");
    let store = Store::new(directory.join("recovery")).unwrap();
    let id = store.save(&unique_id(), session.document_id(), &Snapshot::capture(&session)).unwrap();
    let mut restored = Session::new(Document::new());
    store.load(&id).unwrap().restore(&mut restored);
    assert_eq!(restored.doc, edited);
    let path = directory.join("academic-long-note.docx");
    restored.run("file.save", &json!({"path":path})).unwrap();
    let mut reopened = Session::new(Document::new());
    reopened.run("file.open", &json!({"path":path})).unwrap();
    let restored_note = reopened.doc.parts.get(&note).expect("footnote part remains attached");
    let restored_text = restored_note.blocks.iter().filter_map(|block| block.as_para()).map(|para| para.text.as_str()).collect::<String>();
    assert_eq!(restored_text.chars().next(), Some(char::from_u32(0xfffc).unwrap()));
    assert_eq!(restored_text.len() - "\u{fffc}".len(), note_text.len());
    assert!(restored_text.ends_with(&note_text));
    let after = reopened.layout();
    assert_eq!(before.pages.len(), after.pages.len());
    assert!(after.caret_on(&Pos { story: StoryRef::Part(note), path: Path::top(0), off: note_text.len() + 1 }, after.pages.len() - 1).is_some());
    for (a, b) in before.pages.iter().zip(&after.pages) {
        let options = wordcraft_render::RenderOptions::default();
        assert!(
            wordcraft_render::render_page(&session.doc, a, 1.0, &options).to_straight()
                == wordcraft_render::render_page(&reopened.doc, b, 1.0, &options).to_straight()
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}
