//! File tab (Backstage): Home/New, Open, Info, Save As, Print, Export, Options, About.

use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::theme::{APP_COLOR, Tokens, medium, regular, semibold};
use crate::{WordApp, icons};

const PAGES: [(&str, &str); 9] = [
    ("home", "Home"),
    ("new", "New"),
    ("open", "Open"),
    ("info", "Info"),
    ("save", "Save"),
    ("saveAs", "Save As"),
    ("print", "Print"),
    ("export", "Export"),
    ("options", "Options"),
];

pub fn show(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let mac = app.integrated_titlebar;
    egui::Panel::left("backstage_nav")
        .exact_size(200.0)
        .frame(egui::Frame::NONE.fill(APP_COLOR).inner_margin(egui::Margin { left: 0, right: 0, top: if mac { 0 } else { 12 }, bottom: 12 }))
        .show(ui, |ui| {
            let back = if mac {
                // macOS (#222): the Backstage covers the title bar, so its top band is the title
                // bar: the back button sits beside the traffic lights, centred with them and as far
                // from them as the title bar's own controls, and the rest of the band drags the window.
                let (band, drag) = ui.allocate_exact_size(vec2(200.0, crate::chrome::MAC_TITLE_BAR), Sense::click_and_drag());
                if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                // The chevron's stroke starts at the content edge.
                let c = pos2(band.min.x + crate::chrome::MAC_CONTENT_LEFT + 3.0, band.center().y);
                let resp = ui.interact(Rect::from_center_size(c, vec2(22.0, band.height())), ui.id().with("backstage_back"), Sense::click());
                icons::paint(ui.painter(), Rect::from_center_size(c, vec2(18.0, 18.0)), "chevronLeft", egui::Color32::WHITE, egui::Color32::WHITE);
                resp
            } else {
                let (r, resp) = ui.allocate_exact_size(vec2(200.0, 40.0), Sense::click());
                icons::paint(
                    ui.painter(),
                    Rect::from_center_size(pos2(r.min.x + 26.0, r.center().y), vec2(18.0, 18.0)),
                    "chevronLeft",
                    egui::Color32::WHITE,
                    egui::Color32::WHITE,
                );
                resp
            };
            if back.clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.ui.backstage = false;
                app.canvas.want_focus = true;
            }
            ui.add_space(if mac { 12.0 } else { 6.0 });
            for (id, label) in PAGES {
                let (r, resp) = ui.allocate_exact_size(vec2(200.0, 38.0), Sense::click());
                let active = app.ui.backstage_page == id;
                if active {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(46));
                } else if resp.hovered() {
                    ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(26));
                }
                ui.painter().text(
                    pos2(r.min.x + 22.0, r.center().y),
                    Align2::LEFT_CENTER,
                    tl!(label),
                    if active { semibold(14.0) } else { medium(14.0) },
                    egui::Color32::WHITE,
                );
                if resp.clicked() {
                    match id {
                        "save" => {
                            let _ = app.run("file.save", json!({}));
                        }
                        "saveAs" => {
                            app.save_as_dialog();
                        }
                        "open" => {
                            app.ui.backstage_page = id.into();
                        }
                        _ => app.ui.backstage_page = id.into(),
                    }
                }
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                for (label, id) in [("About", "about"), ("Discord community", "discord")] {
                    let (r, resp) = ui.allocate_exact_size(vec2(200.0, 32.0), Sense::click());
                    if resp.hovered() {
                        ui.painter().rect_filled(r, 0.0, egui::Color32::from_white_alpha(26));
                    }
                    ui.painter().text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, tl!(label), regular(13.0), egui::Color32::WHITE);
                    if resp.clicked() {
                        if id == "discord" {
                            let _ = app.run("ui.discord", json!({}));
                        } else {
                            app.dialog = crate::dialogs::Dialog::open("about", app);
                        }
                    }
                }
            });
        });
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(t.ribbon).inner_margin(egui::Margin::symmetric(40, 30))).show(ui, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| match app.ui.backstage_page.as_str() {
            "home" => home_page(app, ui),
            "new" => new_page(app, ui),
            "open" => open_page(app, ui),
            "info" => info_page(app, ui),
            "print" => print_page(app, ui),
            "export" => export_page(app, ui),
            "options" => options_page(app, ui),
            _ => home_page(app, ui),
        });
    });
}

