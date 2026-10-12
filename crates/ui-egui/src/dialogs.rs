//! Dialogs: Font, Paragraph, Find & Replace, Go To, Insert Table, Page Setup, Link, Bookmark,
//! Word Count, Zoom, Watermark, New/Modify Style, Manage Styles, New/Modify Table Style, Table
//! Properties, Enclose Characters, Command search, Icons, Paste Special, About, Save Changes, the password to open a
//! document and Encrypt with Password, and the mail-merge Recipient List, Insert Merge Field, Find
//! Recipient, merge rules, Match Fields, Check for Errors and Select Table (a workbook's sheet) (Customize Keyboard is in
//! `dialogs_keyboard`, Customize Ribbon in `dialogs_ribbon`). Every dialog ends by running a
//! command (or shows one's result), so agents get the same result without the dialog.

use egui::{Sense, Ui, vec2};
use serde::Serialize;
use serde_json::{Value, json};

use wordcraft_engine::Password;

use crate::WordApp;
use crate::theme::{Tokens, semibold};

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "dialog", rename_all = "camelCase")]
pub enum Dialog {
    Recovery {
        state: crate::recovery::DialogState,
    },
    Mendeley {
        ris: String,
        query: String,
        error: String,
    },
    Sources {
        form: Box<SourcesForm>,
    },
    ChartData {
        form: Box<ChartDataForm>,
    },
    Recipients {
        csv: String,
        error: String,
    },
    Font {
        font: String,
        size: String,
        bold: bool,
        italic: bool,
        underline: bool,
        strike: bool,
        sup: bool,
        sub: bool,
        small_caps: bool,
        caps: bool,
        hidden: bool,
        color: String,
        spacing: f32,
        /// The properties the fields stood for when the dialog opened: OK sends only what the
        /// user changed, so formatting the dialog can't show (a double underline) or left
        /// alone stays as it is.
        #[serde(skip)]
        start: serde_json::Map<String, Value>,
    },
    Paragraph {
        /// Right-to-left reading order (Direction in Word's Paragraph dialog).
        rtl: bool,
        /// Alignment and indents as seen on the page: `left` is the left side whatever the
        /// direction (the model stores them from the paragraph's start edge).
        align: String,
        left: f32,
        right: f32,
        first: f32,
        before: f32,
        after: f32,
        line: f32,
        keep_next: bool,
        keep_lines: bool,
        page_break: bool,
        widow: bool,
        /// Page: 0 Indents and Spacing, 1 Line and Page Breaks, 2 Asian Typography.
        tab: u8,
        /// Asian Typography as the model has it (`para.asianTypography`): kinsoku, word wrap,
        /// hanging punctuation, compress punctuation at line start, space between Asian and
        /// Latin text, space between Asian text and numbers.
        asian: [bool; 6],
        /// `asian` when the dialog opened: OK sets only the flags that changed.
        #[serde(skip)]
        asian_was: [bool; 6],
        /// Mirror indents, don't add space between paragraphs of the same style, suppress line
        /// numbers, don't hyphenate (#512).
        opts: [bool; 4],
        /// `opts` when the dialog opened: OK sets only the ones that changed.
        #[serde(skip)]
        opts_was: [bool; 4],
    },
    /// Tabs (#320).
    Tabs {
        form: Box<crate::dialogs_para::TabsForm>,
    },
    /// Borders and Shading (#320).
    Borders {
        form: Box<crate::dialogs_para::BordersForm>,
    },
    Find {
        query: String,
        replace: String,
        match_case: bool,
        whole_word: bool,
        regex: bool,
        replace_mode: bool,
        message: String,
    },
    Goto {
        page: String,
    },
    InsertTable {
        rows: u32,
        cols: u32,
    },
    PageSetup {
        top: f32,
        bottom: f32,
        left: f32,
        right: f32,
        landscape: bool,
        /// Document grid: kind (0 none, 1 lines, 2 lines and characters, 3 snap to characters),
        /// lines per page and characters per line; `grid_was` as opened (applied only if changed).
        grid: (u8, u32, u32),
        grid_was: (u8, u32, u32),
        /// Section direction: right to left (`layout.sectionRtl`).
        rtl: bool,
    },
    Link {
        url: String,
        text: String,
    },
    Bookmark {
        name: String,
    },
    WordCount {
        stats: Value,
    },
    Zoom {
        percent: f32,
    },
    Watermark {
        text: String,
        diagonal: bool,
    },
    /// Home › Font › Phonetic Guide: ruby text over the selected text (`format.phonetic`).
    Phonetic {
        base: String,
        ruby: String,
        /// OOXML alignment name (`center`, `distributeLetter`…).
        align: String,
        size: f32,
        offset: f32,
        /// The selection already has a phonetic guide (Remove is offered).
        existing: bool,
    },
    NewStyle {
        name: String,
        based_on: String,
        /// Opened from Manage Styles, which comes back when this closes.
        #[serde(skip)]
        back: bool,
    },
    ModifyStyle {
        #[serde(skip)]
        back: bool,
        /// The dialog that comes back when this closes (Table of Contents › Modify…).
        #[serde(skip)]
        resume: Option<Box<Dialog>>,
        id: String,
        name: String,
        font: String,
        size: f32,
        bold: bool,
        italic: bool,
        color: String,
        before: f32,
        after: f32,
    },
    /// Manage Styles: every style with its preview and description; modify, create, delete and
    /// show or hide them.
    ManageStyles {
        alphabetical: bool,
        selected: String,
    },
    /// Table Design › New / Modify Table Style: one formatting region at a time.
    TableStyle {
        /// The style being modified; `None` creates one.
        id: Option<String>,
        name: String,
        /// Style id.
        based_on: String,
        /// Index into `regions` (see [`REGION_LABELS`]).
        region: usize,
        regions: Box<[TableRegion; 7]>,
        /// What the regions showed when the dialog opened (or the base style changed): only
        /// changes are sent, so everything else stays inherited.
        #[serde(skip)]
        basis: Box<[TableRegion; 7]>,
    },
    Commands {
        query: String,
    },
    /// Columns, Symbol and Field ([`crate::dialogs_insert`], #321).
    Insert(Box<crate::dialogs_insert::InsertDialog>),
    /// Caption, Index, Mark Index Entry, Table of Contents, Source Manager and Create / Edit
    /// Source ([`crate::dialogs_refs`], #402).
    Refs(Box<crate::dialogs_refs::RefsDialog>),
    /// Insert › Icons: the icon library, searchable and filtered by category (empty for all).
    Icons {
        query: String,
        category: String,
        selected: String,
    },
    /// Paste Special: the clipboard's formats and the chosen one. The clipboard payload stays out
    /// of the serialized dialog state (it can be megabytes).
    PasteSpecial {
        formats: Vec<String>,
        choice: usize,
        #[serde(skip)]
        payload: Value,
    },
    /// Tabs: 0 About, 1 Contributors, 2 Models.
    About {
        tab: u8,
    },
    /// "Do you want to save changes?" before a user's New, Open, Close, Envelopes, Labels or
    /// Finish & Merge replaces the document (see [`WordApp::run`]); `then` runs once it is
    /// answered (`ui.saveChanges`), and only while the document it asked about
    /// (`Session::document_id`) is still the one open.
    SaveChanges {
        name: String,
        then: String,
        #[serde(skip)]
        params: Value,
        #[serde(skip)]
        document: u64,
    },
    /// Mailings › Select Recipients / Edit Recipient List: a table of recipients to type or
    /// edit (field names on top), or a file to load instead (#240).
    RecipientList {
        fields: Vec<String>,
        rows: Vec<Vec<String>>,
        message: String,
    },
    /// Mailings › Select Recipients › Use an Existing List… picked a workbook with several sheets
    /// (#334): which sheet holds the recipients, and whether its first row names the fields.
    /// OK runs `mailings.recipients` with `params` (the file) plus `sheet` and `headers`.
    SelectTable {
        sheets: Vec<String>,
        selected: usize,
        headers: bool,
        message: String,
        #[serde(skip)]
        params: Value,
    },
    /// Mailings › Insert Merge Field: one of the recipient list's fields (or a typed name).
    InsertMergeField {
        fields: Vec<String>,
        field: String,
    },
    /// A field's code: a new field (Ctrl+F9, `edit` false) or the selected field's code to edit
    /// (double-click a field showing its code, right-click › Edit Field…).
    FieldCode {
        code: String,
        edit: bool,
        message: String,
    },
    /// Mailings › Find Recipient.
    FindRecipient {
        text: String,
        message: String,
    },
    /// Mailings › Rules › If…Then…Else… (`IF`) or Skip Record If… (`SKIPIF`): the condition, and
    /// for If the text to insert either way.
    MergeRule {
        rule: String,
        fields: Vec<String>,
        field: String,
        value: String,
        then: String,
        els: String,
    },
    /// Mailings › Match Fields: which recipient-list field fills each address part.
    MatchFields {
        fields: Vec<String>,
        address: Value,
    },
    /// Mailings › Check for Errors: merge fields the recipient list doesn't have.
    CheckErrors {
        unknown: Vec<String>,
        records: u64,
    },
    /// Table Layout › Properties: the table, the caret's row, column and cell. Only what changed
    /// from `basis` (the values when it opened) is applied, as one `table.properties` step.
    TableProperties {
        form: Box<TableForm>,
        #[serde(skip)]
        basis: Box<TableForm>,
    },
    /// Opening a password-protected document (#55): the password, then `file.open` with
    /// `params` again. The password is never serialised or printed.
    Password {
        name: String,
        #[serde(skip)]
        params: Value,
        #[serde(skip)]
        password: Password,
        message: String,
    },
    /// File › Info › Protect Document › Encrypt with Password: a password typed twice
    /// (`file.encrypt`); left empty, it removes the password.
    EncryptPassword {
        #[serde(skip)]
        password: Password,
        #[serde(skip)]
        confirm: Password,
        message: String,
    },
    /// Home › Paragraph › Multilevel List › Define New Multilevel List (#328).
    DefineList {
        form: Box<crate::dialogs_lists::ListForm>,
    },
    /// Review › Tracking › Track Changes Options (#328).
    TrackOptions {
        form: Box<crate::dialogs_lists::TrackForm>,
    },
    /// Compare / Combine Documents and AutoCorrect Options ([`crate::dialogs_review`], #412).
    Review(Box<crate::dialogs_review::ReviewDialog>),
    /// Tools › Customize Keyboard (#368).
    CustomizeKeyboard {
        form: Box<crate::dialogs_keyboard::KeyboardForm>,
    },
    /// Tools › Customize Ribbon and the Quick Access Toolbar (#379).
    CustomizeRibbon {
        form: Box<crate::dialogs_ribbon::RibbonForm>,
    },
    /// Developer › Controls › Properties.
    Control {
        form: Box<crate::dialogs_controls::ControlForm>,
    },
    /// Home › Font › Enclose Characters: the style (`none` takes the enclosure away, `shrink`,
    /// `enlarge`), the text and the shape (`circle`, `square`, `triangle`, `diamond`).
    Enclose {
        style: String,
        text: String,
        shape: String,
    },
    /// References › Footnotes › Footnote and Endnote (#385).
    NoteOptions {
        form: Box<crate::dialogs_notes::NoteForm>,
    },
    /// View › Templates › Templates and Add-ins (#377).
    Templates {
        form: Box<crate::dialogs_templates::TemplatesForm>,
    },
    /// Templates and Add-ins › Organizer: copy, delete and rename styles (#377).
    Organizer {
        form: Box<crate::dialogs_templates::OrganizerForm>,
    },
    /// Line Numbers, Hyphenation, Language, Envelopes and Labels (#407).
    Layout(Box<crate::dialogs_layout::LayoutDialog>),
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ChartDataForm {
    document_id: u64,
    chart_part: String,
    chart_kind: String,
    legend_position: String,
    original_legend_position: Option<String>,
    chart_title: String,
    original_chart_title: Option<String>,
    horizontal_axis_title: String,
    original_horizontal_axis_title: Option<String>,
    vertical_axis_title: String,
    original_vertical_axis_title: Option<String>,
    horizontal_axis: Option<ChartAxisForm>,
    original_horizontal_axis: Option<Value>,
    vertical_axis: Option<ChartAxisForm>,
    original_vertical_axis: Option<Value>,
    #[serde(skip)]
    original_snapshot: Option<Value>,
    series: Vec<ChartSeriesForm>,
    #[serde(skip)]
    original: Vec<ChartSeriesForm>,
    #[serde(skip)]
    read_only_reason: String,
    #[serde(skip)]
    error: String,
}

#[derive(Clone, Debug, Default, Serialize)]
struct ChartAxisForm {
    role: String,
    editable: bool,
    number_format_editable: bool,
    diagnostic: Option<String>,
    minimum_manual: bool,
    minimum: String,
    original_minimum: Option<f64>,
    maximum_manual: bool,
    maximum: String,
    original_maximum: Option<f64>,
    number_format: String,
    original_number_format: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
struct ChartSeriesForm {
    name: String,
    categories: Vec<String>,
    x_values: Vec<String>,
    values: Vec<String>,
    color: [u8; 3],
    original_color: Option<[u8; 3]>,
    color_editable: bool,
    formatting_diagnostic: Option<String>,
    line_width: Option<f64>,
    original_line_width: Option<f64>,
    line_width_editable: bool,
    marker_symbol: String,
    original_marker_symbol: Option<String>,
    marker_symbol_editable: bool,
    marker_size: u8,
    original_marker_size: Option<u8>,
    marker_size_editable: bool,
    marker_color: [u8; 3],
    original_marker_color: Option<[u8; 3]>,
    marker_color_editable: bool,
    data_label_value: bool,
    original_data_label_value: Option<bool>,
    data_label_category: bool,
    original_data_label_category: Option<bool>,
    data_label_series: bool,
    original_data_label_series: Option<bool>,
    data_label_position: String,
    original_data_label_position: Option<String>,
    data_labels_editable: bool,
    data_labels_diagnostic: Option<String>,
    error_bars: Vec<ChartErrorBarForm>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
struct ChartErrorBarForm {
    direction: String,
    bar_type: Option<String>,
    value_type: Option<String>,
    renderable: bool,
    editable: bool,
    fixed_value: String,
    original_fixed_value: Option<f64>,
    positive: Vec<String>,
    original_positive: Vec<f64>,
    negative: Vec<String>,
    original_negative: Vec<f64>,
    diagnostic: Option<String>,
}

/// The Table Properties dialog's fields. Lengths are in the interface unit
/// ([`wordcraft_geom::Unit`], inches by default).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableForm {
    pub width_on: bool,
    pub width: f32,
    /// `left`, `center` or `right`.
    pub align: String,
    pub indent: f32,
    /// Table direction: right to left (`w:bidiVisual`).
    pub rtl: bool,
    pub row_height_on: bool,
    pub row_height: f32,
    pub row_exact: bool,
    pub allow_break: bool,
    pub header_row: bool,
    pub column_width: f32,
    pub cell_width_on: bool,
    pub cell_width: f32,
    /// `top`, `center` or `bottom`.
    pub valign: String,
}

impl TableForm {
    /// The caret's table, row, column and cell as the dialog shows them.
    pub fn read(app: &WordApp) -> Option<TableForm> {
        use wordcraft_doc::props::{Align, HeightRule, VAlign};
        let s = &app.session;
        let (tp, r, c) = s.sel.focus.path.cell()?;
        let t = s.doc.table(s.sel.focus.story, &tp)?;
        let k = crate::options::unit().pt_per_unit();
        let row = t.rows.get(r).map(|x| x.props.clone()).unwrap_or_default();
        let cell = t.rows.get(r).and_then(|x| x.cells.get(c)).map(|x| x.props.clone()).unwrap_or_default();
        let tw = wordcraft_engine::cmd::page::sect(s).text_width();
        let width = t.props.width_pct.filter(|p| *p > 0.0).map(|p| tw * p.min(100.0) / 100.0).or(t.props.width.filter(|w| *w > 0.0));
        let fin = |v: f32| if v.is_finite() { v } else { 0.0 };
        Some(TableForm {
            width_on: width.is_some(),
            width: fin(width.unwrap_or(tw)) / k,
            align: match t.props.align {
                Some(Align::Center) => "center",
                Some(Align::Right) => "right",
                _ => "left",
            }
            .into(),
            indent: fin(t.props.indent.unwrap_or(0.0)) / k,
            rtl: t.props.bidi_visual,
            row_height_on: row.height.is_some() && row.height_rule != HeightRule::Auto,
            row_height: fin(row.height.unwrap_or(18.0)) / k,
            row_exact: row.height_rule == HeightRule::Exact,
            allow_break: !row.cant_split,
            header_row: row.header,
            column_width: fin(t.grid.get(t.grid_col(r, c)).copied().unwrap_or(72.0)) / k,
            cell_width_on: cell.width.is_some(),
            cell_width: fin(cell.width.unwrap_or(72.0)) / k,
            valign: match cell.valign {
                VAlign::Center => "center",
                VAlign::Bottom => "bottom",
                _ => "top",
            }
            .into(),
        })
    }

    /// `table.properties` parameters for what differs from `basis` (empty when nothing does).
    pub fn changes(&self, basis: &TableForm) -> Value {
        let k = crate::options::unit().pt_per_unit();
        let moved = |a: f32, b: f32| (a - b).abs() > 1e-4;
        let mut v = serde_json::Map::new();
        if self.width_on != basis.width_on || (self.width_on && moved(self.width, basis.width)) {
            v.insert("width".into(), if self.width_on { json!(self.width * k) } else { Value::Null });
        }
        if self.align != basis.align {
            v.insert("align".into(), json!(self.align));
        }
        if moved(self.indent, basis.indent) {
            v.insert("indent".into(), json!(self.indent * k));
        }
        if self.rtl != basis.rtl {
            v.insert("rtl".into(), json!(self.rtl));
        }
        if self.row_height_on != basis.row_height_on
            || (self.row_height_on && (moved(self.row_height, basis.row_height) || self.row_exact != basis.row_exact))
        {
            v.insert("rowHeight".into(), if self.row_height_on { json!(self.row_height * k) } else { Value::Null });
            v.insert("rowHeightRule".into(), json!(if self.row_exact { "exact" } else { "atLeast" }));
        }
        if self.allow_break != basis.allow_break {
            v.insert("allowBreak".into(), json!(self.allow_break));
        }
        if self.header_row != basis.header_row {
            v.insert("headerRow".into(), json!(self.header_row));
        }
        if moved(self.column_width, basis.column_width) {
            v.insert("columnWidth".into(), json!(self.column_width * k));
        }
        if self.cell_width_on != basis.cell_width_on || (self.cell_width_on && moved(self.cell_width, basis.cell_width)) {
            v.insert("cellWidth".into(), if self.cell_width_on { json!(self.cell_width * k) } else { Value::Null });
        }
        if self.valign != basis.valign {
            v.insert("valign".into(), json!(self.valign));
        }
        Value::Object(v)
    }
}

/// The address parts Match Fields lists, with their keys in `mailings.matchFields`' `address`.
pub const ADDRESS_PARTS: [(&str, &str); 9] = [
    ("Courtesy Title", "title"),
    ("First Name", "firstName"),
    ("Last Name", "lastName"),
    ("Company", "company"),
    ("Address", "address"),
    ("City", "city"),
    ("State", "state"),
    ("Postal Code", "postal"),
    ("Country", "country"),
];

/// The `mailings.rules` parameters a Merge Rule dialog stands for, once it names a field.
pub fn rule_params(d: &Dialog) -> Option<Value> {
    let Dialog::MergeRule { rule, field, value, then, els, .. } = d else { return None };
    let field = field.trim();
    if field.is_empty() {
        return None;
    }
    Some(if rule == "SKIPIF" {
        json!({"rule": "SKIPIF", "field": field, "value": value})
    } else {
        json!({"rule": "IF", "field": field, "value": value, "then": then, "else": els})
    })
}

/// The fields a new recipient list starts with.
pub const NEW_LIST_FIELDS: [&str; 5] = ["First Name", "Last Name", "Address", "City", "Postal Code"];

/// A typed recipient list ready for `mailings.recipients`: field names trimmed, blank rows
/// dropped. An error says what to fix.
pub fn recipient_table(fields: &[String], rows: &[Vec<String>]) -> Result<Value, &'static str> {
    let fields: Vec<&str> = fields.iter().map(|f| f.trim()).collect();
    if fields.is_empty() {
        return Err("Add at least one field.");
    }
    if fields.iter().any(|f| f.is_empty()) {
        return Err("Every field needs a name.");
    }
    for (i, f) in fields.iter().enumerate() {
        if fields.iter().skip(i + 1).any(|g| g.eq_ignore_ascii_case(f)) {
            return Err("Field names must be different.");
        }
    }
    let rows: Vec<&Vec<String>> = rows.iter().filter(|r| r.iter().any(|c| !c.trim().is_empty())).collect();
    if rows.is_empty() {
        return Err("Type at least one recipient.");
    }
    Ok(json!({"fields": fields, "rows": rows}))
}

/// One table style region in the Table Style dialog: colours are hex, empty for none.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableRegion {
    pub borders: bool,
    pub fill: String,
    pub bold: bool,
    pub color: String,
}

/// The Table Style dialog's regions, in the order of `table_style::REGIONS` (the param names).
const REGION_LABELS: [&str; 7] = ["Whole Table", "Header Row", "Total Row", "First Column", "Last Column", "Banded Rows", "Banded Columns"];

/// The regions of table style `id` as resolved through its based-on chain.
fn table_regions(app: &WordApp, id: &str) -> Box<[TableRegion; 7]> {
    let Some(st) = app.session.doc.styles.table_style(id) else { return Default::default() };
    let p = &st.parts;
    let hex = |c: Option<wordcraft_doc::Rgb>| c.map(|c| c.hex()).unwrap_or_default();
    let text = |c: &wordcraft_doc::CharProps| match c.color {
        Some(wordcraft_doc::TextColor::Rgb(c)) => c.hex(),
        _ => String::new(),
    };
    let lines = |b: Option<wordcraft_doc::props::Borders>| b.is_some_and(|b| b.any_visible());
    // A region's text: the whole table's formatting with the region's over it.
    let region = |borders: Option<wordcraft_doc::props::Borders>, fill: Option<wordcraft_doc::Rgb>, chr: &wordcraft_doc::CharProps| {
        let mut c = st.chr.clone();
        c.overlay(chr);
        TableRegion { borders: lines(borders), fill: hex(fill), bold: c.bold.unwrap_or(false), color: text(&c) }
    };
    let none = wordcraft_doc::CharProps::default();
    Box::new([
        region(p.borders, p.fill, &none),
        region(p.header_borders, p.header_fill, &p.header_chr),
        region(p.total_borders, p.total_fill, &p.total_chr),
        region(p.first_col_borders, p.first_col_fill, &p.first_col_chr),
        region(p.last_col_borders, p.last_col_fill, &p.last_col_chr),
        region(p.band_borders, p.band_fill, &p.band_chr),
        region(p.col_band_borders, p.col_band_fill, &p.col_band_chr),
    ])
}

/// The params `table.newStyle` / `table.modifyStyle` take for the Table Style dialog's state:
/// only what changed from `basis`, so the rest stays inherited.
fn table_style_params(id: Option<&str>, name: &str, based_on: &str, regions: &[TableRegion; 7], basis: &[TableRegion; 7]) -> Value {
    let mut v = json!({"name": name.trim(), "basedOn": based_on});
    for ((key, r), b) in wordcraft_engine::cmd::table_style::REGIONS.into_iter().zip(regions.iter()).zip(basis.iter()) {
        let ch = region_changes(r, b);
        if ch.as_object().is_some_and(|o| !o.is_empty()) {
            v[key] = ch;
        }
    }
    if let Some(id) = id {
        v["style"] = json!(id);
    }
    v
}

