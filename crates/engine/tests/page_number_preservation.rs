//! Regression coverage for page-number insertion preserving existing header/footer stories.
use std::sync::Arc;

use serde_json::json;
use wordcraft_doc::para::InlineObject;
use wordcraft_doc::props::{Align, CharProps};
use wordcraft_doc::{Block, Document, Paragraph, Path, StoryRef, Table, para_block};
use wordcraft_engine::Session;

fn fields(doc: &Document, story: StoryRef, name: &str) -> usize {
    doc.para_paths(story)
        .iter()
        .filter_map(|path| doc.para(story, path))
        .flat_map(|p| &p.objects)
        .filter(|o| matches!(o, InlineObject::Field { instr, .. } | InlineObject::FieldStart { instr, .. } if instr.split_whitespace().next().is_some_and(|token| token.eq_ignore_ascii_case(name))))
        .count()
}

fn field(instr: &str) -> InlineObject {
    InlineObject::Field { instr: instr.into(), result: "1".into(), locked: false, dirty: false, code: false, form: String::new() }
}

#[test]
fn page_number_insertion_preserves_existing_header_and_footer_stories() {
    for (command, position) in [("insert.header", "top"), ("insert.footer", "bottom")] {
        for format in ["plain", "x of y"] {
            let mut s = Session::new(Document::from_text("Body text"));
            let id = s.run(command, &json!({"text": "Keep this\nSecond line"})).unwrap()["story"].as_u64().unwrap() as u32;
            let story = StoryRef::Part(id);
            s.doc.para_mut(story, &Path::top(0)).unwrap().props.align = Some(Align::Right);
            let mut table = Table::new(1, 2, 300.0);
            table.rows[0].cells[0].blocks =
                vec![para_block(Paragraph::with_text("Keep table", CharProps { bold: Some(true), ..Default::default() }))];
            s.doc.story_mut(story).unwrap().push(Arc::new(Block::Table(table)));
            let before = s.doc.clone();
            let original_blocks = s.doc.story(story).unwrap().clone();
            let original_selection = s.sel.clone();
            let undo_depth = s.undo_depth();
            s.run("insert.pageNumber", &json!({"position": position, "format": format})).unwrap();
            assert!(
                s.doc.plain_text(story).starts_with("Keep this\nSecond line\nKeep table"),
                "existing text and table were lost: {:?}",
                s.doc.plain_text(story)
            );
            assert_eq!(&s.doc.story(story).unwrap()[..original_blocks.len()], original_blocks.as_slice());
            assert_eq!(s.sel, original_selection);
            assert_eq!(s.undo_depth(), undo_depth + 1);
            assert_eq!(fields(&s.doc, story, "PAGE"), 1);
            assert_eq!(fields(&s.doc, story, "NUMPAGES"), usize::from(format == "x of y"));
            let after = s.doc.clone();
            s.run("edit.undo", &json!({})).unwrap();
            assert_eq!(s.doc, before);
            assert_eq!(s.sel, original_selection);
            s.run("edit.redo", &json!({})).unwrap();
            assert_eq!(s.doc, after);
            s.run("insert.pageNumber", &json!({"position": position, "format": format})).unwrap();
            assert_eq!(s.doc, after, "repeated page number insertion changed content");

            let mut reopened = after;
            for _ in 0..2 {
                reopened = wordcraft_docx::read(&wordcraft_docx::write(&reopened).unwrap()).unwrap();
                let id = if position == "top" { reopened.last_section.headers.default } else { reopened.last_section.footers.default }.unwrap();
                let reopened_story = StoryRef::Part(id);
                assert_eq!(reopened.plain_text(StoryRef::Body), "Body text");
                assert!(reopened.plain_text(reopened_story).starts_with("Keep this\nSecond line\nKeep table"));
                assert_eq!(reopened.para(reopened_story, &Path::top(0)).unwrap().props.align, Some(Align::Right));
                let Block::Table(table) = &*reopened.story(reopened_story).unwrap()[2] else { panic!("existing table was lost") };
                let Block::Para(cell_para) = &*table.rows[0].cells[0].blocks[0] else { panic!("table cell paragraph was lost") };
                assert_eq!(cell_para.props_at(0).bold, Some(true));
                assert_eq!(fields(&reopened, reopened_story, "PAGE"), 1);
                assert_eq!(fields(&reopened, reopened_story, "NUMPAGES"), usize::from(format == "x of y"));
            }
        }
    }
}