fn heading(ui: &mut Ui, s: &str) {
    ui.label(egui::RichText::new(tl!(s)).font(semibold(26.0)));
    ui.add_space(16.0);
}

fn template_tile(ui: &mut Ui, app: &mut WordApp, label: &str, template: &str) {
    let t = Tokens::get(ui.ctx());
    ui.vertical(|ui| {
        // The label sits right under the thumbnail whatever row gap the gallery uses.
        ui.spacing_mut().item_spacing.y = 0.0;
        let (r, resp) = ui.allocate_exact_size(vec2(150.0, 194.0), Sense::click());
        ui.painter().rect(
            r,
            2.0,
            egui::Color32::WHITE,
            Stroke::new(if resp.hovered() { 2.0 } else { 1.0 }, if resp.hovered() { t.accent } else { t.border_strong }),
            egui::StrokeKind::Inside,
        );
        let ppp = ui.ctx().pixels_per_point();
        let key = format!("tpl:{template}:{ppp}");
        let tex = ui.ctx().data(|d| d.get_temp::<egui::TextureHandle>(egui::Id::new(&key))).or_else(|| {
            let mut s = wordcraft_engine::Session::new(wordcraft_doc::Document::new());
            let _ = s.run("file.new", &json!({"template": template}));
            let l = s.export_layout();
            let page = l.pages.first()?;
            let img = wordcraft_render::render_page(&s.doc, page, 150.0 / page.w * ppp, &crate::canvas::screen_render_options());
            let px = img.to_straight();
            let h = ui.ctx().load_texture(
                &key,
                egui::ColorImage::from_rgba_unmultiplied([img.width as usize, img.height as usize], &px),
                egui::TextureOptions::LINEAR,
            );
            ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(&key), h.clone()));
            Some(h)
        });
        if let Some(h) = tex {
            ui.painter().image(h.id(), r.shrink(1.0), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        ui.label(egui::RichText::new(tl!(label)).font(regular(12.5)));
        if resp.clicked() {
            let _ = app.run("file.new", json!({"template": template}));
            app.ui.backstage = false;
            app.canvas.want_focus = true;
        }
    });
}

/// The built-in templates in gallery order: display label and the `file.new` template id.
const TEMPLATES: [(&str, &str); 5] =
    [("Blank document", "blank"), ("Studio handbook (sample)", "sample"), ("Letter", "letter"), ("Résumé", "resume"), ("Report", "report")];

/// How many templates Home shows before "More templates" (the rest are on New).
const HOME_TEMPLATES: usize = 4;

/// The templates whose English or translated label, or id, contains `query` (case-insensitive;
/// the id lets "resume" find "Résumé"); all of them for an empty query.
fn matching_templates(query: &str) -> Vec<(&'static str, &'static str)> {
    let q = query.trim().to_lowercase();
    TEMPLATES
        .into_iter()
        .filter(|(label, id)| q.is_empty() || id.contains(&q) || label.to_lowercase().contains(&q) || tl!(label).to_lowercase().contains(&q))
        .collect()
}

/// Home: a greeting, Blank plus a short row of templates with a link to New, then Recent.
fn home_page(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let hour = wordcraft_engine::cmd::now_iso().get(11..13).and_then(|h| h.parse::<u32>().ok()).unwrap_or(9);
    heading(
        ui,
        if hour < 12 {
            "Good morning"
        } else if hour < 18 {
            "Good afternoon"
        } else {
            "Good evening"
        },
    );
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(18.0, 0.0);
        for (label, template) in TEMPLATES.into_iter().take(HOME_TEMPLATES) {
            template_tile(ui, app, label, template);
        }
    });
    ui.add_space(8.0);
    let more = ui.add(egui::Label::new(egui::RichText::new(tl!("More templates →")).font(medium(13.5)).color(t.accent)).sense(Sense::click()));
    if more.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        app.ui.backstage_page = "new".into();
    }
    ui.add_space(24.0);
    open_list(app, ui);
}

