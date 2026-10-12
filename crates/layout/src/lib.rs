//! WordCraft layout: turns a [`Document`] into pages.
//!
//! - [`para`]: one paragraph → lines (shaping, first-fit line breaking, tabs, list labels,
//!   alignment). Results are memoised per paragraph revision in a [`LayoutCache`].
//! - [`layout`]: sections → pages and columns; tables; keep/widow rules; page breaks; headers
//!   and footers; page-number fields.
//! - [`display`]: a page → draw items (glyph runs, rules, fills, images) for the renderers.
//! - [`hit`]: point ↔ position, caret geometry, line navigation, selection rectangles.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod balloons;
mod contour;
pub mod display;
pub mod fields;
pub mod hit;
pub mod kinsoku;
pub mod math;
pub mod outline;
pub mod para;
mod table;
mod textbox;
mod vertical;
pub mod wordart;

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use wordcraft_doc::numbering::{Counters, Level};
use wordcraft_doc::para::{Anchor, Float, FloatAlign, InlineObject, Wrap, WrapText};
use wordcraft_doc::props::{Border, CharProps, Rgb, TableFloat, TextDirection};
use wordcraft_doc::section::{SectionProps, SectionStart};
use wordcraft_doc::{Block, Blocks, Document, Paragraph, Path, StoryRef};
use wordcraft_geom::{Point, Rect, Spin};

pub use fields::FieldCtx;
pub use hit::VisualStep;
pub use para::{LineEnd, ParaLayout};
pub use table::{autofit_widths, cell_margins, measure_table_columns};

/// How the document is viewed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewMode {
    #[default]
    Print,
    /// One page as wide as the window, no page breaks.
    Web,
    /// Draft/Outline: like web, page breaks shown as rules.
    Draft,
    /// One page at a time with a continuous, reflowed reading column.
    Read,
    /// Outline: a continuous view showing document hierarchy.
    Outline,
}

/// How digits look on screen (Word › File › Options › Advanced › Numeral).
///
/// Display only: the document text keeps the typed codepoints, so find, spelling, copying and
/// the saved file all see the original digits. Extended Arabic-Indic digits (U+06F0–U+06F9,
/// Persian/Urdu ۰۱۲۳…) always show as typed; only ASCII and Arabic-Indic (U+0660–U+0669) digits
/// are substituted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NumeralMode {
    /// Western digits everywhere (`0123456789`); the default, today's behaviour.
    #[default]
    Arabic,
    /// Eastern Arabic-Indic digits everywhere (`٠١٢٣٤٥٦٧٨٩`).
    Hindi,
    /// Hindi digits in and around right-to-left text, Western digits elsewhere (numbers take
    /// an even bidi level, so the surrounding text decides).
    Context,
    /// Hindi digits when the system locale is an Arabic-script language, else Western digits.
    /// Call [`NumeralMode::resolve`] before laying out.
    System,
}

impl NumeralMode {
    /// Short UI label (Word's option names).
    pub fn label(self) -> &'static str {
        match self {
            NumeralMode::Arabic => "Arabic",
            NumeralMode::Hindi => "Hindi",
            NumeralMode::Context => "Context",
            NumeralMode::System => "System",
        }
    }

    /// Parse a command value (`"arabic" | "hindi" | "context" | "system"`, any case).
    pub fn parse(s: &str) -> Option<NumeralMode> {
        match s.to_lowercase().as_str() {
            "arabic" => Some(NumeralMode::Arabic),
            "hindi" => Some(NumeralMode::Hindi),
            "context" => Some(NumeralMode::Context),
            "system" => Some(NumeralMode::System),
            _ => None,
        }
    }

    /// Resolve `System` against the system locale (a BCP 47 or POSIX tag such as `ar-EG` or
    /// `fa_IR.UTF-8`, found by the shell: layout never reads the environment). Hindi digits for
    /// Arabic-script languages, else Western; unknown locale = Western. Other modes pass through.
    pub fn resolve(self, system_locale: Option<&str>) -> NumeralMode {
        if self != NumeralMode::System {
            return self;
        }
        if system_locale.is_some_and(hindi_locale) { NumeralMode::Hindi } else { NumeralMode::Arabic }
    }

    /// The displayed form of `c` in a run shaped with direction `rtl`.
    ///
    /// One character maps to exactly one character, so shaped text stays codepoint-aligned
    /// with the document text (only byte offsets shift). A `System` mode that was not
    /// resolved falls back to Western digits.
    pub fn map(self, c: char, rtl: bool) -> char {
        let hindi = match self {
            NumeralMode::Arabic => false,
            NumeralMode::Hindi => true,
            NumeralMode::Context => rtl,
            NumeralMode::System => false,
        };
        if hindi {
            if c.is_ascii_digit() { char::from_u32(0x660 + (c as u32 - '0' as u32)).unwrap_or(c) } else { c }
        } else if ('\u{660}'..='\u{669}').contains(&c) {
            (b'0' + (c as u32 - 0x660) as u8) as char
        } else {
            c
        }
    }
}

/// Does locale `tag` use Hindi digits (Arabic, Persian, Urdu, Pashto, Sindhi, Kurdish)?
fn hindi_locale(tag: &str) -> bool {
    let lang = tag.split(['_', '-', '.', '@']).next().unwrap_or_default().to_ascii_lowercase();
    matches!(lang.as_str(), "ar" | "fa" | "ur" | "ps" | "sd" | "ku" | "ckb")
}

#[derive(Clone, Debug, Default)]
pub struct LayoutOptions {
    pub view: ViewMode,
    /// Width of the window in Web/Draft views, points.
    pub web_width: f32,
    /// Page height in Reading Mode (document points).
    pub read_height: f32,
    /// Transient read-only line-spacing override; ignored by print/export views.
    pub read_line_spacing: Option<f32>,
    pub show_hidden: bool,
    /// Final text ("No Markup"): tracked deletions take no space and draw nothing.
    pub hide_deleted: bool,
    /// Show revisions in balloons: body deletions take no space; other stories remain inline.
    pub deletions_in_balloons: bool,
    /// Check spelling and grammar (squiggles).
    pub proofing: bool,
    /// How digits look on screen (display only; the text is untouched). Callers resolve
    /// `System` ([`NumeralMode::resolve`]); an unresolved `System` shows Western digits.
    pub numeral: NumeralMode,
    /// Print Markup (PDF, print, page images): widen each page by [`balloons::MARKUP_AREA`]
    /// and show the comments there in balloons joined to their anchors, as Word prints them.
    /// The editor draws its own interactive balloons and leaves this off.
    pub comment_balloons: bool,
    /// Show field codes (`{ PAGE }`, shaded) instead of field results (View › Field Codes, Alt+F9).
    /// A field whose own `code` flag is set shows the other one.
    pub field_codes: bool,
    pub outline: Option<outline::OutlineView>,
    pub proof: ProofOptions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProofOptions {
    pub grammar: bool,
    pub spelling: wordcraft_proof::Options,
}

impl Default for ProofOptions {
    fn default() -> Self {
        Self { grammar: true, spelling: wordcraft_proof::Options::default() }
    }
}

impl LayoutOptions {
    /// Whether tracked deletions in `story` take no space and draw nothing.
    pub fn hides_deleted(&self, story: StoryRef) -> bool {
        self.hide_deleted || (self.deletions_in_balloons && story == StoryRef::Body)
    }
}

/// Something placed on a page (page coordinates, points, y down).
#[derive(Clone, Debug)]
pub enum Placed {
    /// Lines `l0..l1` of a paragraph; `y` is the top of line `l0`; `x` the column's left edge.
    /// Turned lines (a table cell's text direction) run as `turn` says from the page point
    /// (`x`, `y`): see [`turn_point`].
    Lines {
        story: StoryRef,
        path: Path,
        para: Arc<ParaLayout>,
        l0: usize,
        l1: usize,
        x: f32,
        y: f32,
        turn: TextDirection,
    },
    Fill {
        rect: Rect,
        color: Rgb,
    },
    Rule {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        border: Border,
    },
    /// A picture in `rect`, turned about the rect's centre by `spin` (as are shapes and charts).
    Image {
        rect: Rect,
        media: String,
        crop: [f32; 4],
        story: StoryRef,
        path: Path,
        off: usize,
        spin: Spin,
    },
    Shape {
        rect: Rect,
        story: StoryRef,
        kind: wordcraft_doc::para::ShapeKind,
        fill: Option<Rgb>,
        stroke: Option<Rgb>,
        stroke_width: f32,
        /// A freeform's paths (ink strokes among them).
        freeform: Option<Arc<wordcraft_doc::freeform::Freeform>>,
        effects: wordcraft_doc::effects::ShapeEffects,
        spin: Spin,
        /// Arrowheads and text warp (see [`wordcraft_doc::connector::ShapeExtra`]).
        extra: wordcraft_doc::connector::ShapeExtra,
    },
    /// A floating chart or diagram, drawn from its items inside `rect`. The object is the U+FFFC at
    /// byte `off` of paragraph `path` (its alt text).
    Graphic {
        rect: Rect,
        graphic: Arc<wordcraft_doc::graphic::Graphic>,
        story: StoryRef,
        path: Path,
        off: usize,
        spin: Spin,
    },
    /// `items` clipped to `rect`: a floating Drawing Canvas's members, kept inside its frame.
    /// `rect` is the unturned frame and `items` sit in it; `spin` turns the clipped whole about
    /// the frame's centre.
    Clip {
        rect: Rect,
        spin: Spin,
        items: Vec<Placed>,
    },
    /// A table cell's area (for hit testing and cell selection). A vertically merged cell is
    /// one `Cell` as tall as all its rows; its continuation rows get a `covered` `Cell` (their
    /// own row's height) so the rows keep a box each, but it isn't a cell of its own on screen.
    Cell {
        rect: Rect,
        covered: bool,
        table: Path,
        row: usize,
        cell: usize,
        story: StoryRef,
    },
    /// The area of a picture, shape or text box, for hit testing, selection handles and dragging
    /// (drawn by `Image`/`Shape`/`Lines`). The object is the U+FFFC at byte `off` of paragraph
    /// `path` in `story`. `rect` is its unrotated frame; `spin` turns it about the centre.
    Object {
        rect: Rect,
        spin: Spin,
        story: StoryRef,
        path: Path,
        off: usize,
        /// The text box story it shows (`Document::parts` id).
        text_box: Option<u32>,
        wrap: Wrap,
        /// Its paragraph's column left and top: where offsets relative to the column and the
        /// paragraph (`Anchor::Column` / `Anchor::Paragraph`) start.
        origin: wordcraft_geom::Point,
    },
}

impl Placed {
    fn translate(&mut self, dx: f32, dy: f32) {
        match self {
            Placed::Lines { x, y, .. } => {
                *x += dx;
                *y += dy;
            }
            Placed::Fill { rect, .. }
            | Placed::Image { rect, .. }
            | Placed::Shape { rect, .. }
            | Placed::Graphic { rect, .. }
            | Placed::Cell { rect, .. } => {
                rect.x += dx;
                rect.y += dy;
            }
            Placed::Object { rect, origin, .. } => {
                rect.x += dx;
                rect.y += dy;
                origin.x += dx;
                origin.y += dy;
            }
            Placed::Clip { rect, items, .. } => {
                rect.x += dx;
                rect.y += dy;
                for it in items {
                    it.translate(dx, dy);
                }
            }
            Placed::Rule { x0, y0, x1, y1, .. } => {
                *x0 += dx;
                *x1 += dx;
                *y0 += dy;
                *y1 += dy;
            }
        }
    }

    /// The page area of turned lines (`None` for anything else).
    pub fn turned_bounds(&self) -> Option<Rect> {
        let Placed::Lines { para, l0, l1, x, y, turn, .. } = self else { return None };
        if !turn.is_turned() {
            return None;
        }
        let first = para.lines.get(*l0)?;
        let last = para.lines.get(l1.checked_sub(1)?)?;
        let len = para.lines.get(*l0..*l1).unwrap_or(&[]).iter().map(|l| l.right).fold(0.0f32, f32::max);
        Some(turn_rect(*turn, *x, *y, Rect::new(0.0, 0.0, len, last.top + last.height - first.top)))
    }

    /// Move an item laid out in a turned frame (a table cell's text running `turn`) onto the
    /// page, the frame's origin landing on page point (`x`, `y`).
    fn turn(&mut self, turn: TextDirection, x: f32, y: f32) {
        if !turn.is_turned() {
            self.translate(x, y);
            return;
        }
        match self {
            Placed::Lines { x: lx, y: ly, turn: t, .. } => {
                (*lx, *ly) = turn_point(turn, x, y, *lx, *ly);
                // Turned text inside turned text keeps its own direction (no 180° text).
                if !t.is_turned() {
                    *t = turn;
                }
            }
            Placed::Fill { rect, .. }
            | Placed::Image { rect, .. }
            | Placed::Shape { rect, .. }
            | Placed::Graphic { rect, .. }
            | Placed::Cell { rect, .. } => {
                *rect = turn_rect(turn, x, y, *rect);
            }
            Placed::Object { rect, origin, .. } => {
                *rect = turn_rect(turn, x, y, *rect);
                (origin.x, origin.y) = turn_point(turn, x, y, origin.x, origin.y);
            }
            Placed::Clip { rect, items, .. } => {
                *rect = turn_rect(turn, x, y, *rect);
                for it in items {
                    it.turn(turn, x, y);
                }
            }
            Placed::Rule { x0, y0, x1, y1, .. } => {
                (*x0, *y0) = turn_point(turn, x, y, *x0, *y0);
                (*x1, *y1) = turn_point(turn, x, y, *x1, *y1);
            }
        }
    }
}

/// Where point (`u`, `v`) of a frame turned `turn` lands on the page, the frame's origin being
/// page point (`x`, `y`): `u` runs along the lines, `v` across them (down for horizontal text).
pub fn turn_point(turn: TextDirection, x: f32, y: f32, u: f32, v: f32) -> (f32, f32) {
    match turn {
        TextDirection::Horizontal => (x + u, y + v),
        TextDirection::Down => (x - v, y + u),
        TextDirection::Up => (x + v, y - u),
    }
}

/// The inverse of [`turn_point`]: the frame point at page point (`px`, `py`).
pub fn unturn_point(turn: TextDirection, x: f32, y: f32, px: f32, py: f32) -> (f32, f32) {
    match turn {
        TextDirection::Horizontal => (px - x, py - y),
        TextDirection::Down => (py - y, x - px),
        TextDirection::Up => (y - py, px - x),
    }
}

/// The page rectangle a frame rectangle covers (see [`turn_point`]).
pub fn turn_rect(turn: TextDirection, x: f32, y: f32, r: Rect) -> Rect {
    match turn {
        TextDirection::Horizontal => Rect::new(x + r.x, y + r.y, r.w, r.h),
        TextDirection::Down => Rect::new(x - r.y - r.h, y + r.x, r.h, r.w),
        TextDirection::Up => Rect::new(x + r.y, y - r.x - r.w, r.h, r.w),
    }
}

/// One laid-out page.
#[derive(Clone, Debug, Default)]
pub struct Page {
    pub w: f32,
    pub h: f32,
    /// Index into `Document::sections()`.
    pub section: usize,
    /// Page number as displayed (after section restarts).
    pub number: u32,
    pub items: Vec<Placed>,
    /// Header / footer items (drawn dimmed while editing the body).
    pub header: Vec<Placed>,
    pub footer: Vec<Placed>,
    /// The body text area (margins).
    pub body: Rect,
    pub header_story: Option<u32>,
    pub footer_story: Option<u32>,
    /// Top-level body block index at the start of the page.
    pub first_block: usize,
    /// Width of the markup area on the right of `w` that holds comment balloons (Print Markup,
    /// [`LayoutOptions::comment_balloons`]); 0 without one. The document page is `w - markup` wide.
    pub markup: f32,
}

/// The type of content involved in a layout overflow diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverflowElement {
    BodyText,
    Table,
    TableCell,
    Image,
    FloatingObject,
    TextBox,
}

/// The boundary a placed item crosses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverflowAxis {
    Horizontal,
    Vertical,
}

/// Clipping is more severe than content merely extending past the normal text area.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OverflowSeverity {
    Warning,
    Clipped,
}

/// A non-mutating report of content that extends beyond its available layout region.
#[derive(Clone, Debug, PartialEq)]
pub struct OverflowDiagnostic {
    /// Zero-based physical page index.
    pub page: usize,
    pub element: OverflowElement,
    pub axis: OverflowAxis,
    pub severity: OverflowSeverity,
    /// The item's page-coordinate bounds, useful for navigation and future highlighting.
    pub bounds: Rect,
}

/// The whole layout.
#[derive(Clone, Debug, Default)]
pub struct DocLayout {
    pub pages: Vec<Page>,
    /// Zero-based pages with vertical overflow or clipping. Oversized indivisible objects and
    /// table rows are kept intact for progress, but callers can surface this diagnostic.
    pub overflow_pages: Vec<usize>,
    /// Per-item horizontal/vertical overflow and clipping reports. These are derived from the
    /// layout only; they do not alter document geometry or the DOCX package.
    pub overflows: Vec<OverflowDiagnostic>,
    /// Zero-based pages where column balancing was requested at a section/document end but
    /// skipped because the page contained content that cannot yet be safely redistributed.
    pub column_balance_skipped: Vec<usize>,
    /// Index: (story, paragraph path) → (page, item index) of each placed piece, in order.
    pub index: HashMap<(StoryRef, Path), Vec<(usize, usize)>>,
    /// Layout time, milliseconds.
    pub ms: f64,
    /// Text box stories laid out (each time a box's text was).
    pub text_boxes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    rev: u64,
    width: u32,
    label: Option<String>,
    page: Option<(u32, u32, u32, u32)>,
    table: u64,
    notes: u64,
    excl: u64,
    eq: u32,
    read_spacing: Option<u32>,
    grid: (u32, u32, u32, bool),
    fit: bool,
    hide_deleted: bool,
}

/// Memoised paragraph layouts.
#[derive(Default)]
pub struct LayoutCache {
    paras: HashMap<Key, Arc<ParaLayout>>,
    /// Outlines of pictures' opaque pixels (Tight and Through wrapping), by media key, the
    /// media's bytes (address and length) and crop.
    outlines: HashMap<(String, usize, usize, [u32; 4]), Option<Arc<Vec<(f32, f32)>>>>,
    env: u64,
    used: HashSet<Key>,
    pub hits: u64,
    pub misses: u64,
}

impl LayoutCache {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.paras.len()
    }
    pub fn is_empty(&self) -> bool {
        self.paras.is_empty()
    }
    pub fn clear(&mut self) {
        self.paras.clear();
        self.outlines.clear();
    }
}