/// A small sample table drawn with `style` (5 columns, a header, three body rows and a total
/// row), showing the regions `look` turns on: the Table Style dialog's live preview.
fn table_style_preview(ui: &mut Ui, style: Option<&wordcraft_doc::styles::TableStyleProps>, look: wordcraft_doc::props::TableLook) {
    use wordcraft_doc::props::{Border, Borders};
    const COLS: usize = 5;
    const ROWS: usize = 5;
    const TEXT: [[&str; COLS]; ROWS] = [
        ["", "Mon", "Tue", "Wed", "Sum"],
        ["North", "4", "7", "2", "13"],
        ["South", "6", "1", "5", "12"],
        ["West", "3", "8", "4", "15"],
        ["Total", "13", "16", "11", "40"],
    ];
    let (rect, _) = ui.allocate_exact_size(vec2(300.0, 120.0), Sense::hover());
    let painter = ui.painter_at(rect);
    // The page is white whatever the interface theme.
    painter.rect_filled(rect, 2.0, egui::Color32::WHITE);
    let t = Tokens::get(ui.ctx());
    painter.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    let Some(st) = style else { return };
    let p = &st.parts;
    let table = rect.shrink2(vec2(14.0, 10.0));
    let (cw, rh) = (table.width() / COLS as f32, table.height() / ROWS as f32);
    let rgb = |c: wordcraft_doc::Rgb| egui::Color32::from_rgb(c.0, c.1, c.2);
    let tb = p.borders.unwrap_or_default();
    for (ri, row) in TEXT.iter().enumerate() {
        let header = look.header_row && ri == 0;
        let total = look.total_row && ri + 1 == ROWS;
        let band = look.banded_rows
            && !header
            && (ri.saturating_sub(usize::from(look.header_row)) / p.band_size.unwrap_or(1).clamp(1, 1000) as usize).is_multiple_of(2);
        for (ci, txt) in row.iter().enumerate() {
            // Sample words in the interface language; numbers as they are.
            let txt = &if txt.chars().any(char::is_alphabetic) { tl!(txt) } else { *txt };
            let first = look.first_column && ci == 0;
            let last = look.last_column && ci + 1 == COLS;
            let col_band = look.banded_columns && !first && ci.saturating_sub(usize::from(look.first_column)).is_multiple_of(2);
            let cell = egui::Rect::from_min_size(table.min + vec2(ci as f32 * cw, ri as f32 * rh), vec2(cw, rh));
            // Regions from lowest to highest priority, as layout applies them.
            let regions = [
                (col_band, p.col_band_fill, &p.col_band_chr, p.col_band_borders),
                (band, p.band_fill, &p.band_chr, p.band_borders),
                (first, p.first_col_fill, &p.first_col_chr, p.first_col_borders),
                (last, p.last_col_fill, &p.last_col_chr, p.last_col_borders),
                (header, p.header_fill, &p.header_chr, p.header_borders),
                (total, p.total_fill, &p.total_chr, p.total_borders),
            ];
            let mut fill = p.fill;
            let mut chr = st.chr.clone();
            let mut b = Borders {
                top: if ri == 0 { tb.top } else { tb.between },
                bottom: if ri + 1 == ROWS { tb.bottom } else { tb.between },
                left: if ci == 0 { tb.left } else { tb.inside_v },
                right: if ci + 1 == COLS { tb.right } else { tb.inside_v },
                between: None,
                inside_v: None,
            };
            for (on, f, c, rb) in regions {
                if !on {
                    continue;
                }
                fill = f.or(fill);
                chr.overlay(c);
                if let Some(rb) = rb {
                    b.overlay(&Borders { between: None, inside_v: None, ..rb });
                }
            }
            if total && p.total_borders.is_none_or(|x| x.top.is_none()) {
                b.top = p.total_border_top.or(b.top);
            }
            if let Some(f) = fill {
                painter.rect_filled(cell, 0.0, rgb(f));
            }
            let edge = |from: egui::Pos2, to: egui::Pos2, e: Option<Border>| {
                if let Some(e) = e.filter(Border::is_visible) {
                    let color = e.color.map_or(egui::Color32::BLACK, rgb);
                    painter.line_segment([from, to], egui::Stroke::new(e.width.clamp(0.5, 3.0) * 1.3, color));
                }
            };
            edge(cell.left_top(), cell.right_top(), b.top);
            edge(cell.left_bottom(), cell.right_bottom(), b.bottom);
            edge(cell.left_top(), cell.left_bottom(), b.left);
            edge(cell.right_top(), cell.right_bottom(), b.right);
            let color = match chr.color {
                Some(wordcraft_doc::TextColor::Rgb(c)) => rgb(c),
                _ => egui::Color32::BLACK,
            };
            let font = egui::FontId::proportional(11.0);
            let pos = cell.left_center() + vec2(4.0, 0.0);
            painter.text(pos, egui::Align2::LEFT_CENTER, tl!(*txt), font.clone(), color);
            if chr.bold.unwrap_or(false) {
                // A second pass a hair to the right reads as bold at this size.
                painter.text(pos + vec2(0.6, 0.0), egui::Align2::LEFT_CENTER, tl!(*txt), font, color);
            }
        }
    }
}

/// The style of the table at the caret, when it is a table style.
fn current_table_style(app: &WordApp) -> Option<String> {
    let s = &app.session;
    let (tp, _, _) = s.sel.focus.path.cell()?;
    let id = s.doc.table(s.sel.focus.story, &tp)?.props.style.clone()?;
    s.doc.styles.get(&id).filter(|st| st.kind == wordcraft_doc::StyleKind::Table).map(|st| st.id.clone())
}

/// The params `table.newStyle` / `table.modifyStyle` need for what changed in one region.
fn region_changes(r: &TableRegion, basis: &TableRegion) -> Value {
    let color = |c: &str| if c.is_empty() { Value::Null } else { json!(c) };
    let mut v = json!({});
    if r.borders != basis.borders {
        v["borders"] = json!(r.borders);
    }
    if r.fill != basis.fill {
        v["fill"] = color(&r.fill);
    }
    if r.bold != basis.bold {
        v["bold"] = json!(r.bold);
    }
    if r.color != basis.color {
        v["color"] = color(&r.color);
    }
    v
}

/// A colour menu: a swatch with the colour grid and No Color. `value` is hex, empty for none.
pub(crate) fn color_menu(ui: &mut Ui, theme: &[wordcraft_doc::Rgb], value: &mut String) {
    let c = wordcraft_doc::Rgb::parse(value);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
        let t = Tokens::get(ui.ctx());
        match c {
            Some(c) => {
                ui.painter().rect_filled(r, 2.0, crate::theme::c32(c));
            }
            None => {
                ui.painter().line_segment([r.left_bottom(), r.right_top()], egui::Stroke::new(1.0, t.text_dim));
            }
        }
        ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.0, t.border_strong), egui::StrokeKind::Inside);
        let label = if c.is_some() { value.clone() } else { tl!("No Color").to_string() };
        ui.menu_button(label, |ui| {
            if ui.button(tl!("No Color")).clicked() {
                value.clear();
                ui.close();
            }
            if let Some(hex) = crate::widgets::color_grid(ui, theme) {
                *value = hex;
                ui.close();
            }
        });
    });
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct SourcesForm {
    query: String,
    editing: String,
    tag: String,
    author: String,
    title: String,
    year: String,
    publisher: String,
    journal: String,
    doi: String,
    format: String,
    exchange: String,
    error: String,
}

impl Dialog {
    pub fn name(&self) -> &'static str {
        match self {
            Dialog::Recovery { .. } => "recovery",
            Dialog::Mendeley { .. } => "mendeley",
            Dialog::Sources { .. } => "sources",
            Dialog::ChartData { .. } => "chartData",
            Dialog::Recipients { .. } => "recipients",
            Dialog::Font { .. } => "font",
            Dialog::Paragraph { .. } => "paragraph",
            Dialog::Tabs { .. } => "tabs",
            Dialog::Borders { .. } => "borders",
            Dialog::Find { replace_mode: false, .. } => "find",
            Dialog::Find { .. } => "replace",
            Dialog::Goto { .. } => "goto",
            Dialog::InsertTable { .. } => "insertTable",
            Dialog::PageSetup { .. } => "pageSetup",
            Dialog::Link { .. } => "link",
            Dialog::Bookmark { .. } => "bookmark",
            Dialog::WordCount { .. } => "wordCount",
            Dialog::Zoom { .. } => "zoom",
            Dialog::Watermark { .. } => "watermark",
            Dialog::Phonetic { .. } => "phonetic",
            Dialog::NewStyle { .. } => "newStyle",
            Dialog::ModifyStyle { .. } => "modifyStyle",
            Dialog::ManageStyles { .. } => "manageStyles",
            Dialog::TableStyle { id: None, .. } => "newTableStyle",
            Dialog::TableStyle { .. } => "modifyTableStyle",
            Dialog::Commands { .. } => "commands",
            Dialog::Insert(d) => d.name(),
            Dialog::Refs(d) => d.name(),
            Dialog::Icons { .. } => "icons",
            Dialog::PasteSpecial { .. } => "pasteSpecial",
            Dialog::About { .. } => "about",
            Dialog::SaveChanges { .. } => "saveChanges",
            Dialog::RecipientList { .. } => "recipientList",
            Dialog::SelectTable { .. } => "selectTable",
            Dialog::InsertMergeField { .. } => "insertMergeField",
            Dialog::FindRecipient { .. } => "findRecipient",
            Dialog::FieldCode { edit: false, .. } => "fieldCode",
            Dialog::FieldCode { .. } => "editFieldCode",
            Dialog::MergeRule { rule, .. } if rule == "SKIPIF" => "ruleSkipIf",
            Dialog::MergeRule { .. } => "ruleIf",
            Dialog::MatchFields { .. } => "matchFields",
            Dialog::CheckErrors { .. } => "checkErrors",
            Dialog::TableProperties { .. } => "tableProperties",
            Dialog::Password { .. } => "password",
            Dialog::EncryptPassword { .. } => "encryptPassword",
            Dialog::DefineList { .. } => "defineList",
            Dialog::TrackOptions { .. } => "trackChangesOptions",
            Dialog::Review(d) => d.name(),
            Dialog::CustomizeKeyboard { .. } => "customizeKeyboard",
            Dialog::CustomizeRibbon { .. } => "customizeRibbon",
            Dialog::Control { .. } => "controlProperties",
            Dialog::Enclose { .. } => "enclose",
            Dialog::NoteOptions { .. } => "noteOptions",
            Dialog::Templates { .. } => "templates",
            Dialog::Organizer { .. } => "organizer",
            Dialog::Layout(d) => d.name(),
        }
    }

    /// The dialog that shows a report command's result: Match Fields, Check for Errors.
    pub fn report(id: &str, v: &Value) -> Option<Dialog> {
        let strings = |k: &str| -> Vec<String> {
            v.get(k).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default()
        };
        match id {
            "mailings.matchFields" => {
                Some(Dialog::MatchFields { fields: strings("fields"), address: v.get("address").cloned().unwrap_or(Value::Null) })
            }
            "mailings.checkErrors" => {
                Some(Dialog::CheckErrors { unknown: strings("unknownFields"), records: v.get("records").and_then(Value::as_u64).unwrap_or(0) })
            }
            _ => None,
        }
    }

    pub fn open(name: &str, app: &mut WordApp) -> Option<Dialog> {
        if let Some(d) = crate::dialogs_insert::open(name, app) {
            return Some(Dialog::Insert(Box::new(d)));
        }
        if let Some(d) = crate::dialogs_refs::open(name, app) {
            return Some(Dialog::Refs(Box::new(d)));
        }
        if let Some(d) = crate::dialogs_layout::open(name, app) {
            return Some(Dialog::Layout(Box::new(d)));
        }
        if matches!(name, "compare" | "combine" | "autoCorrect" | "restrictEditing") {
            return crate::dialogs_review::open(name, app).map(|d| Dialog::Review(Box::new(d)));
        }
        let st = app.session.run("format.state", &json!({})).unwrap_or_default();
        let s = |k: &str| st.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let b = |k: &str| st.get(k).and_then(Value::as_bool).unwrap_or(false);
        Some(match name {
            "recovery" => {
                let mut state = crate::recovery::DialogState::default();
                state.refresh(app);
                Dialog::Recovery { state }
            }
            "mendeley" => Dialog::Mendeley { ris: String::new(), query: String::new(), error: String::new() },
            "sources" => Dialog::Sources { form: Box::new(SourcesForm { format: "bibtex".into(), ..Default::default() }) },
            "chartData" => return None,
            "recipients" => Dialog::Recipients { csv: String::new(), error: String::new() },
            "font" => {
                let mut d = Dialog::Font {
                    font: s("font"),
                    size: st.get("size").and_then(Value::as_f64).map(|v| v.to_string()).unwrap_or_default(),
                    bold: b("bold"),
                    italic: b("italic"),
                    underline: b("underline"),
                    strike: b("strike"),
                    sup: b("superscript"),
                    sub: b("subscript"),
                    small_caps: b("smallCaps"),
                    caps: b("caps"),
                    hidden: b("hidden"),
                    color: s("color"),
                    spacing: st.get("spacing").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                    start: serde_json::Map::new(),
                };
                let props = font_props(&d);
                if let Dialog::Font { start, .. } = &mut d {
                    *start = props;
                }
                d
            }
            "paragraph" | "asianTypography" => {
                let rp = app.session.doc.para_at(&app.session.sel.focus).map(|p| app.session.doc.styles.resolve_para(&p.props));
                let rp = rp?;
                let line = match rp.line_spacing {
                    wordcraft_doc::props::LineSpacing::Multiple(m) => m,
                    _ => 1.0,
                };
                let (vl, vr) = rp.visual_indents();
                Dialog::Paragraph {
                    rtl: rp.bidi,
                    align: format!("{:?}", rp.align.visual(rp.bidi)).to_lowercase(),
                    left: vl / crate::options::unit().pt_per_unit(),
                    right: vr / crate::options::unit().pt_per_unit(),
                    first: rp.indent_first / crate::options::unit().pt_per_unit(),
                    before: rp.space_before,
                    after: rp.space_after,
                    line,
                    keep_next: rp.keep_next,
                    keep_lines: rp.keep_lines,
                    page_break: rp.page_break_before,
                    widow: rp.widow_control,
                    tab: if name == "asianTypography" { 2 } else { 0 },
                    asian: [rp.kinsoku, rp.word_wrap, rp.overflow_punct, rp.top_line_punct, rp.auto_space_de, rp.auto_space_dn],
                    asian_was: [rp.kinsoku, rp.word_wrap, rp.overflow_punct, rp.top_line_punct, rp.auto_space_de, rp.auto_space_dn],
                    opts: [rp.mirror_indents, rp.contextual_spacing, rp.suppress_line_numbers, rp.suppress_hyphens],
                    opts_was: [rp.mirror_indents, rp.contextual_spacing, rp.suppress_line_numbers, rp.suppress_hyphens],
                }
            }
            "tabs" => Dialog::Tabs { form: Box::new(crate::dialogs_para::TabsForm::read(app)?) },
            "borders" | "pageBorders" | "shading" => {
                let tab = match name {
                    "pageBorders" => 1,
                    "shading" => 2,
                    _ => 0,
                };
                Dialog::Borders { form: Box::new(crate::dialogs_para::BordersForm::read(app, tab)?) }
            }
            "find" | "replace" => Dialog::Find {
                query: if app.session.sel.is_collapsed() { app.session.find.query.clone() } else { app.session.selected_text() },
                replace: app.session.find.replace.clone(),
                match_case: app.session.find.match_case,
                whole_word: app.session.find.whole_word,
                regex: app.session.find.regex,
                replace_mode: name == "replace",
                message: String::new(),
            },
            "goto" => Dialog::Goto { page: String::new() },
            "insertTable" => Dialog::InsertTable { rows: 2, cols: 5 },
            "pageSetup" => {
                let sp = wordcraft_engine::cmd::page::sect(&app.session);
                use wordcraft_doc::section::DocGridType;
                let kind = match sp.doc_grid.map(|g| g.kind) {
                    Some(DocGridType::Lines) => 1,
                    Some(DocGridType::LinesAndChars) => 2,
                    Some(DocGridType::SnapToChars) => 3,
                    _ => 0,
                };
                let size = app.session.doc.styles.resolve_char(Some("Normal"), &Default::default()).size;
                let grid = (kind, sp.grid_lines_per_page().min(1000), sp.grid_chars_per_line(size).min(1000));
                Dialog::PageSetup {
                    top: sp.margin_top / crate::options::unit().pt_per_unit(),
                    bottom: sp.margin_bottom / crate::options::unit().pt_per_unit(),
                    left: sp.margin_left / crate::options::unit().pt_per_unit(),
                    right: sp.margin_right / crate::options::unit().pt_per_unit(),
                    landscape: sp.landscape,
                    grid,
                    grid_was: grid,
                    rtl: sp.rtl,
                }
            }
            "link" => Dialog::Link { url: "https://".into(), text: app.session.selected_text() },
            "bookmark" => Dialog::Bookmark { name: String::new() },
            "wordCount" => Dialog::WordCount { stats: app.session.run("review.wordCount", &json!({})).unwrap_or_default() },
            "zoom" => Dialog::Zoom { percent: (app.session.view.zoom * 100.0).round() },
            "watermark" => Dialog::Watermark { text: "CONFIDENTIAL".into(), diagonal: true },
            "phonetic" => {
                let v = wordcraft_engine::cmd::phonetic::state(&app.session);
                let f = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
                let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
                Dialog::Phonetic {
                    base: s("base"),
                    ruby: s("ruby"),
                    align: s("align"),
                    size: f("size"),
                    offset: f("offset"),
                    existing: v.get("existing").and_then(Value::as_bool).unwrap_or(false),
                }
            }
            "newStyle" => Dialog::NewStyle { name: "Style1".into(), based_on: "Normal".into(), back: false },
            "manageStyles" => Dialog::ManageStyles { alphabetical: false, selected: s("style") },
            "newTableStyle" => {
                let based_on = current_table_style(app).unwrap_or_else(|| "TableGrid".into());
                let styles = &app.session.doc.styles;
                let name = (1..1000).map(|i| format!("Table Style {i}")).find(|n| styles.find(n).is_none()).unwrap_or_default();
                let regions = table_regions(app, &based_on);
                Dialog::TableStyle { id: None, name, based_on, region: 0, basis: regions.clone(), regions }
            }
            "modifyTableStyle" => {
                let id = current_table_style(app)?;
                let st = app.session.doc.styles.get(&id)?;
                let (name, based_on) = (st.name.clone(), st.based_on.clone().unwrap_or_default());
                let regions = table_regions(app, &id);
                Dialog::TableStyle { id: Some(id), name, based_on, region: 0, basis: regions.clone(), regions }
            }
            "commands" => Dialog::Commands { query: String::new() },
            "icons" => Dialog::Icons { query: String::new(), category: String::new(), selected: String::new() },
            "tableProperties" => {
                let form = Box::new(TableForm::read(app)?);
                Dialog::TableProperties { basis: form.clone(), form }
            }
            "enclose" => {
                // The selection's text and its enclosure, if it has one.
                let t = wordcraft_engine::cmd::format::enclose_target(&app.session).ok();
                let cur = t.as_ref().and_then(|t| t.current.clone());
                Dialog::Enclose {
                    style: cur.as_ref().map_or("shrink", |e| e.style.name()).into(),
                    shape: cur.as_ref().map_or("circle", |e| e.shape.name()).into(),
                    text: t.map(|t| t.text).unwrap_or_default(),
                }
            }
            "pasteSpecial" => Dialog::paste_special(app, &json!({})),
            "about" => Dialog::About { tab: 0 },
            "contributors" => Dialog::About { tab: 1 },
            "models" => Dialog::About { tab: 2 },
            // The current list to edit, or a new one to type.
            "recipientList" | "newRecipientList" => {
                let m = &app.session.merge;
                if name == "recipientList" && !m.headers.is_empty() {
                    Dialog::RecipientList { fields: m.headers.clone(), rows: m.rows.clone(), message: String::new() }
                } else {
                    let fields: Vec<String> = NEW_LIST_FIELDS.iter().map(|f| f.to_string()).collect();
                    let rows = vec![vec![String::new(); fields.len()]];
                    Dialog::RecipientList { fields, rows, message: String::new() }
                }
            }
            "insertMergeField" => {
                let fields = app.session.merge.headers.clone();
                Dialog::InsertMergeField { field: fields.first().cloned().unwrap_or_default(), fields }
            }
            "findRecipient" => Dialog::FindRecipient { text: String::new(), message: String::new() },
            "fieldCode" => Dialog::FieldCode { code: String::new(), edit: false, message: String::new() },
            "editFieldCode" => {
                let field = wordcraft_engine::cmd::fields::selected(&app.session).into_iter().next()?;
                Dialog::FieldCode { code: field.instr, edit: true, message: String::new() }
            }
            "encryptPassword" => Dialog::EncryptPassword { password: Password::default(), confirm: Password::default(), message: String::new() },
            "ruleIf" | "ruleSkipIf" => {
                let fields = app.session.merge.headers.clone();
                Dialog::MergeRule {
                    rule: if name == "ruleSkipIf" { "SKIPIF" } else { "IF" }.into(),
                    field: fields.first().cloned().unwrap_or_default(),
                    fields,
                    value: String::new(),
                    then: String::new(),
                    els: String::new(),
                }
            }
            "matchFields" | "checkErrors" => {
                let id = if name == "matchFields" { "mailings.matchFields" } else { "mailings.checkErrors" };
                return Self::report(id, &app.session.run(id, &json!({})).unwrap_or_default());
            }
            "defineList" => Dialog::DefineList { form: Box::new(crate::dialogs_lists::ListForm::read(app)) },
            "trackChangesOptions" => Dialog::TrackOptions { form: Box::new(crate::dialogs_lists::TrackForm::read(app)) },
            "customizeKeyboard" => Dialog::CustomizeKeyboard { form: Box::new(crate::dialogs_keyboard::KeyboardForm::read(app)) },
            "customizeRibbon" => {
                Dialog::CustomizeRibbon { form: Box::new(crate::dialogs_ribbon::RibbonForm::read(app, crate::dialogs_ribbon::Page::Ribbon)) }
            }
            "customizeQuickAccess" => {
                Dialog::CustomizeRibbon { form: Box::new(crate::dialogs_ribbon::RibbonForm::read(app, crate::dialogs_ribbon::Page::QuickAccess)) }
            }
            "noteOptions" => Dialog::NoteOptions { form: Box::new(crate::dialogs_notes::NoteForm::read(app)) },
            "templates" => Dialog::Templates { form: Box::new(crate::dialogs_templates::TemplatesForm::read(app)) },
            "organizer" => Dialog::Organizer { form: Box::new(crate::dialogs_templates::OrganizerForm::read(app, None)) },
            "controlProperties" => match crate::dialogs_controls::ControlForm::read(app) {
                Some(form) => Dialog::Control { form: Box::new(form) },
                None => {
                    app.status(tl!("Put the caret in a content control first."));
                    return None;
                }
            },
            _ => return None,
        })
    }

    /// Open a chart's spreadsheet view from its selected package identity. Inspection is
    /// read-only; unsupported charts remain view-only with the engine's diagnostic.
    pub fn chart_data(app: &mut WordApp, chart_part: &str) -> Dialog {
        let document_id = app.session.document_id();
        let result = app.session.run("chart.inspectData", &json!({"documentId": document_id, "chartPart": chart_part}));
        let mut form = ChartDataForm { document_id, chart_part: chart_part.to_string(), ..Default::default() };
        match result {
            Ok(snapshot) => {
                form.original_snapshot = Some(snapshot.clone());
                form.chart_kind = snapshot.get("chartKind").and_then(Value::as_str).unwrap_or_default().to_string();
                form.original_legend_position = snapshot.get("legendPosition").and_then(Value::as_str).map(str::to_string);
                form.legend_position = form.original_legend_position.clone().unwrap_or_default();
                form.original_chart_title = snapshot.get("chartTitle").and_then(Value::as_str).map(str::to_string);
                form.chart_title = form.original_chart_title.clone().unwrap_or_default();
                form.original_horizontal_axis_title = snapshot.get("horizontalAxisTitle").and_then(Value::as_str).map(str::to_string);
                form.horizontal_axis_title = form.original_horizontal_axis_title.clone().unwrap_or_default();
                form.original_vertical_axis_title = snapshot.get("verticalAxisTitle").and_then(Value::as_str).map(str::to_string);
                form.vertical_axis_title = form.original_vertical_axis_title.clone().unwrap_or_default();
                form.original_horizontal_axis = snapshot.get("horizontalAxis").cloned().filter(|value| !value.is_null());
                form.horizontal_axis = form.original_horizontal_axis.as_ref().and_then(chart_axis_form);
                form.original_vertical_axis = snapshot.get("verticalAxis").cloned().filter(|value| !value.is_null());
                form.vertical_axis = form.original_vertical_axis.as_ref().and_then(chart_axis_form);
                form.series = snapshot
                    .get("series")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|series| ChartSeriesForm {
                        name: series.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                        color: series.get("color").and_then(Value::as_str).and_then(parse_rgb_hex).unwrap_or([80, 120, 180]),
                        original_color: series.get("color").and_then(Value::as_str).and_then(parse_rgb_hex),
                        color_editable: series.get("colorEditable").and_then(Value::as_bool).unwrap_or(false),
                        formatting_diagnostic: series.get("formattingDiagnostic").and_then(Value::as_str).map(str::to_string),
                        line_width: series.get("lineWidth").and_then(Value::as_f64),
                        original_line_width: series.get("lineWidth").and_then(Value::as_f64),
                        line_width_editable: series.get("lineWidthEditable").and_then(Value::as_bool).unwrap_or(false),
                        marker_symbol: series.get("markerSymbol").and_then(Value::as_str).unwrap_or_default().to_string(),
                        original_marker_symbol: series.get("markerSymbol").and_then(Value::as_str).map(str::to_string),
                        marker_symbol_editable: series.get("markerSymbolEditable").and_then(Value::as_bool).unwrap_or(false),
                        marker_size: series.get("markerSize").and_then(Value::as_u64).and_then(|value| u8::try_from(value).ok()).unwrap_or(5),
                        original_marker_size: series.get("markerSize").and_then(Value::as_u64).and_then(|value| u8::try_from(value).ok()),
                        marker_size_editable: series.get("markerSizeEditable").and_then(Value::as_bool).unwrap_or(false),
                        marker_color: series.get("markerColor").and_then(Value::as_str).and_then(parse_rgb_hex).unwrap_or([80, 120, 180]),
                        original_marker_color: series.get("markerColor").and_then(Value::as_str).and_then(parse_rgb_hex),
                        marker_color_editable: series.get("markerColorEditable").and_then(Value::as_bool).unwrap_or(false),
                        data_label_value: series
                            .get("dataLabels")
                            .and_then(|labels| labels.get("showValue"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        original_data_label_value: series.get("dataLabels").and_then(|labels| labels.get("showValue")).and_then(Value::as_bool),
                        data_label_category: series
                            .get("dataLabels")
                            .and_then(|labels| labels.get("showCategory"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        original_data_label_category: series.get("dataLabels").and_then(|labels| labels.get("showCategory")).and_then(Value::as_bool),
                        data_label_series: series
                            .get("dataLabels")
                            .and_then(|labels| labels.get("showSeriesName"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        original_data_label_series: series.get("dataLabels").and_then(|labels| labels.get("showSeriesName")).and_then(Value::as_bool),
                        data_label_position: series
                            .get("dataLabels")
                            .and_then(|labels| labels.get("position"))
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        original_data_label_position: series
                            .get("dataLabels")
                            .and_then(|labels| labels.get("position"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        data_labels_editable: series
                            .get("dataLabels")
                            .and_then(|labels| labels.get("editable"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        data_labels_diagnostic: series
                            .get("dataLabels")
                            .and_then(|labels| labels.get("diagnostic"))
                            .and_then(Value::as_str)
                            .map(str::to_string),
                        error_bars: series
                            .get("errorBars")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .map(|bar| ChartErrorBarForm {
                                direction: bar.get("direction").and_then(Value::as_str).unwrap_or("unknown").to_string(),
                                bar_type: bar.get("barType").and_then(Value::as_str).map(str::to_string),
                                value_type: bar.get("valueType").and_then(Value::as_str).map(str::to_string),
                                renderable: bar.get("renderable").and_then(Value::as_bool).unwrap_or(false),
                                editable: bar.get("editable").and_then(Value::as_bool).unwrap_or(false),
                                fixed_value: bar.get("fixedValue").and_then(Value::as_f64).map(|value| value.to_string()).unwrap_or_default(),
                                original_fixed_value: bar.get("fixedValue").and_then(Value::as_f64),
                                positive: bar
                                    .get("positive")
                                    .and_then(Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_f64)
                                    .map(|value| value.to_string())
                                    .collect(),
                                original_positive: bar
                                    .get("positive")
                                    .and_then(Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_f64)
                                    .collect(),
                                negative: bar
                                    .get("negative")
                                    .and_then(Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_f64)
                                    .map(|value| value.to_string())
                                    .collect(),
                                original_negative: bar
                                    .get("negative")
                                    .and_then(Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter_map(Value::as_f64)
                                    .collect(),
                                diagnostic: bar.get("diagnostic").and_then(Value::as_str).map(str::to_string),
                            })
                            .collect(),
                        categories: series
                            .get("categories")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect(),
                        x_values: series
                            .get("xValues")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_f64)
                            .map(|value| value.to_string())
                            .collect(),
                        values: series
                            .get("values")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_f64)
                            .map(|value| value.to_string())
                            .collect(),
                    })
                    .collect();
                if form.series.is_empty() {
                    form.read_only_reason = "validated chart data is empty".into();
                } else {
                    form.original = form.series.clone();
                }
            }
            Err(error) => form.read_only_reason = error.to_string(),
        }
        Dialog::ChartData { form: Box::new(form) }
    }

    /// Paste Special for a clipboard payload (`text`, `html`, `rtf`; empty = WordCraft's own copy).
    pub fn paste_special(app: &WordApp, payload: &Value) -> Dialog {
        let formats: Vec<String> = wordcraft_engine::cmd::paste::available(&app.session, payload).into_iter().map(str::to_string).collect();
        let mut kept = json!({});
        for k in ["text", "html", "rtf"] {
            if let Some(t) = payload.get(k).and_then(Value::as_str) {
                kept[k] = json!(t);
            }
        }
        Dialog::PasteSpecial { formats, choice: 0, payload: kept }
    }

    pub fn modify_style(app: &WordApp, id: &str) -> Option<Dialog> {
        let st = app.session.doc.styles.get(id)?;
        let rc = app.session.doc.styles.resolve_char(
            if st.kind == wordcraft_doc::StyleKind::Paragraph { Some(id) } else { None },
            &wordcraft_doc::CharProps {
                style: if st.kind == wordcraft_doc::StyleKind::Character { Some(id.into()) } else { None },
                ..Default::default()
            },
        );
        let rp = app.session.doc.styles.resolve_para(&wordcraft_doc::ParaProps { style: Some(id.into()), ..Default::default() });
        Some(Dialog::ModifyStyle {
            back: false,
            resume: None,
            id: id.into(),
            name: st.name.clone(),
            font: rc.font,
            size: rc.size,
            bold: rc.bold,
            italic: rc.italic,
            color: match rc.color {
                wordcraft_doc::TextColor::Rgb(c) => c.hex(),
                wordcraft_doc::TextColor::Auto => "auto".into(),
            },
            before: rp.space_before,
            after: rp.space_after,
        })
    }
}

/// Grid picker for Insert › Table (hover to size, click to insert).
pub fn table_grid_picker(ui: &mut Ui, app: &mut WordApp) {
    let t = Tokens::get(ui.ctx());
    let id = egui::Id::new("table_picker_hover");
    let hover: (usize, usize) = ui.data(|d| d.get_temp(id)).unwrap_or((0, 0));
    ui.label(if hover.0 > 0 {
        crate::i18n::fmt(tl!("{cols}×{rows} Table"), &[("cols", &hover.1.to_string()), ("rows", &hover.0.to_string())])
    } else {
        tl!("Insert Table").to_string()
    });
    let mut new_hover = (0, 0);
    let mut clicked = None;
    egui::Grid::new("tgp").spacing(vec2(2.0, 2.0)).show(ui, |ui| {
        for r in 1..=8 {
            for c in 1..=10 {
                let (rect, resp) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
                let on = r <= hover.0 && c <= hover.1;
                ui.painter().rect(
                    rect,
                    1.0,
                    if on { t.checked } else { t.input },
                    egui::Stroke::new(1.0, if on { t.accent } else { t.border_strong }),
                    egui::StrokeKind::Inside,
                );
                if resp.hovered() {
                    new_hover = (r, c);
                }
                if resp.clicked() {
                    clicked = Some((r, c));
                }
            }
            ui.end_row();
        }
    });
    ui.data_mut(|d| d.insert_temp(id, new_hover));
    if let Some((r, c)) = clicked {
        let _ = app.run("insert.table", json!({"rows": r, "cols": c}));
        ui.close();
    }
}

/// Where the open dialog's top edge sits, as (window id, offset below the top of the screen).
fn dialog_top_key() -> egui::Id {
    egui::Id::new("dialog_top")
}

/// Anchors a dialog: centred on the frame it opens, then with its top edge held where that put
/// it, so a taller or shorter tab (Paragraph, Font, Borders and Shading…) only moves the bottom
/// edge and the tab strip stays under the mouse (#507). egui keeps it on screen if it grows past
/// the bottom.
fn place_dialog<'a>(ctx: &egui::Context, id: egui::Id, w: egui::Window<'a>) -> egui::Window<'a> {
    match ctx.data(|d| d.get_temp::<(egui::Id, f32)>(dialog_top_key())) {
        Some((open, top)) if open == id => w.anchor(egui::Align2::CENTER_TOP, vec2(0.0, top)),
        _ => w.anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -40.0)),
    }
}

/// After a dialog's first frame: holds its top edge where centring its measured size put it.
fn remember_dialog_top(ctx: &egui::Context, id: egui::Id, rect: egui::Rect) {
    if ctx.data(|d| d.get_temp::<(egui::Id, f32)>(dialog_top_key())).is_some_and(|(open, _)| open == id) {
        return;
    }
    let screen = ctx.content_rect();
    let top = (screen.center().y - 40.0 - rect.height() / 2.0 - screen.top()).max(0.0);
    ctx.data_mut(|d| d.insert_temp(dialog_top_key(), (id, top)));
}

pub fn show(app: &mut WordApp, ctx: &egui::Context) {
    let Some(mut d) = app.dialog.take() else {
        ctx.data_mut(|d| d.remove::<(egui::Id, f32)>(dialog_top_key()));
        return;
    };
    let mut open = true;
    let mut close = false;
    let title = match &d {
        Dialog::Recovery { .. } => "Recover Unsaved Documents",
        Dialog::Mendeley { .. } => "Mendeley Library",
        Dialog::Sources { .. } => "Citation Sources",
        Dialog::ChartData { .. } => "Edit Chart Data",
        Dialog::Recipients { .. } => "Select Recipients",
        Dialog::Font { .. } => "Font",
        Dialog::Paragraph { .. } => "Paragraph",
        Dialog::Tabs { .. } => "Tabs",
        Dialog::Borders { .. } => "Borders and Shading",
        Dialog::Find { replace_mode: false, .. } => "Find",
        Dialog::Find { .. } => "Find and Replace",
        Dialog::Goto { .. } => "Go To",
        Dialog::InsertTable { .. } => "Insert Table",
        Dialog::PageSetup { .. } => "Page Setup",
        Dialog::Link { .. } => "Insert Hyperlink",
        Dialog::Bookmark { .. } => "Bookmark",
        Dialog::WordCount { .. } => "Word Count",
        Dialog::Zoom { .. } => "Zoom",
        Dialog::Watermark { .. } => "Custom Watermark",
        Dialog::Phonetic { .. } => "Phonetic Guide",
        Dialog::NewStyle { .. } => "Create New Style",
        Dialog::ModifyStyle { .. } => "Modify Style",
        Dialog::ManageStyles { .. } => "Manage Styles",
        Dialog::TableStyle { id: None, .. } => "New Table Style",
        Dialog::TableStyle { .. } => "Modify Table Style",
        Dialog::Commands { .. } => "Search Commands",
        Dialog::Insert(d) => d.title(),
        Dialog::Refs(d) => d.title(),
        Dialog::Icons { .. } => "Icons",
        Dialog::PasteSpecial { .. } => "Paste Special",
        Dialog::About { .. } => "About WordCraft",
        Dialog::SaveChanges { .. } => "WordCraft",
        Dialog::RecipientList { .. } => "Recipient List",
        Dialog::SelectTable { .. } => "Select Table",
        Dialog::InsertMergeField { .. } => "Insert Merge Field",
        Dialog::FindRecipient { .. } => "Find Recipient",
        Dialog::FieldCode { edit: false, .. } => "Field",
        Dialog::FieldCode { .. } => "Edit Field Code",
        Dialog::MergeRule { rule, .. } if rule == "SKIPIF" => "Skip Record If",
        Dialog::MergeRule { .. } => "If…Then…Else",
        Dialog::MatchFields { .. } => "Match Fields",
        Dialog::CheckErrors { .. } => "Check for Errors",
        Dialog::TableProperties { .. } => "Table Properties",
        Dialog::Password { .. } => "Password",
        Dialog::EncryptPassword { .. } => "Encrypt with Password",
        Dialog::DefineList { .. } => "Define New Multilevel List",
        Dialog::TrackOptions { .. } => "Track Changes Options",
        Dialog::Review(d) => d.title(),
        Dialog::CustomizeKeyboard { .. } => "Customize Keyboard",
        Dialog::CustomizeRibbon { .. } => "Customize Ribbon",
        Dialog::Control { .. } => "Content Control Properties",
        Dialog::Enclose { .. } => "Enclose Characters",
        Dialog::NoteOptions { .. } => "Footnote and Endnote",
        Dialog::Templates { .. } => "Templates and Add-ins",
        Dialog::Organizer { .. } => "Organizer",
        Dialog::Layout(d) => d.title(),
    };
    let id = egui::Id::new(("dialog", title));
    let shown = place_dialog(ctx, id, egui::Window::new(tl!(title)).id(id).collapsible(false).resizable(false)).open(&mut open).show(ctx, |ui| {
        ui.set_min_width(340.0);
        close = body(app, ui, &mut d);
    });
    if let Some(r) = shown {
        remember_dialog_top(ctx, id, r.response.rect);
    }
    if open && !close {
        if app.dialog.is_none() {
            app.dialog = Some(d);
        }
    } else {
        app.canvas.want_focus = true;
    }
}

pub(crate) fn buttons(ui: &mut Ui, ok: &str) -> (bool, bool) {
    let mut r = (false, false);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(tl!("Cancel")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                r.1 = true;
            }
            let okb = ui.add(egui::Button::new(egui::RichText::new(ok).color(egui::Color32::WHITE)).fill(crate::theme::APP_COLOR));
            if okb.clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                r.0 = true;
            }
        });
    });
    r
}