/// New: the whole template gallery with a search box; no Recent list (that is on Home and Open).
fn new_page(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    heading(ui, "New");
    let id = egui::Id::new("backstage_template_search");
    let mut query = ui.ctx().data(|d| d.get_temp::<String>(id)).unwrap_or_default();
    let search = ui.add(egui::TextEdit::singleline(&mut query).hint_text(tl!("Search for templates")).desired_width(360.0));
    if search.changed() {
        ui.ctx().data_mut(|d| d.insert_temp(id, query.clone()));
    }
    ui.add_space(18.0);
    let found = matching_templates(&query);
    if found.is_empty() {
        ui.label(egui::RichText::new(tl!("No templates match your search.")).color(t.text_dim));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(18.0, 18.0);
        for (label, template) in found {
            template_tile(ui, app, label, template);
        }
    });
}

fn open_list(app: &mut WordApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.label(egui::RichText::new(tl!("Recent")).font(semibold(16.0)));
    ui.add_space(6.0);
    if app.ui.recent.is_empty() {
        ui.label(egui::RichText::new(tl!("Documents you open will show up here.")).color(t.text_dim));
    }
    for p in app.ui.recent.clone() {
        let name = std::path::Path::new(&p).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(p.clone());
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width().min(700.0), 40.0), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 4.0, t.hover);
        }
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 18.0, r.center().y), vec2(22.0, 22.0)), "printLayout", t.icon, t.accent);
        ui.painter().text(pos2(r.min.x + 40.0, r.min.y + 13.0), Align2::LEFT_CENTER, &name, medium(13.0), t.text);
        ui.painter().text(pos2(r.min.x + 40.0, r.min.y + 29.0), Align2::LEFT_CENTER, &p, regular(11.0), t.text_dim);
        if resp.clicked() {
            let _ = app.run("file.open", json!({"path": p}));
            app.ui.backstage = false;
            app.canvas.want_focus = true;
        }
    }
}

fn open_page(app: &mut WordApp, ui: &mut Ui) {
    heading(ui, "Open");
    if ui.button(egui::RichText::new(tl!("Browse…")).font(medium(14.0))).clicked() {
        let _ = app.run("ui.openFileDialog", json!({}));
    }
    ui.add_space(18.0);
    open_list(app, ui);
}

fn info_page(app: &mut WordApp, ui: &mut Ui) {
    heading(ui, "Info");
    let info = app.session.run("file.info", &json!({})).unwrap_or_default();
    ui.columns(2, |cols| {
        let ui = &mut cols[0];
        ui.label(egui::RichText::new(tl!("Properties")).font(semibold(15.0)));
        // The fields edit a copy of the document's properties that is read afresh every frame,
        // so each change goes into the document as it is typed (#260): waiting for the field to
        // lose focus dropped the keystrokes, and leaving File with Escape never loses it at all.
        // The keystrokes of one visit to a field are a single undo step.
        let mut props = app.session.doc.core.clone();
        let mut edited = None;
        let mut focused = None;
        egui::Grid::new("props").num_columns(2).spacing(vec2(12.0, 6.0)).show(ui, |ui| {
            for (l, v) in [("Title", &mut props.title), ("Subject", &mut props.subject), ("Author", &mut props.creator), ("Keywords", &mut props.keywords), ("Category", &mut props.category)] {
                let label = ui.label(tl!(l));
                let r = ui.add(egui::TextEdit::singleline(v).id_salt(("info-prop", l))).labelled_by(label.id);
                if r.changed() {
                    edited = Some(l);
                }
                if r.has_focus() {
                    focused = Some(l);
                }
                ui.end_row();
            }
        });
        if let Some(field) = edited {
            if app.info_editing == Some((field, app.session.rev())) {
                app.session.join_next_undo();
            }
            let ok = app.run("file.properties", json!({"title": props.title, "subject": props.subject, "author": props.creator, "keywords": props.keywords, "category": props.category})).is_ok();
            app.info_editing = ok.then(|| (field, app.session.rev()));
        }
        if focused != app.info_editing.map(|(f, _)| f) {
            app.info_editing = None;
        }
        // Protect Document › Encrypt with Password (#55).
        ui.add_space(18.0);
        ui.label(egui::RichText::new(tl!("Protect Document")).font(semibold(15.0)));
        let encrypted = info.get("encrypted").and_then(|v| v.as_bool()).unwrap_or(false);
        ui.label(if encrypted {
            tl!("A password is required to open this document.")
        } else {
            tl!("Anyone can open this document. Encrypt it with a password to keep it private.")
        });
        ui.horizontal(|ui| {
            let label = if encrypted { tl!("Change Password…") } else { tl!("Encrypt with Password…") };
            if ui.button(egui::RichText::new(label).font(medium(13.5))).clicked() {
                let _ = app.run("file.encrypt", json!({}));
            }
            if encrypted && ui.button(egui::RichText::new(tl!("Remove Password")).font(medium(13.5))).clicked() {
                let _ = app.run("file.encrypt", json!({"password": null}));
            }
        });
        let ui = &mut cols[1];
        ui.label(egui::RichText::new(tl!("Statistics")).font(semibold(15.0)));
        for (l, k) in [("Pages", "pages"), ("Words", "words"), ("Paragraphs", "paragraphs"), ("Sections", "sections"), ("Comments", "comments")] {
            ui.label(format!("{}: {}", tl!(l), info.get(k).map(|v| v.to_string()).unwrap_or_default()));
        }
        let path = info.get("path").and_then(|v| v.as_str()).unwrap_or(tl!("Not saved yet"));
        ui.label(crate::i18n::fmt(tl!("Location: {path}"), &[("path", path)]));
    });
}