fn hash_of<T: Hash>(t: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

fn env_hash(doc: &Document, opts: &LayoutOptions) -> u64 {
    let s = serde_json::to_string(&(&doc.styles, &doc.numbering, doc.settings.default_tab, &doc.settings.footnote_format)).unwrap_or_default();
    hash_of(&(
        s,
        opts.show_hidden,
        opts.hide_deleted,
        opts.deletions_in_balloons,
        opts.proofing,
        opts.numeral,
        opts.field_codes,
        wordcraft_proof::user_dictionary().len(),
        hyphenation_key(doc),
    ))
}

/// The hyphenation settings, hashable (the zone as its bits).
fn hyphenation_key(doc: &Document) -> (bool, u32, bool, u32) {
    let s = &doc.settings;
    (s.auto_hyphenation, s.hyphenation_zone.to_bits(), s.hyphenate_caps, s.consecutive_hyphen_limit)
}

fn has_page_fields(p: &Paragraph) -> bool {
    p.objects.iter().any(|o| match o {
        InlineObject::Field { instr, .. } => {
            matches!(fields::field_name(instr).as_str(), "PAGE" | "NUMPAGES" | "SECTIONPAGES" | "SECTION")
        }
        _ => false,
    })
}

fn has_total_fields(p: &Paragraph) -> bool {
    p.objects
        .iter()
        .any(|o| matches!(o, InlineObject::Field { instr, .. } if matches!(fields::field_name(instr).as_str(), "NUMPAGES" | "SECTIONPAGES")))
}

#[derive(Clone, Debug, PartialEq)]
struct Totals {
    pages: u32,
    sections: Vec<u32>,
}

impl Totals {
    fn first_guess(count: usize) -> Self {
        Self { pages: 1, sections: vec![1; count] }
    }
    fn of(pages: &[Page], count: usize) -> Self {
        let mut sections = vec![0u32; count];
        for page in pages {
            if let Some(total) = sections.get_mut(page.section) {
                *total = total.saturating_add(1);
            }
        }
        for total in &mut sections {
            *total = (*total).max(1);
        }
        Self { pages: u32::try_from(pages.len()).unwrap_or(u32::MAX).max(1), sections }
    }
    fn section(&self, i: usize) -> u32 {
        self.sections.get(i).copied().unwrap_or(1)
    }
}

/// Walk state while paginating.
struct Ctx<'a> {
    doc: &'a Document,
    cache: &'a mut LayoutCache,
    opts: &'a LayoutOptions,
    hide_deleted: bool,
    counters: Counters,
    fields: FieldCtx,
    notes_hash: u64,
    numbers: HashMap<u32, Arc<ParaLayout>>,
    /// Automatic equation numbers so far.
    eq_count: u32,
    /// Bounds laying out text boxes inside text boxes, for the whole layout.
    boxes: wordcraft_doc::BoxBudget,
    /// Linked text boxes and their per-layout text flows.
    chains: textbox::Chains,
    /// Current section's document grid; not applied to headers/footers.
    grid: para::Grid,
    /// Text-box content found outside its frame before the fit pass. The anchor keys let
    /// the final page pass attach each report after tables and floating objects are positioned.
    box_clips: Vec<(StoryRef, Path, usize, OverflowAxis)>,
    quiet_marks: Vec<usize>,
}

impl Ctx<'_> {
    /// The outline Tight and Through wrapping follow around `o` (see [`contour::wrap_outline`]);
    /// pictures' are worked out once per picture and crop.
    fn wrap_outline(&mut self, o: &InlineObject) -> Option<Arc<Vec<(f32, f32)>>> {
        let (doc, cache) = (self.doc, &mut *self.cache);
        contour::wrap_outline(o, |media, crop| {
            let bytes = doc.media.get(media)?;
            let key = (media.to_string(), Arc::as_ptr(bytes) as usize, bytes.len(), crop.map(f32::to_bits));
            if let Some(o) = cache.outlines.get(&key) {
                return o.clone();
            }
            if cache.outlines.len() >= 256 {
                cache.outlines.clear();
            }
            let outline = contour::picture_outline(bytes, crop).map(Arc::new);
            cache.outlines.insert(key, outline.clone());
            outline
        })
    }

    /// A laid-out line number (cached).
    fn number_para(&mut self, n: u32) -> Arc<ParaLayout> {
        if let Some(p) = self.numbers.get(&n) {
            return p.clone();
        }
        let mut p = Paragraph::with_text(
            &n.to_string(),
            CharProps { size: Some(10.0), color: Some(wordcraft_doc::TextColor::Rgb(Rgb(0x60, 0x60, 0x60))), ..Default::default() },
        );
        p.props.space_after = Some(0.0);
        p.props.space_before = Some(0.0);
        let env = para::ParaEnv {
            doc: self.doc,
            width: 60.0,
            label: None,
            fields: &self.fields,
            show_hidden: false,
            field_codes: false,
            hide_deleted: false,
            table: None,
            proofing: None,
            numeral: self.opts.numeral,
            exclusions: &[],
            eq_number: 0,
            read_line_spacing: None,
            grid: para::Grid::default(),
            fit_objects: false,
        };
        let pl = Arc::new(para::layout_para(&p, &env));
        self.numbers.insert(n, pl.clone());
        pl
    }

    /// The list label of `p`, advancing the list counters: call it once per paragraph, in
    /// document order (only for paragraphs that will show).
    fn next_label(&mut self, p: &Paragraph) -> Option<(String, Level)> {
        let n = p.props.numbering.or_else(|| self.doc.styles.resolve_para(&p.props).numbering).filter(|n| n.num != 0)?;
        self.counters.next_label(&self.doc.numbering, n.num, n.level)
    }

    /// Lay out `p` with its already counted `label`, so laying a paragraph out again (around
    /// floating objects, or after it moved) doesn't advance the list counters twice.
    fn para_labelled(
        &mut self,
        p: &Paragraph,
        width: f32,
        table: Option<&para::CellText>,
        exclusions: &[para::Exclusion],
        label: Option<(String, Level)>,
    ) -> Arc<ParaLayout> {
        let page = if has_page_fields(p) { Some(self.fields.page_key()) } else { None };
        // Automatic equation numbers count through the document.
        let eq_here: u32 = p
            .objects
            .iter()
            .map(|o| match o {
                InlineObject::Equation { math, display, .. } => math::auto_numbers(math, *display),
                _ => 0,
            })
            .sum();
        let eq_number = self.eq_count;
        self.eq_count = self.eq_count.saturating_add(eq_here);
        let key = Key {
            rev: p.rev,
            width: width.to_bits(),
            label: label.as_ref().map(|(t, l)| format!("{t}|{}|{}|{:?}|{:?}|{:?}|{:?}", l.indent, l.hanging, l.suffix, l.align, l.tab, l.chr)),
            page,
            table: table.map(|t| hash_of(&format!("{t:?}"))).unwrap_or(0),
            notes: if p.objects.iter().any(|o| matches!(o, InlineObject::NoteRef { .. })) { self.notes_hash } else { 0 },
            excl: if exclusions.is_empty() { 0 } else { hash_of(&format!("{exclusions:?}")) },
            eq: if eq_here > 0 { eq_number } else { 0 },
            read_spacing: if self.opts.view == ViewMode::Read { self.opts.read_line_spacing.map(f32::to_bits) } else { None },
            grid: self.grid.key(),
            fit: self.boxes.inside(),
            hide_deleted: self.hide_deleted,
        };
        self.cache.used.insert(key.clone());
        if let Some(pl) = self.cache.paras.get(&key) {
            self.cache.hits += 1;
            return pl.clone();
        }
        self.cache.misses += 1;
        let env = para::ParaEnv {
            doc: self.doc,
            width,
            label,
            fields: &self.fields,
            show_hidden: self.opts.show_hidden,
            field_codes: self.opts.field_codes,
            hide_deleted: self.hide_deleted,
            table,
            proofing: self.opts.proofing.then_some(self.opts.proof),
            numeral: self.opts.numeral,
            exclusions,
            eq_number,
            read_line_spacing: if self.opts.view == ViewMode::Read { self.opts.read_line_spacing } else { None },
            grid: self.grid,
            fit_objects: self.boxes.inside(),
        };
        let pl = Arc::new(para::layout_para(p, &env));
        self.cache.paras.insert(key, pl.clone());
        pl
    }
}

/// Which of `p`'s objects (by index) the layout leaves out ([`para::left_out`]): anchored in hidden
/// text unless `show_hidden`, or in a tracked deletion with `hide_deleted`. `table_chr` is the
/// cell's table-style character formatting ([`para::CellText::chr`]), resolved as the paragraph is
/// laid out with (`StyleSheet::resolve_char_in`).
fn left_out_objects(doc: &Document, p: &Paragraph, table_chr: Option<&CharProps>, show_hidden: bool, hide_deleted: bool) -> impl Fn(usize) -> bool {
    let mut left = Vec::new();
    // Nothing can be left out with hidden text shown and markup on.
    if (hide_deleted || !show_hidden) && !p.objects.is_empty() {
        let style = p.props.style.as_deref();
        let is_left = |c: &CharProps| {
            let rc = doc.styles.resolve_char_in(style, table_chr, c);
            para::left_out(&rc, show_hidden, hide_deleted)
        };
        // Runs and objects are both in order: one walk finds each object's run (past the last run,
        // the paragraph mark, as `props_of_char` gives), resolving a run's formatting only once.
        let mut runs = p.run_ranges().peekable();
        let mut last: Option<(usize, bool)> = None;
        for off in p.object_offsets() {
            while runs.next_if(|(r, _)| r.end <= off).is_some() {}
            let l = match runs.peek() {
                Some((r, c)) => match last {
                    Some((end, l)) if end == r.end => l,
                    _ => {
                        let l = is_left(c);
                        last = Some((r.end, l));
                        l
                    }
                },
                None => is_left(&p.mark),
            };
            left.push(l);
        }
    }
    move |k| left.get(k).copied().unwrap_or(false)
}

/// Footnote and endnote numbers (each kind counted separately), in document order, as each
/// section's options say: starting at its "start at", numbered on, or starting again in each
/// section or (footnotes) on each page. `pages` gives the page each footnote was placed on by an
/// earlier pass (empty: numbered on). With `hide_deleted`, notes whose reference mark is a
/// tracked deletion are left out, as in the final text.
fn note_info(doc: &Document, hide_deleted: bool, pages: &HashMap<u32, usize>) -> HashMap<u32, fields::NoteNum> {
    use wordcraft_doc::section::NoteRestart;
    let sections = doc.sections();
    let opts: Vec<[wordcraft_doc::section::NoteOptions; 2]> =
        sections.iter().map(|(_, s)| [doc.note_options(s, false), doc.note_options(s, true)]).collect();
    let mut m = HashMap::new();
    // Per kind (footnotes, endnotes): the last number, and the section and page of the last note.
    let mut last: [Option<(u32, usize, Option<usize>)>; 2] = [None, None];
    let mut order = [0u32; 2];
    let mut si = 0usize;
    // Reading order, text boxes included where they are. A hidden reference mark still takes
    // its number, as in Word; a deleted one (or one in a deleted text box) does not.
    let mut cur: Option<(usize, Vec<bool>)> = None;
    doc.objects_in_reading_order_by_block(&mut |block, p, k| {
        let key = p as *const Paragraph as usize;
        if cur.as_ref().is_none_or(|(at, _)| *at != key) {
            let deleted = left_out_objects(doc, p, None, true, hide_deleted);
            cur = Some((key, (0..p.objects.len()).map(deleted).collect()));
        }
        if cur.as_ref().is_some_and(|(_, deleted)| deleted.get(k).copied().unwrap_or(false)) {
            return false;
        }
        if let Some(InlineObject::NoteRef { kind, id, .. }) = p.objects.get(k) {
            while sections.get(si).is_some_and(|(end, _)| block > *end) && si + 1 < sections.len() {
                si += 1;
            }
            let ki = usize::from(*kind == wordcraft_doc::para::NoteKind::Endnote);
            let Some(o) = opts.get(si).and_then(|o| o.get(ki)) else { return true };
            let page = if ki == 0 { pages.get(id).copied() } else { None };
            let Some(slot) = last.get_mut(ki) else { return true };
            let n = match *slot {
                None => o.num_start,
                Some((prev, psect, ppage)) => {
                    let again = match o.num_restart {
                        NoteRestart::Continuous => false,
                        NoteRestart::EachSect => psect != si,
                        NoteRestart::EachPage => page.is_some() && page != ppage,
                    };
                    if again { o.num_start } else { prev.saturating_add(1) }
                }
            };
            *slot = Some((n, si, page));
            let ord = order.get_mut(ki).map(|c| {
                *c = c.saturating_add(1);
                *c
            });
            m.insert(*id, fields::NoteNum { n, fmt: o.num_fmt, section: si, order: ord.unwrap_or(0) });
        }
        true
    });
    m
}

/// Note part ids → numbers, numbered on (see [`note_info`]).
#[cfg(test)]
fn note_numbers(doc: &Document, hide_deleted: bool) -> HashMap<u32, u32> {
    note_info(doc, hide_deleted, &HashMap::new()).into_iter().map(|(id, n)| (id, n.n)).collect()
}

/// The page each footnote's text starts on.
fn footnote_pages(doc: &Document, pages: &[Page]) -> HashMap<u32, usize> {
    let mut m = HashMap::new();
    for (pi, p) in pages.iter().enumerate() {
        for it in &p.items {
            if let Placed::Lines { story: StoryRef::Part(id), .. } | Placed::Cell { story: StoryRef::Part(id), .. } = it
                && doc.parts.get(id).is_some_and(|part| part.kind == wordcraft_doc::PartKind::Footnote)
            {
                m.entry(*id).or_insert(pi);
            }
        }
    }
    m
}

/// Lay out a block list into a free-standing box of `width` (no page breaks): table cells,
/// headers, footers, notes, text boxes. Returns items relative to (0, 0) and the height. `frame`
/// says where the box sits on its page, for floating objects positioned relative to the page or
/// its margins; without it (cells, notes, text boxes) they're positioned in the box.
#[allow(clippy::too_many_arguments)]
fn layout_box(
    ctx: &mut Ctx,
    story: StoryRef,
    blocks: &Blocks,
    prefix: &[u32],
    width: f32,
    table: Option<&para::CellText>,
    depth: usize,
    frame: Option<PageFrame>,
) -> BoxLayout {
    // Deletions in balloons leave the body's text only: a header, note or text box keeps its own
    // inline (restored for the story around this box when it's done).
    let outer_hide = std::mem::replace(&mut ctx.hide_deleted, ctx.opts.hides_deleted(story));
    let mut items = Vec::new();
    // Pictures behind the text go first; `behind` counts them.
    let mut behind = 0;
    // Wrap areas of the box's floating objects.
    let mut excl: Vec<WrapArea> = Vec::new();
    // Areas of the box's floating tables, which other floating tables may have to avoid.
    let mut float_tables: Vec<Rect> = Vec::new();
    let mut y = 0.0f32;
    let mut prev_after = 0.0f32;
    let mut prev_after_auto = false;
    let mut prev_listed = false;
    let mut prev_style: Option<(String, bool)> = None;
    let mut last_top = 0.0f32;
    for (i, b) in blocks.iter().enumerate() {
        last_top = y;
        let mut path = prefix.to_vec();
        path.push(i as u32);
        match &**b {
            Block::Para(p) => {
                let label = ctx.next_label(p);
                let listed = label.is_some();
                let mut pl = ctx.para_labelled(p, width, table, &[], label.clone());
                // A text frame (a footer's page number in a frame, say) is placed apart, like a
                // floating object, and the text after it wraps around it; it takes no room in
                // the flow.
                if let Some(fr) = pl.rp.frame.clone() {
                    let fw = fr.w.unwrap_or_else(|| natural_width(&pl)).clamp(1.0, 1584.0);
                    let pf = if (fw - width).abs() > 0.01 { ctx.para_labelled(p, fw, table, &[], label) } else { pl };
                    let h = match fr.h {
                        Some(h) if fr.h_exact => h,
                        Some(h) => h.max(pf.height),
                        None => pf.height,
                    };
                    let float = frame_float(&fr);
                    let r = float_rect(frame, (0.0, width), y, fw, h, &float);
                    push_para(&mut items, story, &path, &pf, 0, pf.lines.len(), r.x, r.y, fw);
                    excl.extend(wrap_area(r, &float, None));
                    float_tables.push(r);
                    continue;
                }
                let ctxl = pl.rp.contextual_spacing;
                let same = prev_style.as_ref().is_some_and(|(s, c)| *s == pl.rp.style && (*c || ctxl));
                let mut before = if same && ctxl { 0.0 } else { pl.rp.space_before };
                if same && ctxl {
                    y -= prev_after;
                    prev_after = 0.0;
                }
                // Automatic spacing: none between two list items (as in an HTML list).
                if i > 0 && prev_listed && listed {
                    if prev_after_auto {
                        y -= prev_after;
                        prev_after = 0.0;
                    }
                    if pl.rp.before_auto {
                        before = 0.0;
                    }
                }
                // Automatic spacing: none at the top of a cell or box, and it collapses with the
                // space after the paragraph above (the larger one counts).
                if pl.rp.before_auto && i == 0 {
                    before = 0.0;
                } else if i > 0 && before > 0.0 && prev_after > 0.0 {
                    before = (before - prev_after).max(0.0);
                }
                y += if i == 0 { before } else { before.max(0.0) };
                // Floating objects anchored here: place them, then wrap the text around them.
                // One left out of the layout (hidden, or deleted in the final text) takes no room.
                let mut floats = HashMap::new();
                let left_out = left_out_objects(ctx.doc, p, table.map(|t| &t.chr), ctx.opts.show_hidden, ctx.hide_deleted);
                for (oi, o) in p.objects.iter().enumerate() {
                    let Some((w, h, float)) = floating(o) else { continue };
                    if left_out(oi) {
                        continue;
                    }
                    let r = float_rect(frame, (0.0, width), y, w, h, float);
                    let outline = ctx.wrap_outline(o);
                    excl.extend(wrap_area(r, float, outline.as_deref().map(|v| v.as_slice())));
                    floats.insert(oi, r);
                }
                let rel = rel_exclusions(&excl, 0.0, y);
                if !rel.is_empty() {
                    pl = ctx.para_labelled(p, width, table, &rel, label);
                }
                let lead = pl.lines.first().map_or(0.0, |l| l.top.max(0.0));
                push_para(&mut items, story, &path, &pl, 0, pl.lines.len(), 0.0, y + lead, width);
                // Pictures, shapes and text boxes: drawn, their areas and text boxes' text.
                if !p.objects.is_empty() {
                    let at = ObjFrame { page: frame, col: (0.0, width), para_y: y, table_chr: table.map(|t| &t.chr) };
                    let (back, front) = place_objects(ctx, story, &path, p, &pl, (0, pl.lines.len()), (0.0, y + lead), &at, &floats, depth);
                    let at = behind.min(items.len());
                    behind += back.len();
                    items.splice(at..at, back);
                    items.extend(front);
                }
                // Automatic space after: none after the last paragraph of a cell or box.
                let after = if pl.rp.after_auto && i + 1 == blocks.len() { 0.0 } else { pl.rp.space_after };
                y += pl.height + after;
                prev_after = after;
                prev_after_auto = pl.rp.after_auto;
                prev_listed = listed;
                prev_style = Some((pl.rp.style.clone(), ctxl));
            }
            Block::Table(t) => {
                if depth > 8 {
                    continue;
                }
                let tl = table::layout_table(ctx, story, t, &path, width, depth + 1);
                let (tx, ty) = match t.props.float {
                    // A floating table stands at its own position and takes no room; the text
                    // after it wraps around it.
                    Some(f) => {
                        let legacy = if ctx.doc.settings.compat_mode < 15 { table::first_cell_left_margin(ctx, t) } else { 0.0 };
                        let (r, area) = box_float_table_rect(frame, width, y, &tl, &f, legacy, &float_tables);
                        excl.push((area, false, None, WrapText::BothSides));
                        float_tables.push(area);
                        (r.x, r.y)
                    }
                    // A table in the text flow starts below the floating objects in its way.
                    None => (tl.x, below(&excl, y, tl.x, tl.x + tl.width)),
                };
                let mut row_y = ty;
                for row in &tl.rows {
                    for it in &row.items {
                        let mut it = it.clone();
                        it.translate(tx, row_y);
                        items.push(it);
                    }
                    row_y += row.height;
                }
                if t.props.float.is_none() {
                    y = row_y;
                    prev_after = 0.0;
                    prev_style = None;
                }
            }
        }
    }
    let floats_bottom = float_tables.iter().map(|r| r.bottom()).fold(0.0, f32::max);
    ctx.hide_deleted = outer_hide;
    BoxLayout { items, height: y.max(0.0), floats_bottom, last_top: last_top.max(0.0) }
}

/// The width a text frame without one takes: its widest line, with the paragraph's indents.
fn natural_width(pl: &ParaLayout) -> f32 {
    let widest = (0..pl.lines.len())
        .filter_map(|li| {
            let l = pl.lines.get(li)?;
            Some((pl.x_of(li, l.stop)? - pl.x_of(li, l.start)?).abs())
        })
        .fold(0.0f32, f32::max);
    widest + pl.rp.indent_left.max(0.0) + pl.rp.indent_right.max(0.0)
}