#[test]
fn existing_page_field_in_table_with_switches_is_not_duplicated() {
    let mut s = Session::new(Document::from_text("Body"));
    let id = s.run("insert.footer", &json!({"text": "Keep footer"})).unwrap()["story"].as_u64().unwrap() as u32;
    let mut table = Table::new(1, 1, 300.0);
    let mut para = Paragraph::with_text("Existing page: ", CharProps::default());
    para.insert_object(para.len(), field(" page \\* MERGEFORMAT "), &CharProps::default()).unwrap();
    table.rows[0].cells[0].blocks = vec![para_block(para)];
    s.doc.story_mut(StoryRef::Part(id)).unwrap().push(Arc::new(Block::Table(table)));
    let before = s.doc.clone();
    s.run("insert.pageNumber", &json!({"position": "bottom", "format": "x of y"})).unwrap();
    assert_eq!(s.doc, before);
    let reopened = wordcraft_docx::read(&wordcraft_docx::write(&s.doc).unwrap()).unwrap();
    let footer = StoryRef::Part(reopened.last_section.footers.default.unwrap());
    assert_eq!(fields(&reopened, footer, "PAGE"), 1);
    let instruction = reopened
        .para_paths(footer)
        .iter()
        .filter_map(|path| reopened.para(footer, path))
        .flat_map(|para| &para.objects)
        .find_map(|object| match object {
            InlineObject::Field { instr, .. } => Some(instr.as_str()),
            _ => None,
        })
        .expect("PAGE instruction survived DOCX round trip");
    assert!(instruction.to_ascii_uppercase().contains("MERGEFORMAT"), "switch lost: {instruction:?}");
}

#[test]
fn range_page_field_and_formatted_empty_paragraph_are_preserved() {
    let mut s = Session::new(Document::new());
    let id = s.run("insert.header", &json!({"text": "Keep header"})).unwrap()["story"].as_u64().unwrap() as u32;
    let story = StoryRef::Part(id);
    let para = s.doc.para_mut(story, &Path::top(0)).unwrap();
    para.insert_object(para.len(), InlineObject::FieldStart { instr: "PAGE".into(), locked: true, dirty: true }, &CharProps::default()).unwrap();
    para.insert_text(para.len(), "7", &CharProps::default()).unwrap();
    para.insert_object(para.len(), InlineObject::FieldEnd, &CharProps::default()).unwrap();
    let before = s.doc.clone();
    s.run("insert.pageNumber", &json!({"position": "top"})).unwrap();
    assert_eq!(fields(&s.doc, story, "PAGE"), 1, "the range field is preserved without a duplicate");
    assert_eq!(s.doc, before);
    let reopened = wordcraft_docx::read(&wordcraft_docx::write(&s.doc).unwrap()).unwrap();
    let header = StoryRef::Part(reopened.last_section.headers.default.unwrap());
    assert_eq!(fields(&reopened, header, "PAGE"), 1, "range representation survived DOCX round trip");
}

#[test]
fn numbering_undo_redo_and_docx_reopen_keep_original_blocks() {
    let mut s = Session::new(Document::from_text("Body text"));
    let id = s.run("insert.footer", &json!({"text": "Keep this\nSecond line"})).unwrap()["story"].as_u64().unwrap() as u32;
    let story = StoryRef::Part(id);
    let before = s.doc.clone();
    let selection = s.sel.clone();
    s.run("insert.pageNumber", &json!({"position": "bottom", "format": "x of y"})).unwrap();
    let after = s.doc.clone();
    assert_eq!(s.sel, selection);
    s.run("edit.undo", &json!({})).unwrap();
    assert_eq!(s.doc, before);
    s.run("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc, after);
    for _ in 0..2 {
        let reopened = wordcraft_docx::read(&wordcraft_docx::write(&s.doc).unwrap()).unwrap();
        let footer = StoryRef::Part(reopened.last_section.footers.default.unwrap());
        assert!(s.doc.plain_text(story).starts_with("Keep this\nSecond line"));
        assert!(reopened.plain_text(footer).starts_with("Keep this\nSecond line"));
        assert_eq!(fields(&reopened, footer, "PAGE"), 1);
        assert_eq!(fields(&reopened, footer, "NUMPAGES"), 1);
        s.doc = reopened;
    }
}

#[test]
fn customized_empty_header_paragraph_survives_page_number_insertion() {
    let mut s = Session::new(Document::new());
    let id = s.run("insert.header", &json!({"text": ""})).unwrap()["story"].as_u64().unwrap() as u32;
    let story = StoryRef::Part(id);
    s.doc.para_mut(story, &Path::top(0)).unwrap().props.align = Some(Align::Right);
    let original = s.doc.para(story, &Path::top(0)).unwrap().clone();
    s.run("insert.pageNumber", &json!({"position": "top"})).unwrap();
    assert_eq!(s.doc.para(story, &Path::top(0)), Some(&original));
    assert_eq!(fields(&s.doc, story, "PAGE"), 1);
}