/// The character properties the Font dialog's fields stand for.
fn font_props(d: &Dialog) -> serde_json::Map<String, Value> {
    let Dialog::Font { font, size, bold, italic, underline, strike, sup, sub, small_caps, caps, hidden, color, spacing, .. } = d else {
        return serde_json::Map::new();
    };
    let mut props = serde_json::Map::new();
    props.insert("bold".into(), json!(*bold));
    props.insert("italic".into(), json!(*italic));
    props.insert("strike".into(), json!(*strike));
    props.insert("underline".into(), json!(if *underline { "single" } else { "none" }));
    let vert = if *sup {
        "superscript"
    } else if *sub {
        "subscript"
    } else {
        "baseline"
    };
    props.insert("vertAlign".into(), json!(vert));
    props.insert("smallCaps".into(), json!(*small_caps));
    props.insert("caps".into(), json!(*caps));
    props.insert("hidden".into(), json!(*hidden));
    props.insert("spacing".into(), json!(*spacing));
    if !font.trim().is_empty() {
        props.insert("font".into(), json!(font.trim()));
    }
    if let Ok(s) = size.trim().parse::<f64>() {
        props.insert("size".into(), json!(s));
    }
    if let Some(c) = wordcraft_doc::Rgb::parse(color) {
        props.insert("color".into(), json!({"Rgb": [c.0, c.1, c.2]}));
    }
    props
}

/// Font dialog OK: set the properties the user changed, leave the others alone.
fn apply_font(app: &mut WordApp, d: &Dialog) {
    let Dialog::Font { start, .. } = d else { return };
    let changed: serde_json::Map<String, Value> = font_props(d).into_iter().filter(|(k, v)| start.get(k) != Some(v)).collect();
    if !changed.is_empty() {
        let _ = app.run("format.set", json!({"props": changed}));
    }
}

/// Returns true to close.
fn citation_sources(app: &mut WordApp, ui: &mut Ui, form: &mut SourcesForm) -> bool {
    ui.label("Search, insert, and edit sources attached to this document.");
    ui.horizontal(|ui| {
        ui.label("Search");
        ui.text_edit_singleline(&mut form.query);
    });
    let needle = form.query.to_lowercase();
    let matches: Vec<_> = app
        .session
        .doc
        .sources
        .iter()
        .filter(|source| format!("{} {} {} {} {}", source.title, source.author, source.year, source.tag, source.doi).to_lowercase().contains(&needle))
        .take(100)
        .cloned()
        .collect();
    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
        for source in matches {
            ui.horizontal(|ui| {
                ui.label(format!("{} ({}) — {}", source.author, source.year, source.title));
                if ui.small_button("Insert").clicked()
                    && let Err(error) = app.run("references.citation", json!({"tag": source.tag}))
                {
                    form.error = error;
                }
                if ui.small_button("Edit").clicked() {
                    form.editing = source.tag.clone();
                    form.tag = source.tag.clone();
                    form.author = source.author.clone();
                    form.title = source.title.clone();
                    form.year = source.year.clone();
                    form.publisher = source.publisher.clone();
                    form.journal = source.journal.clone();
                    form.doi = source.doi.clone();
                }
                if ui.small_button("Remove").clicked()
                    && let Err(error) = app.run("references.sources", json!({"remove": source.tag}))
                {
                    form.error = error;
                }
            });
        }
    });

    ui.separator();
    ui.label(if form.editing.is_empty() { "Add source" } else { "Edit source" });
    egui::Grid::new("citation_source_fields").num_columns(2).show(ui, |ui| {
        ui.label("Citation key / ID");
        ui.text_edit_singleline(&mut form.tag);
        ui.end_row();
        ui.label("Title");
        ui.text_edit_singleline(&mut form.title);
        ui.end_row();
        ui.label("Author(s)");
        ui.text_edit_singleline(&mut form.author);
        ui.end_row();
        ui.label("Year");
        ui.text_edit_singleline(&mut form.year);
        ui.end_row();
        ui.label("DOI");
        ui.text_edit_singleline(&mut form.doi);
        ui.end_row();
        ui.label("Journal");
        ui.text_edit_singleline(&mut form.journal);
        ui.end_row();
        ui.label("Publisher");
        ui.text_edit_singleline(&mut form.publisher);
        ui.end_row();
    });
    ui.horizontal(|ui| {
        if ui.button("Save Source").clicked() {
            if form.title.trim().is_empty() {
                form.error = "A source needs a title.".into();
            } else {
                let mut source = app.session.doc.sources.iter().find(|source| source.tag == form.editing).cloned().unwrap_or_default();
                source.tag = form.tag.trim().to_string();
                source.title = form.title.trim().to_string();
                source.author = form.author.trim().to_string();
                source.year = form.year.trim().to_string();
                source.doi = form.doi.trim().to_string();
                source.journal = form.journal.trim().to_string();
                source.publisher = form.publisher.trim().to_string();
                match app.run("references.sources", json!({"add": source})) {
                    Ok(_) => {
                        form.error.clear();
                        form.editing.clear();
                        form.tag.clear();
                        form.author.clear();
                        form.title.clear();
                        form.year.clear();
                        form.publisher.clear();
                        form.journal.clear();
                        form.doi.clear();
                    }
                    Err(error) => form.error = error,
                }
            }
        }
        if ui.button("New").clicked() {
            form.editing.clear();
            form.tag.clear();
            form.author.clear();
            form.title.clear();
            form.year.clear();
            form.publisher.clear();
            form.journal.clear();
            form.doi.clear();
            form.error.clear();
        }
    });

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Interchange format");
        egui::ComboBox::from_id_salt("citation_source_format").selected_text(&form.format).show_ui(ui, |ui| {
            ui.selectable_value(&mut form.format, "bibtex".into(), "BibTeX");
            ui.selectable_value(&mut form.format, "csl-json".into(), "CSL JSON");
        });
        if ui.button("Import from text").clicked() {
            match app.run("references.importSources", json!({"format": form.format, "data": form.exchange})) {
                Ok(result) => {
                    form.error = format!("Imported {} sources ({} new, {} updated).", result["total"], result["added"], result["updated"]);
                    form.exchange.clear();
                }
                Err(error) => form.error = error,
            }
        }
        if ui.button("Export").clicked() {
            match app.run("references.exportSources", json!({"format": form.format})) {
                Ok(result) => form.exchange = result["data"].as_str().unwrap_or("").to_string(),
                Err(error) => form.error = error,
            }
        }
        if ui.button("Copy export").clicked() && !form.exchange.is_empty() {
            ui.ctx().copy_text(form.exchange.clone());
        }
    });
    ui.add(egui::TextEdit::multiline(&mut form.exchange).desired_rows(5).desired_width(540.0).char_limit(8 * 1024 * 1024));
    ui.label("Import pasted BibTeX or CSL JSON; export is copied from this text area.");

    ui.separator();
    ui.label("Citation style");
    ui.horizontal(|ui| {
        for style in &wordcraft_engine::cmd::citations::STYLES {
            if ui.selectable_label(app.session.bib_style == style.name, style.name).clicked()
                && let Err(error) = app.run("references.citationStyle", json!({"style": style.name}))
            {
                form.error = error;
            }
        }
        if ui.button("Insert Bibliography").clicked()
            && let Err(error) = app.run("references.bibliography", json!({}))
        {
            form.error = error;
        }
        if ui.button("Refresh Fields").clicked()
            && let Err(error) = app.run("references.updateCitations", json!({}))
        {
            form.error = error;
        }
    });
    if !form.error.is_empty() {
        ui.colored_label(Tokens::get(ui.ctx()).red, &form.error);
    }
    ui.button("Close").clicked() || ui.input(|input| input.key_pressed(egui::Key::Escape))
}

/// Spreadsheet-like data editor. It only stages text locally until Apply routes the complete
/// validated request through the existing `chart.editData` engine command.
#[cfg(test)]
fn chart_series_values(chart_kind: &str, series: &[ChartSeriesForm]) -> Option<Vec<Value>> {
    series
        .iter()
        .map(|series| {
            let values: Option<Vec<f64>> =
                series.values.iter().map(|value| value.trim().parse::<f64>().ok().filter(|number| number.is_finite())).collect();
            if chart_kind == "scatter" {
                if series.x_values.len() != series.values.len() {
                    return None;
                }
                let x_values: Option<Vec<f64>> =
                    series.x_values.iter().map(|value| value.trim().parse::<f64>().ok().filter(|number| number.is_finite())).collect();
                x_values.zip(values).map(|(x_values, values)| json!({"name": series.name, "xValues": x_values, "values": values}))
            } else {
                values.map(|values| json!({"name": series.name, "categories": series.categories, "values": values}))
            }
        })
        .collect()
}