/// A text frame's placement as a floating object's (`w:framePr` → anchors, alignment, wrap).
fn frame_float(fr: &wordcraft_doc::props::TextFrame) -> Float {
    let anchor = |a: &str, text: Anchor| match a {
        "page" => Anchor::Page,
        "margin" => Anchor::Margin,
        _ => text,
    };
    let align = |a: Option<&str>| match a {
        Some("left" | "top") => Some(FloatAlign::Start),
        Some("center") => Some(FloatAlign::Center),
        Some("right" | "bottom") => Some(FloatAlign::End),
        Some("inside") => Some(FloatAlign::Inside),
        Some("outside") => Some(FloatAlign::Outside),
        _ => None,
    };
    Float {
        wrap: match fr.wrap.as_str() {
            "notBeside" => Wrap::TopAndBottom,
            "none" => Wrap::InFrontOfText,
            "tight" => Wrap::Tight,
            "through" => Wrap::Through,
            _ => Wrap::Square,
        },
        h_rel: anchor(&fr.h_anchor, Anchor::Column),
        v_rel: anchor(&fr.v_anchor, Anchor::Paragraph),
        x: fr.x,
        y: fr.y,
        h_align: align(fr.x_align.as_deref()),
        v_align: align(fr.y_align.as_deref()),
        dist: fr.h_space,
        dist_top: fr.v_space,
        dist_bottom: fr.v_space,
        ..Default::default()
    }
}

/// A laid-out box: items relative to its top-left, the height of its text flow, where its
/// floating tables end (a table cell grows to hold them) and where its last block starts (the
/// height without it).
struct BoxLayout {
    items: Vec<Placed>,
    height: f32,
    floats_bottom: f32,
    last_top: f32,
}

/// Where floating table `tl` goes in a box `width` wide whose next paragraph starts at `y`, and
/// the area text keeps clear of. Positions relative to the page or its margins need the box's
/// `frame`; without one (a table cell) they're relative to the box.
fn box_float_table_rect(
    frame: Option<PageFrame>,
    width: f32,
    y: f32,
    tl: &table::TableLayout,
    f: &TableFloat,
    legacy: f32,
    float_tables: &[Rect],
) -> (Rect, Rect) {
    let h = tl.rows.iter().map(|r| r.height).sum::<f32>().clamp(0.0, 100_000.0);
    let w = tl.width.clamp(1.0, 100_000.0);
    let page_x = |s: &SectionProps| match f.h_rel {
        Anchor::Page => Some((0.0, s.page_w)),
        Anchor::Margin => Some((s.margin_left + s.gutter, s.text_width())),
        _ => None,
    };
    let page_y = |s: &SectionProps| match f.v_rel {
        Anchor::Page => Some((0.0, s.page_h)),
        Anchor::Margin => Some((s.margin_top, s.text_height())),
        _ => None,
    };
    let h_area = frame.and_then(|fr| page_x(fr.sect).map(|(a, e)| (a - fr.origin.0, e))).unwrap_or((0.0, width));
    // In a cell an offset from the page or margin has nothing to measure from: the table stands
    // where the text is.
    let (v_area, y_off) = match frame.and_then(|fr| page_y(fr.sect).map(|(a, e)| (a - fr.origin.1, e))) {
        Some(area) => (area, f.y),
        None if matches!(f.v_rel, Anchor::Page | Anchor::Margin) => ((y, 0.0), 0.0),
        None => ((y, 0.0), f.y),
    };
    // Nor is there an area to align in: the table starts where the text is.
    let v_align = if frame.is_none() && matches!(f.v_rel, Anchor::Page | Anchor::Margin) { None } else { f.v_align };
    let x = align_in(h_area, f.x, w, f.h_align) - if f.h_align.is_none() { legacy } else { 0.0 };
    let r = clear_of_float_tables(Rect::new(x, align_in(v_area, y_off, h, v_align), w, h), f, legacy, h_area.0, float_tables);
    let [dl, dt, dr, db] = f.dist_from_text();
    (r, r.inset(-dl, -dt, -dr, -db))
}

/// Floating table `f` at `r`, moved below the floating tables in its way when it may not
/// overlap them. In Word 2013+ layout one placed by an offset keeps its distance from the text's
/// left edge (`text_left`).
fn clear_of_float_tables(mut r: Rect, f: &TableFloat, legacy: f32, text_left: f32, others: &[Rect]) -> Rect {
    let [dl, dt, dr, db] = f.dist_from_text();
    let keeps_text_distance = legacy == 0.0 && f.h_align.is_none() && f.x >= 0.0 && matches!(f.h_rel, Anchor::Margin | Anchor::Column);
    for _ in 0..64 {
        let area = r.inset(-dl, -dt, -dr, -db);
        let Some(below) = others.iter().filter(|o| !f.overlap && o.intersects(&area)).map(|o| o.bottom()).reduce(f32::max) else { break };
        r.y = below + dt;
        if keeps_text_distance {
            r.x = r.x.max(text_left + dl);
        }
    }
    r
}

/// A text box's default internal margins (Word's: 0.1" left/right, 0.05" top/bottom); a box's
/// own come from its [`wordcraft_doc::TextBody`].
#[cfg(test)]
const BOX_INSET_X: f32 = 7.2;
#[cfg(test)]
const BOX_INSET_Y: f32 = 3.6;

/// Keep what fits in a box `height` tall (items relative to its top): lines that don't fit are
/// dropped (whole lines; a line that only partly fits too) and so is anything starting below.
/// The first line always stays, so even a tiny box shows the start of its text.
fn fit_box(items: Vec<Placed>, height: f32) -> Vec<Placed> {
    let limit = height + 0.5;
    let mut out = Vec::with_capacity(items.len());
    let mut kept_line = false;
    for it in items {
        if let Some(r) = it.turned_bounds() {
            // Turned text (in a table cell) stays whole.
            if r.y < limit {
                out.push(it);
            }
            continue;
        }
        match it {
            Placed::Lines { story, path, para, l0, l1, x, y, turn } => {
                let Some(first) = para.lines.get(l0) else { continue };
                let mut end = l0;
                for k in l0..l1 {
                    let Some(l) = para.lines.get(k) else { break };
                    if y + (l.top - first.top) + l.height > limit && kept_line {
                        break;
                    }
                    end = k + 1;
                    kept_line = true;
                }
                if end > l0 {
                    out.push(Placed::Lines { story, path, para, l0, l1: end, x, y, turn });
                }
            }
            Placed::Fill { rect, color } if rect.y < limit => {
                out.push(Placed::Fill { rect: Rect::new(rect.x, rect.y, rect.w, rect.h.min(limit - rect.y)), color });
            }
            Placed::Rule { x0, y0, x1, y1, border } if y0.min(y1) < limit => {
                out.push(Placed::Rule { x0, y0: y0.min(limit), x1, y1: y1.min(limit), border });
            }
            Placed::Image { rect, .. }
            | Placed::Shape { rect, .. }
            | Placed::Graphic { rect, .. }
            | Placed::Clip { rect, .. }
            | Placed::Cell { rect, .. }
            | Placed::Object { rect, .. }
                if rect.y < limit =>
            {
                out.push(it);
            }
            _ => {}
        }
    }
    out
}

/// Paragraph lines plus their shading and borders.
fn push_para(items: &mut Vec<Placed>, story: StoryRef, path: &[u32], pl: &Arc<ParaLayout>, l0: usize, l1: usize, x: f32, y: f32, width: f32) {
    let (Some(first), Some(last)) = (pl.lines.get(l0), l1.checked_sub(1).and_then(|k| pl.lines.get(k))) else {
        return;
    };
    let h = last.top + last.height - first.top;
    // Indents are logical: a right-to-left paragraph's start indent (and hanging indent) is on the right.
    let start = pl.rp.indent_left.min(pl.rp.indent_left + pl.rp.indent_first);
    let (li, ri) = if pl.rp.bidi { (pl.rp.indent_right, start) } else { (start, pl.rp.indent_right) };
    let left = x + li;
    let right = x + width - ri;
    if let Some(c) = pl.rp.shading {
        let pad = pl.rp.borders.as_ref().map(|b| b.left.map(|l| l.space).unwrap_or(4.0)).unwrap_or(0.0);
        items.push(Placed::Fill { rect: Rect::new(left - pad, y, right - left + pad * 2.0, h), color: c });
    }
    items.push(Placed::Lines { story, path: Path(path.to_vec()), para: pl.clone(), l0, l1, x, y, turn: TextDirection::Horizontal });
    if let Some(b) = &pl.rp.borders {
        let sp = |o: &Option<Border>| o.map(|b| b.space).unwrap_or(0.0);
        let (lx, rx) = (left - sp(&b.left), right + sp(&b.right));
        let (ty, by) = (y - sp(&b.top), y + h + sp(&b.bottom));
        if let Some(t) = b.top.filter(|_| l0 == 0)
            && t.is_visible()
        {
            items.push(Placed::Rule { x0: lx, y0: ty, x1: rx, y1: ty, border: t });
        }
        if let Some(t) = b.bottom.filter(|_| l1 >= pl.lines.len())
            && t.is_visible()
        {
            items.push(Placed::Rule { x0: lx, y0: by, x1: rx, y1: by, border: t });
        }
        if let Some(t) = b.left.filter(Border::is_visible) {
            items.push(Placed::Rule { x0: lx, y0: ty, x1: lx, y1: by, border: t });
        }
        if let Some(t) = b.right.filter(Border::is_visible) {
            items.push(Placed::Rule { x0: rx, y0: ty, x1: rx, y1: by, border: t });
        }
    }
}

#[derive(Clone)]
struct PageBuilder<'a> {
    pages: Vec<Page>,
    sect: &'a SectionProps,
    sect_idx: usize,
    col: usize,
    cols: Vec<(f32, f32)>,
    y: f32,
    top: f32,
    bottom: f32,
    number: u32,
    web: bool,
    /// Footnotes waiting for the bottom of the current page (first the rest of notes continued
    /// from the page before).
    notes: Vec<NoteBox>,
    /// The parts of footnotes that didn't fit on their page, continued on the next one.
    carry: Vec<NoteBox>,
    /// Page bottom before footnotes took space.
    orig_bottom: f32,
    /// Conservative page-wide footnote space learned from the preceding pagination pass.
    reserved_note_h: f32,
    /// Footnote reservation needed on each finalized page in this pass.
    page_note_heights: Vec<f32>,
    /// Reservations produced by the preceding complete-document pass.
    reserved_note_heights: &'a [f32],
    /// Body bottom after reserving the tallest applicable footer story.
    clearance_bottom: f32,
    /// The previous paragraph: (style, contextual spacing, space after) for contextual spacing.
    prev: Option<(String, bool, f32, bool, bool)>,
    /// An empty paragraph mark after an explicit page/column break has already occupied a line.
    after_break_mark: bool,
    /// Wrap areas of floating objects on this page (see [`wrap_area`]).
    excl: Vec<WrapArea>,
    /// Wrap areas of the floating tables on this page, which others may not overlap.
    float_tables: Vec<Rect>,
    /// Line numbering counter.
    line_no: u32,
    /// Index of the first body item of the current page (vertical alignment shifts from here).
    page_items_start: usize,
    /// The current page's notes and vertical alignment have already been finalized.
    page_finished: bool,
    /// Document footnote separators and continuation notice.
    seps: NoteSeps,
    /// Number of behind-text objects inserted at the beginning of each page.
    behind: Vec<usize>,
    /// Greatest bottom reached by columns on the current page.
    max_y: f32,
    /// Bottom of the last body content on the page.
    content_bottom: f32,
    /// Optional safe column-balance cap for the current page.
    balance: Option<(usize, f32)>,
    /// Page containing the last explicit column break.
    col_break_page: Option<usize>,
    /// Section's footnote placement option.
    sect_beneath: bool,
    /// Whether this page places footnotes beneath body text.
    beneath: bool,
}

/// Gap above the footnote separator and its length.
const NOTE_SEP: f32 = 12.0;
/// Length of the default (short) note separator line.
pub(crate) const NOTE_SEP_W: f32 = 144.0;
/// The story of text drawn on a page that belongs to no story of the document (line numbers,
/// note separators and continuation notices): not hit by clicks.
pub const DECOR_STORY: StoryRef = StoryRef::Part(u32::MAX);

/// A note separator or continuation notice laid out from y = 0 (its items and height).
type SepBox = (Vec<Placed>, f32);

/// The document's own footnote or endnote separators and continuation notice, laid out (`None`:
/// the defaults).
#[derive(Clone, Default)]
struct NoteSeps {
    sep: Option<SepBox>,
    cont: Option<SepBox>,
    notice: Option<SepBox>,
}

impl NoteSeps {
    /// Lay out `seps` `width` wide.
    fn new(ctx: &mut Ctx, seps: &wordcraft_doc::NoteSeparators, width: f32) -> NoteSeps {
        let mut lay = |blocks: &Option<Blocks>| blocks.as_ref().map(|b| sep_box(ctx, b, width));
        NoteSeps { sep: lay(&seps.separator), cont: lay(&seps.continuation_separator), notice: lay(&seps.continuation_notice) }
    }
    /// Height of the separator above a page's notes (`cont`: the continuation separator, when the
    /// first is continued from the page before).
    fn sep_h(&self, cont: bool) -> f32 {
        let own = if cont { &self.cont } else { &self.sep };
        own.as_ref().map_or(NOTE_SEP, |s| s.1)
    }
    /// Height of the notice below a note that continues on the next page.
    fn notice_h(&self) -> f32 {
        self.notice.as_ref().map_or(0.0, |s| s.1)
    }
}

/// A note separator or continuation notice story (`blocks`) laid out `width` wide.
fn sep_box(ctx: &mut Ctx, blocks: &Blocks, width: f32) -> SepBox {
    let bl = layout_box(ctx, DECOR_STORY, blocks, &[], width, None, 0, None);
    let mut items = bl.items;
    items.extend(separator_rules(blocks, &items));
    (items, bl.height)
}

/// The lines drawn by the note separators (see [`InlineObject::note_separator`]) in a separator
/// story's laid-out `items` (`blocks` its paragraphs): across each one's place in its line, half
/// way down the line.
fn separator_rules(blocks: &Blocks, items: &[Placed]) -> Vec<Placed> {
    let mut rules = Vec::new();
    for it in items {
        let Placed::Lines { para, path, l0, l1, x, y, turn: TextDirection::Horizontal, .. } = it else { continue };
        let [i] = path.0.as_slice() else { continue };
        let Some(Block::Para(p)) = blocks.get(*i as usize).map(|b| &**b) else { continue };
        let Some(first) = para.lines.get(*l0) else { continue };
        for line in para.lines.get(*l0..*l1).unwrap_or(&[]) {
            for k in line.c0..line.c1 {
                let Some(para::ClKind::Object(oi)) = para.clusters.get(k).map(|c| c.kind) else { continue };
                if p.objects.get(oi).and_then(InlineObject::as_note_separator).is_none() {
                    continue;
                }
                let (Some(a), Some(b)) = (line.cl_left(k), line.cl_right(k)) else { continue };
                let ry = y + line.top - first.top + line.height / 2.0;
                rules.push(Placed::Rule { x0: x + a.min(b), y0: ry, x1: x + a.max(b), y1: ry, border: Border::single(0.5) });
            }
        }
    }
    rules
}
/// The most pages added after the body for footnotes still continuing.
const MAX_NOTE_PAGES: usize = 10_000;

/// A footnote waiting for the bottom of a page: its items laid out from y = 0, and its height.
#[derive(Clone)]
struct NoteBox {
    id: u32,
    items: Vec<Placed>,
    h: f32,
    /// The rest of a note that didn't fit on an earlier page (drawn below the continuation
    /// separator).
    cont: bool,
}

/// The vertical extent of an item laid out in a horizontal frame.
fn v_span(it: &Placed) -> (f32, f32) {
    if let Some(r) = it.turned_bounds() {
        return (r.y, r.y + r.h);
    }
    match it {
        Placed::Lines { para, l0, l1, y, .. } => (*y, item_bottom(*y, para, *l0, *l1).unwrap_or(*y)),
        Placed::Fill { rect, .. }
        | Placed::Image { rect, .. }
        | Placed::Shape { rect, .. }
        | Placed::Graphic { rect, .. }
        | Placed::Clip { rect, .. }
        | Placed::Cell { rect, .. }
        | Placed::Object { rect, .. } => (rect.y, rect.y + rect.h),
        Placed::Rule { y0, y1, .. } => (y0.min(*y1), y0.max(*y1)),
    }
}

fn placed_bounds(it: &Placed) -> Option<Rect> {
    if let Some(r) = it.turned_bounds() {
        return Some(r);
    }
    match it {
        Placed::Lines { para, l0, l1, x, y, .. } => {
            para.lines.get(*l0)?;
            let mut left = f32::INFINITY;
            let mut right = f32::NEG_INFINITY;
            for line in para.lines.get(*l0..*l1)? {
                if line.vis.is_empty() {
                    for at in &line.xs {
                        left = left.min(*x + *at);
                        right = right.max(*x + *at);
                    }
                } else {
                    for cl in &line.vis {
                        left = left.min(*x + cl.x);
                        right = right.max(*x + cl.x + cl.w);
                    }
                }
            }
            let bottom = item_bottom(*y, para, *l0, *l1)?;
            (left.is_finite() && right.is_finite())
                .then_some(Rect::new(left, *y, (right - left).max(0.0), (bottom - *y).max(0.0)))
                .or_else(|| Some(Rect::new(*x, *y, 0.0, (bottom - *y).max(0.0))))
                .map(|mut r| {
                    r.y = *y;
                    r.h = (bottom - *y).max(0.0);
                    r
                })
        }
        Placed::Image { rect, spin, .. } | Placed::Shape { rect, spin, .. } | Placed::Graphic { rect, spin, .. } => Some(spin.bounds(*rect)),
        Placed::Object { rect, spin, .. } => Some(spin.bounds(*rect)),
        Placed::Cell { rect, .. } | Placed::Fill { rect, .. } | Placed::Clip { rect, .. } => Some(*rect),
        Placed::Rule { x0, y0, x1, y1, .. } => Some(Rect::new(x0.min(*x1), y0.min(*y1), (x1 - x0).abs(), (y1 - y0).abs())),
    }
}

fn diagnostic(page: usize, element: OverflowElement, axis: OverflowAxis, severity: OverflowSeverity, bounds: Rect) -> OverflowDiagnostic {
    OverflowDiagnostic { page, element, axis, severity, bounds }
}

fn horizontal_outside(bounds: Rect, area: Rect) -> bool {
    bounds.x < area.x - 0.5 || bounds.right() > area.right() + 0.5
}

fn vertical_outside(bounds: Rect, area: Rect) -> bool {
    bounds.y < area.y - 0.5 || bounds.bottom() > area.bottom() + 0.5
}

fn page_edge_severity(bounds: Rect, page: &Page) -> OverflowSeverity {
    if horizontal_outside(bounds, Rect::new(0.0, 0.0, page.w, page.h)) || vertical_outside(bounds, Rect::new(0.0, 0.0, page.w, page.h)) {
        OverflowSeverity::Clipped
    } else {
        OverflowSeverity::Warning
    }
}

fn paragraph_area(section: Option<&SectionProps>, page: &Page, x: f32) -> Rect {
    let Some(section) = section else { return page.body };
    let columns = section.column_boxes();
    let nearest = columns.iter().min_by(|a, b| (page.body.x + a.0 - x).abs().total_cmp(&(page.body.x + b.0 - x).abs()));
    nearest.map_or(page.body, |(offset, width)| Rect::new(page.body.x + offset, page.body.y, *width, page.body.h))
}

