//! Table borders resize through engine commands, once on release (one undo step).

use egui::{Pos2, Rect, Ui};
use serde_json::json;
use wordcraft_doc::Pos;
use wordcraft_layout::{DocLayout, Placed};

use crate::WordApp;

pub(crate) struct TableDrag {
    pos: Pos,
    start: Pos2,
    size: f32,
    column: bool,
    scale: f32,
    document: u64,
    revision: u64,
    table_width: f32,
}

fn grab(app: &WordApp, layout: &DocLayout, pages: &[Rect], scale: f32, at: Pos2) -> Option<TableDrag> {
    if !scale.is_finite() || scale <= 0.0 || app.session.doc.settings.protection.is_some() {
        return None;
    }
    for (page, screen) in layout.pages.iter().zip(pages) {
        for item in &page.items {
            let Placed::Cell { rect, table, row, cell, story } = item else { continue };
            if *story != app.session.sel.focus.story {
                continue;
            }
            let right = screen.min.x + rect.right() * scale;
            let bottom = screen.min.y + rect.bottom() * scale;
            let left = screen.min.x + rect.x * scale;
            let top = screen.min.y + rect.y * scale;
            let column = (at.x - right).abs() <= 4.0 && at.y >= top + 4.0 && at.y <= bottom - 4.0;
            let row_border = (at.y - bottom).abs() <= 4.0 && at.x >= left + 4.0 && at.x <= right - 4.0;
            if !column && !row_border {
                continue;
            }
            let t = app.session.doc.table(*story, table)?;
            let cl = t.rows.get(*row)?.cells.get(*cell)?;
            // A spanned cell has no individual column border; its underlying columns can
            // still be resized from a row with unmerged cells.
            if column && cl.props.span > 1 {
                continue;
            }
            let mut prefix = table.0.clone();
            prefix.push(u32::try_from(*row).ok()?);
            prefix.push(u32::try_from(*cell).ok()?);
            let path = app.session.doc.para_paths(*story).into_iter().find(|p| p.0.starts_with(&prefix))?;
            // Skip split row fragments: their visible height isn't the whole row's height.
            if !column
                && layout.pages.iter().any(|p| {
                    !std::ptr::eq(p, page)
                        && p.items
                            .iter()
                            .any(|it| matches!(it, Placed::Cell { table: other, row: r, story: st, .. } if other == table && r == row && st == story))
                })
            {
                continue;
            }
            return Some(TableDrag {
                pos: Pos { story: *story, path, off: 0 },
                start: at,
                size: if column { rect.w } else { rect.h },
                column,
                scale,
                document: app.session.document_id(),
                revision: app.session.rev(),
                table_width: page
                    .items
                    .iter()
                    .filter_map(|it| match it {
                        Placed::Cell { rect, table: other, row: r, story: st, .. } if other == table && r == row && st == story => Some(rect.w),
                        _ => None,
                    })
                    .sum(),
            });
        }
    }
    None
}

pub(crate) fn cursor(app: &WordApp, layout: &DocLayout, pages: &[Rect], scale: f32, at: Pos2) -> Option<egui::CursorIcon> {
    let column = app.canvas.table_drag.as_ref().map(|d| d.column).or_else(|| grab(app, layout, pages, scale, at).map(|d| d.column))?;
    Some(if column { egui::CursorIcon::ResizeColumn } else { egui::CursorIcon::ResizeRow })
}