fn chart_series_changes(chart_kind: &str, series: &[ChartSeriesForm], original: &[ChartSeriesForm]) -> Option<Vec<Value>> {
    if series.len() != original.len() {
        return None;
    }
    series
        .iter()
        .zip(original)
        .map(|(series, original)| {
            let mut changes = serde_json::Map::new();
            if series.name != original.name {
                changes.insert("name".into(), json!(series.name));
            }
            if series.color != original.color {
                if !series.color_editable || series.original_color.is_none() {
                    return None;
                }
                changes.insert("color".into(), json!(rgb_hex(series.color)));
            }
            if series.line_width != original.line_width {
                if !series.line_width_editable {
                    return None;
                }
                changes.insert("lineWidth".into(), json!(series.line_width?));
            }
            if series.marker_symbol != original.marker_symbol {
                if !series.marker_symbol_editable {
                    return None;
                }
                changes.insert("markerSymbol".into(), json!(series.marker_symbol));
            }
            if series.marker_size != original.marker_size {
                if !series.marker_size_editable {
                    return None;
                }
                changes.insert("markerSize".into(), json!(series.marker_size));
            }
            if series.marker_color != original.marker_color {
                if !series.marker_color_editable || original.original_marker_color.is_none() {
                    return None;
                }
                changes.insert("markerColor".into(), json!(rgb_hex(series.marker_color)));
            }
            let mut data_labels = serde_json::Map::new();
            for (key, value, original_value) in [
                ("showValue", series.data_label_value, series.original_data_label_value),
                ("showCategory", series.data_label_category, series.original_data_label_category),
                ("showSeriesName", series.data_label_series, series.original_data_label_series),
            ] {
                if let Some(original_value) = original_value
                    && value != original_value
                {
                    if !series.data_labels_editable {
                        return None;
                    }
                    data_labels.insert(key.into(), json!(value));
                }
            }
            if series.data_label_position != series.original_data_label_position.as_deref().unwrap_or_default() {
                if !series.data_labels_editable || series.original_data_label_position.is_none() {
                    return None;
                }
                data_labels.insert("position".into(), json!(series.data_label_position));
            }
            if !data_labels.is_empty() {
                changes.insert("dataLabels".into(), Value::Object(data_labels));
            }
            let mut error_changes = Vec::new();
            if series.error_bars.len() != original.error_bars.len() {
                return None;
            }
            for (bar, old_bar) in series.error_bars.iter().zip(&original.error_bars) {
                if !bar.editable {
                    if bar.fixed_value != old_bar.fixed_value || bar.positive != old_bar.positive || bar.negative != old_bar.negative {
                        return None;
                    }
                    continue;
                }
                if bar.direction != old_bar.direction || bar.value_type != old_bar.value_type || bar.bar_type != old_bar.bar_type {
                    return None;
                }
                let mut item = serde_json::Map::new();
                item.insert("direction".into(), json!(bar.direction));
                match bar.value_type.as_deref() {
                    Some("fixedVal") if bar.fixed_value != old_bar.fixed_value => {
                        let value = bar.fixed_value.trim().parse::<f64>().ok().filter(|value| value.is_finite() && *value >= 0.0)?;
                        item.insert("fixedValue".into(), json!(value));
                    }
                    Some("cust") => {
                        if bar.positive != old_bar.positive {
                            let values: Option<Vec<f64>> = bar
                                .positive
                                .iter()
                                .map(|value| value.trim().parse::<f64>().ok().filter(|value| value.is_finite() && *value >= 0.0))
                                .collect();
                            item.insert("positive".into(), json!(values?));
                        }
                        if bar.negative != old_bar.negative {
                            let values: Option<Vec<f64>> = bar
                                .negative
                                .iter()
                                .map(|value| value.trim().parse::<f64>().ok().filter(|value| value.is_finite() && *value >= 0.0))
                                .collect();
                            item.insert("negative".into(), json!(values?));
                        }
                    }
                    Some("fixedVal") => {}
                    _ => return None,
                }
                if item.len() > 1 {
                    error_changes.push(Value::Object(item));
                }
            }
            if !error_changes.is_empty() {
                changes.insert("errorBars".into(), Value::Array(error_changes));
            }
            if chart_kind == "scatter" {
                if series.x_values.len() != original.x_values.len() || series.values.len() != original.values.len() {
                    return None;
                }
                if series.x_values != original.x_values {
                    let values: Option<Vec<f64>> =
                        series.x_values.iter().map(|value| value.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect();
                    changes.insert("xValues".into(), json!(values?));
                }
            } else if series.categories != original.categories {
                changes.insert("categories".into(), json!(series.categories));
            }
            if series.values != original.values {
                let values: Option<Vec<f64>> = series.values.iter().map(|value| value.trim().parse::<f64>().ok().filter(|v| v.is_finite())).collect();
                changes.insert("values".into(), json!(values?));
            }
            Some(Value::Object(changes))
        })
        .collect()
}

fn parse_rgb_hex(value: &str) -> Option<[u8; 3]> {
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some([
        u8::from_str_radix(value.get(0..2)?, 16).ok()?,
        u8::from_str_radix(value.get(2..4)?, 16).ok()?,
        u8::from_str_radix(value.get(4..6)?, 16).ok()?,
    ])
}

fn rgb_hex([red, green, blue]: [u8; 3]) -> String {
    format!("{red:02X}{green:02X}{blue:02X}")
}

fn chart_data(app: &mut WordApp, ui: &mut Ui, form: &mut ChartDataForm) -> bool {
    ui.label(egui::RichText::new(&form.chart_part).small().weak());
    if !form.read_only_reason.is_empty() {
        ui.colored_label(Tokens::get(ui.ctx()).red, tl!(tl!("This chart is read-only")));
        ui.label(&form.read_only_reason);
    } else {
        ui.label(format!("{} · {} {}", form.chart_kind, form.series.len(), tl!(tl!("Series"))));
        ui.horizontal(|ui| {
            let legend_enabled = form.original_legend_position.is_some();
            ui.add_enabled_ui(legend_enabled, |ui| {
                egui::ComboBox::from_label(tl!(tl!("Legend position"))).selected_text(legend_position_label(&form.legend_position)).show_ui(
                    ui,
                    |ui| {
                        for (value, label) in [("l", tl!(tl!("Left"))), ("r", tl!(tl!("Right"))), ("t", tl!(tl!("Top"))), ("b", tl!(tl!("Bottom")))] {
                            ui.selectable_value(&mut form.legend_position, value.to_string(), label);
                        }
                    },
                );
            });
            if !legend_enabled {
                ui.label(tl!(tl!("No explicitly supported legend position is available for this chart.")));
            }
        });
        ui.horizontal(|ui| {
            if form.original_chart_title.is_some() {
                ui.label(tl!(tl!("Chart title")));
                ui.add_sized([300.0, 24.0], egui::TextEdit::singleline(&mut form.chart_title));
            } else {
                ui.label(tl!(tl!("No safe existing chart title is available to edit.")));
            }
        });
        if !matches!(form.chart_kind.as_str(), "pie" | "doughnut") {
            if let Some(axis) = form.horizontal_axis.as_mut() {
                chart_axis_controls(ui, tl!(tl!(chart_axis_label("Horizontal", &axis.role))), axis);
            }
            if let Some(axis) = form.vertical_axis.as_mut() {
                chart_axis_controls(ui, tl!(tl!(chart_axis_label("Vertical", &axis.role))), axis);
            }
            if form.horizontal_axis.is_none() && form.vertical_axis.is_none() {
                ui.label(tl!(tl!("Axis controls are unavailable because the chart axes cannot be mapped safely.")));
            }
            ui.horizontal(|ui| {
                if form.original_horizontal_axis_title.is_some() {
                    ui.label(tl!(tl!("Horizontal axis title")));
                    ui.add_sized([260.0, 24.0], egui::TextEdit::singleline(&mut form.horizontal_axis_title));
                } else {
                    ui.label(tl!(tl!("No safe existing horizontal axis title is available to edit.")));
                }
            });
            ui.horizontal(|ui| {
                if form.original_vertical_axis_title.is_some() {
                    ui.label(tl!(tl!("Vertical axis title")));
                    ui.add_sized([260.0, 24.0], egui::TextEdit::singleline(&mut form.vertical_axis_title));
                } else {
                    ui.label(tl!(tl!("No safe existing vertical axis title is available to edit.")));
                }
            });
        }
        let mut focus_ids = Vec::new();
        let tab_from = ui
            .input(|input| input.key_pressed(egui::Key::Tab))
            .then(|| (ui.memory(|memory| memory.focused()), ui.input(|input| input.modifiers.shift)))
            .and_then(|(focused, reverse)| focused.map(|focused| (focused, reverse)));
        egui::ScrollArea::vertical().max_height(390.0).show(ui, |ui| {
            ui.add_enabled_ui(true, |ui| {
                egui::ScrollArea::horizontal().id_salt("chart_data_horizontal").show(ui, |ui| {
                    for (series_index, series) in form.series.iter_mut().enumerate() {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(tl!(tl!("Series color")));
                                ui.add_enabled_ui(series.color_editable, |ui| {
                                    egui::color_picker::color_edit_button_srgb(ui, &mut series.color);
                                });
                                if !series.color_editable {
                                    ui.label(series.formatting_diagnostic.as_deref().unwrap_or("This series color is read-only."));
                                }
                                if series.line_width_editable
                                    && let Some(width) = series.line_width.as_mut()
                                {
                                    ui.add(egui::DragValue::new(width).range(0.0..=20.0).speed(0.1).suffix(" pt"));
                                } else if matches!(form.chart_kind.as_str(), "line" | "scatter") {
                                    ui.label(series.formatting_diagnostic.as_deref().unwrap_or("Line width is read-only for this series."));
                                }
                            });
                            if form.chart_kind == "scatter" {
                                ui.horizontal(|ui| {
                                    ui.label(tl!(tl!("Marker")));
                                    ui.add_enabled_ui(series.marker_symbol_editable, |ui| {
                                        egui::ComboBox::from_id_salt(("scatter_marker", series_index))
                                            .selected_text(if series.marker_symbol.is_empty() {
                                                tl!(tl!("Unavailable"))
                                            } else {
                                                tl!(series.marker_symbol.as_str())
                                            })
                                            .show_ui(ui, |ui| {
                                                for symbol in ["circle", "dot", "square", "triangle", "diamond", "star", "plus", "x", "dash", "none"]
                                                {
                                                    ui.selectable_value(&mut series.marker_symbol, symbol.to_string(), tl!(symbol));
                                                }
                                            });
                                    });
                                    if !series.marker_symbol_editable {
                                        ui.label(tl!(tl!("Marker shape is read-only unless the chart has one simple explicit marker.")));
                                    }
                                    ui.label(tl!(tl!("Size")));
                                    ui.add_enabled_ui(series.marker_size_editable, |ui| {
                                        ui.add(egui::DragValue::new(&mut series.marker_size).range(2..=72).suffix(" pt"));
                                    });
                                    ui.label(tl!(tl!("Marker color")));
                                    ui.add_enabled_ui(series.marker_color_editable, |ui| {
                                        egui::color_picker::color_edit_button_srgb(ui, &mut series.marker_color);
                                    });
                                });
                            }
                            ui.horizontal(|ui| {
                                ui.label(tl!(tl!("Data labels")));
                                ui.add_enabled_ui(series.data_labels_editable, |ui| {
                                    if series.original_data_label_value.is_some() {
                                        ui.checkbox(&mut series.data_label_value, tl!(tl!("Value")));
                                    }
                                    if series.original_data_label_category.is_some() {
                                        ui.checkbox(&mut series.data_label_category, tl!(tl!("Category")));
                                    }
                                    if series.original_data_label_series.is_some() {
                                        ui.checkbox(&mut series.data_label_series, tl!(tl!("Series name")));
                                    }
                                    if let Some(original) = &series.original_data_label_position {
                                        let positions: &[&str] = match form.chart_kind.as_str() {
                                            "bar" | "column" => &["ctr", "inBase", "inEnd", "outEnd"],
                                            "line" | "area" | "scatter" => &["ctr", "t", "b", "l", "r"],
                                            "pie" | "doughnut" => &["ctr", "inEnd"],
                                            _ => &[],
                                        };
                                        if positions.contains(&original.as_str()) {
                                            egui::ComboBox::from_id_salt(("chart_data_label_position", series_index))
                                                .selected_text(series.data_label_position.as_str())
                                                .show_ui(ui, |ui| {
                                                    for position in positions {
                                                        ui.selectable_value(&mut series.data_label_position, (*position).to_string(), *position);
                                                    }
                                                });
                                        }
                                    }
                                });
                                ui.label(tl!(tl!("Only explicitly stored data-label properties can be changed; missing settings are not added.")));
                                if let Some(diagnostic) = &series.data_labels_diagnostic {
                                    ui.label(tl!(tl!(diagnostic.as_str())));
                                } else if !series.data_labels_editable {
                                    ui.label(tl!(tl!("Data labels are read-only for this OOXML structure.")));
                                }
                            });
                            for error_bar in &mut series.error_bars {
                                let axis = if error_bar.direction == "x" {
                                    "X"
                                } else if error_bar.direction == "y" {
                                    "Y"
                                } else {
                                    "unknown axis"
                                };
                                if error_bar.editable {
                                    ui.separator();
                                    ui.label(format!("Error bars ({axis}) · {}", error_bar.bar_type.as_deref().unwrap_or("unknown")));
                                    match error_bar.value_type.as_deref() {
                                        Some("fixedVal") => {
                                            ui.horizontal(|ui| {
                                                ui.label(tl!(tl!("Fixed magnitude")));
                                                ui.add_sized(
                                                    [100.0, 22.0],
                                                    egui::TextEdit::singleline(&mut error_bar.fixed_value).id_salt((
                                                        "chart_error_fixed",
                                                        series_index,
                                                        &error_bar.direction,
                                                    )),
                                                );
                                            });
                                        }
                                        Some("cust") => {
                                            for side in 0..2 {
                                                let (label, values) = if side == 0 {
                                                    ("Positive", &mut error_bar.positive)
                                                } else {
                                                    ("Negative", &mut error_bar.negative)
                                                };
                                                if values.is_empty() {
                                                    continue;
                                                }
                                                ui.label(tl!(tl!(label)));
                                                ui.horizontal_wrapped(|ui| {
                                                    for (point, value) in values.iter_mut().enumerate() {
                                                        ui.add_sized(
                                                            [72.0, 22.0],
                                                            egui::TextEdit::singleline(value).id_salt((
                                                                "chart_error_literal",
                                                                series_index,
                                                                &error_bar.direction,
                                                                label,
                                                                point,
                                                            )),
                                                        );
                                                    }
                                                });
                                            }
                                        }
                                        _ => {}
                                    }
                                } else {
                                    let reason = error_bar.diagnostic.as_deref().unwrap_or(if error_bar.renderable {
                                        "this error-bar value source is preserved but not editable"
                                    } else {
                                        "unsupported error-bar structure"
                                    });
                                    ui.label(format!("Error bars ({axis}) are read-only: {}", reason));
                                }
                            }
                            ui.horizontal(|ui| {
                                ui.label(format!("{} {}", tl!(tl!("Series")), series_index + 1));
                                let label = ui.label(tl!(tl!("Series name")));
                                let response = ui
                                    .add_sized(
                                        [210.0, 24.0],
                                        egui::TextEdit::singleline(&mut series.name).id_salt(("chart_series_name", series_index)),
                                    )
                                    .labelled_by(label.id);
                                focus_ids.push(response.id);
                            });
                            egui::Grid::new(("chart_data_grid", series_index)).striped(true).spacing(vec2(8.0, 4.0)).show(ui, |ui| {
                                let category_label = ui.strong(if form.chart_kind == "scatter" { "X" } else { tl!(tl!("Category")) });
                                let value_label = ui.strong(if form.chart_kind == "scatter" { "Y" } else { tl!(tl!("Value")) });
                                ui.end_row();
                                let point_count = if form.chart_kind == "scatter" {
                                    series.x_values.len().min(series.values.len())
                                } else {
                                    series.categories.len().min(series.values.len())
                                };
                                for point in 0..point_count {
                                    let category =
                                        if form.chart_kind == "scatter" { series.x_values.get_mut(point) } else { series.categories.get_mut(point) };
                                    let value = series.values.get_mut(point);
                                    if let (Some(category), Some(value)) = (category, value) {
                                        let response = ui
                                            .add_sized(
                                                [220.0, 24.0],
                                                egui::TextEdit::singleline(category).id_salt(("chart_category", series_index, point)),
                                            )
                                            .labelled_by(category_label.id);
                                        focus_ids.push(response.id);
                                        let response = ui
                                            .add_sized([130.0, 24.0], egui::TextEdit::singleline(value).id_salt(("chart_value", series_index, point)))
                                            .labelled_by(value_label.id);
                                        focus_ids.push(response.id);
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    }
                });
            });
        });
        if let Some((current, reverse)) = tab_from
            && let Some(index) = focus_ids.iter().position(|id| *id == current)
            && !focus_ids.is_empty()
        {
            let next = if reverse { (index + focus_ids.len() - 1) % focus_ids.len() } else { (index + 1) % focus_ids.len() };
            if let Some(next_id) = focus_ids.get(next) {
                ui.memory_mut(|memory| memory.request_focus(*next_id));
            }
        }
        let invalid_value = form.series.iter().enumerate().find_map(|(series_index, series)| {
            series.values.iter().chain(series.x_values.iter()).enumerate().find_map(|(point_index, value)| {
                value.trim().parse::<f64>().ok().filter(|value| value.is_finite()).is_none().then(|| {
                    format!(
                        "{} {}, {} {}: {}",
                        tl!(tl!("Series")),
                        series_index + 1,
                        tl!(tl!("Value")),
                        point_index + 1,
                        tl!(tl!("Enter a valid number"))
                    )
                })
            })
        });
        let invalid_axis = [form.horizontal_axis.as_ref(), form.vertical_axis.as_ref()].into_iter().flatten().find_map(|axis| {
            if !axis.editable {
                return None;
            }
            let minimum = if axis.minimum_manual { axis.minimum.trim().parse::<f64>().ok().filter(|value| value.is_finite()) } else { None };
            let maximum = if axis.maximum_manual { axis.maximum.trim().parse::<f64>().ok().filter(|value| value.is_finite()) } else { None };
            ((axis.minimum_manual && minimum.is_none())
                || (axis.maximum_manual && maximum.is_none())
                || minimum.zip(maximum).is_some_and(|(a, b)| a >= b || !(b - a).is_finite()))
            .then(|| "Axis bounds must be finite numbers and the minimum must be less than the maximum.".to_string())
        });
        let legend_changed = form.original_legend_position.as_deref().is_some_and(|original| form.legend_position != original);
        let chart_title_change = changed_existing_title(form.original_chart_title.as_deref(), &form.chart_title);
        let horizontal_axis_title_change = changed_existing_title(form.original_horizontal_axis_title.as_deref(), &form.horizontal_axis_title);
        let vertical_axis_title_change = changed_existing_title(form.original_vertical_axis_title.as_deref(), &form.vertical_axis_title);
        let horizontal_axis_change = form.horizontal_axis.as_ref().and_then(axis_change_json);
        let vertical_axis_change = form.vertical_axis.as_ref().and_then(axis_change_json);
        let chart_title_changed = chart_title_change.is_some();
        let horizontal_axis_title_changed = horizontal_axis_title_change.is_some();
        let vertical_axis_title_changed = vertical_axis_title_change.is_some();
        let changed = form.series != form.original
            || legend_changed
            || chart_title_changed
            || horizontal_axis_title_changed
            || vertical_axis_title_changed
            || horizontal_axis_change.is_some()
            || vertical_axis_change.is_some();
        let invalid_title = [
            (chart_title_changed, form.chart_title.as_str()),
            (horizontal_axis_title_changed, form.horizontal_axis_title.as_str()),
            (vertical_axis_title_changed, form.vertical_axis_title.as_str()),
        ]
        .iter()
        .any(|(changed, value)| *changed && (value.trim().is_empty() || value.trim() != *value || value.len() > 4096));
        let can_apply = changed && invalid_value.is_none() && invalid_axis.is_none() && !invalid_title;
        if let Some(message) = invalid_value {
            ui.colored_label(Tokens::get(ui.ctx()).red, message);
        }
        if let Some(message) = invalid_axis {
            ui.colored_label(Tokens::get(ui.ctx()).red, tl!(tl!(message.as_str())));
        }
        if invalid_title {
            ui.colored_label(
                Tokens::get(ui.ctx()).red,
                tl!(tl!("Titles cannot be blank or have leading/trailing whitespace; title removal is not supported.")),
            );
        }
        if !form.error.is_empty() {
            ui.colored_label(Tokens::get(ui.ctx()).red, &form.error);
        }
        ui.separator();
        let (apply, cancel) = ui
            .horizontal(|ui| (ui.add_enabled(can_apply, egui::Button::new(tl!(tl!("Apply")))).clicked(), ui.button(tl!(tl!("Cancel"))).clicked()))
            .inner;
        if cancel || ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            return true;
        }
        if apply {
            let Some(series) = chart_series_changes(&form.chart_kind, &form.series, &form.original) else {
                form.error = tl!(tl!("Enter a valid number")).to_string();
                return false;
            };
            let Some(expected_snapshot) = form.original_snapshot.as_ref() else {
                form.error = "the original chart snapshot is missing; reopen the chart editor".into();
                return false;
            };
            match app.run(
                "chart.editData",
                json!({
                    "documentId": form.document_id,
                    "chartPart": form.chart_part,
                    "expected": expected_snapshot,
                    "legendPosition": legend_changed.then_some(form.legend_position.as_str()),
                    "chartTitle": chart_title_change,
                    "horizontalAxisTitle": horizontal_axis_title_change,
                    "verticalAxisTitle": vertical_axis_title_change,
                    "horizontalAxis": horizontal_axis_change,
                    "verticalAxis": vertical_axis_change,
                    "series": series
                }),
            ) {
                Ok(_) => return true,
                Err(error) => form.error = error,
            }
        }
    }
    if !form.read_only_reason.is_empty() && (ui.button(tl!(tl!("Close"))).clicked() || ui.input(|input| input.key_pressed(egui::Key::Escape))) {
        return true;
    }
    false
}

fn legend_position_label(position: &str) -> String {
    match position {
        "l" => tl!(tl!("Left")).to_string(),
        "t" => tl!(tl!("Top")).to_string(),
        "b" => tl!(tl!("Bottom")).to_string(),
        _ => tl!(tl!("Right")).to_string(),
    }
}

fn chart_axis_form(value: &Value) -> Option<ChartAxisForm> {
    let minimum = value.get("minimum").and_then(Value::as_f64);
    let maximum = value.get("maximum").and_then(Value::as_f64);
    Some(ChartAxisForm {
        role: value.get("role")?.as_str()?.to_string(),
        editable: value.get("editable").and_then(Value::as_bool).unwrap_or(false),
        number_format_editable: value.get("numberFormatEditable").and_then(Value::as_bool).unwrap_or(false),
        diagnostic: value.get("diagnostic").and_then(Value::as_str).map(str::to_string),
        minimum_manual: minimum.is_some(),
        minimum: minimum.map_or_else(String::new, |number| number.to_string()),
        original_minimum: minimum,
        maximum_manual: maximum.is_some(),
        maximum: maximum.map_or_else(String::new, |number| number.to_string()),
        original_maximum: maximum,
        number_format: value.get("numberFormat").and_then(Value::as_str).unwrap_or("General").to_string(),
        original_number_format: value.get("numberFormat").and_then(Value::as_str).map(str::to_string),
    })
}

fn chart_axis_label(physical: &str, role: &str) -> &'static str {
    match (physical, role) {
        (_, "scatterX") => "Scatter X axis",
        (_, "scatterY") => "Scatter Y axis",
        ("Horizontal", "category") => "Horizontal category axis",
        ("Vertical", "category") => "Vertical category axis",
        ("Horizontal", _) => "Horizontal value axis",
        _ => "Vertical value axis",
    }
}

fn chart_axis_controls(ui: &mut Ui, label: &str, axis: &mut ChartAxisForm) {
    ui.group(|ui| {
        ui.label(format!("{label} · {}", axis.role));
        if let Some(diagnostic) = &axis.diagnostic {
            ui.label(egui::RichText::new(diagnostic).small().weak());
        }
        ui.add_enabled_ui(axis.editable, |ui| {
            ui.horizontal(|ui| {
                ui.label(tl!(tl!("Minimum")));
                egui::ComboBox::from_id_salt(("chart_axis_min_mode", &axis.role))
                    .selected_text(if axis.minimum_manual { tl!(tl!("Manual")) } else { tl!(tl!("Auto")) })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut axis.minimum_manual, false, tl!(tl!("Auto")));
                        ui.selectable_value(&mut axis.minimum_manual, true, tl!(tl!("Manual")));
                    });
                if axis.minimum_manual {
                    ui.add_sized([100.0, 24.0], egui::TextEdit::singleline(&mut axis.minimum));
                }
                ui.label(tl!(tl!("Maximum")));
                egui::ComboBox::from_id_salt(("chart_axis_max_mode", &axis.role))
                    .selected_text(if axis.maximum_manual { tl!(tl!("Manual")) } else { tl!(tl!("Auto")) })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut axis.maximum_manual, false, tl!(tl!("Auto")));
                        ui.selectable_value(&mut axis.maximum_manual, true, tl!(tl!("Manual")));
                    });
                if axis.maximum_manual {
                    ui.add_sized([100.0, 24.0], egui::TextEdit::singleline(&mut axis.maximum));
                }
            });
            ui.add_enabled_ui(axis.number_format_editable, |ui| {
                ui.label(tl!(tl!("Number format")));
                egui::ComboBox::from_id_salt(("chart_axis_number_format", &axis.role)).selected_text(&axis.number_format).show_ui(ui, |ui| {
                    for format in ["General", "0", "#,##0", "0.0", "0.00", "0%", "0.0%", "0.00%", "0.00E+00"] {
                        ui.selectable_value(&mut axis.number_format, format.to_string(), format);
                    }
                });
            });
        });
    });
}

