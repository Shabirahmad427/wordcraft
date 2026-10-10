//! Section transitions must not finalize a page with the following section's setup.
use super::*;
use wordcraft_doc::props::VAlign;
use wordcraft_doc::{PartKind, Pos};

fn lay(doc: &Document) -> DocLayout {
    layout(doc, &mut LayoutCache::new(), &LayoutOptions::default())
}

fn mixed_sections() -> Document {
    let mut doc = Document::from_text("Previous section body\nFollowing section body");
    doc.para_mut(StoryRef::Body, &Path::top(0)).unwrap().section = Some(Box::new(SectionProps::default()));
    doc.last_section = SectionProps { page_w: 792.0, page_h: 612.0, margin_left: 144.0, margin_top: 36.0, margin_bottom: 36.0, ..Default::default() };
    doc
}

#[test]
fn section_break_preserves_previous_footnote_coordinates() {
    let mut doc = mixed_sections();
    let id = doc.add_part(PartKind::Footnote, vec![wordcraft_doc::para_block(Paragraph::with_text("Academic note", Default::default()))]);
    doc.insert_object(
        &Pos::body(0, 5),
        InlineObject::NoteRef { kind: wordcraft_doc::para::NoteKind::Footnote, id, custom: String::new() },
        &Default::default(),
    )
    .unwrap();
    let mut reference = doc.clone();
    reference.body.truncate(1);
    reference.para_mut(StoryRef::Body, &Path::top(0)).unwrap().section = None;
    reference.last_section = SectionProps::default();
    let before = lay(&reference);
    let after = lay(&doc);
    let position = Pos { story: StoryRef::Part(id), path: Path::top(0), off: 0 };
    let a = before.caret(&position).unwrap();
    let b = after.caret(&position).unwrap();
    assert_eq!((a.page, a.x, a.top, a.height), (b.page, b.x, b.top, b.height));
    assert_eq!((after.pages[0].w, after.pages[0].h), (612.0, 792.0));
    assert_eq!((after.pages[1].w, after.pages[1].h), (792.0, 612.0));
}

#[test]
fn section_break_preserves_previous_vertical_alignment() {
    for previous in [VAlign::Top, VAlign::Center, VAlign::Bottom] {
        for following in [VAlign::Top, VAlign::Center, VAlign::Bottom] {
            let mut doc = mixed_sections();
            doc.section_mut(0).valign = previous;
            doc.last_section.valign = following;
            let mut reference = doc.clone();
            reference.body.truncate(1);
            reference.para_mut(StoryRef::Body, &Path::top(0)).unwrap().section = None;
            reference.last_section = SectionProps { valign: previous, ..Default::default() };
            let a = lay(&reference).caret(&Pos::body(0, 0)).unwrap();
            let b = lay(&doc).caret(&Pos::body(0, 0)).unwrap();
            assert_eq!((a.x, a.top, a.height), (b.x, b.top, b.height), "{previous:?} -> {following:?}");
        }
    }
}

#[test]
fn odd_even_section_break_uses_physical_pages_before_number_restart() {
    for (start, physical) in [(SectionStart::OddPage, 2), (SectionStart::EvenPage, 1)] {
        for restart in [1, 2, 37, 100] {
            let mut doc = mixed_sections();
            doc.last_section.start = start;
            doc.last_section.page_num_start = Some(restart);
            let result = lay(&doc);
            assert_eq!(result.caret(&Pos::body(1, 0)).unwrap().page, physical, "{start:?} restart {restart}");
            assert_eq!(result.pages[physical].number, restart);
            assert_eq!(result.pages[physical].section, 1);
            if physical == 2 {
                assert_eq!((result.pages[1].w, result.pages[1].h), (612.0, 792.0));
                assert_eq!(result.pages[1].section, 0);
                assert!(!result.pages[1].items.iter().any(|item| matches!(item, Placed::Lines { story: StoryRef::Body, .. })));
            }
        }
    }
}

#[test]
fn maximum_page_number_never_panics_when_paginating() {
    let mut doc = Document::from_text(&"Page number safety text ".repeat(1500));
    doc.last_section.page_num_start = Some(u32::MAX);
    let result = lay(&doc);
    assert!(result.pages.len() > 1);
    assert!(result.pages.iter().all(|page| page.number == u32::MAX));
}

#[test]
fn parity_filler_does_not_consume_incoming_first_header() {
    let mut doc = mixed_sections();
    let first = doc.add_part(PartKind::Header, vec![wordcraft_doc::para_block(Paragraph::with_text("First heading", Default::default()))]);
    let other = doc.add_part(PartKind::Header, vec![wordcraft_doc::para_block(Paragraph::with_text("Later heading", Default::default()))]);
    doc.last_section.start = SectionStart::OddPage;
    doc.last_section.title_page = true;
    doc.last_section.headers.first = Some(first);
    doc.last_section.headers.default = Some(other);
    let result = lay(&doc);
    assert_eq!(result.pages.len(), 3);
    assert_eq!(result.pages[1].header_story, None);
    assert_eq!(result.pages[2].header_story, Some(first));
}

#[test]
fn continuous_and_web_sections_keep_flowing_without_filler_pages() {
    let mut doc = mixed_sections();
    doc.last_section = SectionProps { start: SectionStart::Continuous, ..Default::default() };
    let result = lay(&doc);
    assert_eq!(result.pages.len(), 1);
    assert!(result.caret(&Pos::body(1, 0)).unwrap().top > result.caret(&Pos::body(0, 0)).unwrap().top);
    doc.last_section.start = SectionStart::OddPage;
    let web = layout(&doc, &mut LayoutCache::new(), &LayoutOptions { view: ViewMode::Web, web_width: 600.0, ..Default::default() });
    assert_eq!(web.pages.len(), 1);
    assert!(web.caret(&Pos::body(1, 0)).is_some());
}