fn export_page(app: &mut WordApp, ui: &mut Ui) {
    heading(ui, "Export");
    ui.label(tl!("Save a copy in another format."));
    ui.add_space(12.0);
    for (label, ext) in [
        ("PDF document (*.pdf)", "pdf"),
        ("Word document (*.docx)", "docx"),
        ("OpenDocument Text (*.odt)", "odt"),
        ("Rich Text Format (*.rtf)", "rtf"),
        ("Web page (*.html)", "html"),
        ("Markdown (*.md)", "md"),
        ("LaTeX (*.tex)", "tex"),
        ("Plain text (*.txt)", "txt"),
        ("Page image (*.png)", "png"),
    ] {
        if ui.add(egui::Button::new(egui::RichText::new(tl!(label)).font(medium(13.5))).min_size(vec2(320.0, 34.0))).clicked() {
            app.export_dialog(ext);
        }
        ui.add_space(4.0);
    }
}

fn print_page(app: &mut WordApp, ui: &mut Ui) {
    use crate::printing::{Duplex, NativePrintOptions, parse_page_range};
    use std::collections::BTreeSet;

    heading(ui, "Print");
    let layout = app.session.export_layout();
    let page_count = layout.pages.len();
    if let Some(backend) = app.services.native_print.as_ref()
        && !app.print_preview.loaded
    {
        app.print_preview.loaded = true;
        match backend.printers() {
            Ok(printers) => {
                let selected = printers.iter().find(|p| p.is_default).or_else(|| printers.first()).map(|p| p.name.clone()).unwrap_or_default();
                app.print_preview.printers = printers;
                if !selected.is_empty() {
                    match backend.capabilities(&selected) {
                        Ok(caps) => app.print_preview.reset_if_printer_changed(&selected, caps),
                        Err(error) => app.print_preview.error = Some(error),
                    }
                }
            }
            Err(error) => app.print_preview.error = Some(error),
        }
    }

    let native = app.services.native_print.is_some();
    if native {
        if app.print_preview.printers.is_empty() {
            ui.label(tl!("No printers are available."));
        } else {
            let previous = app.print_preview.selected_printer.clone();
            ui.horizontal(|ui| {
                ui.label(tl!("Printer"));
                egui::ComboBox::from_id_salt("print_printer")
                    .selected_text(if previous.is_empty() { tl!("Select printer") } else { &previous })
                    .show_ui(ui, |ui| {
                        for printer in &app.print_preview.printers {
                            let label = if printer.is_default { format!("{} ({})", printer.name, tl!("Default")) } else { printer.name.clone() };
                            ui.selectable_value(&mut app.print_preview.selected_printer, printer.name.clone(), label);
                        }
                    });
            });
            if app.print_preview.selected_printer != previous
                && let Some(backend) = app.services.native_print.as_ref()
            {
                match backend.capabilities(&app.print_preview.selected_printer) {
                    Ok(caps) => app.print_preview.reset_if_printer_changed(&app.print_preview.selected_printer.clone(), caps),
                    Err(error) => app.print_preview.error = Some(error),
                }
            }
        }
    } else {
        ui.label(tl!("The system print dialog is unavailable. You can open a PDF copy to print from another viewer."));
    }

    ui.horizontal(|ui| {
        ui.label(tl!("Pages"));
        ui.add(egui::TextEdit::singleline(&mut app.print_preview.page_range).desired_width(150.0).hint_text(tl!("All pages")));
        if native {
            ui.label(tl!("Copies"));
            ui.add(egui::TextEdit::singleline(&mut app.print_preview.copies).desired_width(48.0));
            ui.checkbox(&mut app.print_preview.collated, tl!("Collate"));
        }
    });
    if native {
        ui.horizontal(|ui| {
            if !app.print_preview.capabilities.duplex.is_empty() {
                ui.label(tl!("Duplex"));
                egui::ComboBox::from_id_salt("print_duplex")
                    .selected_text(match app.print_preview.duplex {
                        Duplex::Simplex => tl!("One-sided"),
                        Duplex::LongEdge => tl!("Two-sided, long edge"),
                        Duplex::ShortEdge => tl!("Two-sided, short edge"),
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.print_preview.duplex, Duplex::Simplex, tl!("One-sided"));
                        if app.print_preview.capabilities.duplex.contains(&Duplex::LongEdge) {
                            ui.selectable_value(&mut app.print_preview.duplex, Duplex::LongEdge, tl!("Two-sided, long edge"));
                        }
                        if app.print_preview.capabilities.duplex.contains(&Duplex::ShortEdge) {
                            ui.selectable_value(&mut app.print_preview.duplex, Duplex::ShortEdge, tl!("Two-sided, short edge"));
                        }
                    });
            }
            if !app.print_preview.capabilities.media.is_empty() {
                ui.label(tl!("Paper size"));
                egui::ComboBox::from_id_salt("print_media")
                    .selected_text(if app.print_preview.media.is_empty() { tl!("Printer default") } else { &app.print_preview.media })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.print_preview.media, String::new(), tl!("Printer default"));
                        for media in &app.print_preview.capabilities.media {
                            ui.selectable_value(&mut app.print_preview.media, media.clone(), media);
                        }
                    });
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut app.print_preview.landscape, tl!("Landscape"));
            ui.checkbox(&mut app.print_preview.fit_to_page, tl!("Fit to page"));
        });
    }

    let pages = parse_page_range(&app.print_preview.page_range, page_count);
    let copies = if native { app.print_preview.copies.trim().parse::<u16>().ok().filter(|n| (1..=999).contains(n)) } else { Some(1) };
    let selected_pages = match &pages {
        Ok(pages) => pages.clone(),
        Err(error) => {
            ui.colored_label(egui::Color32::from_rgb(180, 48, 48), tl!(error.as_str()));
            Vec::new()
        }
    };
    if native && !app.print_preview.copies.trim().is_empty() && copies.is_none() {
        ui.colored_label(egui::Color32::from_rgb(180, 48, 48), tl!("Copies must be between 1 and 999."));
    }

    if !layout.overflows.is_empty() {
        let pages_with_warnings: BTreeSet<usize> = layout.overflows.iter().map(|item| item.page).collect();
        ui.colored_label(
            egui::Color32::from_rgb(180, 76, 32),
            crate::i18n::fmt(tl!("Content exceeds page area on {count} page(s)."), &[("count", &pages_with_warnings.len().to_string())]),
        );
        for page in pages_with_warnings.iter().take(12) {
            ui.label(crate::i18n::fmt(tl!("Page {page} has content that may be clipped."), &[("page", &(page + 1).to_string())]));
        }
    }
    if let Some(error) = app.print_preview.error.as_deref() {
        ui.colored_label(egui::Color32::from_rgb(180, 48, 48), error);
    }
    if let Some(message) = app.print_preview.message.as_deref() {
        ui.label(message);
    }
    if let Some(job_id) = app.print_preview.last_job.clone()
        && ui.button(tl!("Cancel print job")).clicked()
        && let Some(backend) = app.services.native_print.as_ref()
    {
        match backend.cancel(&job_id) {
            Ok(()) => {
                app.print_preview.last_job = None;
                app.print_preview.message = Some(tl!("Print job canceled.").to_string());
                app.print_preview.error = None;
            }
            Err(error) => app.print_preview.error = Some(error),
        }
    }

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let selected_native = native && !app.print_preview.selected_printer.is_empty();
        let can_submit = selected_native || app.services.print.is_some();
        let enabled = can_submit && !selected_pages.is_empty() && copies.is_some();
        let button = ui.add_enabled(enabled, egui::Button::new(egui::RichText::new(tl!("Print")).font(medium(14.0))).min_size(vec2(150.0, 34.0)));
        if button.clicked() {
            let pdf = wordcraft_pdf::export_layout(
                &app.session.doc,
                &layout,
                &wordcraft_pdf::PdfOptions { pages: Some(selected_pages.clone()), ..Default::default() },
            );
            match pdf {
                Ok(bytes) => {
                    if native
                        && !app.print_preview.selected_printer.is_empty()
                        && let Some(backend) = app.services.native_print.as_ref()
                    {
                        let options = NativePrintOptions {
                            printer: app.print_preview.selected_printer.clone(),
                            copies: copies.unwrap_or(1),
                            collated: app.print_preview.collated,
                            duplex: app.print_preview.duplex,
                            media: (!app.print_preview.media.is_empty()).then(|| app.print_preview.media.clone()),
                            landscape: app.print_preview.landscape,
                            fit_to_page: app.print_preview.fit_to_page,
                        };
                        match backend.submit(&bytes, &options) {
                            Ok(job) => {
                                app.print_preview.error = None;
                                app.print_preview.message = Some(job.receipt);
                                app.print_preview.last_job = job.id;
                            }
                            Err(error) => app.print_preview.error = Some(error),
                        }
                    } else if let Some(print) = app.services.print.as_ref() {
                        match print(&bytes) {
                            Ok(()) => app.print_preview.message = Some(tl!("Print dialog opened.").to_string()),
                            Err(error) => app.print_preview.error = Some(error),
                        }
                    }
                }
                Err(error) => app.print_preview.error = Some(error.to_string()),
            }
        }
        ui.label(crate::i18n::fmt(
            tl!("{selected} of {pages} pages"),
            &[("selected", &selected_pages.len().to_string()), ("pages", &page_count.to_string())],
        ));
    });

    ui.add_space(10.0);
    ui.label(egui::RichText::new(tl!("Print preview")).font(semibold(15.0)));
    egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
        ui.horizontal(|ui| {
            for page_index in &selected_pages {
                let Some(page) = layout.pages.get(*page_index) else { continue };
                let key = format!("print:{}:{}:{}", app.session.document_id(), app.session.rev(), page_index);
                let width = 148.0_f32;
                let height = width * page.h / page.w.max(1.0);
                let scale = width / page.w.max(1.0) * ui.ctx().pixels_per_point();
                let document = &app.session.doc;
                let texture = app.previews.get_or(ui.ctx(), &key, || {
                    let image = wordcraft_render::render_page(document, page, scale, &Default::default());
                    let pixels = image.to_straight();
                    Some(egui::ColorImage::from_rgba_unmultiplied([image.width as usize, image.height as usize], &pixels))
                });
                ui.vertical(|ui| {
                    ui.label(crate::i18n::fmt(tl!("Page {page}"), &[("page", &(page_index + 1).to_string())]));
                    if let Some(texture) = texture {
                        ui.image((texture.id(), vec2(width, height)));
                    } else {
                        ui.ctx().request_repaint();
                        ui.allocate_exact_size(vec2(width, height), egui::Sense::hover());
                    }
                });
            }
        });
    });
    ui.add_space(16.0);
    ui.separator();
    ui.add_space(12.0);
    ui.label(tl!("Or save a copy in another format:"));
    for (label, ext) in [("PDF document (*.pdf)", "pdf"), ("Page image (*.png)", "png")] {
        if ui.add(egui::Button::new(egui::RichText::new(tl!(label)).font(medium(13.5))).min_size(vec2(320.0, 34.0))).clicked() {
            app.export_dialog(ext);
        }
        ui.add_space(4.0);
    }
}