fn axis_change_json(axis: &ChartAxisForm) -> Option<Value> {
    if !axis.editable {
        return None;
    }
    let bound = |manual: bool, value: &str, original: Option<f64>| -> Option<Value> {
        if !manual {
            return original.map(|_| json!({"mode":"auto"}));
        }
        let parsed = value.trim().parse::<f64>().ok().filter(|value| value.is_finite())?;
        (Some(parsed) != original).then(|| json!({"mode":"manual", "value":parsed}))
    };
    let minimum = bound(axis.minimum_manual, &axis.minimum, axis.original_minimum);
    let maximum = bound(axis.maximum_manual, &axis.maximum, axis.original_maximum);
    let format = (axis.number_format_editable && axis.number_format != axis.original_number_format.as_deref().unwrap_or("General"))
        .then(|| axis.number_format.clone());
    (minimum.is_some() || maximum.is_some() || format.is_some()).then(|| {
        let mut change = serde_json::Map::new();
        change.insert("minimum".into(), minimum.unwrap_or(Value::Null));
        change.insert("maximum".into(), maximum.unwrap_or(Value::Null));
        change.insert("numberFormat".into(), format.map_or(Value::Null, Value::String));
        Value::Object(change.into_iter().filter(|(_, value)| !value.is_null()).collect())
    })
}

fn changed_existing_title<'a>(original: Option<&str>, draft: &'a str) -> Option<&'a str> {
    original.and_then(|original| (draft != original).then_some(draft))
}