pub(crate) fn pointer(app: &mut WordApp, ui: &Ui, resp: &egui::Response, layout: &DocLayout, pages: &[Rect], scale: f32) -> bool {
    if let Some(d) = app.canvas.table_drag.take() {
        if d.document != app.session.document_id() || d.revision != app.session.rev() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            return true;
        }
        let Some(at) = ui.input(|i| i.pointer.latest_pos()) else { return true };
        let delta = if d.column { at.x - d.start.x } else { at.y - d.start.y };
        if ui.input(|i| i.pointer.primary_down()) {
            if let Some(rect) = app.canvas.canvas_rect {
                let line = if d.column {
                    [egui::pos2(at.x, rect.top()), egui::pos2(at.x, rect.bottom())]
                } else {
                    [egui::pos2(rect.left(), at.y), egui::pos2(rect.right(), at.y)]
                };
                ui.painter().line_segment(line, egui::Stroke::new(1.0, crate::theme::Tokens::get(ui.ctx()).accent));
            }
            app.canvas.table_drag = Some(d);
        } else if delta.abs() >= 1.0 && delta.is_finite() {
            app.session.sel = wordcraft_engine::Selection::caret(d.pos);
            let size = (d.size + delta / d.scale).clamp(if d.column { 6.0 } else { 1.0 }, 1584.0);
            let (command, params) = if d.column {
                ("table.columnWidth", json!({"width": size, "tableWidth": d.table_width}))
            } else {
                ("table.rowHeight", json!({"height": size}))
            };
            let _ = app.run(command, params);
        }
        return true;
    }
    if ui.input(|i| i.pointer.primary_pressed())
        && resp.contains_pointer()
        && let Some(at) = ui.input(|i| i.pointer.latest_pos())
        && let Some(d) = grab(app, layout, pages, scale, at)
    {
        app.canvas.table_drag = Some(d);
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Services;
    use wordcraft_engine::Session;

    fn frame(ctx: &egui::Context, app: &mut WordApp, events: Vec<egui::Event>) {
        let input = egui::RawInput { events, screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1440.0, 900.0))), ..Default::default() };
        ctx.run_ui(input, |ui| {
            app.logic(ui.ctx());
            app.ui(ui);
        })
        .drop_without_applying_deltas();
    }

    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    #[test]
    fn border_drags_resize_rows_and_columns_once_and_can_be_cancelled() {
        for column in [true, false] {
            let ctx = egui::Context::default();
            let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), Services::default());
            app.run("insert.table", json!({"rows": 2, "cols": 2})).unwrap();
            for _ in 0..4 {
                frame(&ctx, &mut app, Vec::new());
            }
            let l = app.session.layout();
            let (rect, table) = l
                .pages
                .iter()
                .flat_map(|p| &p.items)
                .find_map(|it| match it {
                    Placed::Cell { rect, table, row: 0, cell: 0, .. } => Some((*rect, table.clone())),
                    _ => None,
                })
                .unwrap();
            let page = app.canvas.page_rects[0];
            let scale = app.canvas.scale;
            let start = if column {
                egui::pos2(page.min.x + rect.right() * scale, page.min.y + (rect.y + rect.h / 2.0) * scale)
            } else {
                egui::pos2(page.min.x + (rect.x + rect.w / 2.0) * scale, page.min.y + rect.bottom() * scale)
            };
            let end = start + if column { egui::vec2(30.0 * scale, 0.0) } else { egui::vec2(0.0, 30.0 * scale) };
            let before = app.session.doc.table(wordcraft_doc::StoryRef::Body, &table).unwrap().clone();
            let undo = app.session.undo_labels().len();
            frame(&ctx, &mut app, vec![egui::Event::PointerMoved(start)]);
            frame(&ctx, &mut app, vec![button(start, true)]);
            assert!(app.canvas.table_drag.is_some(), "border owns the press");
            for _ in 0..3 {
                frame(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
            }
            assert_eq!(app.session.undo_labels().len(), undo, "moving doesn't edit or accumulate deltas");
            frame(&ctx, &mut app, vec![button(end, false)]);
            assert!(app.canvas.table_drag.is_none());
            assert_eq!(app.session.undo_labels().len(), undo + 1, "the entire drag is one edit");
            let t = app.session.doc.table(wordcraft_doc::StoryRef::Body, &table).unwrap();
            if column {
                assert!((t.grid[0] - (rect.w + 30.0)).abs() < 0.1);
            } else {
                assert!((t.rows[0].props.height.unwrap() - (rect.h + 30.0)).abs() < 0.1);
            }
            let shown = app
                .session
                .layout()
                .pages
                .iter()
                .flat_map(|p| &p.items)
                .find_map(|it| match it {
                    Placed::Cell { rect, row: 0, cell: 0, .. } => Some(*rect),
                    _ => None,
                })
                .unwrap();
            let difference = if column { shown.w - rect.w } else { shown.h - rect.h };
            assert!(
                (difference - 30.0).abs() < if column { 0.1 } else { 1.0 },
                "layout follows the drag: column {column}, before {rect:?}, after {shown:?}, delta {difference}"
            );
            app.run("edit.undo", json!({})).unwrap();
            let t = app.session.doc.table(wordcraft_doc::StoryRef::Body, &table).unwrap();
            assert_eq!(t.grid, before.grid);
            assert_eq!(t.rows[0].props.height, before.rows[0].props.height);
            for _ in 0..3 {
                frame(&ctx, &mut app, Vec::new());
            }
            frame(&ctx, &mut app, vec![egui::Event::PointerMoved(start), button(start, true)]);
            frame(&ctx, &mut app, vec![egui::Event::PointerMoved(end)]);
            frame(
                &ctx,
                &mut app,
                vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            );
            frame(&ctx, &mut app, vec![button(end, false)]);
            assert!(app.canvas.table_drag.is_none());
            assert_eq!(app.session.undo_labels().len(), undo, "Escape cancels without editing");
        }
    }
}