fn options_page(app: &mut WordApp, ui: &mut Ui) {
    heading(ui, "Options");
    ui.label(egui::RichText::new(tl!("General")).font(semibold(15.0)));
    language_picker(app, ui);
    ui.horizontal(|ui| {
        ui.label(tl!("User name:"));
        let mut n = app.session.author.clone();
        if ui.text_edit_singleline(&mut n).changed() {
            app.session.author = n;
        }
    });
    theme_picker(app, ui);
    // The browser can't write to the user's files, so AutoSave can't be turned on there (#176).
    let browser = app.autosave_block() == Some(crate::AutoSaveBlock::Browser);
    let mut autosave = app.autosave && !browser;
    let r = ui.add_enabled(!browser, egui::Checkbox::new(&mut autosave, tl!("AutoSave documents you have saved in WordCraft")));
    if browser {
        r.on_disabled_hover_text(crate::AutoSaveBlock::Browser.reason());
    } else if r.changed() {
        app.autosave = autosave;
        app.session.autosave = autosave;
    }
    let mut dark_page = app.session.view.dark_mode;
    if ui
        .checkbox(&mut dark_page, tl!("Dark page (white text on black)"))
        .on_hover_text(tl!("Show documents with their colours inverted, like View › Switch Modes. Saving, printing and PDFs are unchanged."))
        .changed()
    {
        let _ = app.run("view.darkMode", json!({"value": dark_page}));
    }
    ui.checkbox(&mut app.ui.show_discord, tl!("Show the community button in the title bar"));
    ui.add_space(10.0);
    ui.label(egui::RichText::new(tl!("Display")).font(semibold(15.0)));
    let mut marks = app.session.view.marks;
    if ui.checkbox(&mut marks, tl!("Show all formatting marks")).changed() {
        let _ = app.run("view.marks", json!({"value": marks}));
    }
    let mut ruler = app.session.view.ruler;
    if ui.checkbox(&mut ruler, tl!("Show rulers")).changed() {
        let _ = app.run("view.ruler", json!({"value": ruler}));
    }
    ui.add_space(10.0);
    ui.label(egui::RichText::new(tl!("Agents")).font(semibold(15.0)));
    ui.label(tl!("Every command is available to scripts and AI agents: run `wordcraft-cli mcp` for an MCP server, or start the app with `--control <port>` for the JSON control channel."));
}