fn analyze_overflows(doc: &Document, pages: &[Page], box_clips: &[(StoryRef, Path, usize, OverflowAxis)]) -> Vec<OverflowDiagnostic> {
    let mut overflows = Vec::new();
    let sections = doc.sections();
    for (page_index, page) in pages.iter().enumerate() {
        let section = sections.get(page.section).map(|(_, section)| *section);
        let cells: Vec<_> = page
            .items
            .iter()
            .chain(&page.header)
            .chain(&page.footer)
            .filter_map(|item| match item {
                Placed::Cell { rect, table, row, cell, story, .. } => Some((rect, table, *row, *cell, *story)),
                _ => None,
            })
            .collect();
        for item in page.items.iter().chain(&page.header).chain(&page.footer) {
            match item {
                Placed::Lines { story: StoryRef::Body, path, x, .. } if path.0.len() == 1 => {
                    let Some(bounds) = placed_bounds(item) else { continue };
                    let area = paragraph_area(section, page, *x);
                    if horizontal_outside(bounds, area) {
                        overflows.push(diagnostic(
                            page_index,
                            OverflowElement::BodyText,
                            OverflowAxis::Horizontal,
                            page_edge_severity(bounds, page),
                            bounds,
                        ));
                    }
                    if vertical_outside(bounds, page.body) {
                        overflows.push(diagnostic(
                            page_index,
                            OverflowElement::BodyText,
                            OverflowAxis::Vertical,
                            page_edge_severity(bounds, page),
                            bounds,
                        ));
                    }
                }
                Placed::Lines { story, path, .. } => {
                    let Some(bounds) = placed_bounds(item) else { continue };
                    for (cell_bounds, table_path, row, cell, cell_story) in &cells {
                        let n = table_path.0.len();
                        if *story == *cell_story
                            && path.0.starts_with(&table_path.0)
                            && path.0.get(n) == Some(&(*row as u32))
                            && path.0.get(n + 1) == Some(&(*cell as u32))
                        {
                            if horizontal_outside(bounds, **cell_bounds) {
                                overflows.push(diagnostic(
                                    page_index,
                                    OverflowElement::TableCell,
                                    OverflowAxis::Horizontal,
                                    page_edge_severity(bounds, page),
                                    bounds,
                                ));
                            }
                            if vertical_outside(bounds, **cell_bounds) {
                                overflows.push(diagnostic(
                                    page_index,
                                    OverflowElement::TableCell,
                                    OverflowAxis::Vertical,
                                    page_edge_severity(bounds, page),
                                    bounds,
                                ));
                            }
                        }
                    }
                }
                Placed::Cell { rect, table, story, .. } => {
                    let floating = doc.table(*story, table).is_some_and(|t| t.props.float.is_some());
                    let area = if floating || *story != StoryRef::Body { Rect::new(0.0, 0.0, page.w, page.h) } else { page.body };
                    if horizontal_outside(*rect, area) {
                        let severity = if floating { OverflowSeverity::Clipped } else { page_edge_severity(*rect, page) };
                        overflows.push(diagnostic(page_index, OverflowElement::Table, OverflowAxis::Horizontal, severity, *rect));
                    }
                    if vertical_outside(*rect, area) {
                        let severity = if floating { OverflowSeverity::Clipped } else { page_edge_severity(*rect, page) };
                        overflows.push(diagnostic(page_index, OverflowElement::Table, OverflowAxis::Vertical, severity, *rect));
                    }
                }
                Placed::Object { rect, spin, story, path, off, text_box, wrap, .. } => {
                    let bounds = spin.bounds(*rect);
                    let object = doc.para(*story, path).and_then(|p| p.object_at(*off));
                    let element = if text_box.is_some() {
                        OverflowElement::TextBox
                    } else if matches!(object, Some(InlineObject::Image { .. })) {
                        OverflowElement::Image
                    } else {
                        OverflowElement::FloatingObject
                    };
                    // A page- or margin-positioned floating object may intentionally cross the
                    // text margins (including a full-bleed image). Warn only when it crosses the
                    // physical page edge. Inline body objects belong to the text flow and use body
                    // bounds; header/footer stories have their own placement outside body bounds.
                    let area = if *story == StoryRef::Body && *wrap == Wrap::Inline { page.body } else { Rect::new(0.0, 0.0, page.w, page.h) };
                    if horizontal_outside(bounds, area) {
                        let severity = if *wrap == Wrap::Inline { page_edge_severity(bounds, page) } else { OverflowSeverity::Clipped };
                        overflows.push(diagnostic(page_index, element, OverflowAxis::Horizontal, severity, bounds));
                    }
                    if vertical_outside(bounds, area) {
                        let severity = if *wrap == Wrap::Inline { page_edge_severity(bounds, page) } else { OverflowSeverity::Clipped };
                        overflows.push(diagnostic(page_index, element, OverflowAxis::Vertical, severity, bounds));
                    }
                    if text_box.is_some() {
                        for (_, _, _, axis) in box_clips.iter().filter(|(s, p, o, _)| s == story && p == path && *o == *off) {
                            let severity = if *axis == OverflowAxis::Vertical { OverflowSeverity::Clipped } else { page_edge_severity(bounds, page) };
                            overflows.push(diagnostic(page_index, OverflowElement::TextBox, *axis, severity, bounds));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    overflows.sort_by(|a, b| {
        a.page
            .cmp(&b.page)
            .then((a.element as u8).cmp(&(b.element as u8)))
            .then((a.axis as u8).cmp(&(b.axis as u8)))
            .then((a.severity as u8).cmp(&(b.severity as u8)))
            .then_with(|| a.bounds.x.total_cmp(&b.bounds.x))
            .then_with(|| a.bounds.y.total_cmp(&b.bounds.y))
    });
    overflows.dedup_by(|a, b| a.page == b.page && a.element == b.element && a.axis == b.axis && a.severity == b.severity && a.bounds == b.bounds);
    overflows
}

impl NoteBox {
    /// Lines of the note's own paragraphs, which may be split between pages (not those of its
    /// tables or text boxes).
    fn splittable(&self, it: &Placed) -> bool {
        matches!(it, Placed::Lines { story: StoryRef::Part(s), path, turn: TextDirection::Horizontal, .. } if *s == self.id && path.0.len() == 1)
    }

    /// Where the note may be split, ascending: the bottoms of its lines that have more lines
    /// below them and don't cut through a picture, table or other unsplittable item.
    fn cuts(&self) -> Vec<f32> {
        let blocked: Vec<(f32, f32)> =
            self.items.iter().filter(|it| !self.splittable(it) && !matches!(it, Placed::Fill { .. } | Placed::Rule { .. })).map(v_span).collect();
        let mut last_top = f32::MIN;
        let mut cuts = Vec::new();
        for it in &self.items {
            let Placed::Lines { para, l0, l1, y, .. } = it else { continue };
            if !self.splittable(it) {
                continue;
            }
            let Some(first) = para.lines.get(*l0) else { continue };
            for l in para.lines.get(*l0..*l1).unwrap_or(&[]) {
                last_top = last_top.max(y + l.top - first.top);
                cuts.push(y + l.top - first.top + l.height);
            }
        }
        cuts.retain(|&b| b > 0.01 && b <= last_top + 0.01 && !blocked.iter().any(|&(t, e)| t < b - 0.01 && e > b + 0.01));
        cuts.sort_by(f32::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        cuts
    }

    /// The least of the note that must stay with its reference: its first line (all of it if it
    /// can't be split).
    fn first_piece(&self) -> f32 {
        self.cuts().first().copied().unwrap_or(self.h).min(self.h)
    }

    /// Split the note at line bottom `cut`: the part above (`cut` tall) and the rest, moved up to
    /// start at 0 and marked as a continuation.
    fn split(self, cut: f32) -> (NoteBox, NoteBox) {
        let (mut head, mut tail) = (Vec::new(), Vec::new());
        for it in self.items {
            let (top, bottom) = v_span(&it);
            if bottom <= cut + 0.01 {
                head.push(it);
                continue;
            }
            if top >= cut - 0.01 {
                tail.push(it);
                continue;
            }
            match it {
                Placed::Lines { story, path, para, l0, l1, x, y, turn } => {
                    let Some(first) = para.lines.get(l0) else { continue };
                    let ft = first.top;
                    // The first line that ends below the cut starts the tail.
                    let k = (l0..l1).find(|&k| para.lines.get(k).is_none_or(|l| y + l.top - ft + l.height > cut + 0.01)).unwrap_or(l1);
                    let ky = para.lines.get(k).map_or(y, |l| y + l.top - ft);
                    if k > l0 {
                        head.push(Placed::Lines { story, path: path.clone(), para: para.clone(), l0, l1: k, x, y, turn });
                    }
                    if k < l1 {
                        tail.push(Placed::Lines { story, path, para, l0: k, l1, x, y: ky, turn });
                    }
                }
                Placed::Fill { rect, color } => {
                    head.push(Placed::Fill { rect: Rect::new(rect.x, rect.y, rect.w, cut - rect.y), color });
                    tail.push(Placed::Fill { rect: Rect::new(rect.x, cut, rect.w, rect.y + rect.h - cut), color });
                }
                Placed::Rule { x0, y0, x1, y1, border } => {
                    head.push(Placed::Rule { x0, y0: y0.min(cut), x1, y1: y1.min(cut), border });
                    tail.push(Placed::Rule { x0, y0: y0.max(cut), x1, y1: y1.max(cut), border });
                }
                // Not split (cuts avoid them): stays above.
                other => head.push(other),
            }
        }
        // The rest starts at its first line (the space between paragraphs goes with the break).
        let start = tail.iter().filter(|it| matches!(it, Placed::Lines { .. })).map(|it| v_span(it).0).fold(f32::MAX, f32::min);
        let start = if start == f32::MAX { cut } else { start.max(cut) };
        for it in &mut tail {
            it.translate(0.0, -start);
            // Shading cut at the break starts at the rest's top.
            if let Placed::Fill { rect, .. } = it
                && rect.y < 0.0
            {
                rect.h = (rect.h + rect.y).max(0.0);
                rect.y = 0.0;
            }
        }
        let rest = (self.h - start).max(0.0);
        (NoteBox { id: self.id, items: head, h: cut, cont: self.cont }, NoteBox { id: self.id, items: tail, h: rest, cont: true })
    }
}

impl PageBuilder<'_> {
    /// Space footnotes take at the bottom of the page.
    fn notes_h(&self) -> f32 {
        match self.notes.first() {
            None => 0.0,
            Some(first) => self.seps.sep_h(first.cont) + self.notes.iter().map(|n| n.h).sum::<f32>(),
        }
    }
    /// The lowest bottom of the body text and tables on the current page.
    fn body_floor(&self) -> f32 {
        let Some(pg) = self.pages.last() else { return self.top };
        pg.items
            .iter()
            .skip(self.page_items_start)
            .filter(|it| matches!(it, Placed::Lines { story: StoryRef::Body, .. } | Placed::Cell { story: StoryRef::Body, .. }))
            .map(|it| v_span(it).1)
            .fold(self.top, f32::max)
    }
    fn finish_page(&mut self) {
        if self.pages.is_empty() || self.page_finished {
            return;
        }
        self.apply_valign();
        let note_h = self.flush_notes();
        self.page_note_heights.push(note_h);
        self.page_finished = true;
    }
    /// Place the collected footnotes at the bottom of the current page. When they don't all fit
    /// below the body (`bottom`), the note that doesn't is split at a line and its rest, with
    /// the notes after it, continues on the next page.
    fn flush_notes(&mut self) -> f32 {
        if self.notes.is_empty() {
            return 0.0;
        }
        let total = self.notes_h();
        let x = self.sect.margin_left + self.sect.gutter;
        // The note area starts below the text (a page of continued notes alone: at its top).
        let area = self.bottom.max(self.body_floor());
        let fits = total <= self.orig_bottom - area + 0.01;
        let top = match (fits, self.beneath) {
            (false, _) => area,
            // Below the page's last line of text.
            (true, true) => self.body_floor().min(self.orig_bottom - total),
            (true, false) => self.orig_bottom - total,
        };
        let notes = std::mem::take(&mut self.notes);
        // A page that starts with a continued note gets the continuation separator.
        let cont = notes.first().is_some_and(|n| n.cont);
        let mut y = top + self.seps.sep_h(cont);
        let mut placed = Vec::new();
        // Where the continuation notice goes, below a note that continues on the next page.
        let mut notice_y = None;
        let mut rest = notes.into_iter();
        while let Some(n) = rest.next() {
            let room = self.orig_bottom - y;
            if fits || n.h <= room + 0.01 {
                let ny = y;
                y += n.h;
                placed.push((n, ny));
                continue;
            }
            // Split at the last line that fits with the notice below it. The page's first note
            // always leaves something here (its first line, or all of it if it can't be split),
            // so continuing ends.
            let first = placed.is_empty();
            let cuts = n.cuts();
            let room = room - self.seps.notice_h();
            match cuts.iter().rev().find(|&&c| c <= room + 0.01).or(cuts.first().filter(|_| first)).copied() {
                Some(c) => {
                    let (head, tail) = n.split(c);
                    notice_y = Some(y + head.h);
                    placed.push((head, y));
                    self.carry.push(tail);
                }
                None if first => placed.push((n, y)),
                None => self.carry.push(n),
            }
            self.carry.extend(rest.by_ref());
            break;
        }
        let placed_h = placed.iter().map(|(n, _)| n.h).sum::<f32>();
        if let Some(pg) = self.pages.last_mut() {
            match if cont { &self.seps.cont } else { &self.seps.sep } {
                Some((items, _)) => pg.items.extend(items.iter().cloned().map(|mut it| {
                    it.translate(x, top);
                    it
                })),
                None => {
                    let len = if cont { self.sect.text_width() } else { NOTE_SEP_W };
                    let sy = top + NOTE_SEP / 2.0;
                    pg.items.push(Placed::Rule { x0: x, y0: sy, x1: x + len, y1: sy, border: Border::single(0.5) });
                }
            }
            for (n, ny) in placed {
                for mut it in n.items {
                    it.translate(x, ny);
                    pg.items.push(it);
                }
            }
            if let (Some(ny), Some((items, _))) = (notice_y, &self.seps.notice) {
                pg.items.extend(items.iter().cloned().map(|mut it| {
                    it.translate(x, ny);
                    it
                }));
            }
        }
        // Reserve from the footnote separator to the bottom of the usable body area. If the
        // notes could not fit below the current body, the next pagination pass will move body
        // lines above this separator before notes are placed again.
        (self.orig_bottom - top).max(placed_h + NOTE_SEP).min((self.orig_bottom - self.top).max(0.0))
    }
    /// Section vertical alignment: move the page's body down (centre/bottom).
    fn apply_valign(&mut self) {
        let k = match self.sect.valign {
            wordcraft_doc::props::VAlign::Top => return,
            wordcraft_doc::props::VAlign::Center => 0.5,
            wordcraft_doc::props::VAlign::Bottom => 1.0,
        };
        if self.web {
            return;
        }
        let space = (self.orig_bottom - self.notes_h().max(self.reserved_note_h) - self.y).max(0.0) * k;
        let start = self.page_items_start;
        if let Some(pg) = self.pages.last_mut() {
            for it in pg.items.iter_mut().skip(start) {
                it.translate(0.0, space);
            }
        }
    }
    fn new_page(&mut self, first_block: usize, body_top: f32) {
        self.finish_page();
        self.beneath = self.sect_beneath;
        self.excl.clear();
        self.float_tables.clear();
        if self.sect.line_numbers.as_ref().is_some_and(|l| l.restart == wordcraft_doc::section::LineNumberRestart::Page) {
            self.line_no = 0;
        }
        let s = self.sect;
        self.number = self.number.saturating_add(1);
        let (w, h) = if self.web { (s.page_w, f32::MAX / 4.0) } else { (s.page_w, s.page_h) };
        let body_bottom = if self.web { s.page_h - s.margin_bottom } else { self.clearance_bottom };
        let body = Rect::new(s.margin_left + s.gutter, body_top, s.text_width(), (body_bottom - body_top).max(0.0));
        let mut decor = Vec::new();
        if !self.web {
            // Page borders, measured from the page edge (Word's default), so they surround the
            // header, footer and line numbers.
            if let Some(b) = &s.page_borders {
                let sp = |e: &Option<Border>| e.map(|x| x.space).unwrap_or(24.0);
                let (x0, x1, y0, y1) = (sp(&b.left), s.page_w - sp(&b.right), sp(&b.top), s.page_h - sp(&b.bottom));
                for (e, a, bb) in
                    [(b.top, (x0, y0), (x1, y0)), (b.bottom, (x0, y1), (x1, y1)), (b.left, (x0, y0), (x0, y1)), (b.right, (x1, y0), (x1, y1))]
                {
                    if let Some(e) = e.filter(Border::is_visible) {
                        decor.push(Placed::Rule { x0: a.0, y0: a.1, x1: bb.0, y1: bb.1, border: e });
                    }
                }
            }
            // Lines between columns.
            if s.columns.separator && s.columns.count > 1 {
                let cols = s.column_boxes();
                for w in cols.windows(2) {
                    if let [(ax, aw), (bx, bw)] = w {
                        // Midway across the space between neighbours (leftwards in an RTL section).
                        let mid = if bx >= ax { (ax + aw + bx) / 2.0 } else { (bx + bw + ax) / 2.0 };
                        let x = s.margin_left + s.gutter + mid;
                        decor.push(Placed::Rule { x0: x, y0: body_top, x1: x, y1: s.page_h - s.margin_bottom, border: Border::single(0.5) });
                    }
                }
            }
        }
        self.page_items_start = decor.len();
        self.pages.push(Page { w, h, section: self.sect_idx, number: self.number, body, first_block, items: decor, ..Default::default() });
        self.page_finished = false;
        self.behind.push(0);
        self.col = 0;
        self.cols = s.column_boxes();
        self.top = body_top;
        self.reserved_note_h = self.reserved_note_heights.get(self.pages.len() - 1).copied().unwrap_or(0.0);
        self.reserved_note_h = self.reserved_note_h.min((body_bottom - body_top).max(0.0));
        self.orig_bottom = if self.web { f32::MAX / 8.0 } else { body_bottom };
        self.bottom = self.orig_bottom - if self.web { 0.0 } else { self.reserved_note_h };
        self.y = self.top;
        // Notes continued from the page before come first in this page's note area.
        // (`cap_columns` below keeps the columns above them.)
        if !self.carry.is_empty() && !self.web {
            self.notes = std::mem::take(&mut self.carry);
            self.bottom = self.bottom.min(self.orig_bottom - self.notes_h());
        }
        self.max_y = self.top;
        self.content_bottom = self.top;
        self.cap_columns();
    }
    /// Where the columns end on this page: the page bottom above its footnotes, or higher up
    /// when the columns are balanced.
    fn cap_columns(&mut self) {
        self.bottom = self.uncapped_bottom();
        if let Some((page, h)) = self.balance
            && page + 1 == self.pages.len()
        {
            self.bottom = self.bottom.min(self.top + h);
        }
    }
    /// The bottom of the columns without balancing: above the footnotes.
    fn uncapped_bottom(&self) -> f32 {
        self.orig_bottom - self.notes_h()
    }
    /// Below the text on this page: the tallest column's bottom.
    fn text_bottom(&self) -> f32 {
        self.max_y.max(self.y)
    }
    fn col_x(&self) -> f32 {
        self.sect.margin_left + self.sect.gutter + self.cols.get(self.col).map(|c| c.0).unwrap_or(0.0)
    }
    fn col_w(&self) -> f32 {
        self.cols.get(self.col).map(|c| c.1).unwrap_or_else(|| self.sect.text_width())
    }
    fn at_top(&self) -> bool {
        self.y <= self.top + 0.01
    }
    /// Move to the next column or page.
    fn advance(&mut self, block: usize, body_top: f32) {
        if self.col + 1 < self.cols.len() {
            self.max_y = self.max_y.max(self.y);
            self.col += 1;
            self.y = self.top;
        } else {
            self.new_page(block, body_top);
        }
    }
    fn page(&mut self) -> Option<&mut Page> {
        self.pages.last_mut()
    }
}

/// Place document-end notes in normal text flow, continuing their paragraph lines onto later
/// columns or pages. Non-text objects stay indivisible; if one cannot fit in the remaining space,
/// it moves whole to the next column/page (or overflows an otherwise empty page to guarantee
/// progress).
fn place_endnotes(pb: &mut PageBuilder<'_>, ctx: &mut Ctx<'_>, doc: &Document, ids: &[u32], block: usize) {
    let mut first = true;
    for &id in ids {
        let Some(part) = doc.parts.get(&id) else { continue };
        let BoxLayout { items, height, .. } = layout_box(ctx, StoryRef::Part(id), &part.blocks, &[], pb.col_w(), None, 0, None);
        let mut note = NoteBox { id, items, h: height, cont: false };
        let mut page_steps = 0usize;
        loop {
            if first {
                if pb.y + 20.0 + note.first_piece() > pb.bottom && !pb.at_top() {
                    pb.advance(block, pb.top);
                }
                pb.y += 12.0;
                let (x, y) = (pb.col_x(), pb.y);
                if let Some(page) = pb.page() {
                    page.items.push(Placed::Rule { x0: x, y0: y, x1: x + 144.0, y1: y, border: Border::single(0.5) });
                }
                pb.y += 8.0;
                first = false;
            } else if note.cont {
                if pb.y + NOTE_SEP > pb.bottom && !pb.at_top() {
                    pb.advance(block, pb.top);
                }
                let (x, y, width) = (pb.col_x(), pb.y, pb.col_w());
                if let Some(page) = pb.page() {
                    page.items.push(Placed::Rule {
                        x0: x,
                        y0: y + NOTE_SEP / 2.0,
                        x1: x + width,
                        y1: y + NOTE_SEP / 2.0,
                        border: Border::single(0.5),
                    });
                }
                pb.y += NOTE_SEP;
            }

            let room = (pb.bottom - pb.y).max(0.0);
            if note.h <= room + 0.01 {
                append_note_fragment(pb, note);
                break;
            }
            let cut = note.cuts().into_iter().rev().find(|cut| *cut <= room + 0.01);
            if let Some(cut) = cut {
                let (head, tail) = note.split(cut);
                append_note_fragment(pb, head);
                note = tail;
                pb.advance(block, pb.top);
                page_steps += 1;
            } else if !pb.at_top() {
                pb.advance(block, pb.top);
                page_steps += 1;
            } else {
                let cut = note.cuts().first().copied().unwrap_or(note.h);
                if cut < note.h - 0.01 {
                    let (head, tail) = note.split(cut);
                    append_note_fragment(pb, head);
                    note = tail;
                    pb.advance(block, pb.top);
                    page_steps += 1;
                } else {
                    // An indivisible object or oversized first line is emitted whole. This can
                    // extend past the body rectangle, but cannot clip or discard source content.
                    append_note_fragment(pb, note);
                    break;
                }
            }
            // Splitting consumes at least one line. If hostile input exceeds the normal page
            // bound, emit any remainder intact rather than silently losing it.
            if page_steps >= MAX_NOTE_PAGES {
                append_note_fragment(pb, note);
                break;
            }
        }
    }
}

fn append_note_fragment(pb: &mut PageBuilder<'_>, note: NoteBox) {
    let (x, y) = (pb.col_x(), pb.y);
    if let Some(page) = pb.page() {
        for mut item in note.items {
            item.translate(x, y);
            page.items.push(item);
        }
    }
    pb.y += note.h;
}

/// Reflow a simple final column page into a more even sequence of complete paragraphs. Complex
/// content deliberately keeps the existing pagination path: moving table rows, anchored objects,
/// note anchors or partial paragraphs needs a full source-level repagination pass.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BalanceOutcome {
    Balanced,
    NotNeeded,
    Skipped,
}

fn balance_final_columns(pb: &mut PageBuilder<'_>, doc: &Document, start_block: usize, end_block: usize, quiet_marks: &[usize]) -> BalanceOutcome {
    if pb.cols.len() < 2 {
        return BalanceOutcome::NotNeeded;
    }
    if !pb.notes.is_empty() || !pb.carry.is_empty() || pb.reserved_note_h > 0.01 {
        return BalanceOutcome::Skipped;
    }
    let Some(page) = pb.pages.last_mut() else { return BalanceOutcome::NotNeeded };
    let mut paragraphs: Vec<(usize, Placed, f32)> = Vec::new();
    let mut seen = HashSet::new();
    let separator_x: Vec<f32> = pb
        .cols
        .windows(2)
        .filter_map(|pair| match pair {
            [(left, width), (right, _)] => Some(pb.sect.margin_left + pb.sect.gutter + (left + width + right) / 2.0),
            _ => None,
        })
        .collect();
    for (index, item) in page.items.iter().enumerate() {
        match item {
            Placed::Lines { story: StoryRef::Body, path, para, l0, l1, .. } => {
                let Some(block) = path.0.first().map(|value| *value as usize) else { return BalanceOutcome::Skipped };
                // A continuous section can share its physical page with the section before it.
                // Those already-laid paragraphs are outside the balancing range and stay fixed.
                if block < start_block || block > end_block {
                    continue;
                }
                if path.0.len() != 1
                    || *l0 != 0
                    || *l1 != para.lines.len()
                    || !seen.insert(path.clone())
                    || para.rp.keep_next
                    || para.rp.contextual_spacing
                    || para.rp.page_break_before
                    || para.has_page_fields
                    || !para.notes.is_empty()
                    || para.label.is_some()
                    || para.drop_cap.is_some()
                    || para.lines.iter().any(|line| matches!(line.end, LineEnd::ColumnBreak | LineEnd::PageBreak))
                {
                    return BalanceOutcome::Skipped;
                }
                // Empty continuous-section paragraph marks do not occupy a line. They may be
                // present at the end of the section being balanced; leave them out of the flow.
                if quiet_marks.binary_search(&block).is_ok() {
                    continue;
                }
                let Some(model) = doc.para(StoryRef::Body, path) else { return BalanceOutcome::Skipped };
                if !model.objects.is_empty() {
                    return BalanceOutcome::Skipped;
                }
                let height = para.rp.space_before + para.height + para.rp.space_after;
                if !height.is_finite() || height < 0.0 {
                    return BalanceOutcome::Skipped;
                }
                paragraphs.push((index, item.clone(), height));
            }
            Placed::Rule { x0, x1, .. } if (*x0 - *x1).abs() < 0.01 && separator_x.iter().any(|x| (*x - *x0).abs() < 0.5) => {}
            _ => return BalanceOutcome::Skipped,
        }
    }
    if paragraphs.len() < pb.cols.len() {
        return BalanceOutcome::NotNeeded;
    }

    // All lines are kept intact, but an unequal-width destination must still accommodate every
    // already-shaped line. Otherwise source-level rebreaking is required and this page falls back.
    for (_, item, _) in &paragraphs {
        let Placed::Lines { para, .. } = item else { return BalanceOutcome::Skipped };
        let min_x = para.lines.iter().flat_map(|line| line.xs.iter().copied()).fold(f32::MAX, f32::min);
        let max_x = para.lines.iter().flat_map(|line| line.xs.iter().copied()).fold(f32::MIN, f32::max);
        if min_x < -0.5 || max_x > pb.cols.iter().map(|(_, width)| *width).fold(0.0f32, f32::max) + 0.5 {
            return BalanceOutcome::Skipped;
        }
    }

    let total: f32 = paragraphs.iter().map(|(_, _, height)| *height).sum();
    let mut assignment = vec![0usize; paragraphs.len()];
    let (mut unit, mut remaining_height) = (0usize, total);
    for col in 0..pb.cols.len() {
        let columns_left = pb.cols.len() - col;
        let units_left = paragraphs.len() - unit;
        if units_left < columns_left {
            return BalanceOutcome::Skipped;
        }
        let target = remaining_height / columns_left as f32;
        let first = unit;
        let mut used = 0.0;
        while unit < paragraphs.len() - (columns_left - 1) {
            let height = paragraphs.get(unit).map(|entry| entry.2).unwrap_or(0.0);
            if unit > first && used + height > target && (used - target).abs() <= (used + height - target).abs() {
                break;
            }
            assignment[unit] = col;
            used += height;
            unit += 1;
            if used >= target && col + 1 < pb.cols.len() {
                break;
            }
        }
        remaining_height = (remaining_height - used).max(0.0);
    }
    if unit != paragraphs.len() {
        return BalanceOutcome::Skipped;
    }

    let mut cursors = vec![pb.top; pb.cols.len()];
    let mut moved = Vec::with_capacity(paragraphs.len());
    for (unit, (_, original, _)) in paragraphs.iter().enumerate() {
        let mut item = original.clone();
        let Some(&col) = assignment.get(unit) else { return BalanceOutcome::Skipped };
        let Some((offset, width)) = pb.cols.get(col) else { return BalanceOutcome::Skipped };
        let Placed::Lines { para, x, y, .. } = &mut item else { return BalanceOutcome::Skipped };
        let min_x = para.lines.iter().flat_map(|line| line.xs.iter().copied()).fold(f32::MAX, f32::min);
        let max_x = para.lines.iter().flat_map(|line| line.xs.iter().copied()).fold(f32::MIN, f32::max);
        if min_x < -0.5 || max_x > *width + 0.5 {
            return BalanceOutcome::Skipped;
        }
        let before = para.rp.space_before;
        let placement_y = cursors[col] + before;
        let placement_x = pb.sect.margin_left + pb.sect.gutter + offset;
        let bottom = placement_y + para.height;
        if !bottom.is_finite() || bottom > pb.bottom + 0.5 {
            return BalanceOutcome::Skipped;
        }
        *x = placement_x;
        *y = placement_y;
        cursors[col] = bottom + para.rp.space_after;
        moved.push(item);
    }

    let ids: HashSet<usize> = paragraphs.iter().map(|(index, _, _)| *index).collect();
    page.items = page.items.drain(..).enumerate().filter_map(|(i, item)| (!ids.contains(&i)).then_some(item)).collect();
    page.items.extend(moved);
    pb.col = cursors.iter().enumerate().max_by(|(_, a), (_, b)| a.total_cmp(b)).map(|(i, _)| i).unwrap_or(0);
    pb.y = cursors.get(pb.col).copied().unwrap_or(pb.top);
    pb.prev = None;
    BalanceOutcome::Balanced
}

/// Lay out the whole document.
pub fn layout(doc: &Document, cache: &mut LayoutCache, opts: &LayoutOptions) -> DocLayout {
    if opts.view == ViewMode::Outline {
        return layout_outline(doc, cache, opts);
    }
    let t0 = now_ms();
    let per_page = opts.view == ViewMode::Print
        && doc.sections().iter().any(|(_, section)| doc.note_options(section, false).num_restart == wordcraft_doc::section::NoteRestart::EachPage);
    let mut note_pages = HashMap::new();
    let mut result = DocLayout::default();
    let mut totals = Totals::first_guess(doc.sections().len());
    let needs_totals = doc.para_paths(StoryRef::Body).iter().filter_map(|path| doc.para(StoryRef::Body, path)).any(has_total_fields);
    for _ in 0..if per_page || needs_totals { 4 } else { 1 } {
        result = layout_with_note_pages(doc, cache, opts, &note_pages, &totals);
        let placed = footnote_pages(doc, &result.pages);
        let found = Totals::of(&result.pages, doc.sections().len());
        let note_stable = !per_page || placed == note_pages;
        let totals_stable = !needs_totals || found == totals;
        if note_stable && totals_stable {
            break;
        }
        if per_page {
            note_pages = placed;
        }
        if needs_totals {
            totals = found;
        }
        if !per_page && !needs_totals {
            break;
        }
    }
    result.ms = now_ms() - t0;
    result
}

fn layout_outline(doc: &Document, cache: &mut LayoutCache, opts: &LayoutOptions) -> DocLayout {
    let view = opts.outline.clone().unwrap_or_default();
    let levels = outline::levels(doc);
    let hidden = outline::hidden(&levels, &view);
    let mut projected = doc.clone();
    let mut source_blocks = Vec::new();
    let mut blocks = Vec::new();
    for (index, block) in doc.body.iter().enumerate() {
        if hidden.get(index).copied().unwrap_or(false) {
            continue;
        }
        let replacement = match &**block {
            Block::Para(p) => {
                let mut p = outline::outline_para(doc, p, view.plain);
                p.props.indent_left = Some(outline::indent(&levels, index));
                Arc::new(Block::Para(p))
            }
            _ => block.clone(),
        };
        source_blocks.push(index as u32);
        blocks.push(replacement);
    }
    projected.body = blocks;
    let mut draft = opts.clone();
    draft.view = ViewMode::Draft;
    draft.outline = None;
    let mut result = layout(&projected, cache, &draft);
    let projected_levels = source_blocks.iter().map(|i| levels.get(*i as usize).copied().unwrap_or(outline::BODY)).collect::<Vec<_>>();
    for page in &mut result.pages {
        let mut symbols = Vec::new();
        page.items.retain_mut(|item| {
            let Placed::Lines { story: StoryRef::Body, path, para, l0, x, y, .. } = item else { return true };
            let Some(&compact) = path.0.first() else { return true };
            let Some(&source) = source_blocks.get(compact as usize) else { return false };
            path.0[0] = source;
            if path.0.len() == 1 {
                let level = projected_levels.get(compact as usize).copied().unwrap_or(outline::BODY);
                if view.first_line_only && level == outline::BODY && *l0 > 0 {
                    return false;
                }
                if *l0 == 0 {
                    symbols.extend(outline::symbol_items(
                        para,
                        *x,
                        *y,
                        level,
                        outline::has_children(&levels, source as usize),
                        view.collapsed.contains(&(source as usize)),
                    ));
                }
            }
            true
        });
        page.items.extend(symbols);
    }
    result.index.clear();
    for (pi, page) in result.pages.iter().enumerate() {
        for (ii, item) in page.items.iter().enumerate() {
            if let Placed::Lines { story, path, .. } = item {
                result.index.entry((*story, path.clone())).or_default().push((pi, ii));
            }
        }
    }
    result
}

fn layout_with_note_pages(
    doc: &Document,
    cache: &mut LayoutCache,
    opts: &LayoutOptions,
    note_pages: &HashMap<u32, usize>,
    totals: &Totals,
) -> DocLayout {
    let has_multicolumn_notes = opts.view == ViewMode::Print
        && doc.sections().iter().any(|(_, section)| section.columns.count > 1)
        && doc
            .para_paths(StoryRef::Body)
            .iter()
            .filter_map(|path| doc.para(StoryRef::Body, path))
            .flat_map(|para| &para.objects)
            .any(|object| matches!(object, InlineObject::NoteRef { kind: wordcraft_doc::para::NoteKind::Footnote, .. }));
    if !has_multicolumn_notes {
        let (result, _) = layout_pass(doc, cache, opts, note_pages, &[], totals);
        return result;
    }
    let mut reservations = Vec::new();
    let mut last = None;
    // A note found in a later column can require earlier columns on the same page to reflow.
    // Repeat until the page note areas stop moving. This is bounded for hostile/oscillating
    // pagination; the high-water reservation keeps the bounded fallback from reintroducing
    // overlap if note anchors move between columns or pages while the body is reflowed.
    let mut high_water: Vec<f32> = Vec::new();
    let mut stable = false;
    for _ in 0..24 {
        let (result, actual) = layout_pass(doc, cache, opts, note_pages, &reservations, totals);
        for (i, height) in actual.iter().copied().enumerate() {
            if high_water.len() <= i {
                high_water.resize(i + 1, 0.0);
            }
            high_water[i] = high_water[i].max(height);
        }
        let covered = actual.iter().enumerate().all(|(i, height)| reservations.get(i).is_some_and(|reserved| *height <= *reserved + 0.1));
        if covered && reservations == high_water {
            last = Some(result);
            stable = true;
            break;
        }
        reservations.clone_from(&high_water);
        last = Some(result);
    }
    let mut result = last.unwrap_or_default();
    if !stable {
        // Converge under the accumulated safe reservations if anchors kept moving in the first
        // bounded window. Any newly occupied page adds to the high-water map and gets another pass.
        for _ in 0..8 {
            let current = high_water.clone();
            let (candidate, actual) = layout_pass(doc, cache, opts, note_pages, &current, totals);
            let covered = actual.iter().enumerate().all(|(i, height)| current.get(i).is_some_and(|reserved| *height <= *reserved + 0.1));
            for (i, height) in actual.iter().copied().enumerate() {
                if high_water.len() <= i {
                    high_water.resize(i + 1, 0.0);
                }
                high_water[i] = high_water[i].max(height);
            }
            result = candidate;
            if covered && current == high_water {
                break;
            }
        }
    }
    result
}

fn layout_pass(
    doc: &Document,
    cache: &mut LayoutCache,
    opts: &LayoutOptions,
    note_pages: &HashMap<u32, usize>,
    reserved_note_heights: &[f32],
    totals: &Totals,
) -> (DocLayout, Vec<f32>) {
    let t0 = now_ms();
    let env = env_hash(doc, opts);
    if env != cache.env {
        cache.paras.clear();
        cache.env = env;
    }
    cache.used.clear();
    // Note references are counted as the body shows them.
    let notes = note_info(doc, opts.hides_deleted(StoryRef::Body), note_pages);
    let notes_hash = hash_of(&{
        let mut v: Vec<_> = notes.iter().map(|(a, b)| (*a, b.n, b.fmt)).collect();
        v.sort_by_key(|(a, ..)| *a);
        v
    });
    let mut ctx = Ctx {
        doc,
        cache,
        opts,
        hide_deleted: opts.hides_deleted(StoryRef::Body),
        counters: Counters::default(),
        fields: FieldCtx {
            page: 1,
            pages: totals.pages,
            section_pages: totals.section(0),
            section: 1,
            notes: Arc::new(notes),
            title: doc.core.title.as_str().into(),
            author: doc.core.creator.as_str().into(),
            ..Default::default()
        },
        notes_hash,
        numbers: HashMap::new(),
        eq_count: 0,
        boxes: wordcraft_doc::BoxBudget::default(),
        box_clips: Vec::new(),
        chains: textbox::Chains::default(),
        grid: para::Grid::default(),
        quiet_marks: Vec::new(),
    };
    let sections = doc.sections();
    let web = matches!(opts.view, ViewMode::Web | ViewMode::Draft | ViewMode::Outline);
    let read = opts.view == ViewMode::Read;
    let reflow = web || read;
    ctx.quiet_marks = sections
        .windows(2)
        .filter(|w| w[1].1.start == SectionStart::Continuous)
        .map(|w| w[0].0)
        .filter(|end| doc.body.get(*end).and_then(|b| b.as_para()).is_some_and(|p| p.text.is_empty()))
        .collect();
    let default_sect = SectionProps::default();
    let mut web_sect;
    let first_sect = match sections.first() {
        Some((_, s)) => *s,
        None => &default_sect,
    };
    let sect_ref = if reflow {
        web_sect = first_sect.clone();
        web_sect.page_w = if opts.web_width.is_finite() { opts.web_width.clamp(144.0, 20_000.0) } else { 300.0 };
        if read {
            web_sect.page_h = if opts.read_height.is_finite() { opts.read_height.clamp(144.0, 20_000.0) } else { 500.0 };
            web_sect.margin_bottom = 18.0;
            web_sect.page_borders = None;
            web_sect.line_numbers = None;
        }
        web_sect.margin_left = 18.0;
        web_sect.margin_right = 18.0;
        web_sect.margin_top = 18.0;
        web_sect.gutter = 0.0;
        web_sect.columns = Default::default();
        web_sect.valign = wordcraft_doc::props::VAlign::Top;
        &web_sect
    } else {
        first_sect
    };
    let mut pb = PageBuilder {
        pages: Vec::new(),
        sect: sect_ref,
        sect_idx: 0,
        col: 0,
        cols: Vec::new(),
        y: 0.0,
        top: 0.0,
        bottom: 0.0,
        number: 0,
        web,
        notes: Vec::new(),
        carry: Vec::new(),
        orig_bottom: 0.0,
        reserved_note_h: 0.0,
        page_note_heights: Vec::new(),
        reserved_note_heights,
        clearance_bottom: 0.0,
        prev: None,
        after_break_mark: false,
        excl: Vec::new(),
        float_tables: Vec::new(),
        line_no: 0,
        page_items_start: 0,
        page_finished: true,
        seps: NoteSeps::default(),
        behind: Vec::new(),
        max_y: 0.0,
        content_bottom: 0.0,
        balance: None,
        col_break_page: None,
        sect_beneath: false,
        beneath: false,
    };
    let mut block = 0usize;
    let mut column_balance_skipped = Vec::new();
    let even_odd_headers = ctx.doc.settings.even_odd_headers;
    let turned: Vec<Option<SectionProps>> = sections
        .iter()
        .map(|(si, sect)| {
            if web || !sect.text_direction.is_turned() {
                return None;
            }
            let top = body_top_for(&mut ctx, sect, *si, &sections, false, even_odd_headers);
            Some(vertical::unturned_sect(sect, top))
        })
        .collect();
    let mut previous_turn = TextDirection::Horizontal;
    for (si, (end, sect)) in sections.iter().enumerate() {
        let turn = if reflow { TextDirection::Horizontal } else { sect.text_direction };
        let sect: &SectionProps = match turned.get(si) {
            Some(Some(unturned)) => unturned,
            _ if reflow => sect_ref,
            _ => sect,
        };
        ctx.fields.section = u32::try_from(si).unwrap_or(u32::MAX).saturating_add(1);
        ctx.fields.section_pages = totals.section(si);
        let start = if si == 0 { SectionStart::NextPage } else { sect.start };
        if !web && si > 0 && start == SectionStart::NextColumn && !pb.pages.is_empty() && pb.col + 1 >= pb.cols.len() {
            // Close the outgoing page while its section still owns alignment and note geometry.
            pb.finish_page();
        }
        let mut section_restart = sect.page_num_start;
        if !web && si > 0 && matches!(start, SectionStart::NextPage | SectionStart::EvenPage | SectionStart::OddPage) {
            // Finish the outgoing section while its margins, note area and vertical alignment
            // still describe the page being closed.
            pb.finish_page();
            let (needs_filler, adjusted_restart) =
                section_side(start, section_restart, pb.number, pb.pages.len(), ctx.doc.settings.even_odd_headers, ctx.doc.settings.mirror_margins);
            section_restart = adjusted_restart;
            if needs_filler {
                pb.new_page(block, pb.sect.margin_top);
                pb.finish_page();
            }
        }
        pb.sect = sect;
        pb.sect_idx = si;
        ctx.grid = para::Grid::of(doc, sect);
        pb.sect_beneath = doc.note_options(sect, false).pos == wordcraft_doc::section::NotePos::BeneathText;
        if !web && !doc.footnote_separators.is_empty() {
            pb.seps = NoteSeps::new(&mut ctx, &doc.footnote_separators, sect.text_width());
        }
        let even_odd = ctx.doc.settings.even_odd_headers;
        let body_top = if web || turn.is_turned() { sect.margin_top } else { body_top_for(&mut ctx, sect, si, &sections, false, even_odd) };
        let first_top =
            if web || turn.is_turned() || !sect.title_page { body_top } else { body_top_for(&mut ctx, sect, si, &sections, true, even_odd) };
        pb.clearance_bottom =
            if web || turn.is_turned() { sect.page_h - sect.margin_bottom } else { body_bottom_for(&mut ctx, sect, si, &sections, even_odd) };
        let restart = section_restart;
        if sect.line_numbers.as_ref().is_some_and(|l| l.restart == wordcraft_doc::section::LineNumberRestart::Section) {
            pb.line_no = 0;
        }
        if web && si > 0 {
            // one long page
        } else if pb.pages.is_empty()
            || turn != previous_turn
            || matches!(start, SectionStart::NextPage | SectionStart::EvenPage | SectionStart::OddPage)
        {
            if let Some(n) = restart {
                pb.number = n.saturating_sub(1);
            }
            pb.new_page(block, first_top);
        } else if start == SectionStart::NextColumn {
            // Continue at the next column using the incoming section's column geometry. At the
            // end of the outgoing section's columns, `advance` creates the next physical page.
            let creates_page = pb.pages.is_empty() || pb.col + 1 >= pb.cols.len();
            let next_col = if pb.pages.is_empty() {
                0
            } else if pb.col + 1 < pb.cols.len() {
                pb.col + 1
            } else {
                0
            };
            if creates_page && let Some(n) = restart {
                pb.number = n.saturating_sub(1);
            }
            if !pb.pages.is_empty() {
                pb.advance(block, body_top);
            }
            pb.cols = sect.column_boxes();
            pb.col = next_col.min(pb.cols.len().saturating_sub(1));
            if let Some(page) = pb.pages.last_mut() {
                page.section = si;
                page.w = sect.page_w;
                page.h = sect.page_h;
                page.body = Rect::new(sect.margin_left + sect.gutter, body_top, sect.text_width(), (pb.clearance_bottom - body_top).max(0.0));
            }
            pb.top = body_top;
            pb.orig_bottom = pb.clearance_bottom;
            pb.reserved_note_h =
                pb.reserved_note_heights.get(pb.pages.len().saturating_sub(1)).copied().unwrap_or(0.0).min((pb.orig_bottom - body_top).max(0.0));
            pb.bottom = pb.orig_bottom - pb.reserved_note_h.max(pb.notes_h());
            pb.y = pb.top;
        } else {
            // Continuous: new column layout below the current text (its tallest column).
            pb.cols = sect.column_boxes();
            pb.col = 0;
            pb.top = pb.text_bottom();
            pb.y = pb.top;
            pb.max_y = pb.top;
        }
        previous_turn = turn;
        let last = (*end).min(doc.body.len().saturating_sub(1));
        let section_first_block = block;
        while block <= last {
            let Some(b) = doc.body.get(block) else { break };
            match &**b {
                Block::Para(p) => place_para(&mut ctx, &mut pb, p, block, body_top),
                Block::Table(t) => place_table(&mut ctx, &mut pb, t, block, body_top),
            }
            block += 1;
        }
        let ends_at_column_balance =
            !web && sect.columns.count > 1 && sections.get(si + 1).is_none_or(|(_, next)| next.start == SectionStart::Continuous);
        if ends_at_column_balance
            && balance_final_columns(&mut pb, doc, section_first_block, last, &ctx.quiet_marks) == BalanceOutcome::Skipped
            && !pb.pages.is_empty()
        {
            let page = pb.pages.len() - 1;
            if !column_balance_skipped.contains(&page) {
                column_balance_skipped.push(page);
            }
        }
        if !web && doc.note_options(sect, true).pos == wordcraft_doc::section::NotePos::SectEnd {
            let mut endnotes: Vec<(u32, u32)> = ctx
                .fields
                .notes
                .iter()
                .filter(|(id, note)| note.section == si && doc.parts.get(id).is_some_and(|part| part.kind == wordcraft_doc::PartKind::Endnote))
                .map(|(id, note)| (note.order, *id))
                .collect();
            endnotes.sort_unstable();
            place_endnotes(&mut pb, &mut ctx, doc, &endnotes.into_iter().map(|(_, id)| id).collect::<Vec<_>>(), block);
        }
    }
    if pb.pages.is_empty() {
        pb.new_page(0, sect_ref.margin_top);
    }
    // Endnotes flow after the last body paragraph. Unlike footnotes, they are not anchored to
    // the bottom of a page, but their stories can use the same safe line-boundary splitter.
    let endnotes: Vec<u32> = {
        let mut ids: Vec<(u32, u32)> = ctx
            .fields
            .notes
            .iter()
            .filter(|(id, note)| {
                doc.parts.get(id).is_some_and(|p| p.kind == wordcraft_doc::PartKind::Endnote)
                    && sections
                        .get(note.section)
                        .is_none_or(|(_, section)| doc.note_options(section, true).pos == wordcraft_doc::section::NotePos::DocEnd)
            })
            .map(|(id, note)| (note.order, *id))
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    };
    if !endnotes.is_empty() && !web {
        place_endnotes(&mut pb, &mut ctx, doc, &endnotes, block);
    }
    pb.finish_page();
    // Footnotes still continuing after the last page get pages of their own. Each page takes at
    // least a line of the first note, so this ends; the cap is a backstop.
    let last_block = doc.body.len().saturating_sub(1);
    for _ in 0..MAX_NOTE_PAGES {
        if pb.carry.is_empty() {
            break;
        }
        let top = pb.top;
        pb.new_page(last_block, top);
        pb.finish_page();
    }
    pb.carry.clear();
    let mut pages = pb.pages;
    if web && let Some(p) = pages.first_mut() {
        let bottom = p
            .items
            .iter()
            .filter_map(|it| if let Placed::Lines { y, para, l0, l1, .. } = it { item_bottom(*y, para, *l0, *l1) } else { None })
            .fold(0.0f32, f32::max);
        p.h = bottom + 36.0;
    }
    ctx.grid = para::Grid::default();
    if !web {
        headers_footers(&mut ctx, &mut pages, &sections);
        if doc.settings.mirror_margins {
            mirror_even_pages(&mut pages, &sections);
        }
    }
    for page in &mut pages {
        if let Some((_, physical)) = sections.get(page.section) {
            vertical::turn_page(page, physical.text_direction, physical);
        }
    }
    // Evict paragraph layouts that weren't used this pass if the cache grew large.
    if ctx.cache.paras.len() > 4 * ctx.cache.used.len().max(1024) {
        let used = std::mem::take(&mut ctx.cache.used);
        ctx.cache.paras.retain(|k, _| used.contains(k));
        ctx.cache.used = used;
    }
    let mut index: HashMap<(StoryRef, Path), Vec<(usize, usize)>> = HashMap::new();
    for (pi, p) in pages.iter().enumerate() {
        for (ii, it) in p.items.iter().enumerate() {
            if let Placed::Lines { story, path, .. } = it {
                index.entry((*story, path.clone())).or_default().push((pi, ii));
            }
        }
    }
    let overflows = analyze_overflows(doc, &pages, &ctx.box_clips);
    let mut overflow_pages: Vec<usize> = overflows.iter().filter(|o| o.axis == OverflowAxis::Vertical).map(|o| o.page).collect();
    overflow_pages.sort_unstable();
    overflow_pages.dedup();
    let note_heights = pb.page_note_heights.clone();
    let mut layout = DocLayout { pages, overflow_pages, overflows, column_balance_skipped, index, ms: now_ms() - t0, text_boxes: ctx.boxes.total() };
    if opts.comment_balloons && !matches!(opts.view, ViewMode::Web | ViewMode::Draft | ViewMode::Outline | ViewMode::Read) {
        balloons::add(&mut ctx, &mut layout);
    }
    layout.text_boxes = ctx.boxes.total();
    (layout, note_heights)
}

fn item_bottom(y: f32, para: &ParaLayout, l0: usize, l1: usize) -> Option<f32> {
    let f = para.lines.get(l0)?;
    let l = para.lines.get(l1.checked_sub(1)?)?;
    Some(y + l.top + l.height - f.top)
}

/// Whether a page-starting section needs a physical filler page to land on the requested side.
/// Numbering restarts normally decouple page numbers from physical sides; an odd/even header set
/// or mirrored margins makes the side visible again.
fn section_side(start: SectionStart, restart: Option<u32>, last: u32, pages: usize, even_odd: bool, mirror: bool) -> (bool, Option<u32>) {
    match start {
        SectionStart::OddPage | SectionStart::EvenPage => {
            let want_odd = start == SectionStart::OddPage;
            let next_is_odd = last.saturating_add(1) % 2 == 1;
            let filler = pages > 0 && next_is_odd != want_odd && (restart.is_none() || even_odd || mirror);
            let restart = restart.map(|n| if (n % 2 == 1) == want_odd { n } else { n.saturating_add(1) });
            (filler, restart)
        }
        SectionStart::NextPage => {
            let filler = even_odd && pages > 0 && restart.is_some_and(|n| (n % 2 == 1) != ((pages + 1) % 2 == 1));
            (filler, restart)
        }
        SectionStart::Continuous | SectionStart::NextColumn => (false, restart),
    }
}

/// Where body text starts: the top margin, or below the header if it's taller.
fn linked_story_ids(sections: &[(usize, &SectionProps)], section: usize, headers: bool) -> wordcraft_doc::section::HeaderSet {
    let get = |s: &SectionProps| if headers { s.headers } else { s.footers };
    let Some((_, current)) = sections.get(section) else { return Default::default() };
    let mut set = get(current);
    for k in (0..section).rev() {
        if set.default.is_some() && set.first.is_some() && set.even.is_some() {
            break;
        }
        if let Some((_, previous)) = sections.get(k) {
            let old = get(previous);
            set.default = set.default.or(old.default);
            set.first = set.first.or(old.first);
            set.even = set.even.or(old.even);
        }
    }
    set
}

fn story_height(ctx: &mut Ctx, sect: &SectionProps, id: Option<u32>, y: f32) -> f32 {
    let Some(id) = id else { return 0.0 };
    let Some(part) = ctx.doc.parts.get(&id) else { return 0.0 };
    let blocks = part.blocks.clone();
    let frame = PageFrame { sect, origin: (sect.margin_left + sect.gutter, y) };
    layout_box(ctx, StoryRef::Part(id), &blocks, &[], sect.text_width(), None, 0, Some(frame)).height
}

/// Reserve the tallest header variant, including variants inherited through link-to-previous.
fn body_top_for(ctx: &mut Ctx, sect: &SectionProps, section: usize, sections: &[(usize, &SectionProps)], first_page: bool, even_odd: bool) -> f32 {
    let ids = linked_story_ids(sections, section, true);
    let h = [ids.default, ids.even.filter(|_| even_odd), ids.first.filter(|_| first_page && sect.title_page)]
        .into_iter()
        .map(|id| story_height(ctx, sect, id, sect.header))
        .fold(0.0, f32::max);
    sect.margin_top.max(sect.header + h)
}

/// Reserve the tallest footer variant above the page's bottom margin.
fn body_bottom_for(ctx: &mut Ctx, sect: &SectionProps, section: usize, sections: &[(usize, &SectionProps)], even_odd: bool) -> f32 {
    let ids = linked_story_ids(sections, section, false);
    let h = [ids.default, ids.first.filter(|_| sect.title_page), ids.even.filter(|_| even_odd)]
        .into_iter()
        .map(|id| story_height(ctx, sect, id, sect.page_h - sect.footer))
        .fold(0.0, f32::max);
    (sect.page_h - sect.margin_bottom).min(sect.page_h - sect.footer - h)
}

/// Where a box of laid-out items sits on its page, for floating objects positioned relative to
/// the page or its margins.
#[derive(Clone, Copy)]
struct PageFrame<'a> {
    sect: &'a SectionProps,
    /// Page position of the box's (0, 0); (0, 0) for body text.
    origin: (f32, f32),
}

/// A floating (not inline) picture, chart or shape: its size and placement.
fn floating(o: &InlineObject) -> Option<(f32, f32, &Float)> {
    o.frame().filter(|(_, _, float)| float.wrap != Wrap::Inline)
}

/// Where a floating object goes, in the coordinates of the box `frame` places on the page.
/// `col` is the column (or cell) left edge and width, `y0` the anchor paragraph's top. Without a
/// frame (table cells, text boxes, notes) positions relative to the page or margins fall back
/// to the box itself.
fn float_rect(frame: Option<PageFrame>, col: (f32, f32), y0: f32, w: f32, h: f32, float: &Float) -> Rect {
    let (w, h) = (w.clamp(1.0, 4000.0), h.clamp(1.0, 4000.0));
    // Each axis: the reference area's start and extent, in page coordinates.
    let page_x = |s: &SectionProps| {
        let left = s.margin_left + s.gutter;
        match float.h_rel {
            Anchor::Page => Some((0.0, s.page_w)),
            Anchor::Margin => Some((left, s.text_width())),
            Anchor::LeftMargin | Anchor::InsideMargin => Some((0.0, left)),
            Anchor::RightMargin | Anchor::OutsideMargin => Some((s.page_w - s.margin_right, s.margin_right)),
            _ => None,
        }
    };
    let page_y = |s: &SectionProps| match float.v_rel {
        Anchor::Page => Some((0.0, s.page_h)),
        Anchor::Margin => Some((s.margin_top, s.page_h - s.margin_top - s.margin_bottom)),
        Anchor::TopMargin | Anchor::InsideMargin => Some((0.0, s.margin_top)),
        Anchor::BottomMargin | Anchor::OutsideMargin => Some((s.page_h - s.margin_bottom, s.margin_bottom)),
        _ => None,
    };
    let hx = frame.and_then(|f| page_x(f.sect).map(|(a, e)| (a - f.origin.0, e))).unwrap_or(col);
    let vy = frame.and_then(|f| page_y(f.sect).map(|(a, e)| (a - f.origin.1, e))).unwrap_or((y0, 0.0));
    Rect::new(align_in(hx, float.x, w, float.h_align), align_in(vy, float.y, h, float.v_align), w, h)
}

/// Where an object of `size` starts on one axis of `area` (start, extent): at `offset` from the
/// area's start, or aligned in it when `align` is set.
fn align_in((start, extent): (f32, f32), offset: f32, size: f32, align: Option<FloatAlign>) -> f32 {
    match align {
        None => start + if offset.is_finite() { offset.clamp(-31_680.0, 31_680.0) } else { 0.0 },
        Some(FloatAlign::Start | FloatAlign::Inside) => start,
        Some(FloatAlign::Center) => start + (extent - size) / 2.0,
        Some(FloatAlign::End | FloatAlign::Outside) => start + extent - size,
    }
}

/// An area text keeps clear of: its bounds, whether text may only go above and below it, and
/// (Tight and Through wrapping) the outline text follows inside it, and the sides text may go on.
type WrapArea = (Rect, bool, Option<Arc<contour::Contour>>, WrapText);

/// The area text keeps clear of around a floating object at `r` (its rectangle grown by the
/// distances from text; Tight and Through: around `outline`, unit coordinates across `r`, when
/// it has one), and whether text may only go above and below it. `None` when text flows over
/// or under the object.
fn wrap_area(r: Rect, float: &Float, outline: Option<&[(f32, f32)]>) -> Option<WrapArea> {
    if matches!(float.wrap, Wrap::Inline | Wrap::BehindText | Wrap::InFrontOfText) {
        return None;
    }
    if matches!(float.wrap, Wrap::Tight | Wrap::Through)
        && let Some((area, c)) = outline.and_then(|o| contour::contour_area(r, float, o))
    {
        return Some((area, false, Some(Arc::new(c)), float.wrap_text));
    }
    let d = |v: f32| if v.is_finite() { v.clamp(0.0, 1584.0) } else { 0.0 };
    let (side, top, bottom) = (d(float.dist), d(float.dist_top), d(float.dist_bottom));
    // A rotated object keeps text clear of its rotated bounds.
    let r = float.spin().bounds(r);
    Some((Rect::new(r.x - side, r.y - top, r.w + side * 2.0, r.h + top + bottom), float.wrap == Wrap::TopAndBottom, None, float.wrap_text))
}

/// Wrap areas relative to a paragraph whose column starts at `x0` and whose top is `y0`.
fn rel_exclusions(excl: &[WrapArea], x0: f32, y0: f32) -> Vec<para::Exclusion> {
    excl.iter()
        .map(|(r, tb, c, side)| para::Exclusion {
            top: r.y - y0,
            bottom: r.bottom() - y0,
            left: r.x - x0,
            right: r.right() - x0,
            top_bottom: *tb,
            contour: c.clone(),
            side: *side,
        })
        .filter(|e| e.bottom > 0.0)
        .collect()
}

/// The items drawing floating object `o` at `rect` (a group: its members), turned by its own
/// spin inside `outer` (its group's, about `outer`'s centre point).
fn float_items(o: &InlineObject, rect: Rect, outer: (Spin, Point), story: StoryRef, path: &[u32], off: usize) -> Vec<Placed> {
    let own = o.frame().map(|(_, _, f)| f.spin()).unwrap_or_default();
    let (ospin, oc) = outer;
    let rect = carried(rect, ospin, oc);
    let spin = own.within(ospin);
    match o {
        InlineObject::Image { media, crop, .. } => {
            vec![Placed::Image { rect, media: media.clone(), crop: *crop, story, path: Path(path.to_vec()), off, spin }]
        }
        InlineObject::Shape { kind, fill, stroke, stroke_width, effects, freeform, extra, .. } => {
            vec![Placed::Shape {
                rect,
                story,
                kind: *kind,
                fill: *fill,
                stroke: *stroke,
                stroke_width: *stroke_width,
                effects: *effects,
                freeform: freeform.clone(),
                spin,
                extra: extra.clone(),
            }]
        }
        InlineObject::Graphic { graphic, .. } => {
            vec![Placed::Graphic { rect, graphic: graphic.clone(), story, path: Path(path.to_vec()), off, spin }]
        }
        InlineObject::Group { .. } => {
            let centre = Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            match o.canvas_style() {
                // A Drawing Canvas: its background and outline, then its members clipped to its
                // frame, all turned together: the members sit in the unturned frame and the clip
                // carries the turn (drawn as the turn around the clip).
                Some(c) => {
                    let members = group_members(o, rect)
                        .into_iter()
                        .flat_map(|(r, m)| float_items(m, r, (Spin::default(), centre), story, path, off))
                        .collect();
                    vec![
                        Placed::Shape {
                            rect,
                            story,
                            kind: wordcraft_doc::para::ShapeKind::Rectangle,
                            fill: c.fill,
                            stroke: c.stroke,
                            stroke_width: c.stroke_width,
                            effects: Default::default(),
                            freeform: None,
                            spin,
                            extra: Default::default(),
                        },
                        Placed::Clip { rect, spin, items: members },
                    ]
                }
                None => group_members(o, rect).into_iter().flat_map(|(r, c)| float_items(c, r, (spin, centre), story, path, off)).collect(),
            }
        }
        _ => Vec::new(),
    }
}

/// A group's members, each with its rectangle when the group is at `rect`.
/// `rect` moved (not turned) to where `spin` about `centre` takes its centre: a group member's
/// frame inside its turned group.
fn carried(rect: Rect, spin: Spin, centre: Point) -> Rect {
    let (cx, cy) = (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    let (nx, ny) = spin.apply(centre.x, centre.y, cx, cy);
    Rect::new(rect.x + nx - cx, rect.y + ny - cy, rect.w, rect.h)
}

fn group_members(o: &InlineObject, rect: Rect) -> Vec<(Rect, &InlineObject)> {
    o.group_rects(rect.x, rect.y, rect.w, rect.h).into_iter().map(|([x, y, w, h], c)| (Rect::new(x, y, w, h), c)).collect()
}

/// Position the floating objects anchored in `p` for a paragraph whose text starts at `y0` in
/// the current column, add their wrap areas to the page (replacing any from an earlier attempt,
/// from `excl_mark` on) and lay the paragraph out around them, with its list `label`.
fn anchor_floats(
    ctx: &mut Ctx,
    pb: &mut PageBuilder,
    p: &Paragraph,
    y0: f32,
    before: f32,
    excl_mark: usize,
    label: &Option<(String, Level)>,
) -> (HashMap<usize, Rect>, Arc<ParaLayout>) {
    pb.excl.truncate(excl_mark);
    ctx.fields.page = pb.number;
    let (col_x, width) = (pb.col_x(), pb.col_w());
    let mut rects = HashMap::new();
    if !pb.web {
        let frame = PageFrame { sect: pb.sect, origin: (0.0, 0.0) };
        // An object left out of the layout (hidden, or deleted in the final text) is not placed, so
        // it takes no room either.
        let left_out = left_out_objects(ctx.doc, p, None, ctx.opts.show_hidden, ctx.hide_deleted);
        for (oi, o) in p.objects.iter().enumerate() {
            let Some((w, h, float)) = floating(o) else { continue };
            if left_out(oi) {
                continue;
            }
            // Positions relative to the paragraph start above its space before (Word for Windows).
            let r = float_rect(Some(frame), (col_x, width), y0 - before, w, h, float);
            rects.insert(oi, r);
            let outline = ctx.wrap_outline(o);
            if let Some(area) = wrap_area(r, float, outline.as_deref().map(|v| v.as_slice())) {
                pb.excl.push(area);
            }
        }
    }
    let rel = rel_exclusions(&pb.excl, col_x, y0);
    (rects, ctx.para_labelled(p, width, None, &rel, label.clone()))
}

/// Where a paragraph's floating objects are positioned from, when they aren't placed already:
/// the box on its page, the column (left, width) and the paragraph's top. Also where offsets
/// relative to the column and paragraph start, for dragging.
#[derive(Clone, Copy)]
struct ObjFrame<'a> {
    page: Option<PageFrame<'a>>,
    col: (f32, f32),
    para_y: f32,
    /// A table's character formatting under the paragraph's (see `left_out_objects`).
    table_chr: Option<&'a CharProps>,
}

/// The pictures, shapes and text boxes in lines `l0..l1` of paragraph `p` (story `story`, at
/// `path`), whose lines start at (x, y): floating ones drawn, every one's area (hit testing,
/// selection) and text boxes' text. Floating rects come from `floats` (by object index) or are
/// worked out from `at`. Returns what goes behind the text, and what goes in front.
#[allow(clippy::too_many_arguments)]
fn place_objects(
    ctx: &mut Ctx,
    story: StoryRef,
    path: &[u32],
    p: &Paragraph,
    pl: &ParaLayout,
    (l0, l1): (usize, usize),
    (x, y): (f32, f32),
    at: &ObjFrame,
    floats: &HashMap<usize, Rect>,
    depth: usize,
) -> (Vec<Placed>, Vec<Placed>) {
    let (mut behind, mut front) = (Vec::new(), Vec::new());
    let Some(fl) = pl.lines.get(l0) else { return (behind, front) };
    // Objects left out of the layout (hidden, or deleted in the final text) aren't placed.
    let left_out = left_out_objects(ctx.doc, p, at.table_chr, ctx.opts.show_hidden, ctx.hide_deleted);
    for li in l0..l1 {
        let Some(line) = pl.lines.get(li) else { continue };
        for k in line.c0..line.c1 {
            let Some(c) = pl.clusters.get(k) else { continue };
            let para::ClKind::Object(oi) = c.kind else { continue };
            if left_out(oi) {
                continue;
            }
            let Some(obj) = p.objects.get(oi) else { continue };
            let Some((w, h, float)) = obj.frame() else { continue };
            let floating = float.wrap != Wrap::Inline;
            let rect = if floating {
                floats.get(&oi).copied().unwrap_or_else(|| float_rect(at.page, at.col, at.para_y, w, h, float))
            } else {
                let cx = x + line.cl_left(k).unwrap_or(0.0);
                display::inline_rect(Some(obj), cx, y + (line.baseline - fl.top), c.adv, c.obj_h)
            };
            let layer = if float.wrap == Wrap::BehindText { &mut behind } else { &mut front };
            // Floating ones are drawn here; inline ones with their line.
            if floating {
                layer.extend(float_items(obj, rect, (Spin::default(), Point::default()), story, path, c.start));
            }
            let text_box = match obj {
                InlineObject::Shape { story: Some(id), .. } if ctx.doc.parts.get(id).is_some_and(|p| p.kind == wordcraft_doc::PartKind::TextBox) => {
                    Some(*id)
                }
                _ => None,
            };
            // A box linked from another shows (and edits) that box's story.
            let shown = text_box.map(|id| textbox::shown_story(ctx, id));
            front.push(Placed::Object {
                rect,
                spin: float.spin(),
                story,
                path: Path(path.to_vec()),
                off: c.start,
                text_box: shown,
                wrap: float.wrap,
                origin: Point::new(at.col.0, at.para_y),
            });
            // Its text (a group: its text boxes', each where the group's turn carries it; the
            // text itself stays upright), unless the box budget says no (a box inside itself,
            // too deep, too many).
            let centre = Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
            let boxes: Vec<(u32, Rect)> = match obj {
                InlineObject::Group { .. } => group_members(obj, rect)
                    .into_iter()
                    .filter_map(|(r, c)| {
                        c.text_box()
                            .filter(|id| ctx.doc.parts.get(id).is_some_and(|p| p.kind == wordcraft_doc::PartKind::TextBox))
                            .map(|id| (id, carried(r, float.spin(), centre)))
                    })
                    .collect(),
                _ => text_box.map(|id| (id, rect)).into_iter().collect(),
            };
            for (id, rect) in boxes {
                // Lay the box once for both presentation and clipping diagnostics. Linked boxes
                // compute their diagnostics while splitting the chain and may not have a single
                // independent source extent.
                let laid_out = textbox::box_items(ctx, id, rect, depth + 1);
                if let Some((horizontal, vertical)) = laid_out.overflow {
                    if horizontal {
                        ctx.box_clips.push((story, Path(path.to_vec()), c.start, OverflowAxis::Horizontal));
                    }
                    if vertical {
                        ctx.box_clips.push((story, Path(path.to_vec()), c.start, OverflowAxis::Vertical));
                    }
                }
                let layer = if float.wrap == Wrap::BehindText { &mut behind } else { &mut front };
                layer.extend(laid_out.items);
            }
        }
    }
    (behind, front)
}

/// Footnotes referenced in a paragraph laid out as `pl`: in its text, and inside its text boxes
/// (on the box's cluster), as (cluster, note id).
fn para_notes(doc: &Document, p: &Paragraph, pl: &ParaLayout) -> Vec<(usize, u32)> {
    let mut notes = pl.notes.clone();
    if p.objects.iter().any(|o| !o.text_boxes().is_empty()) {
        for (ci, c) in pl.clusters.iter().enumerate() {
            if let para::ClKind::Object(oi) = c.kind
                && let Some(o) = p.objects.get(oi)
            {
                for id in o.text_boxes() {
                    notes.extend(doc.notes_in_text_box(id).into_iter().map(|n| (ci, n)));
                }
            }
        }
    }
    notes
}

/// Place a body paragraph. An empty paragraph that only carries a section break before a
/// continuous section takes no room (Word for Windows: the text after it follows right on, and
/// columns balanced above it don't count it).
fn place_para(ctx: &mut Ctx, pb: &mut PageBuilder, p: &Paragraph, block: usize, body_top: f32) {
    if !pb.web && ctx.quiet_marks.binary_search(&block).is_ok() {
        // Its (empty) line stays where the text ended, even at the bottom of a full column.
        let (y, prev, bottom, content_bottom, at) = (pb.y, pb.prev.clone(), pb.bottom, pb.content_bottom, (pb.pages.len(), pb.col));
        pb.bottom = f32::MAX / 4.0;
        place_para_in_flow(ctx, pb, p, block, body_top);
        pb.bottom = bottom;
        if (pb.pages.len(), pb.col) == at {
            (pb.y, pb.prev, pb.content_bottom) = (y, prev, content_bottom);
        }
        return;
    }
    place_para_in_flow(ctx, pb, p, block, body_top);
}

fn place_para_in_flow(ctx: &mut Ctx, pb: &mut PageBuilder, p: &Paragraph, block: usize, body_top: f32) {
    let width = pb.col_w();
    // Floating objects anchored here join the page's wrap areas before the text is laid out; if
    // the paragraph then starts somewhere else (another column or page, or contextual spacing
    // drops its space before), they are placed again there.
    let space_before = ctx.doc.styles.resolve_para(&p.props).space_before;
    let excl_mark = pb.excl.len();
    let guess = pb.y + space_before;
    let label = ctx.next_label(p);
    let (mut float_rects, mut pl) = anchor_floats(ctx, pb, p, guess, space_before, excl_mark, &label);
    // The anchor paragraph's top (above its space before) the floats were placed from.
    let mut anchor_y = pb.y;
    let spot = |pb: &PageBuilder| (pb.pages.len(), pb.col);
    let placed_at = spot(pb);
    let broke_before = pl.rp.page_break_before && !pb.at_top() && !pb.web;
    if broke_before {
        pb.new_page(block, body_top);
    }
    let mut before = pl.rp.space_before;
    let prev_para = pb.prev.clone();
    // At the top of a page, measured in Word for Windows: no space before after a paragraph that
    // ended with a page break, nor (from Word 2013's layout, compatibility mode 15) for a page
    // break before; a section break or, in older modes, a page break before keeps it.
    let after_break_mark = std::mem::take(&mut pb.after_break_mark);
    if pb.at_top() && (after_break_mark || (broke_before && ctx.doc.settings.compat_mode >= 15)) {
        before = 0.0;
    } else if pb.at_top()
        && let Some((_, _, after, _, _)) = &prev_para
    {
        // After a page break before or a section break the space between the paragraphs is
        // still counted as one: the previous paragraph's space after (left on its page) comes
        // off the space before here (Word: 24 pt before after 10 pt after leaves 14 pt).
        before = (before - after.max(0.0)).max(0.0);
    }
    if let Some((style, ctxl, mut after, after_auto, listed)) = pb.prev.take()
        && !pb.at_top()
    {
        // Contextual spacing: no space between paragraphs of the same style when either asks for it.
        if style == pl.rp.style {
            if ctxl {
                pb.y -= after;
                after = 0.0;
            }
            if pl.rp.contextual_spacing {
                before = 0.0;
            }
        }
        // Automatic spacing: none between two list items (as in an HTML list; measured in Word:
        // any two items, of one list or two, at any level).
        if listed && label.is_some() {
            if after_auto {
                pb.y -= after;
                after = 0.0;
            }
            if pl.rp.before_auto {
                before = 0.0;
            }
        }
        // The space between two paragraphs is the larger of the first's space after and the
        // second's space before, not their sum.
        if before > 0.0 && after > 0.0 {
            before = (before - after).max(0.0);
        }
    }
    // Automatic spacing: none at the top of the document.
    if pl.rp.before_auto && block == 0 {
        before = 0.0;
    }
    pb.y += before;
    let mut applied_before = before;
    // Keep lines together: if it doesn't fit but would on an empty column, move it. Keep with
    // next alone moves a paragraph only when it would fit here by itself (Word for Windows): one
    // that doesn't breaks as usual, its last lines going over with the next paragraph.
    if pl.rp.keep_lines || (pl.rp.keep_next && pb.y + pl.height <= pb.bottom + 0.01) {
        let need = pl.height + if pl.rp.keep_next { next_first_line(ctx, block, width, 0, pl.rp.space_after) } else { 0.0 };
        // (Against the whole column: balanced columns grow to hold it.)
        if pb.y + need > pb.bottom && !pb.at_top() && need <= pb.uncapped_bottom() - pb.top {
            pb.advance(block, body_top);
            pb.y += pl.rp.space_before.min(0.0);
            applied_before = pl.rp.space_before.min(0.0);
        }
    }
    // A different top only matters when there is something to wrap around.
    if spot(pb) != placed_at || ((pb.y - guess).abs() > 0.01 && !pb.excl.is_empty()) {
        let y0 = pb.y;
        (float_rects, pl) = anchor_floats(ctx, pb, p, y0, applied_before, excl_mark, &label);
        anchor_y = y0 - applied_before;
    }
    let mut n = pl.lines.len();
    let mut l0 = 0;
    while l0 < n {
        // How many lines fit?
        let Some(first) = pl.lines.get(l0) else { break };
        // A paragraph's first line may start below its top (pushed down by a floating object).
        let lead = if l0 == 0 { first.top.max(0.0) } else { 0.0 };
        let mut l1 = l0;
        let mut forced_break = None;
        // A page break followed only by the paragraph mark: the mark's (empty) line stays on this
        // page with the break, so nothing of the paragraph goes to the next page (Word).
        let mut mark_with_break = false;
        let mut new_notes: Vec<NoteBox> = Vec::new();
        let notes = para_notes(ctx.doc, p, &pl);
        while l1 < n {
            let Some(l) = pl.lines.get(l1) else { break };
            // Footnotes referenced on this line go to the bottom of this page.
            let mut line_notes = Vec::new();
            if !pb.web {
                for (ci, id) in &notes {
                    if *ci >= l.c0
                        && *ci < l.c1
                        && !pb.notes.iter().chain(new_notes.iter()).any(|x| x.id == *id)
                        && let Some(part) = ctx.doc.parts.get(id).filter(|p| p.kind == wordcraft_doc::PartKind::Footnote)
                    {
                        let blocks = part.blocks.clone();
                        let BoxLayout { items, height: h, .. } =
                            layout_box(ctx, StoryRef::Part(*id), &blocks, &[], pb.sect.text_width(), None, 0, None);
                        line_notes.push(NoteBox { id: *id, items, h, cont: false });
                    }
                }
            }
            let candidate_notes_h: f32 = pb.notes.iter().chain(&new_notes).chain(&line_notes).map(|note| note.h).sum();
            let candidate_note_h =
                if pb.notes.is_empty() && new_notes.is_empty() && line_notes.is_empty() { 0.0 } else { NOTE_SEP + candidate_notes_h };
            let limit = pb.orig_bottom - pb.reserved_note_h.max(candidate_note_h);
            let bottom = pb.y + lead + (l.top + l.height - first.top);
            // Notes too long for the rest of the page continue on the next one: the line still
            // fits if the notes before its last one, and that one's first line, fit below it.
            // The notes then take the rest of the page.
            let split = bottom > limit + 0.01
                && line_notes.split_last().is_some_and(|(last, others)| {
                    let before: f32 = pb.notes.iter().chain(&new_notes).chain(others).map(|n| n.h).sum();
                    let sep = pb.seps.sep_h(pb.notes.first().is_some_and(|n| n.cont));
                    pb.body_floor().max(bottom) + sep + before + last.first_piece() + pb.seps.notice_h() <= pb.orig_bottom + 0.01
                });
            if bottom > limit + 0.01 && l1 > l0 && !split {
                break;
            }
            // A line holding nothing but a page or column break stays at the bottom of the page
            // even when it doesn't fit: it only ends the page, and moving it would leave a blank
            // page behind it (Word does the same).
            let only_break = matches!(l.end, LineEnd::PageBreak | LineEnd::ColumnBreak)
                && p.text
                    .get(l.start..l.stop)
                    .is_some_and(|t| t.chars().all(|c| matches!(c, wordcraft_doc::para::PAGE_BREAK | wordcraft_doc::para::COLUMN_BREAK)));
            if bottom > limit + 0.01 && l1 == l0 && !pb.at_top() && !split && !only_break {
                break;
            }
            new_notes.extend(line_notes);
            l1 += 1;
            if matches!(l.end, LineEnd::PageBreak | LineEnd::ColumnBreak) && !pb.web {
                forced_break = Some(l.end);
                if l.end == LineEnd::PageBreak && l1 + 1 == n && pl.lines.get(l1).is_some_and(|m| m.start >= m.stop) {
                    l1 += 1;
                    mark_with_break = true;
                }
                break;
            }
            if split {
                break;
            }
        }
        // Widow/orphan control (2-line minimum at either end).
        if forced_break.is_none() && l1 < n && pl.rp.widow_control && n >= 2 {
            if l1 - l0 == 1 && l0 == 0 && !pb.at_top() {
                l1 = l0; // orphan: move the first line to the next page
            } else if n - l1 == 1 && l1 - l0 >= 3 {
                l1 -= 1; // widow: take one more line along
            }
        }
        // A row (text both sides of a floating object) stays together.
        while l1 > l0 && l1 < n && pl.lines.get(l1).is_some_and(|l| l.beside) {
            l1 -= 1;
        }
        if l1 == l0 {
            if pb.at_top() {
                l1 = l0 + 1; // can't fit even one line on an empty page: overflow
            } else {
                pb.advance(block, body_top);
                if l0 == 0 {
                    let y0 = pb.y;
                    (float_rects, pl) = anchor_floats(ctx, pb, p, y0, 0.0, excl_mark, &label);
                    anchor_y = y0;
                    n = pl.lines.len();
                }
                continue;
            }
        }
        // Keep only the notes of the lines that are placed.
        let placed_end = pl.lines.get(l1.saturating_sub(1)).map(|l| l.c1).unwrap_or(0);
        new_notes.retain(|n| notes.iter().any(|(ci, nid)| *nid == n.id && *ci < placed_end));
        pb.notes.extend(new_notes);
        pb.bottom = pb.orig_bottom - pb.reserved_note_h.max(pb.notes_h());
        let x = pb.col_x();
        let y = pb.y + lead;
        // Notes that no longer fit below the text (a long one, or the rest of one continued from
        // the page before) get what's left under it and continue on the next page.
        if !pb.notes.is_empty()
            && !pb.web
            && let Some(placed_bottom) = item_bottom(y, &pl, l0, l1)
            && placed_bottom > pb.bottom + 0.01
        {
            pb.bottom = pb.body_floor().max(placed_bottom).min(pb.orig_bottom);
        }
        let mut items = Vec::new();
        push_para(&mut items, StoryRef::Body, &[block as u32], &pl, l0, l1, x, y, width);
        // Pictures, shapes and text boxes in these lines (web view: floats placed by this piece).
        let at = ObjFrame {
            page: Some(PageFrame { sect: pb.sect, origin: (0.0, 0.0) }),
            col: (x, width),
            para_y: if pb.web { y } else { anchor_y },
            table_chr: None,
        };
        let (behind, front) = place_objects(ctx, StoryRef::Body, &[block as u32], p, &pl, (l0, l1), (x, y), &at, &float_rects, 0);
        items.extend(front);
        // Line numbers in the left margin.
        if let Some(ln) = pb.sect.line_numbers.clone().filter(|_| !pb.web && pl.rp.style != "Header" && p.props.suppress_line_numbers != Some(true))
            && let Some(first) = pl.lines.get(l0)
        {
            for li in l0..l1 {
                let Some(line) = pl.lines.get(li).filter(|l| !l.beside) else { continue };
                pb.line_no = pb.line_no.saturating_add(1);
                let n = pb.line_no.saturating_add(ln.start.saturating_sub(1));
                if ln.count_by > 1 && !n.is_multiple_of(ln.count_by) {
                    continue;
                }
                let num = ctx.number_para(n);
                let w = num.lines.first().and_then(|l| l.xs.last().copied()).unwrap_or(10.0);
                let dist = if ln.distance > 0.0 { ln.distance } else { 18.0 };
                let ly = y + (line.top - first.top) + (line.baseline - line.top) - num.lines.first().map(|l| l.baseline).unwrap_or(10.0);
                items.push(Placed::Lines {
                    story: DECOR_STORY,
                    path: Path(vec![]),
                    para: num.clone(),
                    l0: 0,
                    l1: 1,
                    x: pb.sect.margin_left + pb.sect.gutter - dist - w,
                    y: ly,
                    turn: TextDirection::Horizontal,
                });
            }
        }
        let n_behind = behind.len();
        if let Some(pg) = pb.pages.last_mut() {
            for (i, it) in behind.into_iter().enumerate() {
                pg.items.insert(i.min(pg.items.len()), it);
            }
            pg.items.extend(items);
        }
        if let Some(b) = pb.behind.last_mut() {
            *b += n_behind;
        }
        if let (Some(f), Some(l)) = (pl.lines.get(l0), pl.lines.get(l1 - 1)) {
            pb.y = y + (l.top + l.height - f.top);
            pb.content_bottom = pb.content_bottom.max(pb.y);
        }
        l0 = l1;
        // A paragraph that starts with a page break starts on the new page, its space before
        // there (Word).
        let starts_with_break = l0 == 1
            && pl.lines.first().is_some_and(|l| {
                p.text.get(l.start..l.stop).is_some_and(|t| !t.is_empty() && t.chars().all(|c| c == wordcraft_doc::para::PAGE_BREAK))
            });
        match forced_break {
            Some(LineEnd::PageBreak) if mark_with_break => {
                pb.new_page(block, body_top);
                pb.after_break_mark = true;
            }
            Some(LineEnd::PageBreak) if starts_with_break && !pb.web => {
                pb.new_page(block, body_top);
                pb.y += pl.rp.space_before.max(0.0);
            }
            Some(LineEnd::PageBreak) => pb.new_page(block, body_top),
            Some(LineEnd::ColumnBreak) => {
                pb.col_break_page = Some(pb.pages.len().saturating_sub(1));
                pb.advance(block, body_top);
                // Unequal columns: the text after the break is broken for its own column's width
                // (Word), not the width of the column it started in.
                if (pb.col_w() - width).abs() > 0.5
                    && let Some(resume) = pl.lines.get(l0).map(|l| l.start)
                {
                    let y0 = pb.y;
                    let (rects, relaid) = anchor_floats(ctx, pb, p, y0, 0.0, excl_mark, &label);
                    if let Some(k) = relaid.lines.iter().position(|l| l.start == resume) {
                        (float_rects, pl) = (rects, relaid);
                        anchor_y = y0;
                        n = pl.lines.len();
                        l0 = k;
                    }
                }
            }
            _ if l0 < n => pb.advance(block, body_top),
            _ => {}
        }
    }
    pb.y += pl.rp.space_after;
    pb.prev = Some((pl.rp.style.clone(), pl.rp.contextual_spacing, pl.rp.space_after, pl.rp.after_auto, label.is_some()));
}

/// Height of the first line of the block after `block` (for keep-with-next).
/// The room the paragraphs after `block` need on its page for keep with next: the next one's
/// first line, or all of it and what it keeps with in turn when it is kept with the next too
/// (a chain moves together in Word).
fn next_first_line(ctx: &mut Ctx, block: usize, width: f32, depth: usize, preceding_after: f32) -> f32 {
    match ctx.doc.body.get(block + 1).map(|b| &**b) {
        Some(Block::Para(p)) => {
            // Don't advance list counters for a lookahead: lay out without the label.
            let rp = ctx.doc.styles.resolve_para(&p.props);
            let env = para::ParaEnv {
                doc: ctx.doc,
                width,
                label: None,
                fields: &ctx.fields,
                show_hidden: ctx.opts.show_hidden,
                field_codes: ctx.opts.field_codes,
                hide_deleted: ctx.opts.hides_deleted(StoryRef::Body),
                table: None,
                proofing: None,
                numeral: ctx.opts.numeral,
                exclusions: &[],
                eq_number: ctx.eq_count,
                read_line_spacing: if ctx.opts.view == ViewMode::Read { ctx.opts.read_line_spacing } else { None },
                grid: ctx.grid,
                fit_objects: false,
            };
            let _ = rp;
            let pl = para::layout_para(p, &env);
            // Adjacent paragraph spacing uses the larger positive before/after value. This
            // gap participates in the kept-together fit check, including the first line.
            let before = pl.rp.space_before;
            let gap = if before > 0.0 && preceding_after > 0.0 { before.max(preceding_after) } else { before + preceding_after };
            if pl.rp.keep_next && depth < 8 {
                gap + pl.height + next_first_line(ctx, block + 1, width, depth + 1, pl.rp.space_after)
            } else {
                // Widow/orphan control moves a paragraph whose first line alone fits to the next
                // page. A kept predecessor must reserve that two-line minimum too, or it can be
                // stranded at the bottom while the whole following paragraph advances.
                let line_need = match (pl.rp.widow_control, pl.lines.first(), pl.lines.get(1)) {
                    (true, Some(first), Some(second)) => second.top + second.height - first.top,
                    (_, Some(first), _) => first.height,
                    _ => 0.0,
                };
                gap + line_need
            }
        }
        Some(Block::Table(_)) => 24.0,
        None => 0.0,
    }
}

fn place_table(ctx: &mut Ctx, pb: &mut PageBuilder, t: &wordcraft_doc::Table, block: usize, body_top: f32) {
    pb.prev = None;
    let width = pb.col_w();
    let mut tl = table::layout_table(ctx, StoryRef::Body, t, &[block as u32], width, 1);
    if let Some(f) = t.props.float.filter(|_| !pb.web) {
        // Before Word 2013 layout (compatibility mode 15) an offset places the first cell's text,
        // so the edge sits a cell margin further out.
        let legacy = if ctx.doc.settings.compat_mode < 15 { table::first_cell_left_margin(ctx, t) } else { 0.0 };
        let h: f32 = tl.rows.iter().map(|r| r.height).sum();
        if h <= pb.uncapped_bottom() - pb.top + 0.01 {
            place_floating_table(pb, &tl, &f, legacy, block, body_top);
            return;
        }
        // Too tall for a page: Word lets it run across pages like a table in the text, from its
        // own left edge.
        tl.x = float_table_x(pb, tl.width, &f, legacy) - pb.col_x();
    }
    // A table in the text flow starts below the floating objects in its way.
    below_floats(pb, pb.col_x() + tl.x, pb.col_x() + tl.x + tl.width);
    let header_rows: Vec<usize> = (0..t.rows.len()).take_while(|r| t.rows.get(*r).is_some_and(|row| row.props.header)).collect();
    let place = |pb: &mut PageBuilder, row: &table::RowLayout| {
        let (x, y) = (pb.col_x() + tl.x, pb.y);
        if let Some(pg) = pb.page() {
            for it in &row.items {
                let mut it = it.clone();
                it.translate(x, y);
                pg.items.push(it);
            }
        }
        pb.y += row.height;
        pb.content_bottom = pb.content_bottom.max(pb.y);
    };
    for (ri, row) in tl.rows.iter().enumerate() {
        let splittable = !header_rows.contains(&ri) && !t.rows.get(ri).is_some_and(|r| r.props.cant_split);
        let mut rest: Option<table::RowLayout> = None;
        // On a new page holding only the repeated header rows.
        let mut fresh = false;
        // Each pass places the part of the row that fits, then breaks the page.
        for _ in 0..1000 {
            let cur = rest.as_ref().unwrap_or(row);
            if pb.y + cur.height <= pb.bottom + 0.01 {
                break;
            }
            let split = if splittable { table::split_row(cur, pb.bottom - pb.y) } else { None };
            match split {
                Some((a, b)) => {
                    place(pb, &a);
                    table_notes(ctx, pb, &a.items);
                    rest = Some(b);
                }
                // Like Word, a row that does not fit on an empty page, or under the header rows
                // repeated there, is placed anyway and runs into the bottom margin.
                None if pb.at_top() || fresh => break,
                None => {}
            }
            pb.advance(block, body_top);
            // Repeat header rows.
            if !header_rows.contains(&ri) {
                for hr in &header_rows {
                    if let Some(h) = tl.rows.get(*hr) {
                        place(pb, h);
                    }
                }
            }
            fresh = true;
        }
        let last = rest.as_ref().unwrap_or(row);
        place(pb, last);
        table_notes(ctx, pb, &last.items);
    }
}

/// Footnotes referenced by placed table-cell lines are added to the page note area as in body text.
fn table_notes(ctx: &mut Ctx, pb: &mut PageBuilder, items: &[Placed]) {
    if pb.web {
        return;
    }
    let mut ids = Vec::new();
    for item in items {
        let Placed::Lines { story: StoryRef::Body, para, l0, l1, .. } = item else { continue };
        let (Some(first), Some(last)) = (para.lines.get(*l0), para.lines.get(l1.saturating_sub(1))) else { continue };
        for (cluster, id) in &para.notes {
            if *cluster >= first.c0 && *cluster < last.c1 && !ids.contains(id) && !pb.notes.iter().any(|note| note.id == *id) {
                ids.push(*id);
            }
        }
    }
    let before = pb.notes_h();
    for id in ids {
        let Some(part) = ctx.doc.parts.get(&id).filter(|part| part.kind == wordcraft_doc::PartKind::Footnote) else { continue };
        let blocks = part.blocks.clone();
        let BoxLayout { items, height, .. } = layout_box(ctx, StoryRef::Part(id), &blocks, &[], pb.sect.text_width(), None, 0, None);
        pb.notes.push(NoteBox { id, items, h: height, cont: false });
    }
    pb.bottom -= pb.notes_h() - before;
}

/// Place floating table `tl` (`w:tblpPr`): at its own position, which takes no room in the text
/// flow; the text after it wraps around it.
fn place_floating_table(pb: &mut PageBuilder, tl: &table::TableLayout, f: &TableFloat, legacy: f32, block: usize, body_top: f32) {
    let h: f32 = tl.rows.iter().map(|r| r.height).sum::<f32>().clamp(0.0, 100_000.0);
    let w = tl.width.clamp(1.0, 100_000.0);
    let [dl, dt, dr, db] = f.dist_from_text();
    for attempt in 0..2 {
        let s = pb.sect;
        let h_area = float_table_h_area(pb, f);
        // The text a table anchored to it stands in: where the next paragraph starts.
        let v_area = match f.v_rel {
            Anchor::Page => (0.0, s.page_h),
            Anchor::Margin => (s.margin_top, s.text_height()),
            _ => (pb.y, 0.0),
        };
        let x = float_table_x(pb, w, f, legacy);
        let r = clear_of_float_tables(Rect::new(x, align_in(v_area, f.y, h, f.v_align), w, h), f, legacy, h_area.0, &pb.float_tables);
        // Like Word, a table that doesn't fit below the text before it moves to the next page with
        // its anchor: one placed relative to the paragraph when it would run off the page, one
        // placed on the page or margin when it would also cover the text already on the page.
        let moves = if f.v_rel == Anchor::Paragraph {
            r.bottom() > pb.bottom + 0.01
        } else {
            let covers_text = r.y < pb.y - 0.01 && r.bottom() > pb.top + 0.01 && r.x < pb.col_x() + pb.col_w() && r.right() > pb.col_x();
            covers_text && pb.y + h > pb.bottom + 0.01
        };
        if attempt == 0 && moves && !pb.at_top() {
            pb.advance(block, body_top);
            continue;
        }
        if let Some(pg) = pb.page() {
            let mut y = r.y;
            for row in &tl.rows {
                for it in &row.items {
                    let mut it = it.clone();
                    it.translate(r.x, y);
                    pg.items.push(it);
                }
                y += row.height;
            }
        }
        let area = r.inset(-dl, -dt, -dr, -db);
        pb.excl.push((area, false, None, WrapText::BothSides));
        pb.float_tables.push(area);
        return;
    }
}

/// The area a floating table's `x` is measured in: its start and width on the page.
fn float_table_h_area(pb: &PageBuilder, f: &TableFloat) -> (f32, f32) {
    let s = pb.sect;
    match f.h_rel {
        Anchor::Page => (0.0, s.page_w),
        Anchor::Margin => (s.margin_left + s.gutter, s.text_width()),
        _ => (pb.col_x(), pb.col_w()),
    }
}

/// A floating table's left edge on the page (`legacy`: the first cell's left margin before Word
/// 2013 layout, when an offset places the cell's text).
fn float_table_x(pb: &PageBuilder, w: f32, f: &TableFloat, legacy: f32) -> f32 {
    let x = align_in(float_table_h_area(pb, f), f.x, w, f.h_align);
    if f.h_align.is_none() { x - legacy } else { x }
}

/// Move `pb.y` below the floating objects in the way of something spanning `x0..x1` that
/// starts there (a table in the text flow doesn't wrap around them).
fn below_floats(pb: &mut PageBuilder, x0: f32, x1: f32) {
    pb.y = below(&pb.excl, pb.y, x0, x1);
}

/// `y`, moved below the wrap areas in `excl` in the way of something spanning `x0..x1` that
/// starts there.
fn below(excl: &[WrapArea], mut y: f32, x0: f32, x1: f32) -> f32 {
    for _ in 0..64 {
        let in_the_way = excl.iter().filter(|(r, ..)| r.y <= y + 0.01 && r.bottom() > y && r.x < x1 && r.right() > x0);
        let Some(bottom) = in_the_way.map(|(r, ..)| r.bottom()).reduce(f32::max) else { break };
        y = bottom;
    }
    y
}

fn headers_footers(ctx: &mut Ctx, pages: &mut [Page], sections: &[(usize, &SectionProps)]) {
    let total = pages.len() as u32;
    // Pages per section.
    let mut per_section: HashMap<usize, u32> = HashMap::new();
    for p in pages.iter() {
        *per_section.entry(p.section).or_default() += 1;
    }
    let even_odd = ctx.doc.settings.even_odd_headers;
    let mut first_of_section: HashSet<usize> = HashSet::new();
    let mut seen = HashSet::new();
    for (i, p) in pages.iter().enumerate() {
        if seen.insert(p.section) {
            first_of_section.insert(i);
        }
    }
    for (i, page) in pages.iter_mut().enumerate() {
        let Some((_, sect)) = sections.get(page.section) else { continue };
        // Inherit header/footer references from earlier sections (Word's "link to previous").
        let pick = |get: &dyn Fn(&SectionProps) -> wordcraft_doc::section::HeaderSet| -> Option<u32> {
            let mut set = get(sect);
            for k in (0..page.section).rev() {
                if set.default.is_some() && set.first.is_some() && set.even.is_some() {
                    break;
                }
                if let Some((_, s)) = sections.get(k) {
                    let o = get(s);
                    set.default = set.default.or(o.default);
                    set.first = set.first.or(o.first);
                    set.even = set.even.or(o.even);
                }
            }
            if sect.title_page && first_of_section.contains(&i) {
                return set.first;
            }
            if even_odd && page.number % 2 == 0 {
                return set.even.or(set.default);
            }
            set.default
        };
        let hid = pick(&|s: &SectionProps| s.headers);
        let fid = pick(&|s: &SectionProps| s.footers);
        ctx.fields.page = page.number;
        ctx.fields.page_format = sect.page_num_format;
        ctx.fields.pages = total;
        ctx.fields.section_pages = per_section.get(&page.section).copied().unwrap_or(1);
        ctx.fields.section = page.section as u32 + 1;
        let w = sect.text_width();
        let x = sect.margin_left + sect.gutter;
        if let Some(id) = hid
            && let Some(part) = ctx.doc.parts.get(&id)
        {
            let blocks = part.blocks.clone();
            let frame = PageFrame { sect, origin: (x, sect.header) };
            let mut items = layout_box(ctx, StoryRef::Part(id), &blocks, &[], w, None, 0, Some(frame)).items;
            for it in &mut items {
                it.translate(x, sect.header);
            }
            page.header = items;
            page.header_story = Some(id);
        }
        if let Some(id) = fid
            && let Some(part) = ctx.doc.parts.get(&id)
        {
            let blocks = part.blocks.clone();
            // The footer's height fixes where it sits, which page-relative floats need.
            let h = layout_box(ctx, StoryRef::Part(id), &blocks, &[], w, None, 0, None).height;
            let frame = PageFrame { sect, origin: (x, sect.page_h - sect.footer - h) };
            let mut items = layout_box(ctx, StoryRef::Part(id), &blocks, &[], w, None, 0, Some(frame)).items;
            for it in &mut items {
                it.translate(x, sect.page_h - sect.footer - h);
            }
            page.footer = items;
            page.footer_story = Some(id);
        }
    }
}

fn mirror_even_pages(pages: &mut [Page], sections: &[(usize, &SectionProps)]) {
    for page in pages.iter_mut().filter(|page| page.number % 2 == 0) {
        let Some((_, section)) = sections.get(page.section) else { continue };
        let dx = section.margin_right - section.margin_left - section.gutter;
        if !dx.is_finite() || dx.abs() < 0.001 {
            continue;
        }
        let border_count = section
            .page_borders
            .as_ref()
            .map(|b| [b.top, b.bottom, b.left, b.right].into_iter().flatten().filter(Border::is_visible).count())
            .unwrap_or(0);
        for item in page.items.iter_mut().skip(border_count).chain(page.header.iter_mut()).chain(page.footer.iter_mut()) {
            item.translate(dx, 0.0);
        }
        page.body.x += dx;
    }
}

/// Is an object floating (not laid out inline)?
pub fn is_floating(o: &InlineObject) -> bool {
    o.is_floating()
}

/// Milliseconds since an arbitrary epoch (monotonic on native; the browser's clock on the web).
pub fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
    }
    // `Instant::now()` panics on wasm32-unknown-unknown, so ask the browser's clock.
    #[cfg(target_arch = "wasm32")]
    {
        let ms = js_sys::Date::now();
        if ms.is_finite() { ms } else { 0.0 }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_bidi;
#[cfg(test)]
mod tests_complex;
#[cfg(test)]
mod tests_grid;
#[cfg(test)]
mod tests_numeral;
#[cfg(test)]
mod tests_pages;
#[cfg(test)]
mod tests_textbox;
#[cfg(test)]
mod tests_vertical;