fn body(app: &mut WordApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    match d {
        Dialog::Recovery { state } => crate::recovery::show(app, ui, state),
        Dialog::ChartData { form } => chart_data(app, ui, form),
        Dialog::Mendeley { ris, query, error } => {
            ui.label("In Mendeley: File → Export All → RIS. Paste the exported text below.");
            #[cfg(not(target_arch = "wasm32"))]
            if ui.button("Choose RIS File…").clicked() {
                let _ = app
                    .ask_file(crate::file_dialogs::FileDialogRequest::Open { purpose: "mendeley".into() }, crate::file_dialogs::AfterPick::Mendeley);
            }
            ui.add(egui::TextEdit::multiline(ris).desired_rows(6).desired_width(520.0).char_limit(8 * 1024 * 1024));
            if ui.button("Import RIS").clicked() {
                match app.run("mendeley.import", json!({"ris": ris})) {
                    Ok(result) => {
                        *error = String::new();
                        app.status(format!("Mendeley: {} added, {} updated", result["added"], result["updated"]));
                    }
                    Err(e) => *error = e,
                }
            }
            if !error.is_empty() {
                ui.colored_label(Tokens::get(ui.ctx()).red, error.as_str());
            }
            ui.separator();
            ui.label("Search imported references");
            ui.text_edit_singleline(query);
            let needle = query.to_lowercase();
            let sources: Vec<_> = app
                .session
                .doc
                .sources
                .iter()
                .filter(|s| s.tag.starts_with("Mendeley_") && format!("{} {} {}", s.title, s.author, s.year).to_lowercase().contains(&needle))
                .take(100)
                .cloned()
                .collect();
            egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                for source in sources {
                    ui.horizontal(|ui| {
                        ui.label(format!("{} ({}) — {}", source.author, source.year, source.title));
                        if ui.button("Insert Citation").clicked() {
                            match app.run("references.citation", json!({"tag": source.tag})) {
                                Ok(_) => *error = String::new(),
                                Err(e) => *error = e,
                            }
                        }
                    });
                }
            });
            ui.label("Citation styles");
            ui.horizontal(|ui| {
                for style in &wordcraft_engine::cmd::citations::STYLES {
                    if ui.selectable_label(app.session.bib_style == style.name, style.name).clicked()
                        && let Err(e) = app.run("references.citationStyle", json!({"style": style.name}))
                    {
                        *error = e;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Insert Bibliography").clicked()
                    && let Err(e) = app.run("references.bibliography", json!({}))
                {
                    *error = e;
                }
                if ui.button("Refresh").clicked()
                    && let Err(e) = app.run("mendeley.refresh", json!({}))
                {
                    *error = e;
                }
            });
            ui.button("Close").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape))
        }
        Dialog::Sources { form } => citation_sources(app, ui, form),
        Dialog::Recipients { csv, error } => {
            ui.label(tl!("Paste CSV data with field names in the first row."));
            ui.add(egui::TextEdit::multiline(csv).desired_rows(10).desired_width(480.0).char_limit(1_000_000));
            if !error.is_empty() {
                ui.colored_label(Tokens::get(ui.ctx()).red, error.as_str());
            }
            // Enter adds a CSV row; only the button submits this multiline form.
            let (ok, cancel) = ui.horizontal(|ui| (ui.button(tl!("Use Recipients")).clicked(), ui.button(tl!("Cancel")).clicked())).inner;
            if cancel || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                return true;
            }
            if ok {
                match app.run("mailings.recipients", json!({"csv": csv})) {
                    Ok(_) => return true,
                    Err(e) => *error = e,
                }
            }
            false
        }
        Dialog::Font { font, size, bold, italic, underline, strike, sup, sub, small_caps, caps, hidden, color, spacing, .. } => {
            // Build the sample text format for the preview pane below.
            let preview_size = size.trim().parse::<f32>().unwrap_or(12.0).clamp(6.0, 72.0);
            let preview_color = wordcraft_doc::Rgb::parse(color).map(crate::theme::c32).unwrap_or_else(|| ui.visuals().text_color());
            let mut fmt = egui::TextFormat {
                font_id: egui::FontId::new(preview_size, egui::FontFamily::Proportional),
                color: preview_color,
                ..Default::default()
            };
            if *bold {
                fmt.font_id = crate::theme::semibold(preview_size);
            }
            if *italic {
                fmt.italics = true;
            }
            if *underline {
                fmt.underline = egui::Stroke::new(1.0, preview_color);
            }
            if *strike {
                fmt.strikethrough = egui::Stroke::new(1.0, preview_color);
            }
            egui::Grid::new("fontdlg").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label(tl!("Font:"));
                ui.text_edit_singleline(font);
                ui.end_row();
                ui.label(tl!("Size:"));
                ui.text_edit_singleline(size);
                ui.end_row();
                ui.label(tl!("Font color:"));
                ui.text_edit_singleline(color);
                ui.end_row();
                ui.label(tl!("Spacing (pt):"));
                ui.add(egui::DragValue::new(spacing).speed(0.1).range(-20.0..=20.0));
                ui.end_row();
            });
            ui.separator();
            ui.label(egui::RichText::new(tl!("Effects")).font(semibold(12.5)));
            egui::Grid::new("fx").num_columns(2).show(ui, |ui| {
                ui.checkbox(bold, tl!("Bold"));
                ui.checkbox(italic, tl!("Italic"));
                ui.end_row();
                ui.checkbox(underline, tl!("Underline"));
                ui.checkbox(strike, tl!("Strikethrough"));
                ui.end_row();
                ui.checkbox(sup, tl!("Superscript"));
                ui.checkbox(sub, tl!("Subscript"));
                ui.end_row();
                ui.checkbox(small_caps, tl!("Small caps"));
                ui.checkbox(caps, tl!("All caps"));
                ui.end_row();
                ui.checkbox(hidden, tl!("Hidden"));
                ui.end_row();
            });
            // Live preview of the chosen font/size/colour/effects.
            ui.separator();
            ui.label(egui::RichText::new(tl!("Preview")).small().weak());
            let sample = if *caps { tl!("AaBbYyZz").to_uppercase() } else { tl!("AaBbYyZz").to_string() };
            let mut job = egui::text::LayoutJob::single_section(sample, fmt);
            job.wrap.max_width = ui.available_width().min(400.0);
            ui.label(job);
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                apply_font(app, d);
            }
            ok || cancel
        }
        Dialog::Paragraph {
            rtl,
            align,
            left,
            right,
            first,
            before,
            after,
            line,
            keep_next,
            keep_lines,
            page_break,
            widow,
            tab,
            asian,
            asian_was,
            opts,
            opts_was,
        } => {
            ui.horizontal(|ui| {
                for (k, name) in ["Indents and Spacing", "Line and Page Breaks", "Asian Typography"].into_iter().enumerate() {
                    if ui.selectable_label(*tab as usize == k, tl!(name)).clicked() {
                        *tab = k as u8;
                    }
                }
            });
            ui.separator();
            match *tab {
                0 => {
                    ui.label(egui::RichText::new(tl!("General")).font(semibold(12.5)));
                    ui.horizontal(|ui| {
                        ui.label(tl!("Direction:"));
                        ui.radio_value(rtl, true, tl!("Right-to-left"));
                        ui.radio_value(rtl, false, tl!("Left-to-right"));
                    });
                    const ALIGNS: [(&str, &str); 4] = [("left", "Left"), ("center", "Center"), ("right", "Right"), ("justify", "Justify")];
                    ui.horizontal(|ui| {
                        ui.label(tl!("Alignment:"));
                        let shown = ALIGNS.iter().find(|(v, _)| v == align).map_or(align.as_str(), |(_, l)| tl!(l));
                        egui::ComboBox::from_id_salt("para_align").selected_text(shown).show_ui(ui, |ui| {
                            for (v, l) in ALIGNS {
                                ui.selectable_value(align, v.to_string(), tl!(l));
                            }
                        });
                    });
                    ui.label(egui::RichText::new(tl!("Indentation")).font(semibold(12.5)));
                    // Mirrored indents are the inside and outside ones (they swap on even pages).
                    let (l_label, r_label) = if opts[0] { ("Inside:", "Outside:") } else { ("Left:", "Right:") };
                    egui::Grid::new("ind").num_columns(4).show(ui, |ui| {
                        ui.label(tl!(l_label));
                        ui.add(
                            egui::DragValue::new(left)
                                .speed(3.6 / crate::options::unit().pt_per_unit())
                                .suffix(crate::options::unit().suffix())
                                .max_decimals(2),
                        );
                        ui.label(tl!(r_label));
                        ui.add(
                            egui::DragValue::new(right)
                                .speed(3.6 / crate::options::unit().pt_per_unit())
                                .suffix(crate::options::unit().suffix())
                                .max_decimals(2),
                        );
                        ui.end_row();
                        ui.label(tl!("First line:"));
                        ui.add(
                            egui::DragValue::new(first)
                                .speed(3.6 / crate::options::unit().pt_per_unit())
                                .suffix(crate::options::unit().suffix())
                                .max_decimals(2),
                        );
                        ui.end_row();
                    });
                    ui.checkbox(&mut opts[0], tl!("Mirror indents"));
                    ui.label(egui::RichText::new(tl!("Spacing")).font(semibold(12.5)));
                    egui::Grid::new("sp").num_columns(4).show(ui, |ui| {
                        ui.label(tl!("Before:"));
                        ui.add(egui::DragValue::new(before).speed(1.0).range(0.0..=1584.0).suffix(format!(" {}", tl!("pt"))));
                        ui.label(tl!("Line spacing:"));
                        ui.add(egui::DragValue::new(line).speed(0.05).range(0.5..=5.0));
                        ui.end_row();
                        ui.label(tl!("After:"));
                        ui.add(egui::DragValue::new(after).speed(1.0).range(0.0..=1584.0).suffix(format!(" {}", tl!("pt"))));
                        ui.end_row();
                    });
                    ui.checkbox(&mut opts[1], tl!("Don't add space between paragraphs of the same style"));
                }
                1 => {
                    ui.label(egui::RichText::new(tl!("Line and Page Breaks")).font(semibold(12.5)));
                    ui.checkbox(widow, tl!("Widow/Orphan control"));
                    ui.checkbox(keep_next, tl!("Keep with next"));
                    ui.checkbox(keep_lines, tl!("Keep lines together"));
                    ui.checkbox(page_break, tl!("Page break before"));
                    ui.label(egui::RichText::new(tl!("Formatting exceptions")).font(semibold(12.5)));
                    ui.checkbox(&mut opts[2], tl!("Suppress line numbers"));
                    ui.checkbox(&mut opts[3], tl!("Don't hyphenate"));
                }
                _ => {
                    let [kinsoku, word_wrap, overflow, top_line, de, dn] = asian;
                    ui.label(egui::RichText::new(tl!("Line breaking")).font(semibold(12.5)));
                    ui.checkbox(kinsoku, tl!("Apply Asian line-breaking rules (kinsoku)"));
                    // The model's flag is "wrap whole words"; the box offers the opposite.
                    let mut mid_word = !*word_wrap;
                    if ui.checkbox(&mut mid_word, tl!("Allow Latin text to wrap in the middle of a word")).changed() {
                        *word_wrap = !mid_word;
                    }
                    ui.checkbox(overflow, tl!("Allow hanging punctuation"));
                    ui.label(egui::RichText::new(tl!("Character Spacing")).font(semibold(12.5)));
                    ui.checkbox(top_line, tl!("Compress punctuation at the start of a line"));
                    ui.checkbox(de, tl!("Add space between Asian and Latin text"));
                    ui.checkbox(dn, tl!("Add space between Asian text and numbers"));
                }
            }
            ui.add_space(6.0);
            let tabs = ui.button(tl!("Tabs…")).clicked();
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok || tabs {
                apply_paragraph(
                    app,
                    *rtl,
                    align,
                    *left,
                    *right,
                    *first,
                    *before,
                    *after,
                    *line,
                    [*keep_next, *keep_lines, *page_break, *widow],
                    (*opts, *opts_was),
                );
                apply_asian(app, *asian, *asian_was);
            }
            // Tabs… keeps what was set here and moves on to the Tabs dialog.
            if tabs {
                app.dialog = Dialog::open("tabs", app);
            }
            ok || cancel || tabs
        }
        Dialog::Tabs { form } => crate::dialogs_para::tabs(app, ui, form),
        Dialog::Borders { form } => crate::dialogs_para::borders(app, ui, form),
        Dialog::Find { query, replace, match_case, whole_word, regex, replace_mode, message } => {
            ui.horizontal(|ui| {
                if ui.selectable_label(!*replace_mode, tl!("Find")).clicked() {
                    *replace_mode = false;
                }
                if ui.selectable_label(*replace_mode, tl!("Replace")).clicked() {
                    *replace_mode = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label(tl!("Find what:"));
                ui.add(egui::TextEdit::singleline(query).desired_width(240.0));
            });
            if *replace_mode {
                ui.horizontal(|ui| {
                    ui.label(tl!("Replace with:"));
                    ui.add(egui::TextEdit::singleline(replace).desired_width(228.0));
                });
            }
            ui.horizontal(|ui| {
                ui.checkbox(match_case, tl!("Match case"));
                ui.checkbox(whole_word, tl!("Whole words"));
                ui.checkbox(regex, tl!("Wildcards (regex)"));
            });
            let opts = json!({"text": query, "with": replace, "matchCase": *match_case, "wholeWord": *whole_word, "regex": *regex});
            if !*replace_mode {
                ui.horizontal(|ui| {
                    let lit = app.session.find.highlight;
                    if ui.selectable_label(lit, tl!("Reading Highlight")).clicked() {
                        let mut v = opts.clone();
                        v["highlight"] = json!(!lit);
                        match app.run("edit.advancedFind", v) {
                            Ok(r) if !lit => *message = crate::i18n::fmt(tl!("{count} items highlighted"), &[("count", &r["count"].to_string())]),
                            Ok(_) => message.clear(),
                            Err(e) => *message = e,
                        }
                    }
                });
            }
            if !message.is_empty() {
                ui.label(egui::RichText::new(message.as_str()).weak());
            }
            let mut close = false;
            ui.horizontal(|ui| {
                if *replace_mode {
                    if ui.button(tl!("Replace All")).clicked() {
                        match app.run("edit.replaceAll", opts.clone()) {
                            Ok(r) => *message = format!("All done. We made {} replacements.", r["replaced"]),
                            Err(e) => *message = e,
                        }
                    }
                    if ui.button(tl!("Replace")).clicked() {
                        let _ = app.run("edit.replace", opts.clone());
                    }
                }
                if ui.button(tl!("Find Next")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let _ = app.session.run("edit.find", &opts);
                    match app.run("edit.findNext", json!({})) {
                        Ok(r) => *message = format!("Match {} of {}", r["index"].as_u64().unwrap_or(0) + 1, r["count"]),
                        Err(e) => *message = e,
                    }
                }
                if ui.button(tl!("Close")).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
            close
        }
        Dialog::Goto { page } => {
            ui.horizontal(|ui| {
                ui.label(tl!("Enter page number:"));
                ui.text_edit_singleline(page);
            });
            let (ok, cancel) = buttons(ui, tl!("Go To"));
            if ok && let Ok(n) = page.trim().parse::<u64>() {
                let _ = app.run("edit.goto", json!({"page": n}));
            }
            ok || cancel
        }
        Dialog::Enclose { style, text, shape } => {
            ui.label(egui::RichText::new(tl!("Style")).font(semibold(12.5)));
            ui.horizontal(|ui| {
                for (v, l) in [("none", "None"), ("shrink", "Shrink text"), ("enlarge", "Enlarge symbol")] {
                    ui.radio_value(style, v.to_string(), tl!(l));
                }
            });
            ui.add_space(6.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.label(tl!("Text:"));
                    ui.add(egui::TextEdit::singleline(text).desired_width(110.0).char_limit(2));
                    ui.add_space(6.0);
                    enclose_preview(ui, style, text, shape);
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.label(tl!("Enclosure"));
                    for (v, l) in [("circle", "Circle"), ("square", "Square"), ("triangle", "Triangle"), ("diamond", "Diamond")] {
                        ui.add_enabled_ui(style != "none", |ui| ui.selectable_value(shape, v.to_string(), tl!(l)));
                    }
                });
            });
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let params = if style == "none" { json!({"shape": null}) } else { json!({"shape": shape, "style": style, "text": text}) };
                let _ = app.run("format.enclose", params);
            }
            ok || cancel
        }
        Dialog::TableProperties { form, basis } => {
            table_properties(ui, form);
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let changes = form.changes(basis);
                if changes.as_object().is_some_and(|m| !m.is_empty()) {
                    let _ = app.run("table.properties", changes);
                }
            }
            ok || cancel
        }
        Dialog::InsertTable { rows, cols } => {
            egui::Grid::new("it").num_columns(2).show(ui, |ui| {
                ui.label(tl!("Number of columns:"));
                ui.add(egui::DragValue::new(cols).range(1..=63));
                ui.end_row();
                ui.label(tl!("Number of rows:"));
                ui.add(egui::DragValue::new(rows).range(1..=1000));
                ui.end_row();
            });
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let _ = app.run("insert.table", json!({"rows": *rows, "cols": *cols}));
            }
            ok || cancel
        }
        Dialog::PageSetup { top, bottom, left, right, landscape, rtl, grid, grid_was } => {
            ui.label(egui::RichText::new(tl!("Margins")).font(semibold(12.5)));
            egui::Grid::new("ps").num_columns(4).show(ui, |ui| {
                ui.label(tl!("Top:"));
                ui.add(
                    egui::DragValue::new(top)
                        .speed(3.6 / crate::options::unit().pt_per_unit())
                        .suffix(crate::options::unit().suffix())
                        .max_decimals(2),
                );
                ui.label(tl!("Bottom:"));
                ui.add(
                    egui::DragValue::new(bottom)
                        .speed(3.6 / crate::options::unit().pt_per_unit())
                        .suffix(crate::options::unit().suffix())
                        .max_decimals(2),
                );
                ui.end_row();
                ui.label(tl!("Left:"));
                ui.add(
                    egui::DragValue::new(left)
                        .speed(3.6 / crate::options::unit().pt_per_unit())
                        .suffix(crate::options::unit().suffix())
                        .max_decimals(2),
                );
                ui.label(tl!("Right:"));
                ui.add(
                    egui::DragValue::new(right)
                        .speed(3.6 / crate::options::unit().pt_per_unit())
                        .suffix(crate::options::unit().suffix())
                        .max_decimals(2),
                );
                ui.end_row();
            });
            ui.label(egui::RichText::new(tl!("Orientation")).font(semibold(12.5)));
            ui.horizontal(|ui| {
                ui.radio_value(landscape, false, tl!("Portrait"));
                ui.radio_value(landscape, true, tl!("Landscape"));
            });
            ui.label(egui::RichText::new(tl!("Document Grid")).font(semibold(12.5)));
            ui.horizontal_wrapped(|ui| {
                ui.radio_value(&mut grid.0, 0, tl!("No grid"));
                ui.radio_value(&mut grid.0, 1, tl!("Lines only"));
                ui.radio_value(&mut grid.0, 2, tl!("Lines and characters"));
                ui.radio_value(&mut grid.0, 3, tl!("Text snaps to characters"));
            });
            egui::Grid::new("ps_grid").num_columns(4).show(ui, |ui| {
                ui.add_enabled(grid.0 >= 2, egui::Label::new(tl!("Characters per line:")));
                ui.add_enabled(grid.0 >= 2, egui::DragValue::new(&mut grid.2).range(1..=1000));
                ui.add_enabled(grid.0 >= 1, egui::Label::new(tl!("Lines per page:")));
                ui.add_enabled(grid.0 >= 1, egui::DragValue::new(&mut grid.1).range(1..=1000));
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.label(tl!("Section direction:"));
                ui.radio_value(rtl, false, tl!("Left-to-right"));
                ui.radio_value(rtl, true, tl!("Right-to-left"));
            });
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let _ = app.run("layout.orientation", json!({"value": if *landscape { "landscape" } else { "portrait" }}));
                let _ =
                    app.run("layout.margins", json!({"top": *top * crate::options::unit().pt_per_unit(), "bottom": *bottom * crate::options::unit().pt_per_unit(), "left": *left * crate::options::unit().pt_per_unit(), "right": *right * crate::options::unit().pt_per_unit()}));
                if grid != grid_was {
                    let kind = ["none", "lines", "linesAndChars", "snapToChars"].get(grid.0 as usize).copied().unwrap_or("none");
                    let mut v = json!({"type": kind});
                    if grid.1 != grid_was.1 {
                        v["linesPerPage"] = json!(grid.1);
                    }
                    if grid.2 != grid_was.2 {
                        v["charsPerLine"] = json!(grid.2);
                    }
                    let _ = app.run("layout.documentGrid", v);
                }
                if *rtl != wordcraft_engine::cmd::page::sect(&app.session).rtl {
                    let _ = app.run("layout.sectionRtl", json!({"value": *rtl}));
                }
            }
            ok || cancel
        }
        Dialog::Link { url, text } => {
            egui::Grid::new("lnk").num_columns(2).show(ui, |ui| {
                ui.label(tl!("Text to display:"));
                ui.text_edit_singleline(text);
                ui.end_row();
                ui.label(tl!("Address:"));
                ui.text_edit_singleline(url);
                ui.end_row();
            });
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let _ = app.run("insert.link", json!({"url": url, "text": text}));
            }
            ok || cancel
        }
        Dialog::Bookmark { name } => {
            ui.label(tl!("Bookmark name:"));
            ui.text_edit_singleline(name);
            let marks = app.session.doc.bookmarks();
            for (n, _) in &marks {
                if ui.selectable_label(false, n).clicked() {
                    let _ = app.run("edit.goto", json!({"bookmark": n}));
                }
            }
            let (ok, cancel) = buttons(ui, tl!("Add"));
            if ok {
                let _ = app.run("insert.bookmark", json!({"name": name}));
            }
            ok || cancel
        }
        Dialog::WordCount { stats } => {
            egui::Grid::new("wc").num_columns(2).spacing(vec2(30.0, 6.0)).show(ui, |ui| {
                for (l, k) in [
                    ("Pages", "pages"),
                    ("Words", "words"),
                    ("Characters (no spaces)", "characters"),
                    ("Characters (with spaces)", "charactersWithSpaces"),
                    ("Paragraphs", "paragraphs"),
                    ("Lines", "lines"),
                ] {
                    ui.label(tl!(l));
                    ui.label(stats.get(k).map(|v| v.to_string()).unwrap_or_default());
                    ui.end_row();
                }
            });
            let mut include = stats.get("includeTextBoxes").and_then(Value::as_bool).unwrap_or(true);
            if ui.checkbox(&mut include, tl!("Count words in text boxes and notes")).changed()
                && let Ok(v) = app.run("review.wordCount", json!({"includeTextBoxes": include}))
            {
                *stats = v;
            }
            let (ok, cancel) = buttons(ui, tl!("Close"));
            ok || cancel
        }
        Dialog::Zoom { percent } => {
            ui.horizontal(|ui| {
                for p in [200.0, 100.0, 75.0] {
                    ui.radio_value(percent, p, format!("{p}%"));
                }
            });
            // Logarithmic like the status bar slider, and wide enough to aim: the default 100 pt
            // linear slider jumped from 100% to 480% within a short drag (issue #67).
            ui.spacing_mut().slider_width = 220.0;
            ui.add(egui::Slider::new(percent, 10.0..=500.0).logarithmic(true).step_by(1.0).suffix("%"));
            let mut fit = None;
            ui.horizontal(|ui| {
                if ui.button(tl!("Page width")).clicked() {
                    fit = Some("pageWidth");
                }
                if ui.button(tl!("Whole page")).clicked() {
                    fit = Some("onePage");
                }
                if ui.button(tl!("Many pages")).clicked() {
                    fit = Some("multiplePages");
                }
            });
            if let Some(f) = fit {
                let _ = app.run("view.zoom", json!({"value": f}));
                return true;
            }
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let _ = app.run("view.zoom", json!({"value": *percent}));
            }
            ok || cancel
        }
        Dialog::Watermark { text, diagonal } => {
            ui.horizontal(|ui| {
                ui.label(tl!("Text:"));
                ui.text_edit_singleline(text);
            });
            ui.horizontal(|ui| {
                ui.radio_value(diagonal, true, tl!("Diagonal"));
                ui.radio_value(diagonal, false, tl!("Horizontal"));
            });
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let _ = app.run("design.watermark", json!({"text": text, "diagonal": *diagonal}));
            }
            ok || cancel
        }
        Dialog::Phonetic { base, ruby, align, size, offset, existing } => {
            // Base and ruby text are usually Chinese or Japanese: fonts for them load from the next frame.
            app.want_system_cjk = true;
            const ALIGNS: [(&str, &str); 6] = [
                ("center", "Center"),
                ("distributeLetter", "Distribute letters"),
                ("distributeSpace", "Distribute spacing"),
                ("left", "Left"),
                ("right", "Right"),
                ("rightVertical", "Right (vertical)"),
            ];
            egui::Grid::new("phonetic").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label(tl!("Base text:"));
                ui.label(egui::RichText::new(base.as_str()).strong());
                ui.end_row();
                ui.label(tl!("Ruby text:"));
                ui.text_edit_singleline(ruby);
                ui.end_row();
                ui.label(tl!("Alignment:"));
                let shown = ALIGNS.iter().find(|(k, _)| k == align).map_or("Center", |(_, l)| l);
                egui::ComboBox::from_id_salt("phonetic_align").selected_text(tl!(shown)).show_ui(ui, |ui| {
                    for (k, l) in ALIGNS {
                        ui.selectable_value(align, k.to_string(), tl!(l));
                    }
                });
                ui.end_row();
                ui.label(tl!("Offset (pt):"));
                ui.add(egui::DragValue::new(offset).speed(0.5).range(-100.0..=100.0));
                ui.end_row();
                ui.label(tl!("Size (pt):"));
                ui.add(egui::DragValue::new(size).speed(0.5).range(1.0..=200.0));
                ui.end_row();
            });
            let mut remove = false;
            if *existing {
                ui.add_space(4.0);
                remove = ui.button(tl!("Remove")).clicked();
            }
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if remove {
                let _ = app.run("format.phonetic", json!({"ruby": null}));
            } else if ok {
                let _ = app.run("format.phonetic", json!({"ruby": ruby, "align": align, "size": *size, "offset": *offset}));
            }
            ok || cancel || remove
        }
        Dialog::NewStyle { name, based_on, back } => {
            egui::Grid::new("ns").num_columns(2).show(ui, |ui| {
                ui.label(tl!("Name:"));
                ui.text_edit_singleline(name);
                ui.end_row();
                ui.label(tl!("Style based on:"));
                ui.text_edit_singleline(based_on);
                ui.end_row();
            });
            ui.label(egui::RichText::new(tl!("The new style takes the formatting of the current paragraph.")).small().weak());
            let (ok, cancel) = buttons(ui, tl!("OK"));
            let mut created = None;
            if ok {
                created =
                    app.run("styles.create", json!({"name": name, "basedOn": based_on})).ok().and_then(|v| v.get("id")?.as_str().map(str::to_string));
            }
            if (ok || cancel) && *back {
                app.dialog = Some(Dialog::ManageStyles { alphabetical: false, selected: created.unwrap_or_default() });
            }
            ok || cancel
        }
        Dialog::ModifyStyle { back, resume, id, name, font, size, bold, italic, color, before, after } => {
            egui::Grid::new("ms").num_columns(2).show(ui, |ui| {
                ui.label(tl!("Name:"));
                ui.text_edit_singleline(name);
                ui.end_row();
                ui.label(tl!("Font:"));
                ui.text_edit_singleline(font);
                ui.end_row();
                ui.label(tl!("Size:"));
                ui.add(egui::DragValue::new(size).range(1.0..=1638.0).speed(0.5));
                ui.end_row();
                ui.label(tl!("Color:"));
                ui.text_edit_singleline(color);
                ui.end_row();
                ui.label(tl!("Space before/after:"));
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(before).range(0.0..=1584.0));
                    ui.add(egui::DragValue::new(after).range(0.0..=1584.0));
                });
                ui.end_row();
            });
            ui.horizontal(|ui| {
                ui.checkbox(bold, tl!("Bold"));
                ui.checkbox(italic, tl!("Italic"));
            });
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let mut chr = json!({"font": font, "size": *size, "bold": *bold, "italic": *italic});
                if let Some(c) = wordcraft_doc::Rgb::parse(color) {
                    chr["color"] = json!({"Rgb": [c.0, c.1, c.2]});
                }
                let _ =
                    app.run("styles.modify", json!({"style": id, "name": name, "chr": chr, "para": {"spaceBefore": *before, "spaceAfter": *after}}));
            }
            if (ok || cancel) && *back {
                app.dialog = Some(Dialog::ManageStyles { alphabetical: false, selected: id.clone() });
            }
            if (ok || cancel)
                && let Some(r) = resume.take()
            {
                app.dialog = Some(*r);
            }
            ok || cancel
        }
        Dialog::ManageStyles { alphabetical, selected } => manage_styles(app, ui, alphabetical, selected),
        Dialog::TableStyle { id, name, based_on, region, regions, basis } => {
            let styles: Vec<(String, String)> = app
                .session
                .doc
                .styles
                .styles
                .iter()
                .filter(|s| s.kind == wordcraft_doc::StyleKind::Table && Some(&s.id) != id.as_ref())
                .map(|s| (s.id.clone(), s.name.clone()))
                .collect();
            let base_name = styles.iter().find(|(i, _)| i == based_on).map(|(_, n)| n.clone()).unwrap_or_else(|| based_on.clone());
            let mut rebased = false;
            egui::Grid::new("tstyle").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label(tl!("Name:"));
                ui.text_edit_singleline(name);
                ui.end_row();
                ui.label(tl!("Style based on:"));
                egui::ComboBox::from_id_salt("tstyle_base").width(200.0).selected_text(base_name).show_ui(ui, |ui| {
                    for (sid, sname) in &styles {
                        if ui.selectable_label(sid == based_on, sname).clicked() && sid != based_on {
                            *based_on = sid.clone();
                            rebased = true;
                        }
                    }
                });
                ui.end_row();
                ui.label(tl!("Apply formatting to:"));
                egui::ComboBox::from_id_salt("tstyle_region")
                    .width(200.0)
                    .selected_text(tl!(REGION_LABELS.get(*region).copied().unwrap_or("Whole Table")))
                    .show_ui(ui, |ui| {
                        for (i, n) in REGION_LABELS.iter().enumerate() {
                            ui.selectable_value(region, i, tl!(n));
                        }
                    });
                ui.end_row();
            });
            // A new style shows its new base's formatting; a modified one keeps its own.
            if rebased && id.is_none() {
                *regions = table_regions(app, based_on);
                *basis = regions.clone();
            }
            ui.separator();
            let theme = app.session.doc.settings.theme_colors.clone();
            if let Some(r) = regions.get_mut(*region) {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut r.borders, tl!("Borders"));
                    ui.checkbox(&mut r.bold, tl!("Bold"));
                });
                egui::Grid::new("tstyle_colors").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                    ui.label(tl!("Fill color:"));
                    color_menu(ui, &theme, &mut r.fill);
                    ui.end_row();
                    ui.label(tl!("Font color:"));
                    color_menu(ui, &theme, &mut r.color);
                    ui.end_row();
                });
            }
            // Live preview: the style as OK would leave it, on the current table's options with
            // the region being edited turned on.
            let v = table_style_params(id.as_deref(), name, based_on, regions, basis);
            let mut look = app
                .session
                .sel
                .focus
                .path
                .cell()
                .and_then(|(tp, _, _)| app.session.doc.table(app.session.sel.focus.story, &tp).map(|t| t.props.look))
                .unwrap_or_default();
            match *region {
                1 => look.header_row = true,
                2 => look.total_row = true,
                3 => look.first_column = true,
                4 => look.last_column = true,
                5 => look.banded_rows = true,
                6 => look.banded_columns = true,
                _ => {}
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(tl!("Preview")).small().weak());
            table_style_preview(ui, wordcraft_engine::cmd::table_style::preview(&app.session, &v).as_ref(), look);
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                let cmd = if id.is_some() { "table.modifyStyle" } else { "table.newStyle" };
                if let Err(e) = app.run(cmd, v) {
                    app.status(e);
                    return false;
                }
            }
            ok || cancel
        }
        Dialog::Insert(f) => crate::dialogs_insert::body(app, ui, f),
        Dialog::Refs(f) => crate::dialogs_refs::body(app, ui, f),
        Dialog::Commands { query } => {
            let r = ui.add(egui::TextEdit::singleline(query).hint_text(tl!("Type a command, e.g. \"insert table\"")).desired_width(380.0));
            r.request_focus();
            let q = query.to_lowercase();
            let localized_q = crate::i18n::lowercase(query);
            let reg = app.session.registry.clone();
            let mut hits: Vec<&wordcraft_engine::CommandSpec> = reg
                .all()
                .iter()
                .filter(|c| {
                    !q.is_empty()
                        && (c.label.to_lowercase().contains(&q)
                            || crate::i18n::lowercase(tl!(c.label)).contains(&localized_q)
                            || c.id.to_lowercase().contains(&q)
                            || c.location.to_lowercase().contains(&q)
                            || crate::i18n::lowercase(&crate::i18n::location(c.location)).contains(&localized_q))
                })
                .collect();
            hits.sort_by_key(|c| {
                let id = c.id.to_ascii_lowercase();
                let canonical = c.label.to_lowercase();
                let localized = crate::i18n::lowercase(tl!(c.label));
                if id == q {
                    0
                } else if id.starts_with(&q) {
                    1
                } else if canonical.starts_with(&q) || localized.starts_with(&localized_q) {
                    2
                } else {
                    3
                }
            });
            hits.truncate(14);
            let mut close = false;
            for c in hits {
                let sc = crate::widgets::shortcut_text(app, c.id);
                let label = format!("{}   —   {}", tl!(c.label), crate::i18n::location(c.location));
                if ui.add(egui::Button::new(label).shortcut_text(sc).min_size(vec2(380.0, 0.0))).clicked() {
                    let _ = app.run(c.id, json!({}));
                    close = true;
                }
            }
            close || ui.input(|i| i.key_pressed(egui::Key::Escape))
        }
        Dialog::Icons { query, category, selected } => icons_body(app, ui, query, category, selected),
        Dialog::PasteSpecial { formats, choice, payload } => {
            ui.label(egui::RichText::new(tl!("Paste as:")).font(semibold(12.5)));
            if formats.is_empty() {
                ui.label(egui::RichText::new(tl!("The clipboard is empty.")).weak());
            }
            for (i, f) in formats.iter().enumerate() {
                let label = wordcraft_engine::cmd::paste::FORMATS.iter().find(|(id, _)| id == f).map(|(_, l)| *l).unwrap_or(f.as_str());
                ui.radio_value(choice, i, tl!(label));
            }
            if let Some(f) = formats.get(*choice) {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(tl!("Result")).font(semibold(12.5)));
                let hint = match f.as_str() {
                    "formatted" => "Keeps the fonts and formatting of the copied text.",
                    "rtf" => "Reads the clipboard's rich text and keeps its formatting.",
                    "html" => "Reads the clipboard's web content and keeps its formatting.",
                    _ => "Drops all formatting and pastes plain text.",
                };
                ui.label(egui::RichText::new(tl!(hint)).weak());
            }
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok && let Some(f) = formats.get(*choice) {
                let mut params = payload.clone();
                params["as"] = json!(f);
                let _ = app.run("edit.pasteSpecial", params);
            }
            ok || cancel
        }
        Dialog::About { tab } => {
            ui.set_width(660.0);
            ui.horizontal(|ui| {
                for (i, l) in ["About", "Contributors", "Models"].into_iter().enumerate() {
                    let i = i as u8;
                    if ui.selectable_label(*tab == i, tl!(l)).clicked() {
                        *tab = i;
                    }
                }
            });
            ui.separator();
            match *tab {
                1 => crate::credits::contributors_ui(ui),
                2 => crate::credits::models_ui(ui),
                _ => {
                    ui.label(egui::RichText::new(tl!("WordCraft")).font(semibold(22.0)));
                    let build = option_env!("WORDCRAFT_BUILD_SHA").map(|s| s.get(..8).unwrap_or(s)).unwrap_or(tl!("development build"));
                    ui.label(crate::i18n::fmt(tl!("Version {version} ({build})"), &[("version", env!("CARGO_PKG_VERSION")), ("build", build)]));
                    ui.label(tl!(
                        "A free, open-source word processor written from scratch in Rust.\nPart of the Crafting Apps from the ArtCraft team."
                    ));
                    ui.add_space(6.0);
                    ui.hyperlink_to(tl!("getartcraft.com/apps/wordcraft"), "https://getartcraft.com/apps/wordcraft");
                    ui.hyperlink_to(tl!("Join us on Discord: discord.gg/artcraft"), "https://discord.gg/artcraft");
                    ui.hyperlink_to(tl!("Source code: github.com/storytold/wordcraft"), "https://github.com/storytold/wordcraft");
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(tl!("MIT OR Apache-2.0. Copyright (c) 2026 ArtCraft Team and the WordCraft contributors."))
                            .small()
                            .weak(),
                    );
                }
            }
            let (ok, cancel) = buttons(ui, tl!("Close"));
            ok || cancel
        }
        Dialog::SaveChanges { name, .. } => {
            ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Do you want to save changes to {name}?"), &[("name", name)])).font(semibold(15.0)));
            ui.label(tl!("Your changes will be lost if you don't save them."));
            ui.add_space(8.0);
            let buttons = ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let cancel = ui.button(tl!("Cancel"));
                    let dont_save = ui.button(tl!("Don't Save"));
                    let save = ui.add(egui::Button::new(egui::RichText::new(tl!("Save")).color(egui::Color32::WHITE)).fill(crate::theme::APP_COLOR));
                    [(cancel, "cancel"), (dont_save, "dontSave"), (save, "save")]
                })
                .inner
            });
            let buttons = buttons.inner;
            // Enter clicks the focused button; it means Save only when none of them has focus.
            let focused = buttons.iter().any(|(b, _)| b.has_focus());
            let answer = if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                Some("cancel")
            } else if let Some((_, a)) = buttons.iter().find(|(b, _)| b.clicked()) {
                Some(*a)
            } else if !focused && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                Some("save")
            } else {
                None
            };
            let Some(answer) = answer else { return false };
            // `ui.saveChanges` reads the prompt from `app.dialog`, which `show` has taken out.
            app.dialog = Some(d.clone());
            let _ = app.run("ui.saveChanges", json!({"answer": answer}));
            true
        }
        Dialog::RecipientList { fields, rows, message } => recipient_list(app, ui, fields, rows, message),
        Dialog::InsertMergeField { fields, field } => {
            if fields.is_empty() {
                ui.label(egui::RichText::new(tl!("Select recipients first to choose from their fields.")).small().weak());
            } else {
                ui.label(tl!("Fields:"));
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for f in fields.iter() {
                        let resp = ui.selectable_label(field == f, f);
                        if resp.clicked() {
                            field.clone_from(f);
                        }
                    }
                });
            }
            ui.horizontal(|ui| {
                ui.label(tl!("Field:"));
                ui.text_edit_singleline(field);
            });
            let (ok, cancel) = buttons(ui, tl!("Insert"));
            if ok && !field.trim().is_empty() {
                let _ = app.run("mailings.insertField", json!({"field": field.trim()}));
                return true;
            }
            cancel
        }
        Dialog::Password { name, params, password, message } => {
            ui.label(crate::i18n::fmt(tl!("{name} is protected with a password."), &[("name", name)]));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(tl!("Password:"));
                let r = ui.add(egui::TextEdit::singleline(password.as_mut_string()).password(true).desired_width(220.0));
                if password.is_empty() && !r.has_focus() {
                    r.request_focus();
                }
            });
            if !message.is_empty() {
                ui.label(egui::RichText::new(message.as_str()).small().color(ui.visuals().error_fg_color));
            }
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok && !password.is_empty() {
                let mut p = params.clone();
                if let Some(o) = p.as_object_mut() {
                    o.insert("password".into(), json!(password.as_str()));
                }
                // `execute`, not `run`: Save Changes was asked when the open began.
                match app.execute("file.open", p) {
                    Ok(_) => {
                        app.ui.backstage = false;
                        return true;
                    }
                    Err(e) if wordcraft_engine::io::wrong_password(&e) => {
                        *message = tl!("That password isn't right. Check it (passwords are case-sensitive) and try again.").to_string();
                        password.as_mut_string().clear();
                    }
                    Err(e) => *message = e,
                }
            }
            cancel
        }
        Dialog::EncryptPassword { password, confirm, message } => {
            ui.label(tl!("Anyone who opens the document will need this password."));
            ui.add_space(4.0);
            egui::Grid::new("encrypt_password").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label(tl!("Password:"));
                let r = ui.add(egui::TextEdit::singleline(password.as_mut_string()).password(true).desired_width(220.0));
                if password.is_empty() && confirm.is_empty() && !r.has_focus() {
                    r.request_focus();
                }
                ui.end_row();
                ui.label(tl!("Confirm password:"));
                ui.add(egui::TextEdit::singleline(confirm.as_mut_string()).password(true).desired_width(220.0));
                ui.end_row();
            });
            ui.label(
                egui::RichText::new(tl!(
                    "Keep the password somewhere safe: without it nobody, WordCraft included, can open the document. Leave both boxes empty to remove the password."
                ))
                .small()
                .weak(),
            );
            if !message.is_empty() {
                ui.label(egui::RichText::new(message.as_str()).small().color(ui.visuals().error_fg_color));
            }
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok {
                if password != confirm {
                    *message = tl!("The passwords don't match.").to_string();
                    confirm.as_mut_string().clear();
                    return false;
                }
                let pw = if password.is_empty() { Value::Null } else { json!(password.as_str()) };
                match app.run("file.encrypt", json!({"password": pw})) {
                    Ok(_) => {
                        app.status(if password.is_empty() {
                            tl!("The password is removed. Save the document to keep the change.")
                        } else {
                            tl!("The document will be saved with a password. Save it to protect it.")
                        });
                        return true;
                    }
                    Err(e) => *message = e,
                }
            }
            cancel
        }
        Dialog::DefineList { form } => crate::dialogs_lists::define_list(app, ui, form),
        Dialog::TrackOptions { form } => crate::dialogs_lists::track_options(app, ui, form),
        Dialog::Review(d) => crate::dialogs_review::body(app, ui, d),
        Dialog::CustomizeKeyboard { form } => crate::dialogs_keyboard::customize_keyboard(app, ui, form),
        Dialog::CustomizeRibbon { form } => crate::dialogs_ribbon::customize_ribbon(app, ui, form),
        Dialog::FieldCode { code, edit, message } => {
            ui.label(tl!("Field codes:"));
            let r = ui.add(egui::TextEdit::singleline(code).desired_width(320.0).font(egui::TextStyle::Monospace));
            if !r.has_focus() && code.is_empty() {
                r.request_focus();
            }
            ui.label(egui::RichText::new("PAGE · NUMPAGES · DATE \\@ \"MMMM d, yyyy\" · AUTHOR · TITLE · SEQ Figure").small().weak());
            if !message.is_empty() {
                ui.label(egui::RichText::new(message.as_str()).small().weak());
            }
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok && !code.trim().is_empty() {
                let r = if *edit {
                    app.run("fields.setCode", json!({"code": code.trim()}))
                } else {
                    app.run("insert.field", json!({"instr": code.trim()}))
                };
                match r {
                    Ok(_) => return true,
                    Err(e) => *message = e,
                }
            }
            cancel
        }
        Dialog::Control { form } => crate::dialogs_controls::body(app, ui, form),
        Dialog::SelectTable { sheets, selected, headers, message, params } => {
            ui.label(tl!("Choose the sheet that holds the recipients."));
            ui.add_space(4.0);
            // A double-click picks the sheet as OK does.
            let mut picked = false;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_width(300.0);
                egui::ScrollArea::vertical().id_salt("select_table").max_height(220.0).show(ui, |ui| {
                    for (i, name) in sheets.iter().enumerate() {
                        let r = ui.selectable_label(*selected == i, name.as_str());
                        if r.clicked() || r.double_clicked() {
                            *selected = i;
                        }
                        picked |= r.double_clicked();
                    }
                });
            });
            ui.checkbox(headers, tl!("First row of data contains column headers"));
            if !message.is_empty() {
                ui.label(egui::RichText::new(message.as_str()).color(Tokens::get(ui.ctx()).red));
            }
            let (ok, cancel) = buttons(ui, tl!("OK"));
            if ok || picked {
                let mut p = params.clone();
                if let Some(o) = p.as_object_mut() {
                    o.insert("sheet".into(), json!(*selected + 1));
                    o.insert("headers".into(), json!(*headers));
                }
                match app.load_recipients(p) {
                    Ok(_) => return true,
                    Err(e) => *message = e,
                }
            }
            cancel
        }
        Dialog::NoteOptions { form } => crate::dialogs_notes::note_options(app, ui, form),
        Dialog::Templates { form } => crate::dialogs_templates::templates(app, ui, form),
        Dialog::Organizer { form } => crate::dialogs_templates::organizer(app, ui, form),
        Dialog::Layout(d) => crate::dialogs_layout::body(app, ui, d),
        Dialog::FindRecipient { text, message } => {
            ui.horizontal(|ui| {
                ui.label(tl!("Find:"));
                ui.text_edit_singleline(text);
            });
            if !message.is_empty() {
                ui.label(egui::RichText::new(message.as_str()).small().weak());
            }
            let (ok, cancel) = buttons(ui, tl!("Find"));
            if ok && !text.trim().is_empty() {
                match app.run("mailings.findRecipient", json!({"text": text.trim()})) {
                    Ok(_) => return true,
                    Err(e) => *message = e,
                }
            }
            cancel
        }
        Dialog::MergeRule { rule, fields, field, value, then, els } => {
            let skip = rule == "SKIPIF";
            ui.label(tl!(if skip {
                "Leave a recipient out of the merge when a field has this value."
            } else {
                "Insert one text or another, depending on a field's value."
            }));
            ui.add_space(4.0);
            egui::Grid::new("merge_rule").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
                ui.label(tl!("Field:"));
                if fields.is_empty() {
                    ui.text_edit_singleline(field);
                } else {
                    egui::ComboBox::from_id_salt("merge_rule_field").selected_text(field.as_str()).width(200.0).show_ui(ui, |ui| {
                        for f in fields.iter() {
                            ui.selectable_value(field, f.clone(), f);
                        }
                    });
                }
                ui.end_row();
                ui.label(tl!("Equals:"));
                ui.text_edit_singleline(value);
                ui.end_row();
                if !skip {
                    ui.label(tl!("Then insert:"));
                    ui.text_edit_singleline(then);
                    ui.end_row();
                    ui.label(tl!("Otherwise insert:"));
                    ui.text_edit_singleline(els);
                    ui.end_row();
                }
            });
            let (ok, cancel) = buttons(ui, tl!("OK"));
            let params = rule_params(d);
            if ok && let Some(params) = params {
                let _ = app.run("mailings.rules", params);
                return true;
            }
            cancel
        }
        Dialog::MatchFields { fields, address } => {
            if fields.is_empty() {
                ui.label(egui::RichText::new(tl!("Select recipients first to match their fields.")).small().weak());
            } else {
                ui.label(tl!("The recipient-list field that fills each part of an address block and greeting line."));
                ui.add_space(4.0);
                egui::Grid::new("match_fields").num_columns(2).spacing(vec2(24.0, 6.0)).striped(true).show(ui, |ui| {
                    for (part, key) in ADDRESS_PARTS {
                        ui.label(tl!(part));
                        match address.get(key).and_then(Value::as_str) {
                            Some(f) => ui.label(egui::RichText::new(f).strong()),
                            None => ui.label(egui::RichText::new(tl!("(not matched)")).weak()),
                        };
                        ui.end_row();
                    }
                });
            }
            let (ok, cancel) = buttons(ui, tl!("Close"));
            ok || cancel
        }
        Dialog::CheckErrors { unknown, records } => {
            if unknown.is_empty() {
                ui.label(tl!("No errors found: every merge field is in the recipient list."));
            } else {
                ui.label(tl!("These merge fields aren't in the recipient list:"));
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for f in unknown.iter() {
                        ui.label(egui::RichText::new(format!("• {f}")).color(Tokens::get(ui.ctx()).red));
                    }
                });
            }
            ui.add_space(4.0);
            ui.label(egui::RichText::new(crate::i18n::fmt(tl!("Recipients: {count}"), &[("count", &records.to_string())])).weak());
            let (ok, cancel) = buttons(ui, tl!("Close"));
            ok || cancel
        }
    }
}