/// File ▸ Options ▸ Interface theme: Light, Dark, or follow the system's appearance (#115).
fn theme_picker(app: &mut WordApp, ui: &mut Ui) {
    use crate::theme::Appearance;
    ui.horizontal(|ui| {
        ui.label(tl!("Interface theme:"));
        egui::ComboBox::from_id_salt("interface_theme").selected_text(tl!(app.ui.theme.label())).width(220.0).show_ui(ui, |ui| {
            for a in Appearance::ALL {
                if ui.selectable_label(app.ui.theme == a, tl!(a.label())).clicked() {
                    let _ = app.run("ui.theme", json!({"value": a.code()}));
                }
            }
        });
    });
}

/// File ▸ Options ▸ Interface language: follow the system (the default) or pick one (#8).
fn language_picker(app: &mut WordApp, ui: &mut Ui) {
    use crate::i18n::{AUTO, Lang};
    // The language names are in their own scripts: fonts for them load from the next frame.
    app.want_system_cjk = true;
    let system = crate::i18n::system_lang();
    let auto_label = crate::i18n::fmt(tl!("Automatic ({language})"), &[("language", system.name())]);
    let current = if app.ui.language == AUTO { auto_label.clone() } else { Lang::from_pref(&app.ui.language).name().to_string() };
    ui.horizontal(|ui| {
        ui.label(tl!("Interface language:"));
        egui::ComboBox::from_id_salt("interface_language").selected_text(current).width(220.0).show_ui(ui, |ui| {
            if ui.selectable_label(app.ui.language == AUTO, auto_label.as_str()).clicked() {
                let _ = app.run("ui.language", json!({"value": AUTO}));
            }
            for lang in Lang::all() {
                if ui.selectable_label(app.ui.language == lang.code(), lang.name()).clicked() {
                    let _ = app.run("ui.language", json!({"value": lang.code()}));
                }
            }
        });
    });
    ui.label(egui::RichText::new(tl!("Automatic follows your system's language. Menus and commands change; your documents don't.")).small().weak());
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use serde_json::json;

    use super::matching_templates;
    use crate::{Services, WordApp};

    #[test]
    fn template_search_matches_labels_case_insensitively() {
        let ids = |q: &str| matching_templates(q).into_iter().map(|(_, id)| id).collect::<Vec<_>>();
        assert_eq!(ids(""), ["blank", "sample", "letter", "resume", "report"]);
        assert_eq!(ids("  LETTER "), ["letter"]);
        assert_eq!(ids("re"), ["resume", "report"]);
        assert_eq!(ids("résumé"), ["resume"]);
        assert!(ids("no such template").is_empty());
    }

    /// Home and New are different pages (#141): Home has the short template row, the link to New
    /// and Recent; New has every template and no Recent list.
    #[test]
    fn home_and_new_show_different_sections() {
        let mut a = WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::new()), Services::default());
        a.run("ui.backstage", json!({"value": true, "page": "home"})).unwrap();
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            a,
        );
        h.run_steps(4);
        assert!(h.query_by_label("Recent").is_some(), "Home lists recent documents");
        assert!(h.query_by_label("Letter").is_some(), "Home shows the first templates");
        assert!(h.query_by_label("Report").is_none(), "Home shows only a short row of templates");
        h.get_by_label("More templates →").click();
        h.run_steps(4);
        assert_eq!(h.state().ui.backstage_page, "new", "the link opens New");
        assert!(h.query_by_label("Report").is_some(), "New shows every template");
        assert!(h.query_by_label("Recent").is_none(), "New has no Recent list");
    }

    /// #260: File › Info's Title and Author fields dropped every keystroke (they edited a copy
    /// re-read each frame and only wrote it back when the field lost focus), so a save had no
    /// title or author. Typing now lands in the document as it happens, one undo step per field,
    /// and the browser's Save downloads it with the save stamps advanced (#262).
    #[test]
    fn info_fields_keep_typed_properties_through_a_web_save() {
        use egui::accesskit::Role;
        let got: std::rc::Rc<std::cell::RefCell<Vec<(String, Vec<u8>)>>> = Default::default();
        let sink = got.clone();
        let services = Services {
            download: Some(Box::new(move |n: &str, b: &[u8]| {
                sink.borrow_mut().push((n.to_string(), b.to_vec()));
                Ok(())
            })),
            ..Default::default()
        };
        let mut a = WordApp::new(wordcraft_engine::Session::new(wordcraft_doc::Document::from_text("XYZ")), services);
        a.session.author = "GAMMA".into();
        a.run("ui.backstage", json!({"value": true, "page": "info"})).unwrap();
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            a,
        );
        h.run_steps(4);
        for (field, text) in [("Title", "ALPHA"), ("Author", "BETA")] {
            h.get_by_role_and_label(Role::TextInput, field).click();
            h.run_steps(2);
            for c in text.chars() {
                h.get_by_role_and_label(Role::TextInput, field).type_text(&c.to_string());
                h.run_steps(2);
            }
            assert_eq!(h.get_by_role_and_label(Role::TextInput, field).value().as_deref(), Some(text), "{field} shows what was typed");
        }
        let core = h.state().session.doc.core.clone();
        assert_eq!((core.title.as_str(), core.creator.as_str()), ("ALPHA", "BETA"));
        assert!(h.state().session.dirty, "the property edits are unsaved changes");

        // Save (web: a download) writes them into docProps/core.xml, with fresh save stamps.
        let a = h.state_mut();
        let created = a.session.doc.core.created.clone();
        a.run("file.save", json!({})).unwrap();
        a.run("file.save", json!({})).unwrap();
        let (name, bytes) = got.borrow_mut().pop().unwrap();
        let back = wordcraft_engine::io::open_bytes(&name, &bytes).unwrap();
        assert_eq!((back.core.title.as_str(), back.core.creator.as_str()), ("ALPHA", "BETA"));
        assert_eq!(back.core.last_modified_by, "GAMMA");
        assert_eq!(back.core.revision, core.revision + 2, "each save advances the revision");
        assert!(created.is_empty() && !back.core.created.is_empty(), "the first save sets the creation time");
        assert!(!back.core.modified.is_empty());
        assert!(!a.session.dirty);

        // Each field's typing is one undo step.
        a.session.run("edit.undo", &json!({})).unwrap();
        assert_eq!((a.session.doc.core.title.as_str(), a.session.doc.core.creator.as_str()), ("ALPHA", ""));
        a.session.run("edit.undo", &json!({})).unwrap();
        assert_eq!(a.session.doc.core.title, "");
    }
}
