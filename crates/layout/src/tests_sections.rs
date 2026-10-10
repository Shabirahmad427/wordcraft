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
fn inherited_first_even_headers_and_footers_clear_the_body_area() {
    let mut doc = Document::from_text(&"Body text fills the page with enough words to span several pages. ".repeat(180));
    let mut story = |kind, text: &str| doc.add_part(kind, vec![wordcraft_doc::para_block(Paragraph::with_text(text, Default::default()))]);
    let default_header = story(PartKind::Header, "Default header");
    let first_header = story(PartKind::Header, &"First page heading repeats over several wrapped lines. ".repeat(35));
    let even_header = story(PartKind::Header, &"Even page heading takes additional vertical room. ".repeat(22));
    let default_footer = story(PartKind::Footer, "Default footer");
    let first_footer = story(PartKind::Footer, &"First page footer has several lines of publication details. ".repeat(28));
    let even_footer = story(PartKind::Footer, &"Even page footer has several lines of notes. ".repeat(20));
    doc.settings.even_odd_headers = true;
    doc.last_section.title_page = true;
    doc.last_section.headers =
        wordcraft_doc::section::HeaderSet { default: Some(default_header), first: Some(first_header), even: Some(even_header) };
    doc.last_section.footers =
        wordcraft_doc::section::HeaderSet { default: Some(default_footer), first: Some(first_footer), even: Some(even_footer) };
    let result = lay(&doc);
    assert!(result.pages.len() >= 3);
    for page in &result.pages {
        let header_bottom = page
            .header
            .iter()
            .filter_map(|item| match item {
                Placed::Lines { y, para, l0, l1, .. } => item_bottom(*y, para, *l0, *l1),
                _ => None,
            })
            .fold(0.0f32, f32::max);
        let footer_top = page
            .footer
            .iter()
            .filter_map(|item| match item {
                Placed::Lines { y, .. } => Some(*y),
                _ => None,
            })
            .fold(f32::MAX, f32::min);
        assert!(page.body.y + 0.1 >= header_bottom, "header overlaps body on physical page {}", page.number);
        assert!(page.body.bottom() + 5.0 <= footer_top, "body overlaps footer on physical page {}", page.number);
    }
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

#[test]
fn long_footnote_continues_until_every_line_is_placed_without_overlap() {
    let body = format!("{}\nClosing paragraph remains after the note.", "Citation paragraph with measured results.");
    let mut doc = Document::from_text(&body);
    let note_text = "Long footnote continuation preserves each sentence and its original order. ".repeat(150);
    let id = doc.add_part(PartKind::Footnote, vec![wordcraft_doc::para_block(Paragraph::with_text(&note_text, Default::default()))]);
    doc.insert_object(
        &Pos::body(0, 12),
        InlineObject::NoteRef { kind: wordcraft_doc::para::NoteKind::Footnote, id, custom: String::new() },
        &Default::default(),
    )
    .unwrap();
    let result = lay(&doc);
    assert!(result.pages.len() >= 3, "a long note must continue onto later pages");
    let expected = para::layout_para(
        doc.para(StoryRef::Part(id), &Path::top(0)).unwrap(),
        &para::ParaEnv {
            doc: &doc,
            width: doc.last_section.text_width(),
            label: None,
            fields: &FieldCtx::default(),
            show_hidden: false,
            hide_deleted: false,
            table: None,
            proofing: false,
            exclusions: &[],
            eq_number: 0,
        },
    );
    let mut ranges = Vec::new();
    let mut pages = std::collections::BTreeSet::new();
    for (page_index, page) in result.pages.iter().enumerate() {
        let mut body_bottom = page.body.y;
        let mut note_top = f32::MAX;
        for item in &page.items {
            if let Placed::Lines { story, para, l0, l1, y, .. } = item {
                if *story == StoryRef::Body {
                    if let Some(bottom) = item_bottom(*y, para, *l0, *l1) {
                        body_bottom = body_bottom.max(bottom);
                    }
                } else if *story == StoryRef::Part(id) {
                    pages.insert(page_index);
                    if let (Some(first), Some(last)) = (para.lines.get(*l0), l1.checked_sub(1).and_then(|i| para.lines.get(i))) {
                        note_top = note_top.min(*y);
                        assert!(item_bottom(*y, para, *l0, *l1).unwrap_or(f32::MAX) <= page.h - doc.last_section.margin_bottom + 1.0);
                        ranges.push((*l0, *l1));
                        let _ = (first, last);
                    }
                }
            }
        }
        if note_top.is_finite() && body_bottom > page.body.y {
            assert!(body_bottom <= note_top + 0.5, "body overlaps footnote on page {page_index}: {body_bottom} > {note_top}");
        }
    }
    assert!(pages.len() >= 3, "footnote text was not continued: {pages:?}");
    let covered: usize = ranges.iter().map(|(a, b)| b - a).sum();
    assert_eq!(covered, expected.lines.len(), "some footnote lines disappeared or were duplicated");
    assert!(result.caret(&Pos { story: StoryRef::Body, path: Path::top(1), off: 0 }).is_some());
}

#[test]
fn final_equal_width_columns_balance_complete_paragraphs_in_order() {
    let text = (0..30).map(|n| format!("Research result {n}: measured values and observations.")).collect::<Vec<_>>().join("\n");
    let mut doc = Document::from_text(&text);
    doc.last_section.columns = wordcraft_doc::section::Columns { count: 2, space: 36.0, ..Default::default() };
    let result = lay(&doc);
    assert_eq!(result.pages.len(), 1);
    let page = &result.pages[0];
    let mut columns: Vec<Vec<(usize, f32, f32)>> = vec![Vec::new(), Vec::new()];
    for item in &page.items {
        if let Placed::Lines { story: StoryRef::Body, path, x, y, para, l0, l1 } = item {
            let block = path.0.first().copied().unwrap_or(0) as usize;
            let column = usize::from(*x > page.body.x + 200.0);
            let bottom = item_bottom(*y, para, *l0, *l1).unwrap_or(*y);
            columns[column].push((block, *y, bottom));
        }
    }
    assert!(columns[0].len() >= 2 && columns[1].len() >= 2, "both columns receive balanced content: {columns:?}");
    assert!((columns[0].last().unwrap().2 - columns[1].last().unwrap().2).abs() < 32.0, "column bottoms should be close: {columns:?}");
    let mut order: Vec<_> = columns.into_iter().flatten().collect();
    order.sort_by_key(|(block, _, _)| *block);
    assert_eq!(order.iter().map(|(block, _, _)| *block).collect::<Vec<_>>(), (0..30).collect::<Vec<_>>());
}

#[test]
fn complex_multicolumn_page_keeps_existing_table_and_float_flow() {
    let mut doc = Document::from_text(&"Article body around a float and following table. ".repeat(60));
    doc.last_section.columns = wordcraft_doc::section::Columns { count: 2, space: 36.0, ..Default::default() };
    let float = wordcraft_doc::para::Float {
        wrap: Wrap::Square,
        h_rel: Anchor::Column,
        v_rel: Anchor::Paragraph,
        x: 0.0,
        y: 0.0,
        dist: 6.0,
        ..Default::default()
    };
    doc.insert_object(
        &Pos::body(0, 0),
        InlineObject::Shape {
            kind: wordcraft_doc::para::ShapeKind::Rectangle,
            w: 72.0,
            h: 100.0,
            fill: None,
            stroke: None,
            stroke_width: 1.0,
            float,
            story: None,
        },
        &Default::default(),
    )
    .unwrap();
    let result = lay(&doc);
    assert!(!result.pages.is_empty());
    assert!(result.pages.iter().flat_map(|page| &page.items).any(|item| matches!(item, Placed::Shape { .. })));
    let lines: usize = result
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .map(|item| if let Placed::Lines { story: StoryRef::Body, l0, l1, .. } = item { l1 - l0 } else { 0 })
        .sum();
    assert!(lines >= 2, "complex text remains laid out around its object");
}

#[test]
fn oversized_inline_image_overflows_once_and_following_text_advances() {
    let mut doc = Document::from_text("Before image\nAfter image remains reachable");
    doc.last_section.page_h = 220.0;
    doc.last_section.margin_top = 30.0;
    doc.last_section.margin_bottom = 30.0;
    doc.insert_object(
        &Pos::body(0, 6),
        InlineObject::Image {
            media: "synthetic-oversized".into(),
            w: 48.0,
            h: 900.0,
            alt: "Oversized regression object".into(),
            float: Default::default(),
            crop: [0.0; 4],
        },
        &Default::default(),
    )
    .unwrap();
    let result = lay(&doc);
    let image_lines: Vec<_> = result
        .pages
        .iter()
        .flat_map(|page| &page.items)
        .filter_map(|item| match item {
            Placed::Lines { story: StoryRef::Body, path, para, l0, l1, .. }
                if path.0 == vec![0] && para.clusters.iter().any(|c| matches!(c.kind, para::ClKind::Object(_))) =>
            {
                item_bottom(0.0, para, *l0, *l1).map(|height| (para.clone(), height))
            }
            _ => None,
        })
        .collect();
    assert_eq!(image_lines.len(), 1, "the oversized object stays in one intact line");
    assert!(image_lines[0].1 >= 900.0);
    assert!(result.caret(&Pos::body(1, 0)).is_some(), "body pagination reaches text after the object");
}