/// The Recipient List dialog's body: a file button, the editable table, OK/Cancel. True to close.
fn recipient_list(app: &mut WordApp, ui: &mut Ui, fields: &mut Vec<String>, rows: &mut Vec<Vec<String>>, message: &mut String) -> bool {
    const CELL_W: f32 = 120.0;
    const X_W: f32 = 18.0;
    const ROW_H: f32 = 24.0;
    // Wide enough for every field (up to a limit; more scroll sideways).
    let gap = ui.spacing().item_spacing.x;
    let table_w = fields.len() as f32 * (CELL_W + X_W + 2.0 * gap) + X_W + 2.0 * gap;
    ui.set_min_width(table_w.clamp(340.0, 720.0));
    ui.label(tl!("Type the recipients, or load them from a file."));
    if ui.button(tl!("Use an Existing List…")).clicked() {
        // The picker takes over; the list it loads replaces this one.
        let _ = app.run("ui.openRecipientList", json!({}));
        return true;
    }
    ui.add_space(6.0);
    let mut remove_field = None;
    let can_remove = fields.len() > 1;
    let mut remove_row = None;
    let mut x_w = X_W;
    egui::ScrollArea::horizontal().id_salt("recipients_h").max_width(720.0).show(ui, |ui| {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                for (i, f) in fields.iter_mut().enumerate() {
                    ui.add_sized([CELL_W, ROW_H - 4.0], egui::TextEdit::singleline(f).font(egui::TextStyle::Button).hint_text(tl!("Field name")));
                    let x = ui.add_enabled(can_remove, egui::Button::new("×").small().min_size(vec2(X_W, X_W))).on_hover_text(tl!("Remove field"));
                    x_w = x.rect.width();
                    if x.clicked() {
                        remove_field = Some(i);
                    }
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_salt("recipients_v").max_height(260.0).show_rows(ui, ROW_H, rows.len(), |ui, range| {
                for r in range {
                    let Some(row) = rows.get_mut(r) else { continue };
                    ui.horizontal(|ui| {
                        for c in row.iter_mut() {
                            ui.add_sized([CELL_W, ROW_H - 4.0], egui::TextEdit::singleline(c));
                            // Under the field's remove button.
                            ui.allocate_exact_size(vec2(x_w, X_W), Sense::hover());
                        }
                        if ui.add(egui::Button::new("×").small().min_size(vec2(X_W, X_W))).on_hover_text(tl!("Remove recipient")).clicked() {
                            remove_row = Some(r);
                        }
                    });
                }
            });
        });
    });
    if let Some(i) = remove_field.filter(|_| fields.len() > 1) {
        fields.remove(i);
        for r in rows.iter_mut() {
            if i < r.len() {
                r.remove(i);
            }
        }
    }
    if let Some(r) = remove_row.filter(|r| *r < rows.len()) {
        rows.remove(r);
    }
    ui.horizontal(|ui| {
        if ui.button(tl!("New Entry")).clicked() {
            rows.push(vec![String::new(); fields.len()]);
        }
        if ui.button(tl!("Add Field")).clicked() {
            fields.push(String::new());
            for r in rows.iter_mut() {
                r.push(String::new());
            }
        }
    });
    if !message.is_empty() {
        ui.label(egui::RichText::new(message.as_str()).color(Tokens::get(ui.ctx()).red));
    }
    let (ok, cancel) = buttons(ui, tl!("OK"));
    if !ok {
        return cancel;
    }
    match recipient_table(fields, rows).map_err(|e| tl!(e).to_string()).and_then(|params| app.load_recipients(params)) {
        Ok(_) => true,
        Err(e) => {
            *message = e;
            false
        }
    }
}

/// The Table Properties dialog's fields: Table, Row, Column and Cell sections.
fn table_properties(ui: &mut Ui, f: &mut TableForm) {
    let unit = crate::options::unit();
    fn len(v: &mut f32, lo: f32, unit: wordcraft_geom::Unit) -> egui::DragValue<'_> {
        egui::DragValue::new(v).speed(0.01).range(lo..=22.0).suffix(unit.suffix()).max_decimals(2)
    }
    let heading = |ui: &mut Ui, s: &str| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(tl!(s)).font(semibold(12.5)));
    };
    heading(ui, "Table");
    egui::Grid::new("tp_table").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        ui.checkbox(&mut f.width_on, tl!("Preferred width:"));
        ui.add_enabled(f.width_on, len(&mut f.width, 0.1, unit));
        ui.end_row();
        ui.label(tl!("Alignment:"));
        ui.horizontal(|ui| {
            for (v, l) in [("left", "Left"), ("center", "Center"), ("right", "Right")] {
                ui.radio_value(&mut f.align, v.to_string(), tl!(l));
            }
        });
        ui.end_row();
        ui.label(tl!("Indent from left:"));
        ui.add_enabled(f.align == "left", egui::DragValue::new(&mut f.indent).speed(0.01).range(-11.0..=22.0).suffix(unit.suffix()).max_decimals(2));
        ui.end_row();
        ui.label(tl!("Table direction:"));
        ui.horizontal(|ui| {
            ui.radio_value(&mut f.rtl, false, tl!("Left-to-right"));
            ui.radio_value(&mut f.rtl, true, tl!("Right-to-left"));
        });
        ui.end_row();
    });
    heading(ui, "Row");
    egui::Grid::new("tp_row").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        ui.checkbox(&mut f.row_height_on, tl!("Specify height:"));
        ui.horizontal(|ui| {
            ui.add_enabled(f.row_height_on, len(&mut f.row_height, 0.02, unit));
            ui.add_enabled_ui(f.row_height_on, |ui| {
                egui::ComboBox::from_id_salt("tp_rule").selected_text(tl!(if f.row_exact { "Exactly" } else { "At least" })).show_ui(ui, |ui| {
                    ui.selectable_value(&mut f.row_exact, false, tl!("At least"));
                    ui.selectable_value(&mut f.row_exact, true, tl!("Exactly"));
                });
            });
        });
        ui.end_row();
    });
    ui.checkbox(&mut f.allow_break, tl!("Allow row to break across pages"));
    ui.checkbox(&mut f.header_row, tl!("Repeat as header row on each page"));
    heading(ui, "Column");
    egui::Grid::new("tp_col").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        ui.label(tl!("Preferred width:"));
        ui.add(len(&mut f.column_width, 0.1, unit));
        ui.end_row();
    });
    heading(ui, "Cell");
    egui::Grid::new("tp_cell").num_columns(2).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
        ui.checkbox(&mut f.cell_width_on, tl!("Preferred width:"));
        ui.add_enabled(f.cell_width_on, len(&mut f.cell_width, 0.1, unit));
        ui.end_row();
        ui.label(tl!("Vertical alignment:"));
        ui.horizontal(|ui| {
            for (v, l) in [("top", "Top"), ("center", "Center"), ("bottom", "Bottom")] {
                ui.radio_value(&mut f.valign, v.to_string(), tl!(l));
            }
        });
        ui.end_row();
    });
}

/// The Paragraph dialog's OK. `align`, `left` and `right` are as seen on the page; `flags` are
/// keep with next, keep lines together, page break before, widow/orphan control; `opts` (now,
/// when opened) are mirror indents, contextual spacing, suppress line numbers and don't
/// hyphenate, of which only the changed ones are set.
#[allow(clippy::too_many_arguments)]
fn apply_paragraph(
    app: &mut WordApp,
    rtl: bool,
    align: &str,
    left: f32,
    right: f32,
    first: f32,
    before: f32,
    after: f32,
    line: f32,
    flags: [bool; 4],
    opts: ([bool; 4], [bool; 4]),
) {
    let [keep_next, keep_lines, page_break, widow] = flags;
    // Direction first: the alignment and indents are as seen on the page, and their logical
    // values depend on it.
    let _ = app.run(if rtl { "para.rtl" } else { "para.ltr" }, json!({}));
    let _ = app.run("para.align", json!({"value": align}));
    let (start, end) = if rtl { (right, left) } else { (left, right) };
    let mut props = json!({
        "indentLeft": start * crate::options::unit().pt_per_unit(), "indentRight": end * crate::options::unit().pt_per_unit(), "indentFirst": first * crate::options::unit().pt_per_unit(),
        "spaceBefore": before, "spaceAfter": after, "lineSpacing": {"rule": "multiple", "value": line},
        "keepNext": keep_next, "keepLines": keep_lines, "pageBreakBefore": page_break, "widowControl": widow,
    });
    let (now, was) = opts;
    for (key, (on, was)) in ["mirrorIndents", "contextualSpacing", "suppressLineNumbers", "suppressHyphens"].into_iter().zip(now.into_iter().zip(was))
    {
        if on != was
            && let Some(m) = props.as_object_mut()
        {
            m.insert(key.into(), json!(on));
        }
    }
    let _ = app.run("para.set", json!({ "props": props }));
}

/// The Paragraph dialog's Asian Typography: sets the flags that changed since it opened, so an
/// untouched tab adds no direct formatting.
fn apply_asian(app: &mut WordApp, asian: [bool; 6], was: [bool; 6]) {
    let keys = ["kinsoku", "wordWrap", "overflowPunct", "topLinePunct", "autoSpaceDE", "autoSpaceDN"];
    let changed: serde_json::Map<String, Value> =
        keys.iter().zip(asian.iter().zip(was)).filter(|(_, (now, was))| **now != *was).map(|(k, (now, _))| (k.to_string(), json!(now))).collect();
    if !changed.is_empty() {
        let _ = app.run("para.asianTypography", Value::Object(changed));
    }
}

/// Manage Styles' body. Reads the list from `styles.manage`'s data (never re-running the command,
/// which would ask to open this dialog again) and acts through commands.
fn manage_styles(app: &mut WordApp, ui: &mut Ui, alphabetical: &mut bool, selected: &mut String) -> bool {
    let list = wordcraft_engine::cmd::para::manage_list(&app.session, *alphabetical);
    let list = list.as_array().cloned().unwrap_or_default();
    let str_of = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    if !list.iter().any(|x| str_of(x, "id") == *selected) {
        *selected = list.first().map(|x| str_of(x, "id")).unwrap_or_default();
    }
    ui.horizontal(|ui| {
        ui.label(tl!("Sort order:"));
        egui::ComboBox::from_id_salt("manage_styles_sort")
            .selected_text(if *alphabetical { tl!("Alphabetical") } else { tl!("As Recommended") })
            .show_ui(ui, |ui| {
                ui.selectable_value(alphabetical, false, tl!("As Recommended"));
                ui.selectable_value(alphabetical, true, tl!("Alphabetical"));
            });
    });
    let t = Tokens::get(ui.ctx());
    let shown = egui::Id::new("manage_styles_shown");
    egui::Frame::NONE.stroke(egui::Stroke::new(1.0, t.border)).inner_margin(4).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt("manage_styles_list").max_height(200.0).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_width(380.0);
            for st in &list {
                let id = str_of(st, "id");
                let glyph = match st.get("type").and_then(Value::as_str) {
                    Some("character") => "a",
                    Some("linked") => "¶a",
                    Some("table") => "⊞",
                    _ => "¶",
                };
                let hidden = st.get("hidden").and_then(Value::as_bool).unwrap_or(false);
                let mut name = egui::RichText::new(format!("{glyph}  {}", str_of(st, "name")));
                if hidden {
                    name = name.weak();
                }
                let r = ui.selectable_label(*selected == id, name);
                // Bring the selection into view once, when it changes (opening, New Style…).
                if *selected == id && ui.data(|d| d.get_temp::<String>(shown)).as_deref() != Some(id.as_str()) {
                    r.scroll_to_me(Some(egui::Align::Center));
                    ui.data_mut(|d| d.insert_temp(shown, id.clone()));
                }
                if r.clicked() {
                    ui.data_mut(|d| d.insert_temp(shown, id.clone()));
                    *selected = id.clone();
                }
                if r.double_clicked() {
                    *selected = id;
                    if let Some(Dialog::ModifyStyle { id, name, font, size, bold, italic, color, before, after, .. }) =
                        Dialog::modify_style(app, selected)
                    {
                        app.dialog = Some(Dialog::ModifyStyle { back: true, resume: None, id, name, font, size, bold, italic, color, before, after });
                    }
                }
            }
        });
    });
    let leave = |ui: &mut Ui| {
        ui.data_mut(|d| d.remove::<String>(shown));
        true
    };
    if app.dialog.is_some() {
        return leave(ui);
    }
    let Some(cur) = list.iter().find(|x| str_of(x, "id") == *selected).cloned() else { return close_button(ui) && leave(ui) };
    let ty = str_of(&cur, "type");
    ui.add_space(6.0);
    crate::previews::style_preview(app, ui, selected, &ty, vec2(388.0, if ty == "table" { 64.0 } else { 40.0 }));
    ui.add_space(4.0);
    ui.add(egui::Label::new(egui::RichText::new(describe(&cur)).small()).wrap());
    ui.add_space(6.0);
    let builtin = cur.get("builtIn").and_then(Value::as_bool).unwrap_or(true);
    if ty != "table" {
        let mut gallery = cur.get("inGallery").and_then(Value::as_bool).unwrap_or(false);
        if ui.checkbox(&mut gallery, tl!("Show in the Styles gallery")).changed() {
            let _ = app.run("styles.setVisibility", json!({"style": selected, "gallery": gallery}));
        }
    }
    let mut hidden = cur.get("hidden").and_then(Value::as_bool).unwrap_or(false);
    if ui.checkbox(&mut hidden, tl!("Hide from style lists")).changed() {
        let _ = app.run("styles.setVisibility", json!({"style": selected, "hidden": hidden}));
    }
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        // Table styles are edited from Table Design.
        if ui.add_enabled(ty != "table", egui::Button::new(tl!("Modify…"))).clicked()
            && let Some(Dialog::ModifyStyle { id, name, font, size, bold, italic, color, before, after, .. }) = Dialog::modify_style(app, selected)
        {
            app.dialog = Some(Dialog::ModifyStyle { back: true, resume: None, id, name, font, size, bold, italic, color, before, after });
        }
        if ui.button(tl!("New Style…")).clicked() {
            app.dialog = Some(Dialog::NewStyle { name: "Style1".into(), based_on: str_of(&cur, "name"), back: true });
        }
        let del = ui.add_enabled(!builtin, egui::Button::new(tl!("Delete")));
        let del = if builtin { del.on_disabled_hover_text(tl!("Built-in styles can't be deleted.")) } else { del };
        if del.clicked() {
            let _ = app.run("styles.delete", json!({"style": selected}));
            selected.clear();
        }
    });
    (app.dialog.is_some() || close_button(ui)) && leave(ui)
}

/// A dialog's single Close button (Escape too).
fn close_button(ui: &mut Ui) -> bool {
    ui.add_space(8.0);
    let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        close |= ui.add(egui::Button::new(egui::RichText::new(tl!("Close")).color(egui::Color32::WHITE)).fill(crate::theme::APP_COLOR)).clicked();
    });
    close
}

/// A style's description: its type, the formatting it sets and what it is based on.
fn describe(st: &Value) -> String {
    let f = st.get("format").cloned().unwrap_or_default();
    let fs = |k: &str| f.get(k).and_then(Value::as_str);
    let fnum = |k: &str| f.get(k).and_then(Value::as_f64);
    let pt = |x: f64| format!("{}", (x * 10.0).round() / 10.0);
    let mut parts: Vec<String> = vec![
        match st.get("type").and_then(Value::as_str) {
            Some("character") => tl!("Character style"),
            Some("linked") => tl!("Linked style (paragraph and character)"),
            Some("table") => tl!("Table style"),
            _ => tl!("Paragraph style"),
        }
        .to_string(),
    ];
    if st.get("builtIn").and_then(Value::as_bool).unwrap_or(false) {
        parts.push(tl!("Built-in style").to_string());
    }
    if let Some(x) = fs("font") {
        parts.push(crate::i18n::fmt(tl!("Font: {font}"), &[("font", x)]));
    }
    if let Some(x) = fnum("size") {
        parts.push(crate::i18n::fmt(tl!("{size} pt"), &[("size", &pt(x))]));
    }
    if f.get("bold").and_then(Value::as_bool) == Some(true) {
        parts.push(tl!("Bold").to_string());
    }
    if f.get("italic").and_then(Value::as_bool) == Some(true) {
        parts.push(tl!("Italic").to_string());
    }
    if let Some(x) = fs("color") {
        parts.push(crate::i18n::fmt(tl!("Color: {color}"), &[("color", &format!("#{x}"))]));
    }
    if let Some(x) = fnum("spaceBefore") {
        parts.push(crate::i18n::fmt(tl!("Space before: {pt} pt"), &[("pt", &pt(x))]));
    }
    if let Some(x) = fnum("spaceAfter") {
        parts.push(crate::i18n::fmt(tl!("Space after: {pt} pt"), &[("pt", &pt(x))]));
    }
    if let Some(b) = st.get("basedOn").and_then(Value::as_str) {
        parts.push(crate::i18n::fmt(tl!("Based on: {style}"), &[("style", b)]));
    }
    parts.join(", ")
}

/// path data; Insert (or a double click) runs `insert.icon`. Returns true to close.
fn icons_body(app: &mut WordApp, ui: &mut Ui, query: &mut String, category: &mut String, selected: &mut String) -> bool {
    use wordcraft_render::icon_lib::{self, Category};
    let t = Tokens::get(ui.ctx());
    ui.set_width(560.0);
    // Typing goes to the search box (never to the document behind the dialog).
    ui.add(egui::TextEdit::singleline(query).hint_text(tl!("Search icons")).desired_width(560.0)).request_focus();
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        if ui.selectable_label(category.is_empty(), tl!("All")).clicked() {
            category.clear();
        }
        for c in Category::ALL {
            if ui.selectable_label(*category == c.name(), tl!(c.name())).clicked() {
                *category = c.name().to_string();
            }
        }
    });
    ui.add_space(4.0);
    let hits = icon_lib::search(query, Category::parse(category));
    let mut insert = false;
    egui::ScrollArea::vertical().max_height(300.0).min_scrolled_height(300.0).auto_shrink([true, false]).show(ui, |ui| {
        if hits.is_empty() {
            ui.label(egui::RichText::new(tl!("No icons match your search.")).color(t.text_dim));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            for icon in &hits {
                let (rect, resp) = ui.allocate_exact_size(vec2(56.0, 56.0), Sense::click());
                let on = *selected == icon.id;
                let fill = if on {
                    t.checked
                } else if resp.hovered() {
                    t.hover
                } else {
                    egui::Color32::TRANSPARENT
                };
                let edge = if on { egui::Stroke::new(1.5, t.accent) } else { egui::Stroke::new(1.0, t.border) };
                ui.painter().rect(rect, 4.0, fill, edge, egui::StrokeKind::Inside);
                crate::icons::library(ui.painter(), rect.shrink(12.0), icon, t.text);
                let resp = resp.on_hover_text(icon.name);
                if resp.clicked() {
                    *selected = icon.id.to_string();
                }
                if resp.double_clicked() {
                    *selected = icon.id.to_string();
                    insert = true;
                }
            }
        });
    });
    let name = icon_lib::find(selected).map(|i| i.name).unwrap_or("");
    ui.add_space(4.0);
    ui.label(egui::RichText::new(name).color(t.text_dim));
    let (ok, cancel) = buttons(ui, tl!("Insert"));
    if (ok || insert) && !selected.is_empty() {
        let _ = app.run("insert.icon", json!({"id": selected.as_str()}));
        return true;
    }
    cancel
}

/// Enclose Characters' preview: the text in the chosen shape, shrunk or with the shape enlarged
/// (drawn, so it doesn't depend on the interface font having ○ □ △ ◇).
fn enclose_preview(ui: &mut Ui, style: &str, text: &str, shape: &str) {
    let t = Tokens::get(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(110.0, 64.0), Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 2.0, t.input);
    p.rect_stroke(r, 2.0, egui::Stroke::new(1.0, t.border_strong), egui::StrokeKind::Inside);
    let ink = ui.visuals().text_color();
    let c = r.center();
    let (box_h, text_px) = match style {
        "none" => (0.0, 26.0),
        "enlarge" => (48.0, 26.0),
        _ => (34.0, 20.0),
    };
    let h = box_h / 2.0;
    let stroke = egui::Stroke::new(1.5, ink);
    match (shape, box_h > 0.0) {
        (_, false) => {}
        ("square", _) => {
            p.rect_stroke(egui::Rect::from_center_size(c, vec2(box_h, box_h)), 0.0, stroke, egui::StrokeKind::Middle);
        }
        ("triangle", _) => {
            let pts = vec![egui::pos2(c.x, c.y - h), egui::pos2(c.x + h * 1.1, c.y + h), egui::pos2(c.x - h * 1.1, c.y + h)];
            p.add(egui::Shape::closed_line(pts, stroke));
        }
        ("diamond", _) => {
            let pts = vec![egui::pos2(c.x, c.y - h), egui::pos2(c.x + h, c.y), egui::pos2(c.x, c.y + h), egui::pos2(c.x - h, c.y)];
            p.add(egui::Shape::closed_line(pts, stroke));
        }
        _ => {
            p.circle_stroke(c, h, stroke);
        }
    }
    let px = match (style, shape) {
        ("shrink", "triangle" | "diamond") => text_px * 0.75,
        _ => text_px,
    };
    let dy = if shape == "triangle" && box_h > 0.0 { h * 0.3 } else { 0.0 };
    p.text(c + vec2(0.0, dy), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(px), ink);
}

#[cfg(test)]
mod tests {
    use super::*;
    use wordcraft_doc::props::Align;
    use wordcraft_engine::Session;

    fn app_with(text: &str) -> WordApp {
        let mut app = WordApp::new(Session::new(wordcraft_doc::Document::new()), crate::Services::default());
        let _ = app.run("document.setText", json!({"text": text}));
        app
    }

    #[test]
    fn chart_data_dialog_explains_unsupported_charts_without_mutating_the_document() {
        use std::sync::Arc;
        use wordcraft_doc::graphic::{Embedded, EmbeddedRef, Graphic, GraphicKind};
        use wordcraft_doc::para::InlineObject;
        let mut app = app_with("");
        let object = InlineObject::Graphic {
            w: 160.0,
            h: 90.0,
            alt: String::new(),
            float: Default::default(),
            graphic: Arc::new(Graphic {
                kind: GraphicKind::Chart,
                source: Some(Arc::new(Embedded {
                    xml: String::new(),
                    refs: vec![EmbeddedRef {
                        id: "chart".into(),
                        kind: "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart".into(),
                        target: "word/charts/chart1.xml".into(),
                        external: false,
                    }],
                })),
                ..Default::default()
            }),
        };
        app.session.doc.insert_object(&wordcraft_doc::Pos::body(0, 0), object, &Default::default()).unwrap();
        let before = serde_json::to_value(&app.session.doc).unwrap();
        let dirty = app.session.dirty;
        app.run("ui.chartData", json!({"chartPart": "word/charts/chart1.xml"})).unwrap();
        let Some(Dialog::ChartData { form }) = &app.dialog else { panic!("chart editor did not open") };
        assert!(!form.read_only_reason.is_empty(), "a chart without a validated DOCX workbook has a reason");
        assert!(form.series.is_empty(), "unsupported charts never receive editable guessed data");
        assert_eq!(serde_json::to_value(&app.session.doc).unwrap(), before);
        assert_eq!(app.session.dirty, dirty, "opening the editor does not dirty the document");
        let context = egui::Context::default();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| show(&mut app, ui.ctx()));
        output.textures_delta.clear();
        assert!(app.dialog.is_some(), "the read-only explanation stays open until the user closes it");
    }

    #[test]
    fn chart_data_cancel_discards_the_staged_draft() {
        use egui_kittest::kittest::Queryable;
        let mut app = app_with("Document content must stay put");
        let document = serde_json::to_value(&app.session.doc).unwrap();
        let dirty = app.session.dirty;
        let draft = ChartSeriesForm { name: "Draft label".into(), categories: vec!["A".into()], values: vec!["1".into()], ..Default::default() };
        app.dialog = Some(Dialog::ChartData {
            form: Box::new(ChartDataForm {
                document_id: app.session.document_id(),
                chart_part: "word/charts/chart1.xml".into(),
                chart_kind: "column".into(),
                series: vec![draft.clone()],
                original: vec![ChartSeriesForm { name: "Original label".into(), ..draft }],
                ..Default::default()
            }),
        });
        let mut fonts = false;
        let mut harness = egui_kittest::Harness::builder().with_size(vec2(900.0, 650.0)).build_ui_state(
            move |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                if fonts {
                    show(app, &ctx);
                } else {
                    crate::theme::install_fonts_for(&ctx, false);
                    fonts = true;
                }
            },
            app,
        );
        for _ in 0..3 {
            harness.step();
        }
        let _apply_button = harness.get_by_label("Apply");
        harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Series name").focus();
        harness.step();
        assert!(harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Series name").is_focused());
        harness.key_press(egui::Key::Tab);
        harness.step();
        assert!(
            harness.get_by_role_and_label(egui::accesskit::Role::TextInput, "Category").is_focused(),
            "Tab advances to the next spreadsheet cell"
        );
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(harness.state().dialog.is_none());
        assert_eq!(serde_json::to_value(&harness.state().session.doc).unwrap(), document);
        assert_eq!(harness.state().session.dirty, dirty);
    }

    #[test]
    fn chart_data_formatter_draft_sends_only_changed_chart_properties() {
        let original = ChartSeriesForm {
            name: "Measurement".into(),
            categories: vec!["A".into(), "B".into()],
            values: vec!["1".into(), "2".into()],
            ..Default::default()
        };
        let unchanged = chart_series_changes("column", std::slice::from_ref(&original), std::slice::from_ref(&original)).unwrap();
        assert_eq!(unchanged, [json!({})]);
        let renamed = ChartSeriesForm { name: "Updated measurement".into(), ..original.clone() };
        let changes = chart_series_changes("column", &[renamed], std::slice::from_ref(&original)).unwrap();
        assert_eq!(changes, [json!({"name": "Updated measurement"})]);
        assert_eq!(legend_position_label("l"), "Left");
        assert_eq!(legend_position_label("t"), "Top");
        assert_eq!(legend_position_label("b"), "Bottom");
        assert_eq!(legend_position_label("r"), "Right");
    }

    #[test]
    fn chart_series_format_draft_emits_only_supported_color_and_width_changes() {
        let original = ChartSeriesForm {
            color: [0x33, 0x66, 0x99],
            original_color: Some([0x33, 0x66, 0x99]),
            color_editable: true,
            line_width: Some(2.25),
            original_line_width: Some(2.25),
            line_width_editable: true,
            ..Default::default()
        };
        assert_eq!(chart_series_changes("line", std::slice::from_ref(&original), std::slice::from_ref(&original)), Some(vec![json!({})]));
        let changed = ChartSeriesForm { color: [0, 0x80, 0xFF], line_width: Some(1.5), ..original.clone() };
        assert_eq!(chart_series_changes("line", &[changed], &[original]), Some(vec![json!({"color":"0080FF", "lineWidth":1.5})]));
    }

    #[test]
    fn scatter_marker_controls_emit_only_supported_changes() {
        let original = ChartSeriesForm {
            marker_symbol: "circle".into(),
            original_marker_symbol: Some("circle".into()),
            marker_symbol_editable: true,
            marker_size: 5,
            original_marker_size: Some(5),
            marker_size_editable: true,
            marker_color: [0x11, 0x22, 0x33],
            original_marker_color: Some([0x11, 0x22, 0x33]),
            marker_color_editable: true,
            ..Default::default()
        };
        let edited = ChartSeriesForm { marker_symbol: "diamond".into(), marker_size: 12, marker_color: [0xAA, 0xBB, 0xCC], ..original.clone() };
        assert_eq!(
            chart_series_changes("scatter", &[edited], &[original]),
            Some(vec![json!({
                "markerSymbol":"diamond", "markerSize":12, "markerColor":"AABBCC"
            })])
        );
        let locked = ChartSeriesForm { marker_symbol_editable: false, ..ChartSeriesForm::default() };
        let invalid = ChartSeriesForm { marker_symbol: "star".into(), ..locked.clone() };
        assert_eq!(chart_series_changes("scatter", &[invalid], &[locked]), None);
    }

    #[test]
    fn data_label_controls_emit_only_existing_supported_properties() {
        let original = ChartSeriesForm {
            data_label_value: true,
            original_data_label_value: Some(true),
            data_label_category: false,
            original_data_label_category: Some(false),
            data_label_series: false,
            original_data_label_series: Some(false),
            data_label_position: "outEnd".into(),
            original_data_label_position: Some("outEnd".into()),
            data_labels_editable: true,
            ..ChartSeriesForm::default()
        };
        let changed = ChartSeriesForm { data_label_value: false, data_label_category: true, data_label_position: "inEnd".into(), ..original.clone() };
        assert_eq!(
            chart_series_changes("column", &[changed], &[original]),
            Some(vec![json!({"dataLabels":{"showValue":false,"showCategory":true,"position":"inEnd"}})])
        );
        let locked = ChartSeriesForm { data_labels_editable: false, ..ChartSeriesForm::default() };
        let invalid = ChartSeriesForm { data_label_value: true, original_data_label_value: Some(false), ..locked.clone() };
        assert_eq!(chart_series_changes("column", &[invalid], &[locked]), None);
    }

    #[test]
    fn error_bar_controls_emit_only_validated_changed_values() {
        let original = ChartSeriesForm {
            error_bars: vec![ChartErrorBarForm {
                direction: "y".into(),
                bar_type: Some("both".into()),
                value_type: Some("cust".into()),
                renderable: true,
                editable: true,
                positive: vec!["1".into(), "2".into()],
                original_positive: vec![1.0, 2.0],
                negative: vec!["0.5".into(), "1".into()],
                original_negative: vec![0.5, 1.0],
                ..Default::default()
            }],
            ..Default::default()
        };
        let changed = ChartSeriesForm {
            error_bars: vec![ChartErrorBarForm { positive: vec!["1.25".into(), "2.25".into()], ..original.error_bars[0].clone() }],
            ..original.clone()
        };
        assert_eq!(
            chart_series_changes("column", &[changed], std::slice::from_ref(&original)),
            Some(vec![json!({"errorBars":[{"direction":"y", "positive":[1.25,2.25]}]})])
        );
        let invalid = ChartSeriesForm {
            error_bars: vec![ChartErrorBarForm { positive: vec!["NaN".into(), "2".into()], ..original.error_bars[0].clone() }],
            ..original.clone()
        };
        assert_eq!(chart_series_changes("column", &[invalid], &[original]), None);

        let fixed = ChartSeriesForm {
            error_bars: vec![ChartErrorBarForm {
                direction: "x".into(),
                bar_type: Some("both".into()),
                value_type: Some("fixedVal".into()),
                renderable: true,
                editable: true,
                fixed_value: "2.5".into(),
                original_fixed_value: Some(1.0),
                ..Default::default()
            }],
            ..Default::default()
        };
        assert_eq!(
            chart_series_changes(
                "scatter",
                std::slice::from_ref(&fixed),
                &[ChartSeriesForm {
                    error_bars: vec![ChartErrorBarForm { fixed_value: "1".into(), ..fixed.error_bars[0].clone() }],
                    ..ChartSeriesForm::default()
                }]
            ),
            Some(vec![json!({"errorBars":[{"direction":"x", "fixedValue":2.5}]})])
        );
        let locked = ChartSeriesForm {
            error_bars: vec![ChartErrorBarForm { editable: false, positive: vec!["3".into()], ..fixed.error_bars[0].clone() }],
            ..ChartSeriesForm::default()
        };
        assert_eq!(chart_series_changes("scatter", std::slice::from_ref(&locked), std::slice::from_ref(&locked)), Some(vec![json!({})]));
    }

    #[test]
    fn unsupported_series_format_controls_cannot_emit_edits() {
        let series = ChartSeriesForm { color: [1, 2, 3], original_color: None, color_editable: false, ..Default::default() };
        let original = series.clone();
        assert_eq!(chart_series_changes("column", std::slice::from_ref(&series), &[original]), Some(vec![json!({})]));
        let edited = ChartSeriesForm { color: [4, 5, 6], ..series.clone() };
        assert_eq!(chart_series_changes("column", &[edited], &[series]), None);
    }

    #[test]
    fn chart_axis_draft_serializes_only_supported_changed_values() {
        let original = json!({
            "role":"value",
            "minimum":null,
            "maximum":100.0,
            "numberFormat":"General",
            "editable":true,
            "numberFormatEditable":true,
            "diagnostic":null
        });
        let mut axis = chart_axis_form(&original).unwrap();
        assert_eq!(axis_change_json(&axis), None);
        axis.minimum_manual = true;
        axis.minimum = "0".into();
        axis.number_format = "0.00".into();
        assert_eq!(axis_change_json(&axis), Some(json!({"minimum":{"mode":"manual","value":0.0}, "numberFormat":"0.00"})));
        axis.minimum_manual = false;
        axis.original_minimum = Some(0.0);
        assert_eq!(axis_change_json(&axis), Some(json!({"minimum":{"mode":"auto"}, "numberFormat":"0.00"})));

        let read_only = chart_axis_form(&json!({"role":"category", "editable":false, "numberFormatEditable":false})).unwrap();
        assert_eq!(axis_change_json(&read_only), None, "read-only axes cannot generate changes");
    }

    #[test]
    fn chart_title_draft_only_edits_existing_supported_titles() {
        assert_eq!(changed_existing_title(Some("Old"), "New"), Some("New"));
        assert_eq!(changed_existing_title(Some("Old"), "Old"), None);
        assert_eq!(changed_existing_title(None, "New"), None, "the dialog does not invent OOXML title structures");
    }

    #[test]
    fn scatter_chart_form_serializes_paired_numeric_x_and_y_without_categories() {
        let series = ChartSeriesForm {
            name: "Independent series".into(),
            categories: vec!["ignored".into()],
            x_values: vec!["1.25".into(), "2".into()],
            values: vec!["4".into(), "8.5".into()],
            ..Default::default()
        };
        assert_eq!(
            chart_series_values("scatter", std::slice::from_ref(&series)),
            Some(vec![json!({
                "name": "Independent series",
                "xValues": [1.25, 2.0],
                "values": [4.0, 8.5]
            })])
        );
        let mut incomplete = series;
        incomplete.x_values.pop();
        assert_eq!(chart_series_values("scatter", &[incomplete]), None, "scatter pairs must retain equal lengths");
    }

    fn props(app: &WordApp) -> wordcraft_doc::props::ParaProps {
        app.session.doc.para_at(&app.session.sel.focus).map(|p| p.props.clone()).unwrap_or_default()
    }

    /// Ordinary Turkish queries must find dotted-I labels, while canonical English labels and
    /// ASCII command IDs retain their old case-insensitive search in every interface language.
    #[test]
    fn command_search_respects_turkish_i_pairs_and_keeps_canonical_matches() {
        use egui_kittest::kittest::Queryable;
        let mut fonts = false;
        let mut h = egui_kittest::Harness::builder().with_size(vec2(1440.0, 900.0)).build_ui_state(
            move |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                crate::i18n::set_current(crate::i18n::Lang::from_pref(&app.ui.language));
                if fonts {
                    show(app, &ctx);
                } else {
                    crate::theme::install_fonts_for(&ctx, false);
                    fonts = true;
                }
            },
            app_with("Unchanged document"),
        );
        let before = serde_json::to_value(&h.state().session.doc).unwrap();
        for (language, query, expected) in [
            ("tr", "italik", "İtalik   —   Giriş › Yazı Tipi"),
            ("tr", "İTALİK", "İtalik   —   Giriş › Yazı Tipi"),
            ("tr", "içindekiler", "İçindekiler Tablosu   —   Başvurular › İçindekiler Tablosu"),
            ("tr", "YAZI TİPİ", "İtalik   —   Giriş › Yazı Tipi"),
            ("tr", "INSERT", "Tablo   —   Ekle › Tablolar"),
            ("tr", "INSERT.TABLE", "Tablo   —   Ekle › Tablolar"),
            ("tr", "FORMAT.ITALIC", "İtalik   —   Giriş › Yazı Tipi"),
            ("tr", "TABLE OF CONTENTS", "İçindekiler Tablosu   —   Başvurular › İçindekiler Tablosu"),
            ("en", "ITALIC", "Italic   —   Home › Font"),
            ("en", "FORMAT.ITALIC", "Italic   —   Home › Font"),
            ("en", "INSERT.TABLE", "Table   —   Insert › Tables"),
        ] {
            h.state_mut().ui.language = language.into();
            h.state_mut().dialog = Some(Dialog::Commands { query: query.into() });
            h.run_steps(4);
            assert!(
                h.query(egui_kittest::kittest::By::new().role(egui::accesskit::Role::Button).label_contains(expected)).is_some(),
                "{language}: {query} did not find {expected}"
            );
            assert_eq!(crate::i18n::lowercase("Iİıi"), if language == "tr" { "ıiıi" } else { "ii\u{307}ıi" });
        }
        assert_eq!(serde_json::to_value(&h.state().session.doc).unwrap(), before, "search does not change the document");
        crate::i18n::set_current(crate::i18n::Lang::EN);
    }

    /// Open the Paragraph dialog and press OK without changing anything.
    fn ok_unchanged(app: &mut WordApp) {
        ok_with(app, |_| {});
    }

    /// Open the Paragraph dialog, change its options (`opts`) with `edit` and press OK.
    fn ok_with(app: &mut WordApp, edit: impl FnOnce(&mut [bool; 4])) {
        let Some(Dialog::Paragraph {
            rtl, align, left, right, first, before, after, line, keep_next, keep_lines, page_break, widow, mut opts, ..
        }) = Dialog::open("paragraph", app)
        else {
            panic!("no paragraph dialog")
        };
        let was = opts;
        edit(&mut opts);
        apply_paragraph(app, rtl, &align, left, right, first, before, after, line, [keep_next, keep_lines, page_break, widow], (opts, was));
    }

    #[test]
    fn paragraph_dialog_sets_only_the_options_that_changed() {
        // #512: mirror indents, same-style spacing, suppress line numbers, don't hyphenate.
        let mut app = app_with("Hello");
        ok_unchanged(&mut app);
        let p = props(&app);
        assert_eq!((p.mirror_indents, p.contextual_spacing, p.suppress_line_numbers, p.suppress_hyphens), (None, None, None, None));
        ok_with(&mut app, |o| *o = [true; 4]);
        let p = props(&app);
        assert_eq!(
            (p.mirror_indents, p.contextual_spacing, p.suppress_line_numbers, p.suppress_hyphens),
            (Some(true), Some(true), Some(true), Some(true))
        );
        let Some(Dialog::Paragraph { opts, .. }) = Dialog::open("paragraph", &mut app) else { panic!() };
        assert_eq!(opts, [true; 4], "the dialog shows what is set");
        ok_with(&mut app, |o| o[3] = false);
        assert_eq!(props(&app).suppress_hyphens, Some(false));
        assert_eq!(props(&app).mirror_indents, Some(true));
    }

    #[test]
    fn paragraph_dialog_shows_direction_and_visual_alignment() {
        let mut app = app_with("سلام دنیا");
        let _ = app.run("para.rtl", json!({}));
        let _ = app.run("para.set", json!({"props": {"indentLeft": 36.0}}));
        let Some(Dialog::Paragraph { rtl, align, left, right, .. }) = Dialog::open("paragraph", &mut app) else { panic!() };
        assert!(rtl);
        assert_eq!(align, "right", "a start-aligned right-to-left paragraph shows as right aligned");
        assert_eq!((left, right), (0.0, 0.5), "the start indent is the right one");
    }

    #[test]
    fn paragraph_dialog_ok_keeps_what_it_shows() {
        for (rtl, align) in [(false, "para.alignCenter"), (false, "para.alignRight"), (true, "para.alignLeft"), (true, "para.alignRight")] {
            let mut app = app_with("متن");
            let _ = app.run(if rtl { "para.rtl" } else { "para.ltr" }, json!({}));
            let _ = app.run(align, json!({}));
            let _ = app.run("para.set", json!({"props": {"indentLeft": 18.0, "indentRight": 9.0}}));
            let before = props(&app);
            ok_unchanged(&mut app);
            let after = props(&app);
            assert_eq!((after.bidi, after.align), (before.bidi, before.align), "{align} in rtl={rtl}");
            assert_eq!((after.indent_left, after.indent_right), (Some(18.0), Some(9.0)), "{align} in rtl={rtl}");
        }
    }

    #[test]
    fn paragraph_dialog_changes_direction() {
        let mut app = app_with("Hello");
        let Some(Dialog::Paragraph { align, left, right, first, before, after, line, keep_next, keep_lines, page_break, widow, .. }) =
            Dialog::open("paragraph", &mut app)
        else {
            panic!()
        };
        // Right to left, aligned right on the page, 1" from the right edge.
        apply_paragraph(
            &mut app,
            true,
            "right",
            left,
            right + 1.0,
            first,
            before,
            after,
            line,
            [keep_next, keep_lines, page_break, widow],
            ([false; 4], [false; 4]),
        );
        let _ = align;
        let p = props(&app);
        assert_eq!(p.bidi, Some(true));
        assert_eq!(p.align, Some(Align::Left), "right on the page is the start edge");
        assert_eq!(p.indent_left, Some(72.0), "the right indent is the start indent");
    }

    /// Switching the Paragraph dialog's tabs re-centred it, so the tab strip jumped up and down
    /// by half the height difference (#507): the top edge now stays put and only the bottom moves.
    #[test]
    fn switching_paragraph_tabs_keeps_the_top_edge() {
        use egui_kittest::kittest::Queryable;
        let mut app = app_with("Some text");
        app.dialog = Dialog::open("paragraph", &mut app);
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1440.0, 900.0)).build_ui_state(
            |ui, app: &mut WordApp| {
                let ctx = ui.ctx().clone();
                app.logic(&ctx);
                app.ui(ui);
            },
            app,
        );
        let mut tops = vec![];
        let mut oks = vec![];
        for page in [0u8, 1, 2, 0] {
            if let Some(Dialog::Paragraph { tab, .. }) = &mut h.state_mut().dialog {
                *tab = page;
            }
            for _ in 0..4 {
                h.step();
            }
            tops.push(h.get_by_label("Indents and Spacing").rect().top());
            oks.push(h.get_by_role_and_label(egui::accesskit::Role::Button, "OK").rect().top());
        }
        assert!((oks[0] - oks[1]).abs() > 10.0, "the tabs differ in height: {oks:?}");
        for t in &tops {
            assert!((t - tops[0]).abs() <= 0.5, "the tab strip stays put: {tops:?}");
        }
    }

    #[test]
    fn font_dialog_ok_keeps_what_the_user_left_alone() {
        // Issue #466: OK sent every field, clearing small caps, spacing and double underline
        // when only the size changed.
        let mut app = app_with("Example");
        let _ = app.run("select.all", json!({}));
        let _ = app.run("format.set", json!({"props": {"smallCaps": true, "caps": true, "hidden": true, "spacing": 2, "underline": "double"}}));
        let Some(mut d) = Dialog::open("font", &mut app) else { panic!("no font dialog") };
        if let Dialog::Font { size, small_caps, spacing, .. } = &mut d {
            assert!(*small_caps, "the dialog shows small caps");
            assert_eq!(*spacing, 2.0);
            *size = "14".into();
        }
        apply_font(&mut app, &d);
        let c = app.session.doc.para_at(&wordcraft_doc::Pos::body(0, 0)).map(|p| p.props_of_char(0).clone()).unwrap_or_default();
        assert_eq!(c.size, Some(14.0));
        assert_eq!((c.small_caps, c.caps, c.hidden, c.spacing), (Some(true), Some(true), Some(true), Some(2.0)));
        assert_eq!(c.underline, Some(wordcraft_doc::props::Underline::Double));
        // A field the user does change still applies.
        let Some(mut d) = Dialog::open("font", &mut app) else { panic!("no font dialog") };
        if let Dialog::Font { small_caps, .. } = &mut d {
            *small_caps = false;
        }
        apply_font(&mut app, &d);
        let c = app.session.doc.para_at(&wordcraft_doc::Pos::body(0, 0)).map(|p| p.props_of_char(0).clone()).unwrap_or_default();
        assert_eq!((c.small_caps, c.caps, c.size), (Some(false), Some(true), Some(14.0)));
    }
}
