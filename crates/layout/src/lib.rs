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
pub mod styleref;
mod table;
mod textbox;
mod vertical;
pub mod wordart;

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use wordcraft_doc::numbering::{Counters, Level};
use wordcraft_doc::para::{Anchor, Float, FloatAlign, InlineObject, Wrap, WrapText};
use wordcraft_doc::props::{Border, CharProps, HeightRule, Rgb, TableFloat, TextDirection};
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
    /// Tracked deletions that take no space and draw nothing although markup is shown: those
    /// Track Changes Options mark Hidden (deletions, or moved-away text). `hide_deleted` and
    /// `deletions_in_balloons` leave every deletion out whatever this says.
    pub hide_marked: HideDeleted,
    /// Show revisions in balloons: tracked deletions in the body take no space and draw nothing
    /// (the editor shows them in balloons beside the page). Headers, footers, notes and text
    /// boxes get no balloons, so their deletions stay inline.
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
    /// The word being typed, which gets no spelling squiggle until the caret leaves it (#1552).
    pub typing_word: Option<TypingWord>,
    /// Document file name for FILENAME fields ([`FieldCtx::filename`]).
    pub filename: Option<Arc<str>>,
    /// Document full path for FILENAME \p fields ([`FieldCtx::filepath`]).
    pub filepath: Option<Arc<str>>,
}

/// The word being typed ([`LayoutOptions::typing_word`]): the paragraph by its revision
/// ([`Paragraph::rev`]) and the word's byte range in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TypingWord {
    pub rev: u64,
    pub start: usize,
    pub end: usize,
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
    /// Which tracked deletions in `story` take no space and draw nothing.
    pub fn hides_deleted(&self, story: StoryRef) -> HideDeleted {
        if self.hide_deleted || (self.deletions_in_balloons && story == StoryRef::Body) { HideDeleted::ALL } else { self.hide_marked }
    }
}

/// Which tracked deletions are left out of the layout: ordinary ones, and the text a tracked
/// move took away (the source of a move, `w:moveFrom`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HideDeleted {
    pub plain: bool,
    pub moved_from: bool,
}

impl HideDeleted {
    pub const ALL: HideDeleted = HideDeleted { plain: true, moved_from: true };
    /// Whether any deletion is left out.
    pub fn any(self) -> bool {
        self.plain || self.moved_from
    }
    /// Whether text deleted by revision `del` (`None`: not deleted) is left out.
    pub fn hides(self, doc: &Document, del: Option<u32>) -> bool {
        del.is_some() && if doc.move_name(del).is_some() { self.moved_from } else { self.plain }
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
        fill: Option<wordcraft_doc::fill::Fill>,
        stroke: Option<Rgb>,
        stroke_width: f32,
        /// A freeform's paths (ink strokes among them).
        freeform: Option<Arc<wordcraft_doc::freeform::Freeform>>,
        effects: wordcraft_doc::effects::ShapeEffects,
        spin: Spin,
        /// Arrowheads and text warp (see [`wordcraft_doc::connector::ShapeExtra`]).
        extra: wordcraft_doc::connector::ShapeExtra,
        /// The floating object it draws (a Drawing Canvas's members: the canvas's), for leaving
        /// exactly one object out of a page (see [`crate::display::DisplayOptions::hide_object`]).
        object: Option<wordcraft_doc::Pos>,
        /// The description it is tagged with as a figure in tagged PDF (see
        /// [`shape_figure_alt`]); empty for a shape that is an artifact.
        alt: String,
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
        /// Its row is exactly as tall as it says and the content is taller: what runs past the
        /// cell is cut off when drawn, as in Word.
        clip: bool,
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

/// Which header/footer of its section a page shows ([`Page::hf_slot`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HfSlot {
    #[default]
    Default,
    /// The first page of a section with "Different First Page".
    First,
    /// An even page with "Different Odd & Even Pages".
    Even,
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
    /// Which of its section's headers/footers this page shows (first page, even page or the
    /// default), whether or not the section has one: double-clicking an empty header area
    /// creates that one (#960).
    pub hf_slot: HfSlot,
    /// Top-level body block index at the start of the page.
    pub first_block: usize,
    /// Width of the markup area on the right of `w` that holds comment balloons (Print Markup,
    /// [`LayoutOptions::comment_balloons`]); 0 without one. The document page is `w - markup` wide.
    pub markup: f32,
    /// The top-level body tables placed on this page as floating tables (`w:tblpPr`): a click
    /// outside one doesn't land in its cells ([`DocLayout::hit`]).
    pub float_tables: Vec<Path>,
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
    page: Option<(u32, u32, u32, u32, u64, wordcraft_doc::section::NumFormat)>,
    /// The pages a split paragraph's fields are on after its first (`FieldCtx::page_from`).
    page_from: u64,
    table: u64,
    notes: u64,
    excl: u64,
    eq: u32,
    read_spacing: Option<u32>,
    grid: (u32, u32, u32, bool),
    fit: bool,
    /// Heights of the paragraph's inline boxes that resize to fit their text.
    fitted: u64,
    hide_deleted: HideDeleted,
    /// STYLEREF results in headers and footers ([`FieldCtx::styleref_key`]).
    sref: u64,
    /// The word being typed in this paragraph, left without a spelling squiggle.
    typing: Option<(usize, usize)>,
}

/// Memoised paragraph layouts.
#[derive(Default)]
pub struct LayoutCache {
    paras: HashMap<Key, Arc<ParaLayout>>,
    /// Narrowest and widest content of paragraphs measured for table autofit ([`Ctx::para_measured`]).
    measures: HashMap<Key, (f32, f32)>,
    /// Outlines of pictures' opaque pixels (Tight and Through wrapping), by media key, the
    /// media's bytes (address and length) and crop.
    outlines: HashMap<(String, usize, usize, [u32; 4]), Option<Arc<Vec<(f32, f32)>>>>,
    env: u64,
    used: HashSet<Key>,
    /// The STYLEREF style index of the last pass and what it was built from: the body blocks
    /// (shared, so an edited block is a different pointer) and `env` (styles and numbering). Kept
    /// while both are unchanged, so headers using STYLEREF don't rebuild it on every pass.
    styleref: Option<(Blocks, u64, Arc<styleref::StyleIndex>)>,
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
        self.measures.clear();
        self.outlines.clear();
        self.styleref = None;
    }

    /// The STYLEREF style index for `doc`, rebuilt only when the body or the styles changed.
    fn style_index(&mut self, doc: &Document) -> Arc<styleref::StyleIndex> {
        if let Some((body, env, ix)) = &self.styleref
            && *env == self.env
            && body.len() == doc.body.len()
            && body.iter().zip(doc.body.iter()).all(|(a, b)| Arc::ptr_eq(a, b))
        {
            return ix.clone();
        }
        let ix = Arc::new(styleref::StyleIndex::build(doc));
        self.styleref = Some((doc.body.to_vec(), self.env, ix.clone()));
        ix
    }
}

fn hash_of<T: Hash>(t: &T) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

fn env_hash(doc: &Document, opts: &LayoutOptions) -> u64 {
    let s = serde_json::to_string(&(&doc.styles, &doc.numbering, doc.settings.default_tab, &doc.settings.footnote_format)).unwrap_or_default();
    // Outline view lays paragraphs out changed (no indents; plain without text formatting).
    let outline = (opts.view == ViewMode::Outline).then(|| opts.outline.as_ref().is_some_and(|o| o.plain));
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    opts.show_hidden.hash(&mut h);
    opts.hide_deleted.hash(&mut h);
    opts.hide_marked.hash(&mut h);
    opts.deletions_in_balloons.hash(&mut h);
    opts.proofing.hash(&mut h);
    opts.numeral.hash(&mut h);
    opts.field_codes.hash(&mut h);
    opts.proof.hash(&mut h);
    wordcraft_proof::dictionary_generation().hash(&mut h);
    hyphenation_key(doc).hash(&mut h);
    outline.hash(&mut h);
    opts.filename.hash(&mut h);
    opts.filepath.hash(&mut h);
    h.finish()
}

/// The hyphenation settings, hashable (the zone as its bits).
fn hyphenation_key(doc: &Document) -> (bool, u32, bool, u32) {
    let s = &doc.settings;
    (s.auto_hyphenation, s.hyphenation_zone.to_bits(), s.hyphenate_caps, s.consecutive_hyphen_limit)
}

fn has_page_fields(p: &Paragraph) -> bool {
    p.objects.iter().any(|o| match o {
        InlineObject::Field { instr, .. } => {
            matches!(fields::field_name(instr).as_str(), "PAGE" | "NUMPAGES" | "SECTIONPAGES" | "SECTION" | "PAGEREF")
        }
        _ => false,
    })
}

/// Does `p` show a PAGE field, whose result is the number of the page it lands on?
fn has_page_number_fields(p: &Paragraph) -> bool {
    p.objects.iter().any(|o| matches!(o, InlineObject::Field { instr, .. } if fields::field_name(instr) == "PAGE"))
}

/// Does `p` show a page total, which depends on how many pages the document has?
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
    /// Count each section on every page containing its body, including a shared continuous
    /// start page. The page owner still selects headers and footers; empty sections count one.
    fn of(pages: &[Page], sections: &[(usize, &SectionProps)]) -> Totals {
        let mut per = vec![0u32; sections.len()];
        for page in pages {
            let mut present = HashSet::from([page.section]);
            for item in &page.items {
                let path = match item {
                    Placed::Lines { story: StoryRef::Body, path, .. }
                    | Placed::Image { story: StoryRef::Body, path, .. }
                    | Placed::Graphic { story: StoryRef::Body, path, .. }
                    | Placed::Object { story: StoryRef::Body, path, .. }
                    | Placed::Cell { story: StoryRef::Body, table: path, .. } => path,
                    _ => continue,
                };
                let Some(&block) = path.0.first() else { continue };
                present.insert(sections.partition_point(|(end, _)| *end < block as usize));
            }
            for section in present {
                if let Some(n) = per.get_mut(section) {
                    *n = n.saturating_add(1);
                }
            }
        }
        for total in &mut per {
            *total = (*total).max(1);
        }
        Self { pages: u32::try_from(pages.len()).unwrap_or(u32::MAX).max(1), sections: per }
    }
    fn section(&self, i: usize) -> u32 {
        self.sections.get(i).copied().unwrap_or(1)
    }
}

/// The most extra layout passes for page totals in the body: a total that changes the page
/// count (its digits pushing text to another page) is tried again, this many times at most.
const MAX_TOTAL_PASSES: usize = 3;

/// A layout pass: finished, or to be done again with the page totals it found, or with the
/// floats placed above their anchors it found ([`EarlyFloats`]).
enum Pass {
    Done(DocLayout, Vec<f32>),
    Again(Totals),
    Rewrap(EarlyFloats),
}
/// Walk state while paginating.
struct Ctx<'a> {
    doc: &'a Document,
    cache: &'a mut LayoutCache,
    opts: &'a LayoutOptions,
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
    /// Whether tracked deletions are left out in the story being laid out
    /// ([`LayoutOptions::hides_deleted`]): the body's, or the story of the [`layout_box`] at work.
    hide_deleted: HideDeleted,
    /// Whether body text (or its notes) laid out so far shows a page total (`NUMPAGES`,
    /// `SECTIONPAGES`), which is only known once the whole body is paginated.
    uses_totals: bool,
    /// Whether to note `uses_totals` now (off while laying out headers and footers).
    track_totals: bool,
    /// The width a top-level table's percentage width is a share of, when it isn't the box's:
    /// an inherited header or footer's tables follow the section that defines it.
    pct_base: Option<f32>,
    /// Empty body paragraphs with hidden marks or continuous section breaks, in order:
    /// they take no room (see [`place_para`]).
    quiet_marks: Vec<usize>,
    /// Whether even pages are mirrored (mirrored margins, on pages): see [`mirror_even_pages`].
    mirror: bool,
    /// Cache-key hashes of the table-style text formatting cells were laid out with
    /// ([`Ctx::table_key`]).
    table_keys: Vec<(para::CellText, u64)>,
    /// While a header or footer with tables is laid out: its page, for floats in its cells placed
    /// on the page (see [`with_pin`]).
    pin: Option<Pin>,
    footer_wraps: HashMap<(usize, u32, bool), Vec<WrapArea>>,
    header_tables: HashMap<(usize, u32, bool), Vec<Rect>>,
    /// Heights of text boxes that resize to fit their text, by (story, width), worked out once
    /// per layout ([`textbox::fitted_height`]).
    fitted: HashMap<(u32, u32), Option<f32>>,
    /// Each body float's latest placement: on which page and with what wrap area, when it is
    /// above its anchor paragraph with text before it ([`EarlyFloats`]).
    floats_above: HashMap<(usize, usize), Option<(usize, WrapArea)>>,
}

/// A header or footer (`story`) laid out for page `number` of a section (`sect`, `mirror`): its
/// table cells' floats placed on the page rather than in their cell ([`Float::outside_cell`],
/// Word's picture watermark in a header that starts with a table), drawn behind the header's
/// text (`behind`) or in front, in page coordinates. Once each (`seen`): a cell may be laid out
/// more than once.
struct Pin {
    story: StoryRef,
    sect: SectionProps,
    number: u32,
    mirror: bool,
    behind: Vec<Placed>,
    front: Vec<Placed>,
    seen: HashSet<(Vec<u32>, usize)>,
}

/// Whether a float's position on both axes is measured on the page (from its edges or margins),
/// not from its column, paragraph, character or line: one that can be placed on the page from
/// anywhere.
fn page_anchored(f: &Float) -> bool {
    use Anchor::*;
    matches!(f.h_rel, Page | Margin | LeftMargin | RightMargin | InsideMargin | OutsideMargin)
        && matches!(f.v_rel, Page | Margin | TopMargin | BottomMargin | InsideMargin | OutsideMargin)
}

/// Run `f`, a layout of header or footer `id` for page `number` of `sect`, with the floats in its
/// table cells that are placed on the page put there (see [`Pin`]). Returns `f`'s result and those
/// floats' items, in page coordinates: behind the header's text, and in front.
fn with_pin<T>(ctx: &mut Ctx, id: u32, sect: &SectionProps, number: u32, f: impl FnOnce(&mut Ctx) -> T) -> (T, Vec<Placed>, Vec<Placed>) {
    let tables = ctx.doc.parts.get(&id).is_some_and(|p| p.blocks.iter().any(|b| matches!(**b, Block::Table(_))));
    if !tables {
        return (f(ctx), Vec::new(), Vec::new());
    }
    let pin = Pin {
        story: StoryRef::Part(id),
        sect: sect.clone(),
        number,
        mirror: ctx.mirror,
        behind: Vec::new(),
        front: Vec::new(),
        seen: HashSet::new(),
    };
    let outer = ctx.pin.replace(pin);
    let out = f(ctx);
    let pin = std::mem::replace(&mut ctx.pin, outer);
    let (behind, front) = pin.map(|p| (p.behind, p.front)).unwrap_or_default();
    (out, behind, front)
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
            hide_deleted: Default::default(),
            table: None,
            proofing: None,
            typing_word: None,
            numeral: self.opts.numeral,
            exclusions: &[],
            eq_number: 0,
            read_line_spacing: None,
            grid: para::Grid::default(),
            fit_objects: false,
            fitted: &[],
        };
        let pl = Arc::new(para::layout_para(&p, &env));
        self.numbers.insert(n, pl.clone());
        pl
    }

    /// The list label of `p`, advancing the list counters: call it once per paragraph, in
    /// document order (only for paragraphs that will show).
    /// Whether `p` gets a list label ([`Ctx::next_label`] is `Some`), without counting it.
    fn has_label(&self, p: &Paragraph) -> bool {
        p.props
            .numbering
            .or_else(|| self.doc.styles.resolve_para(&p.props).numbering)
            .filter(|n| n.num != 0)
            .is_some_and(|n| self.doc.numbering.shows_label(n.num, n.level))
    }
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
        let fitted = self.fitted_boxes(p);
        let (key, eq_number) = self.para_key(p, width, table, exclusions, label.as_ref(), &fitted);
        self.cache.used.insert(key.clone());
        if let Some(pl) = self.cache.paras.get(&key) {
            self.cache.hits += 1;
            return pl.clone();
        }
        self.cache.misses += 1;
        let typing = key.typing;
        let pl = Arc::new(para::layout_para(p, &self.para_env(width, label, table, exclusions, eq_number, typing, &fitted)));
        self.cache.paras.insert(key, pl.clone());
        pl
    }

    /// What `measure` reads from `p` laid out as [`Ctx::para_labelled`] would, remembering only
    /// the result: a table's columns measure every cell once at an unbounded width, and keeping
    /// those layouts would double what a long table holds (and walk them all again on each pass).
    fn para_measured(
        &mut self,
        p: &Paragraph,
        width: f32,
        table: Option<&para::CellText>,
        label: Option<(String, Level)>,
        measure: impl FnOnce(&ParaLayout) -> (f32, f32),
    ) -> (f32, f32) {
        let fitted = self.fitted_boxes(p);
        let (key, eq_number) = self.para_key(p, width, table, &[], label.as_ref(), &fitted);
        self.cache.used.insert(key.clone());
        if let Some(m) = self.cache.measures.get(&key) {
            self.cache.hits += 1;
            return *m;
        }
        self.cache.misses += 1;
        let typing = key.typing;
        let m = measure(&para::layout_para(p, &self.para_env(width, label, table, &[], eq_number, typing, &fitted)));
        self.cache.measures.insert(key, m);
        m
    }

    /// The heights of `p`'s inline text boxes that resize to fit their text, by object index
    /// ([`para::ParaEnv::fitted`]).
    fn fitted_boxes(&mut self, p: &Paragraph) -> Vec<(usize, f32)> {
        let mut out = Vec::new();
        for (k, o) in p.objects.iter().enumerate() {
            let InlineObject::Shape { story: Some(id), w, float, .. } = o else { continue };
            if float.wrap != Wrap::Inline || float.rot != 0.0 {
                continue;
            }
            let key = (*id, w.to_bits());
            let h = match self.fitted.get(&key) {
                Some(h) => *h,
                None => {
                    let h = textbox::fitted_height(self, *id, *w);
                    self.fitted.insert(key, h);
                    h
                }
            };
            if let Some(h) = h {
                out.push((k, h));
            }
        }
        out
    }

    /// The cache key of `p` laid out with these inputs, and its first automatic equation number
    /// (advancing the count past its equations).
    fn para_key(
        &mut self,
        p: &Paragraph,
        width: f32,
        table: Option<&para::CellText>,
        exclusions: &[para::Exclusion],
        label: Option<&(String, Level)>,
        fitted: &[(usize, f32)],
    ) -> (Key, u32) {
        let page = if has_page_fields(p) { Some(self.fields.page_key()) } else { None };
        if self.track_totals && !self.uses_totals && has_total_fields(p) {
            self.uses_totals = true;
        }
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
            label: label.map(|(t, l)| format!("{t}|{}|{}|{:?}|{:?}|{:?}|{:?}", l.indent, l.hanging, l.suffix, l.align, l.tab, l.chr)),
            page_from: if page.is_some() && !self.fields.page_from.is_empty() { hash_of(&self.fields.page_from) } else { 0 },
            page,
            table: table.map(|t| self.table_key(t)).unwrap_or(0),
            notes: if p.objects.iter().any(|o| matches!(o, InlineObject::NoteRef { .. })) { self.notes_hash } else { 0 },
            excl: if exclusions.is_empty() { 0 } else { hash_of(&format!("{exclusions:?}")) },
            eq: if eq_here > 0 { eq_number } else { 0 },
            read_spacing: if self.opts.view == ViewMode::Read { self.opts.read_line_spacing.map(f32::to_bits) } else { None },
            grid: self.grid.key(),
            fit: self.boxes.inside(),
            fitted: if fitted.is_empty() { 0 } else { hash_of(&format!("{fitted:?}")) },
            hide_deleted: self.hide_deleted,
            sref: if self.fields.styleref.is_some() && styleref::has_styleref(p) { self.fields.styleref_key(p) } else { 0 },
            typing: self.opts.typing_word.filter(|t| t.rev == p.rev).map(|t| (t.start, t.end)),
        };
        (key, eq_number)
    }

    /// The cache-key hash of a cell's table-style text formatting. Hashing it means formatting it
    /// as text, which a long table would otherwise do for each of its paragraphs on every pass; a
    /// document's tables only give their text a few different formats.
    fn table_key(&mut self, t: &para::CellText) -> u64 {
        if let Some((_, h)) = self.table_keys.iter().find(|(c, _)| c == t) {
            return *h;
        }
        let h = hash_of(&format!("{t:?}"));
        if self.table_keys.len() >= 64 {
            self.table_keys.clear();
        }
        self.table_keys.push((t.clone(), h));
        h
    }

    fn para_env<'b>(
        &'b self,
        width: f32,
        label: Option<(String, Level)>,
        table: Option<&'b para::CellText>,
        exclusions: &'b [para::Exclusion],
        eq_number: u32,
        typing_word: Option<(usize, usize)>,
        fitted: &'b [(usize, f32)],
    ) -> para::ParaEnv<'b> {
        para::ParaEnv {
            doc: self.doc,
            width,
            label,
            fields: &self.fields,
            show_hidden: self.opts.show_hidden,
            field_codes: self.opts.field_codes,
            hide_deleted: self.hide_deleted,
            table,
            proofing: self.opts.proofing.then_some(self.opts.proof),
            typing_word,
            numeral: self.opts.numeral,
            exclusions,
            eq_number,
            read_line_spacing: if self.opts.view == ViewMode::Read { self.opts.read_line_spacing } else { None },
            grid: self.grid,
            fit_objects: self.boxes.inside(),
            fitted,
        }
    }
}

/// Which of `p`'s objects (by index) the layout leaves out ([`para::left_out`]): anchored in hidden
/// text unless `show_hidden`, in a tracked deletion with `hide_deleted`, or a hidden drawing
/// (`Float::hidden`, whatever is shown). `table_chr` is the
/// cell's table-style character formatting ([`para::CellText::chr`]), resolved as the paragraph is
/// laid out with (`StyleSheet::resolve_char_in`).
fn left_out_objects(
    doc: &Document,
    p: &Paragraph,
    table_chr: Option<&CharProps>,
    show_hidden: bool,
    hide_deleted: HideDeleted,
) -> impl Fn(usize) -> bool {
    let mut left = Vec::new();
    // No text left out with hidden text shown and markup on (hidden drawings still are, below).
    if (hide_deleted.any() || !show_hidden) && !p.objects.is_empty() {
        let style = p.props.style.as_deref();
        let is_left = |c: &CharProps| {
            let rc = doc.styles.resolve_char_in(style, table_chr, c);
            para::left_out(doc, &rc, show_hidden, hide_deleted)
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
    // A hidden drawing is left out whatever is shown (Word shows it in no view).
    move |k| left.get(k).copied().unwrap_or(false) || p.objects.get(k).and_then(InlineObject::frame).is_some_and(|(_, _, f)| f.hidden)
}

/// Footnote and endnote numbers (each kind counted separately), in document order, as each
/// section's options say: starting at its "start at", numbered on, or starting again in each
/// section or (footnotes) on each page. `pages` gives the page each footnote was placed on by an
/// earlier pass (empty: numbered on). With `hide_deleted`, notes whose reference mark is a
/// tracked deletion are left out, as in the final text.
fn note_info(doc: &Document, hide_deleted: HideDeleted, pages: &HashMap<u32, usize>) -> HashMap<u32, fields::NoteNum> {
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
    // its number, as in Word; a deleted one (or one in a deleted or hidden text box) does not,
    // and `para_notes` doesn't place its note either.
    let mut cur: Option<(usize, Vec<bool>)> = None;
    doc.objects_in_reading_order_by_block(&mut |block, p, k| {
        let key = p as *const Paragraph as usize;
        if cur.as_ref().is_none_or(|(at, _)| *at != key) {
            let gone = left_out_objects(doc, p, None, true, hide_deleted);
            cur = Some((key, (0..p.objects.len()).map(gone).collect()));
        }
        if cur.as_ref().is_some_and(|(_, gone)| gone.get(k).copied().unwrap_or(false)) {
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
fn note_numbers(doc: &Document, hide_deleted: HideDeleted) -> HashMap<u32, u32> {
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

/// Cell identity is independent of optional table-style formatting.
enum BoxContext<'a> {
    Other,
    Cell(Option<&'a para::CellText>),
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
    context: BoxContext<'_>,
    depth: usize,
    frame: Option<PageFrame>,
) -> BoxLayout {
    // Deletions in balloons leave the body's text only: a header, note or text box keeps its own
    // inline (restored for the story around this box when it's done).
    let outer_hide = std::mem::replace(&mut ctx.hide_deleted, ctx.opts.hides_deleted(story));
    let empty_cell = para::CellText::default();
    let (table, in_cell) = match context {
        BoxContext::Other => (None, false),
        BoxContext::Cell(table) => (Some(table.unwrap_or(&empty_cell)), true),
    };
    // Word 16.113.3: the override applies in modes 11/12/14, but not mode 15.
    let allow_same_style_space =
        in_cell && ctx.doc.settings.allow_space_of_same_style_in_table && matches!(ctx.doc.settings.compat_mode, 11 | 12 | 14);
    let mut items = Vec::new();
    // Pictures behind the text go first; `behind` counts them.
    let mut behind = 0;
    // Wrap areas of the box's floating objects.
    let mut excl: Vec<WrapArea> = Vec::new();
    // Areas of the box's floating tables, which other floating tables may have to avoid.
    let mut float_tables: Vec<Rect> = Vec::new();
    // Deepest a floating picture or shape that takes part in the text flow reaches, so the box
    // (a table cell especially) grows to hold it instead of letting it hang past the box's own
    // bottom edge (#3108). Behind-text and in-front-of-text objects don't: a page-anchored logo or
    // watermark behind a cell's text must not stretch its row (see `align_h` in table.rs).
    let mut float_objects_bottom = 0.0f32;
    let mut table_areas = Vec::new();
    let mut page_table_wraps = Vec::new();
    let mut y = 0.0f32;
    let mut prev_after = 0.0f32;
    let mut prev_after_auto = false;
    let mut prev_listed = false;
    let mut prev_style: Option<(String, bool)> = None;
    let mut last_top = 0.0f32;
    // Word has no frames in a text box: a framed paragraph there (its style's frame, usually)
    // runs with the box's text (#2357).
    let frames = !matches!(story, StoryRef::Part(id) if ctx.doc.parts.get(&id).is_some_and(|p| p.kind == wordcraft_doc::PartKind::TextBox));
    // The bottom of the last paragraph's lines.
    let mut prev_end = 0.0f32;
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
                if let Some(fr) = pl.rp.frame.clone().filter(|_| frames) {
                    let fw = fr.w.unwrap_or_else(|| natural_width(&pl)).clamp(1.0, 1584.0);
                    let pf = if (fw - width).abs() > 0.01 { ctx.para_labelled(p, fw, table, &[], label) } else { pl };
                    let h = match fr.h {
                        Some(h) if fr.h_exact => h,
                        Some(h) => h.max(pf.height),
                        None => pf.height,
                    };
                    let float = frame_float(&fr);
                    let r = float_rect(frame, (0.0, width), y, fw, h, &float);
                    push_para(&mut items, story, &path, &pf, 0, pf.lines.len(), r.x, r.y, fw, Edges::own(&pf, 0, pf.lines.len()));
                    excl.extend(wrap_area(r, &float, None));
                    float_tables.push(r);
                    continue;
                }
                // A legacy table compatibility flag lets same-style paragraphs in a table keep their
                // spacing (`allow_same_style_space`).
                let ctxl = pl.rp.contextual_spacing && !allow_same_style_space;
                let mut before = pl.rp.space_before;
                if let Some((style, previous_contextual)) = &prev_style
                    && *style == pl.rp.style
                    && !allow_same_style_space
                {
                    if *previous_contextual {
                        y -= prev_after;
                        before = (before - prev_after).max(0.0);
                        prev_after = 0.0;
                    }
                    if ctxl {
                        before = 0.0;
                    }
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
                } else if i > 0 && before > 0.0 && prev_after > 0.0 && !ctx.doc.settings.sum_paragraph_spacing {
                    before = (before - prev_after).max(0.0);
                }
                y += if i == 0 { before } else { before.max(0.0) };
                // Floating objects anchored here: place them, then wrap the text around them.
                // One left out of the layout (hidden, or deleted in the final text) takes no room.
                let mut floats = HashMap::new();
                // A header or footer cell's floats placed on the page: no room taken in the cell.
                let mut pinned = HashMap::new();
                let left_out = left_out_objects(ctx.doc, p, table.map(|t| &t.chr), ctx.opts.show_hidden, ctx.hide_deleted);
                for (oi, o) in p.objects.iter().enumerate() {
                    let Some((w, h, float)) = floating(o) else { continue };
                    if left_out(oi) {
                        continue;
                    }
                    if let Some(pin) = ctx.pin.as_ref().filter(|pin| in_cell && pin.story == story && float.outside_cell && page_anchored(float)) {
                        let page = PageFrame { sect: &pin.sect, origin: (0.0, 0.0), number: pin.number, mirror: pin.mirror };
                        pinned.insert(oi, float_rect(Some(page), (0.0, width), y, w, h, float));
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
                // Bordered paragraphs: room for their borders, those of a group joined in one box.
                let cell = table.map(|t| &t.para);
                let (above, below) = (joined(ctx.doc, blocks, i, i.checked_sub(1), cell), joined(ctx.doc, blocks, i, Some(i + 1), cell));
                let mut edges = Edges::own(&pl, 0, pl.lines.len());
                if above {
                    edges.top = 0.0;
                }
                if below {
                    edges.bottom = 0.0;
                }
                let lead = pl.lines.first().map_or(0.0, |l| l.top.max(0.0)) + edges.top;
                if above {
                    edges.up = (y + lead - prev_end).max(0.0);
                }
                prev_end = push_para(&mut items, story, &path, &pl, 0, pl.lines.len(), 0.0, y + lead, width, edges);
                // Pictures, shapes and text boxes: drawn, their areas and text boxes' text.
                if !p.objects.is_empty() {
                    let at = ObjFrame { page: frame, col: (0.0, width), para_y: y, table_chr: table.map(|t| &t.chr), pinned: Some(&pinned) };
                    let (back, front) = place_objects(ctx, story, &path, p, &pl, (0, pl.lines.len()), (0.0, y + lead), &at, &floats, depth);
                    for placed in back.iter().chain(front.iter()) {
                        if let Placed::Object { rect, spin, wrap, .. } = placed
                            && !matches!(wrap, Wrap::Inline | Wrap::BehindText | Wrap::InFrontOfText)
                        {
                            float_objects_bottom = float_objects_bottom.max(spin.bounds(*rect).bottom());
                        }
                    }
                    let at = behind.min(items.len());
                    behind += back.len();
                    items.splice(at..at, back);
                    items.extend(front);
                }
                // Automatic space after: none after the last paragraph of a cell or box.
                let after = if pl.rp.after_auto && i + 1 == blocks.len() { 0.0 } else { pl.rp.space_after };
                y += edges.top + pl.height + edges.bottom + after;
                prev_after = after;
                prev_after_auto = pl.rp.after_auto;
                prev_listed = listed;
                prev_style = Some((pl.rp.style.clone(), ctxl));
            }
            Block::Table(t) => {
                if depth > 8 {
                    continue;
                }
                // Page tables do not contribute to the footer text's intrinsic height.
                if frame.is_none()
                    && !in_cell
                    && matches!(story, StoryRef::Part(id) if ctx.doc.parts.get(&id).is_some_and(|p| p.kind == wordcraft_doc::PartKind::Footer))
                    && t.props.float.is_some_and(|f| f.v_rel == Anchor::Page)
                {
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
                        table_areas.push(area);
                        if f.v_rel == Anchor::Page {
                            page_table_wraps.push((area, false, None, WrapText::BothSides));
                        }
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
    let floats_bottom = float_tables.iter().map(|r| r.bottom()).fold(float_objects_bottom, f32::max);
    ctx.hide_deleted = outer_hide;
    BoxLayout { items, height: y.max(0.0), floats_bottom, last_top: last_top.max(0.0), table_areas, page_table_wraps }
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
    table_areas: Vec<Rect>,
    page_table_wraps: Vec<WrapArea>,
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
    // Across, as laid out before an even page is mirrored (see `float_rect`).
    let page_x = |fr: &PageFrame| {
        let (left, _, shift) = fr.sides();
        match f.h_rel {
            Anchor::Page => Some((-shift, fr.sect.page_w)),
            Anchor::Margin => Some((left - shift, fr.sect.text_width())),
            _ => None,
        }
    };
    let page_y = |s: &SectionProps| match f.v_rel {
        Anchor::Page => Some((0.0, s.page_h)),
        Anchor::Margin => Some((s.margin_top, s.text_height())),
        _ => None,
    };
    let h_area = frame.and_then(|fr| page_x(&fr).map(|(a, e)| (a - fr.origin.0, e))).unwrap_or((0.0, width));
    // In a cell an offset from the page or margin has nothing to measure from: the table stands
    // where the text is.
    let (v_area, y_off) = match frame.and_then(|fr| page_y(fr.sect).map(|(a, e)| (a - fr.origin.1, e))) {
        Some(area) => (area, f.y),
        None if matches!(f.v_rel, Anchor::Page | Anchor::Margin) => ((y, 0.0), 0.0),
        None => ((y, 0.0), f.y),
    };
    // Nor is there an area to align in: the table starts where the text is.
    let v_align = if frame.is_none() && matches!(f.v_rel, Anchor::Page | Anchor::Margin) { None } else { f.v_align };
    let x = align_in(h_area, f.x, w, PageFrame::h_align(frame.as_ref(), f.h_align)) - if f.h_align.is_none() { legacy } else { 0.0 };
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

/// Word joins neighbouring paragraphs with the same borders and indents into one box: the top
/// border goes above the first of them only, the bottom one below the last, and the shading and
/// side borders run on through the space between them (`table`: the cell's paragraph properties).
fn border_group(doc: &Document, a: &Paragraph, b: &Paragraph, table: Option<&wordcraft_doc::props::ParaProps>) -> bool {
    let (a, b) = (doc.styles.resolve_para_in(&a.props, table), doc.styles.resolve_para_in(&b.props, table));
    a.borders.is_some() && a.borders == b.borders && a.indent_left == b.indent_left && a.indent_right == b.indent_right
}

/// Whether paragraph `i` of `blocks` and the one at `j` are of one border group.
fn joined(doc: &Document, blocks: &Blocks, i: usize, j: Option<usize>, table: Option<&wordcraft_doc::props::ParaProps>) -> bool {
    match (blocks.get(i).map(|b| &**b), j.and_then(|j| blocks.get(j)).map(|b| &**b)) {
        (Some(Block::Para(a)), Some(Block::Para(b))) => border_group(doc, a, b, table),
        _ => false,
    }
}

/// The room a paragraph's top or bottom border takes in the text flow: its distance from the
/// text plus its width (Word).
fn border_room(b: Option<Border>) -> f32 {
    b.filter(Border::is_visible).map_or(0.0, |b| b.space + b.width)
}

/// How a piece of a paragraph meets the paragraphs around it: the room taken by its top and
/// bottom borders (0: none drawn there — the paragraph goes on from or into one of its border
/// group, or it is split there) and how far its shading and side borders reach up to join the
/// paragraph above in its group.
#[derive(Clone, Copy, Default)]
struct Edges {
    top: f32,
    bottom: f32,
    up: f32,
}

impl Edges {
    /// A paragraph on its own: its top and bottom borders at its ends.
    fn own(pl: &ParaLayout, l0: usize, l1: usize) -> Edges {
        let b = pl.rp.borders.unwrap_or_default();
        Edges { top: if l0 == 0 { border_room(b.top) } else { 0.0 }, bottom: if l1 >= pl.lines.len() { border_room(b.bottom) } else { 0.0 }, up: 0.0 }
    }
}

/// Paragraph lines plus their shading and borders; returns the bottom of the lines. The
/// shading fills the box out to the borders, the room they take included (Word).
#[allow(clippy::too_many_arguments)]
fn push_para(
    items: &mut Vec<Placed>,
    story: StoryRef,
    path: &[u32],
    pl: &Arc<ParaLayout>,
    l0: usize,
    l1: usize,
    x: f32,
    y: f32,
    width: f32,
    e: Edges,
) -> f32 {
    let (Some(first), Some(last)) = (pl.lines.get(l0), l1.checked_sub(1).and_then(|k| pl.lines.get(k))) else {
        return y;
    };
    let h = last.top + last.height - first.top;
    // Indents are logical: a right-to-left paragraph's start indent (and hanging indent) is on the right.
    let start = pl.rp.indent_left.min(pl.rp.indent_left + pl.rp.indent_first);
    let (li, ri) = if pl.rp.bidi { (pl.rp.indent_right, start) } else { (start, pl.rp.indent_right) };
    let left = x + li;
    let right = x + width - ri;
    let (top, bottom) = (y - e.top - e.up, y + h + e.bottom);
    if let Some(c) = pl.rp.shading {
        let pad = pl.rp.borders.as_ref().map(|b| b.left.map(|l| l.space).unwrap_or(4.0)).unwrap_or(0.0);
        items.push(Placed::Fill { rect: Rect::new(left - pad, top, right - left + pad * 2.0, bottom - top), color: c });
    }
    items.push(Placed::Lines { story, path: Path(path.to_vec()), para: pl.clone(), l0, l1, x, y, turn: TextDirection::Horizontal });
    if let Some(b) = &pl.rp.borders {
        let sp = |o: &Option<Border>| o.map(|b| b.space).unwrap_or(0.0);
        let (lx, rx) = (left - sp(&b.left), right + sp(&b.right));
        // The top and bottom lines lie beyond their distance from the text.
        if let Some(t) = b.top.filter(|t| e.top > 0.0 && t.is_visible()) {
            let ty = y - t.space - t.width / 2.0;
            items.push(Placed::Rule { x0: lx, y0: ty, x1: rx, y1: ty, border: t });
        }
        if let Some(t) = b.bottom.filter(|t| e.bottom > 0.0 && t.is_visible()) {
            let by = y + h + t.space + t.width / 2.0;
            items.push(Placed::Rule { x0: lx, y0: by, x1: rx, y1: by, border: t });
        }
        if let Some(t) = b.left.filter(Border::is_visible) {
            items.push(Placed::Rule { x0: lx, y0: top, x1: lx, y1: bottom, border: t });
        }
        if let Some(t) = b.right.filter(Border::is_visible) {
            items.push(Placed::Rule { x0: rx, y0: top, x1: rx, y1: bottom, border: t });
        }
    }
    y + h
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
    /// The next section starts on a new page: this section's closing mark, when empty, needs no
    /// room (Word leaves it on a full page rather than starting a page for it).
    next_starts_page: bool,
    /// The number the next page takes instead of counting on: a section restarting its
    /// numbering, possibly at 0 (a cover page numbered 0).
    restart_at: Option<u32>,
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
    /// Where each top-level block after the first starts on the current page: its first item's
    /// index counted after the items behind the text (justified vertical alignment spreads the
    /// blocks apart there).
    block_marks: Vec<usize>,
    /// The vertical alignment of the section the current page started in: a page finished when
    /// the next section starts is aligned as its own section asks, not the next one.
    page_valign: wordcraft_doc::section::PageVAlign,
    /// The document's own footnote separators, laid out for the section's text width.
    seps: NoteSeps,
    /// Where the last paragraph's lines ended: page count, column and y.
    para_end: Option<(usize, usize, f32)>,
    /// Per page: how many items were inserted at the front of its list (behind the text), so a
    /// [`Snap`] can take them out again.
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
    /// Top of the footer of the pages being started: a footer reaching above the bottom margin
    /// pushes the body up (`f32::MAX`: no footer).
    /// The wrap areas pages start with ([`EarlyFloats`]) ...
    early: &'a EarlyFloats,
    /// ... and how many of the current page's `excl` they are (its first ones).
    seeded: usize,
    /// No layout pass follows for floats found above their anchors: each is kept clear of the
    /// text laid out before it ([`settle_early`]).
    settle: bool,
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
    let bl = layout_box(ctx, DECOR_STORY, blocks, &[], width, BoxContext::Other, 0, None);
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
    /// A top-level block starts here (see `block_marks`).
    fn mark_block(&mut self) {
        if let Some(pg) = self.pages.last() {
            let behind = self.behind.last().copied().unwrap_or(0);
            self.block_marks.push(pg.items.len().saturating_sub(behind));
        }
    }
    /// Justified vertical alignment: share the room left below the page's text out equally
    /// between the gaps of its blocks, so the last line sits on the bottom margin (above the
    /// footnotes). A page with one block, or several columns, stays at the top.
    fn justify_page(&mut self) {
        if self.web || self.cols.len() > 1 {
            return;
        }
        let behind = self.behind.last().copied().unwrap_or(0);
        let first = self.page_items_start;
        // The room is measured below the last paragraph's space after: Word for Mac 16.113.4 keeps
        // it inside the justified height (72 pt after leaves the last line 72 pt above the margin).
        let space = (self.orig_bottom - self.notes_h() - self.y).max(0.0);
        let Some(pg) = self.pages.last_mut() else { return };
        let front = pg.items.len().saturating_sub(behind);
        // Blocks that moved on to the next page whole leave a mark at the end: not a gap here.
        let mut cuts: Vec<usize> = self.block_marks.iter().copied().filter(|&m| m > first && m < front).collect();
        cuts.sort_unstable();
        cuts.dedup();
        if cuts.is_empty() || space <= 0.0 {
            return;
        }
        let gap = space / cuts.len() as f32;
        for (i, it) in pg.items.iter_mut().enumerate().skip(behind.saturating_add(first)) {
            let k = cuts.partition_point(|&c| c <= i.saturating_sub(behind));
            if k > 0 {
                it.translate(0.0, gap * k as f32);
            }
        }
    }
    /// Section vertical alignment: move the page's body down (centre/bottom), or spread its
    /// blocks out (justified).
    fn apply_valign(&mut self) {
        let k = match self.page_valign {
            wordcraft_doc::section::PageVAlign::Top => return,
            wordcraft_doc::section::PageVAlign::Justify => return self.justify_page(),
            wordcraft_doc::section::PageVAlign::Center => 0.5,
            wordcraft_doc::section::PageVAlign::Bottom => 1.0,
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
    /// Takes back the page a page break has just started while nothing is on it, so a page start
    /// that follows (a next-page section) doesn't leave it blank: Word keeps the break's mark on
    /// the page before, and starts one page.
    #[allow(dead_code)] // Used by the section-break path once it can distinguish an explicit page break.
    fn take_back_break_page(&mut self) -> bool {
        let empty = self.after_break_mark
            && self.at_top()
            && self.notes.is_empty()
            && self.pages.len() > 1
            && self.pages.last().is_some_and(|p| p.items.len() == self.page_items_start);
        if !empty {
            return false;
        }
        self.pages.pop();
        self.behind.pop();
        self.number = self.number.saturating_sub(1);
        self.after_break_mark = false;
        // The page before was aligned when it was left: nothing on it moves again.
        self.page_items_start = self.pages.last().map_or(0, |p| p.items.len());
        true
    }
    fn new_page(&mut self, first_block: usize, body_top: f32) {
        self.finish_page();
        self.beneath = self.sect_beneath;
        self.excl.clear();
        // Floats placed above their anchors further down this page are known from its start.
        let early = self.early;
        self.excl.extend(early.get(&self.pages.len()).into_iter().flatten().map(|(_, _, a)| a.clone()));
        self.seeded = self.excl.len();
        self.float_tables.clear();
        if self.sect.line_numbers.as_ref().is_some_and(|l| l.restart == wordcraft_doc::section::LineNumberRestart::Page) {
            self.line_no = 0;
        }
        let s = self.sect;
        self.number = self.restart_at.take().unwrap_or_else(|| self.number.saturating_add(1));
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
        self.block_marks.clear();
        self.page_valign = s.valign;
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
        let BoxLayout { items, height, .. } = layout_box(ctx, StoryRef::Part(id), &part.blocks, &[], pb.col_w(), BoxContext::Other, 0, None);
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

const MAX_NOTE_PASSES: usize = 3;

/// Lay out the whole document.
pub fn layout(doc: &Document, cache: &mut LayoutCache, opts: &LayoutOptions) -> DocLayout {
    if opts.view == ViewMode::Outline {
        return layout_outline(doc, cache, opts);
    }
    let t0 = now_ms();
    let per_page = opts.view == ViewMode::Print
        && doc.sections().iter().any(|(_, s)| doc.note_options(s, false).num_restart == wordcraft_doc::section::NoteRestart::EachPage);
    let mut note_pages = HashMap::new();
    let mut note_passes = 1;
    let mut totals = Totals::first_guess(doc.sections().len());
    let mut passes = 0;
    let (mut early, mut rewraps) = (EarlyFloats::new(), 0);
    loop {
        match layout_with_note_pages(doc, cache, opts, &note_pages, &totals, passes < MAX_TOTAL_PASSES, (&early, rewraps < MAX_REWRAP_PASSES)) {
            Pass::Again(found) => {
                totals = found;
                passes += 1;
            }
            Pass::Rewrap(found) => {
                merge_early(&mut early, found);
                rewraps += 1;
            }
            Pass::Done(mut result, _) => {
                if per_page && note_passes < MAX_NOTE_PASSES {
                    let placed = footnote_pages(doc, &result.pages);
                    if placed != note_pages {
                        note_pages = placed;
                        note_passes += 1;
                        continue;
                    }
                }
                result.ms = now_ms() - t0;
                break result;
            }
        }
    }
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
    may_repeat: bool,
    (early, may_rewrap): (&EarlyFloats, bool),
) -> Pass {
    let has_multicolumn_notes = opts.view == ViewMode::Print
        && doc.sections().iter().any(|(_, section)| section.columns.count > 1)
        && doc
            .para_paths(StoryRef::Body)
            .iter()
            .filter_map(|path| doc.para(StoryRef::Body, path))
            .flat_map(|para| &para.objects)
            .any(|object| matches!(object, InlineObject::NoteRef { kind: wordcraft_doc::para::NoteKind::Footnote, .. }));
    if !has_multicolumn_notes {
        return match layout_pass(doc, cache, opts, note_pages, &[], totals, may_repeat, (early, may_rewrap)) {
            Pass::Done(result, _) => Pass::Done(result, Vec::new()),
            other => other,
        };
    }
    let mut reservations = Vec::new();
    let mut last = None;
    let mut high_water: Vec<f32> = Vec::new();
    let mut stable = false;
    for _ in 0..24 {
        let (result, actual) = match layout_pass(doc, cache, opts, note_pages, &reservations, totals, may_repeat, (early, may_rewrap)) {
            Pass::Done(result, actual) => (result, actual),
            other => return other,
        };
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
        for _ in 0..8 {
            let current = high_water.clone();
            let (candidate, actual) = match layout_pass(doc, cache, opts, note_pages, &current, totals, may_repeat, (early, may_rewrap)) {
                Pass::Done(candidate, actual) => (candidate, actual),
                other => return other,
            };
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
    Pass::Done(result, Vec::new())
}

fn layout_pass(
    doc: &Document,
    cache: &mut LayoutCache,
    opts: &LayoutOptions,
    note_pages: &HashMap<u32, usize>,
    reserved_note_heights: &[f32],
    totals: &Totals,
    may_repeat: bool,
    (early, may_rewrap): (&EarlyFloats, bool),
) -> Pass {
    let t0 = now_ms();
    let env = env_hash(doc, opts);
    if env != cache.env {
        cache.paras.clear();
        cache.measures.clear();
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
            filename: opts.filename.clone().unwrap_or_else(|| "".into()),
            filepath: opts.filepath.clone().unwrap_or_else(|| "".into()),
            ..Default::default()
        },
        notes_hash,
        numbers: HashMap::new(),
        eq_count: 0,
        boxes: wordcraft_doc::BoxBudget::default(),
        box_clips: Vec::new(),
        uses_totals: false,
        chains: textbox::Chains::default(),
        grid: para::Grid::default(),
        track_totals: true,
        pct_base: None,
        quiet_marks: Vec::new(),
        mirror: false,
        table_keys: Vec::new(),
        fitted: HashMap::new(),
        header_tables: HashMap::new(),
        footer_wraps: HashMap::new(),
        pin: None,
        floats_above: HashMap::new(),
    };
    let sections = doc.sections();
    let web = matches!(opts.view, ViewMode::Web | ViewMode::Draft | ViewMode::Outline);
    let read = opts.view == ViewMode::Read;
    let reflow = web || read;
    ctx.mirror = doc.settings.mirror_margins && !reflow;
    ctx.quiet_marks = sections
        .windows(2)
        .filter(|w| w[1].1.start == SectionStart::Continuous)
        .map(|w| w[0].0)
        .filter(|end| doc.body.get(*end).and_then(|b| b.as_para()).is_some_and(|p| p.text.is_empty()))
        .collect();
    if !opts.show_hidden {
        ctx.quiet_marks.extend(
            doc.body
                .iter()
                .enumerate()
                .filter_map(|(i, b)| b.as_para().filter(|p| p.mark.hidden == Some(true) && p.plain_text().is_empty()).map(|_| i)),
        );
    }
    ctx.quiet_marks.sort_unstable();
    ctx.quiet_marks.dedup();
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
        web_sect.valign = wordcraft_doc::section::PageVAlign::Top;
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
        next_starts_page: false,
        restart_at: None,
        excl: Vec::new(),
        float_tables: Vec::new(),
        line_no: 0,
        page_items_start: 0,
        page_finished: true,
        block_marks: Vec::new(),
        page_valign: wordcraft_doc::section::PageVAlign::Top,
        seps: NoteSeps::default(),
        para_end: None,
        behind: Vec::new(),
        max_y: 0.0,
        content_bottom: 0.0,
        balance: None,
        col_break_page: None,
        sect_beneath: false,
        beneath: false,
        early,
        seeded: 0,
        settle: !may_rewrap && !reflow,
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
    let mut boxes_discarded = 0usize;
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
            // still describe the page being closed. If that section ended with a page-break-only
            // mark, the mark already opened an empty page; the section start takes that page back.
            if !pb.take_back_break_page() {
                pb.finish_page();
            }
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
        ctx.fields.section = u32::try_from(si).unwrap_or(u32::MAX).saturating_add(1);
        ctx.fields.section_pages = totals.section(si);
        let even_odd = ctx.doc.settings.even_odd_headers;
        let (heads, feet) = (linked_story_ids(&sections, si, true), linked_story_ids(&sections, si, false));
        let hb = |id| defining_width(&sections, si, |s| s.headers, id);
        let fb = |id| defining_width(&sections, si, |s| s.footers, id);
        let body_top = if reflow || turn.is_turned() {
            sect.margin_top
        } else {
            with_pct_base(&mut ctx, hb(heads.default), |c| body_top_for(c, sect, si, &sections, false, even_odd))
        };
        let first_top = if reflow || turn.is_turned() || !sect.title_page {
            body_top
        } else {
            with_pct_base(&mut ctx, hb(heads.first), |c| body_top_for(c, sect, si, &sections, true, even_odd))
        };
        let foot_top =
            if reflow || turn.is_turned() { f32::MAX } else { with_pct_base(&mut ctx, fb(feet.default), |c| foot_top_for(c, sect, feet.default)) };
        let first_foot = if reflow || turn.is_turned() || !sect.title_page {
            foot_top
        } else {
            with_pct_base(&mut ctx, fb(feet.first), |c| foot_top_for(c, sect, feet.first))
        };
        pb.clearance_bottom = if reflow || turn.is_turned() {
            sect.page_h - sect.margin_bottom
        } else {
            body_bottom_for(&mut ctx, sect, si, &sections, even_odd).min(foot_top).min(first_foot)
        };
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
            // A page holds text running one way: a section turning the other way starts a page.
            if let Some(n) = restart {
                pb.restart_at = Some(n);
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
                pb.restart_at = Some(n);
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
        let outline = (opts.view == ViewMode::Outline).then(|| OutlinePass::new(doc, opts.outline.clone().unwrap_or_default()));
        let balance = !web
            && pb.cols.len() > 1
            && sect.valign == wordcraft_doc::section::PageVAlign::Top
            && sections.get(si + 1).is_some_and(|(_, s)| s.start == SectionStart::Continuous);
        pb.col_break_page = None;
        pb.next_starts_page = !web && sections.get(si + 1).is_some_and(|(_, s)| s.start != SectionStart::Continuous);
        let mut from = balance.then(|| (block, Snap::take(&mut pb, &ctx)));
        while block <= last {
            let Some(b) = doc.body.get(block) else { break };
            let before = balance.then(|| (pb.pages.len(), Snap::take(&mut pb, &ctx)));
            pb.mark_block();
            match (&**b, &outline) {
                (Block::Para(p), Some(o)) => place_outline_para(&mut ctx, &mut pb, p, block, o),
                (Block::Table(_), Some(o)) if o.hidden.get(block).copied().unwrap_or(false) => {}
                (Block::Para(p), None) => place_para(&mut ctx, &mut pb, p, block, body_top),
                (Block::Table(t), _) => place_table(&mut ctx, &mut pb, t, block, body_top),
            }
            if let Some((pages, snap)) = before
                && pb.pages.len() != pages
            {
                from = Some((block, snap));
            }
            block += 1;
        }
        if let Some((first, snap)) = from {
            boxes_discarded = boxes_discarded.saturating_add(balance_columns(&mut ctx, &mut pb, (first, block), &snap, body_top));
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
    if !reflow && may_rewrap {
        let mut above: Vec<((usize, usize), usize, WrapArea)> =
            ctx.floats_above.drain().filter_map(|(k, v)| v.map(|(page, area)| (k, page, area))).collect();
        above.sort_by_key(|(k, ..)| *k);
        let mut found = EarlyFloats::new();
        for ((block, oi), page, area) in above {
            found.entry(page).or_default().push((block, oi, area));
        }
        if !early_covered(&found, early) {
            return Pass::Rewrap(found);
        }
    }
    if !reflow {
        let found = Totals::of(&pages, &sections);
        if ctx.uses_totals && may_repeat && found != *totals {
            return Pass::Again(found);
        }
        ctx.track_totals = false;
        // Page references in headers and footers are filled in after pagination, like their page
        // totals: they don't make the body lay out again.
        if !web && headers_footers_refer_to_pages(doc) {
            let marks = bookmark_pages(doc, &pages, &sections);
            ctx.fields.bookmarks_key = marks_key(&marks);
            ctx.fields.bookmark_pages = Arc::new(marks);
        }
        headers_footers(&mut ctx, &mut pages, &sections, &found);
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
    if ctx.cache.paras.len() + ctx.cache.measures.len() > 4 * ctx.cache.used.len().max(1024) {
        let used = std::mem::take(&mut ctx.cache.used);
        ctx.cache.paras.retain(|k, _| used.contains(k));
        ctx.cache.measures.retain(|k, _| used.contains(k));
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
    layout.text_boxes = ctx.boxes.total().saturating_sub(boxes_discarded);
    Pass::Done(layout, note_heights)
}

/// Pagination state saved before a block, to lay the blocks from there out again (balancing
/// columns): the builder without its pages, how far its last page had got, and the walk state.
struct Snap<'a> {
    pb: PageBuilder<'a>,
    pages: usize,
    items: usize,
    behind: usize,
    counters: Counters,
    eq_count: u32,
    page_field: u32,
    boxes: usize,
}

impl<'a> Snap<'a> {
    fn take(pb: &mut PageBuilder<'a>, ctx: &Ctx) -> Self {
        // The pages stay out of the copy: it would cost the whole document per block.
        let (pages, behind) = (std::mem::take(&mut pb.pages), std::mem::take(&mut pb.behind));
        let copy = pb.clone();
        (pb.pages, pb.behind) = (pages, behind);
        Snap {
            pb: copy,
            pages: pb.pages.len(),
            items: pb.pages.last().map_or(0, |p| p.items.len()),
            behind: pb.behind.last().copied().unwrap_or(0),
            counters: ctx.counters.clone(),
            eq_count: ctx.eq_count,
            page_field: ctx.fields.page,
            boxes: ctx.boxes.total(),
        }
    }

    /// Go back to this state: drop the pages added since and what was added to the last one
    /// (appended, or inserted at the front behind the text).
    fn restore(&self, pb: &mut PageBuilder<'a>, ctx: &mut Ctx) {
        let (mut pages, mut behind) = (std::mem::take(&mut pb.pages), std::mem::take(&mut pb.behind));
        pages.truncate(self.pages);
        behind.truncate(self.pages);
        if let Some(pg) = pages.last_mut() {
            let k = behind.last().copied().unwrap_or(0).saturating_sub(self.behind).min(pg.items.len());
            pg.items.drain(..k);
            pg.items.truncate(self.items);
        }
        if let Some(b) = behind.last_mut() {
            *b = self.behind;
        }
        *pb = self.pb.clone();
        (pb.pages, pb.behind) = (pages, behind);
        ctx.counters = self.counters.clone();
        ctx.eq_count = self.eq_count;
        ctx.fields.page = self.page_field;
    }
}

/// One balancing pass: blocks `first..end` laid out again from `snap` with the columns on `page`
/// ending `h` below `top`; true if the page's text fits them.
fn balance_pass<'a>(
    ctx: &mut Ctx,
    pb: &mut PageBuilder<'a>,
    snap: &Snap<'a>,
    (first, end): (usize, usize),
    (page, top, h): (usize, f32, f32),
    body_top: f32,
) -> bool {
    snap.restore(pb, ctx);
    pb.balance = Some((page, h));
    pb.cap_columns();
    for block in first..end {
        match ctx.doc.body.get(block).map(|b| &**b) {
            Some(Block::Para(p)) => place_para(ctx, pb, p, block, body_top),
            Some(Block::Table(t)) => place_table(ctx, pb, t, block, body_top),
            None => break,
        }
        // Spilled onto another page: too short (and no need to lay out the rest).
        if pb.pages.len() > page + 1 {
            break;
        }
    }
    pb.balance = None;
    pb.cap_columns();
    pb.pages.len() == page + 1 && pb.content_bottom <= top + h + 0.5
}

/// Balancing lays out at most this many pages again per pass (a block reaching the last page
/// from further back is left unbalanced), so its cost stays a small multiple of one page.
const BALANCE_MAX_PAGES: usize = 16;
/// Passes of the binary search on the column height (enough for well under a point).
const BALANCE_PASSES: usize = 12;

/// Balance the columns on the last page of a section followed by a continuous break, as Word
/// does: lay blocks `first..end` out again from `snap` (taken before `first`) with the shortest
/// columns that still hold that page's text — a binary search on the column height, where a
/// height fits when nothing moves to another page and nothing (an unsplittable row, a line
/// taller than the column) runs past it. Keep-together, keep-with-next and widow/orphan control
/// apply as usual, so the columns may come out a little uneven. Not balanced: a page with an
/// explicit column break, which says where the columns end. Returns the text boxes laid out in the
/// passes thrown away.
fn balance_columns<'a>(ctx: &mut Ctx, pb: &mut PageBuilder<'a>, blocks: (usize, usize), snap: &Snap<'a>, body_top: f32) -> usize {
    let page = pb.pages.len().saturating_sub(1);
    let ncols = pb.cols.len().max(1) as f32;
    let top = pb.top;
    let full = pb.uncapped_bottom() - top;
    let used = pb.content_bottom.min(pb.uncapped_bottom()) - top;
    if pb.col_break_page == Some(page)
        || snap.pages == 0
        || (page + 1).saturating_sub(snap.pages) > BALANCE_MAX_PAGES
        || used <= 1.0
        || full <= 1.0
        || pb.cols.len() < 2
    {
        return 0;
    }
    let pass = |ctx: &mut Ctx, pb: &mut PageBuilder<'a>, h: f32| balance_pass(ctx, pb, snap, blocks, (page, top, h), body_top);
    // The text on the page fills `ncols` columns at least `used / ncols` high; the original layout
    // (`full`) fits, and so, nearly always, do columns as high as its tallest one.
    let mut lo = (used / ncols - 1.0).max(0.0);
    let mut hi = full;
    let mut last = None;
    let discard = |ctx: &Ctx| ctx.boxes.total().saturating_sub(snap.boxes);
    let mut thrown = discard(ctx);
    if used < full - 0.5 {
        let ok = pass(ctx, pb, used);
        if ok {
            hi = used;
        }
        last = Some((used, ok));
    }
    for _ in 0..BALANCE_PASSES {
        if hi - lo <= 1.0 {
            break;
        }
        let mid = (lo + hi) / 2.0;
        thrown = discard(ctx);
        let ok = pass(ctx, pb, mid);
        if ok {
            hi = mid;
        } else {
            lo = mid;
        }
        last = Some((mid, ok));
    }
    // End on the best height (the full column, as laid out first, when none was shorter).
    if last != Some((hi, true)) {
        thrown = discard(ctx);
        if !pass(ctx, pb, hi) && hi < full {
            thrown = discard(ctx);
            pass(ctx, pb, full);
        }
    }
    thrown
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

/// The text width of the section that defines header or footer `id`, when section `si` inherits
/// it rather than naming it (`None` when it names it). Measured in Word: an inherited part's
/// percentage-width tables are sized by that section, though it's laid out on `si`'s pages.
fn defining_width(
    sections: &[(usize, &SectionProps)],
    si: usize,
    get: fn(&SectionProps) -> wordcraft_doc::section::HeaderSet,
    id: Option<u32>,
) -> Option<f32> {
    let id = id?;
    let names = |s: &SectionProps| {
        let h = get(s);
        [h.default, h.first, h.even].contains(&Some(id))
    };
    let own = sections.get(si)?.1;
    if names(own) {
        return None;
    }
    sections.iter().take(si).rev().find(|(_, s)| names(s)).map(|(_, s)| s.text_width()).filter(|w| (w - own.text_width()).abs() > 0.01)
}

/// Lay out with percentage-width tables sized by `base` (see [`defining_width`]).
fn with_pct_base<T>(ctx: &mut Ctx, base: Option<f32>, f: impl FnOnce(&mut Ctx) -> T) -> T {
    let before = std::mem::replace(&mut ctx.pct_base, base);
    let out = f(ctx);
    ctx.pct_base = before;
    out
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
    let frame = PageFrame { sect, origin: (sect.margin_left + sect.gutter, y), number: 1, mirror: ctx.mirror };
    let track = std::mem::replace(&mut ctx.track_totals, false);
    let (h, ..) = with_pin(ctx, id, sect, 1, |ctx| {
        layout_box(ctx, StoryRef::Part(id), &blocks, &[], sect.text_width(), BoxContext::Other, 0, Some(frame)).height
    });
    ctx.track_totals = track;
    h
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

fn foot_top_for(ctx: &mut Ctx, sect: &SectionProps, footer: Option<u32>) -> f32 {
    let Some(id) = footer else { return f32::MAX };
    let Some(part) = ctx.doc.parts.get(&id) else { return f32::MAX };
    let blocks = part.blocks.clone();
    // A page total in the footer is filled in after pagination, like the header's: it doesn't
    // make the body lay out again.
    let track = std::mem::replace(&mut ctx.track_totals, false);
    let (laid, ..) =
        with_pin(ctx, id, sect, 1, |ctx| layout_box(ctx, StoryRef::Part(id), &blocks, &[], sect.text_width(), BoxContext::Other, 0, None));
    ctx.track_totals = track;
    let top = sect.page_h - sect.footer - laid.height;
    // Flow-table strokes extend above their cell boxes. Floating decorations keep their wrap rules.
    let flow_cells: Vec<_> = laid
        .items
        .iter()
        .filter_map(|item| match item {
            Placed::Cell { rect, table, story: own, .. }
                if *own == StoryRef::Part(id)
                    && table.0.len() == 1
                    && table
                        .0
                        .first()
                        .and_then(|i| blocks.get(*i as usize))
                        .is_some_and(|b| matches!(&**b, Block::Table(t) if t.props.float.is_none())) =>
            {
                Some(rect)
            }
            _ => None,
        })
        .collect();
    laid.items
        .iter()
        .filter_map(|item| match item {
            Placed::Rule { x0, x1, y0, y1, border }
                if border.is_visible()
                    && (y0 - y1).abs() < 0.01
                    && flow_cells.iter().any(|r| (r.y - y0).abs() < 0.01 && *x0 <= r.x + 0.01 && *x1 >= r.right() - 0.01) =>
            {
                Some(top + y0 - border.width / 2.0)
            }
            _ => None,
        })
        .fold(top, f32::min)
}

/// Where a box of laid-out items sits on its page, for floating objects positioned relative to
/// the page or its margins.
#[derive(Clone, Copy)]
struct PageFrame<'a> {
    sect: &'a SectionProps,
    /// Page position of the box's (0, 0); (0, 0) for body text.
    origin: (f32, f32),
    /// The page's number: inside and outside are left and right on odd pages, the other way
    /// round on even ones.
    number: u32,
    /// Whether an even page is mirrored ([`mirror_even_pages`]).
    mirror: bool,
}

impl PageFrame<'_> {
    fn even(&self) -> bool {
        self.number.is_multiple_of(2)
    }

    /// The widths of the page's left and right margins (the gutter is on the inside), and how
    /// far [`mirror_even_pages`] moves what is laid out on it.
    fn sides(&self) -> (f32, f32, f32) {
        let (inside, outside) = (self.sect.margin_left + self.sect.gutter, self.sect.margin_right);
        let shift = outside - inside;
        if self.mirror && self.even() && shift.is_finite() { (outside, inside, shift) } else { (inside, outside, 0.0) }
    }

    /// Alignment `a` across the page: inside and outside become right and left on an even page.
    fn h_align(frame: Option<&Self>, a: Option<FloatAlign>) -> Option<FloatAlign> {
        match a {
            Some(FloatAlign::Inside) if frame.is_some_and(|f| f.even()) => Some(FloatAlign::End),
            Some(FloatAlign::Outside) if frame.is_some_and(|f| f.even()) => Some(FloatAlign::Start),
            a => a,
        }
    }
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
    // Thinner than a point stays thin (a 0.25 pt rule, a flat line): Word keeps the size.
    let (w, h) = (wordcraft_geom::finite(w).clamp(0.0, 4000.0), wordcraft_geom::finite(h).clamp(0.0, 4000.0));
    // Each axis: the reference area's start and extent, in page coordinates. Across, as laid out
    // before an even page is mirrored (less the shift it then gets).
    let page_x = |f: &PageFrame| {
        let (s, even) = (f.sect, f.even());
        let (left, right, shift) = f.sides();
        let (start, extent) = match float.h_rel {
            Anchor::Page => (0.0, s.page_w),
            Anchor::Margin => (left, s.text_width()),
            Anchor::LeftMargin => (0.0, left),
            Anchor::RightMargin => (s.page_w - right, right),
            Anchor::InsideMargin if even => (s.page_w - right, right),
            Anchor::OutsideMargin if !even => (s.page_w - right, right),
            Anchor::InsideMargin | Anchor::OutsideMargin => (0.0, left),
            _ => return None,
        };
        Some((start - shift, extent))
    };
    let page_y = |s: &SectionProps| match float.v_rel {
        Anchor::Page => Some((0.0, s.page_h)),
        Anchor::Margin => Some((s.margin_top, s.page_h - s.margin_top - s.margin_bottom)),
        Anchor::TopMargin | Anchor::InsideMargin => Some((0.0, s.margin_top)),
        Anchor::BottomMargin | Anchor::OutsideMargin => Some((s.page_h - s.margin_bottom, s.margin_bottom)),
        _ => None,
    };
    let hx = frame.and_then(|f| page_x(&f).map(|(a, e)| (a - f.origin.0, e))).unwrap_or(col);
    let vy = frame.and_then(|f| page_y(f.sect).map(|(a, e)| (a - f.origin.1, e))).unwrap_or((y0, 0.0));
    let h_align = PageFrame::h_align(frame.as_ref(), float.h_align);
    // Without a frame (a cell) `vy` has no height to align in: the object starts at the anchor
    // line instead of being pushed up by half its height or more.
    let v_align = if frame.is_none() { None } else { float.v_align };
    Rect::new(align_in(hx, float.x, w, h_align), align_in(vy, float.y, h, v_align), w, h)
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

/// Wrap areas a page starts with (#2048): by page index, the floating objects (block, object
/// index) placed above the top of their anchor paragraph, with text before it on the page, and
/// their areas. Layout places a float only when it reaches its anchor, so the text above that
/// was laid out without it: the pass is done again with these known from the page's start.
type EarlyFloats = HashMap<usize, Vec<(usize, usize, WrapArea)>>;

/// Layout passes made again at most for [`EarlyFloats`] (a float moving with the text it pushes
/// down may move again). The pass after the last one keeps each float clear of the text laid
/// out before it ([`settle_early`]).
const MAX_REWRAP_PASSES: usize = 3;

/// Whether the text kept clear of `seed` is kept clear of `area` too: the same wrap side, `area`
/// inside `seed`, and no outline on `seed` other than `area`'s own in the same place.
fn covers(seed: &WrapArea, area: &WrapArea) -> bool {
    let (s, r) = (seed.0, area.0);
    let inside = r.x >= s.x - 0.5 && r.y >= s.y - 0.5 && r.right() <= s.right() + 0.5 && r.bottom() <= s.bottom() + 0.5;
    let same_place = (r.x - s.x).abs() < 0.5 && (r.y - s.y).abs() < 0.5 && (r.w - s.w).abs() < 0.5 && (r.h - s.h).abs() < 0.5;
    let outline = seed.2.is_none() || (same_place && seed.2 == area.2);
    (seed.3 == area.3 || seed.1) && (seed.1 || !area.1) && inside && outline
}

/// In a pass no other follows ([`MAX_REWRAP_PASSES`]), a float at `r` with wrap area `area`
/// placed above the top of its paragraph (`top`), beside text laid out before it, that the page
/// did not start with room for: it moves into the room the page started with for it (`seed`), or
/// else down to its paragraph's top, so no text laid out before it runs under it. `wrap` gives
/// the wrap area of a rectangle.
fn settle_early(
    r: Rect,
    area: Option<WrapArea>,
    seed: Option<&WrapArea>,
    top: f32,
    wrap: impl Fn(Rect) -> Option<WrapArea>,
) -> (Rect, Option<WrapArea>) {
    let Some(a) = area.as_ref().filter(|a| a.0.y < top - 0.5) else { return (r, area) };
    if seed.is_some_and(|s| covers(s, a)) {
        return (r, area);
    }
    if let Some(s) = seed {
        let moved = Rect { x: r.x + (s.0.x - a.0.x), y: r.y + (s.0.y - a.0.y), ..r };
        let moved_area = wrap(moved);
        if moved_area.as_ref().is_some_and(|m| covers(s, m)) {
            return (moved, moved_area);
        }
    }
    let down = Rect { y: r.y + (top - a.0.y), ..r };
    (down, wrap(down))
}

/// The early floats a pass found, `found`, are all among those the pass started with, `early`,
/// each inside the area the page started with for it.
fn early_covered(found: &EarlyFloats, early: &EarlyFloats) -> bool {
    found.iter().all(|(page, v)| {
        v.iter().all(|(b, k, area)| early.get(page).is_some_and(|w| w.iter().any(|(eb, ek, seed)| (eb, ek) == (b, k) && covers(seed, area))))
    })
}

/// Adds the early floats a pass found to those pages start with (#2048). Nothing is taken away:
/// a float whose wrap area pushes its own anchor onto the next page has no settled place (back
/// on the first page, its anchor fits there again), so the first page keeps the room it was
/// given and the float goes on the next, rather than the passes swapping between the two. A
/// float found somewhere else on the same page keeps text clear of both places (one plain
/// rectangle around them), so the areas only grow.
fn merge_early(early: &mut EarlyFloats, found: EarlyFloats) {
    for (page, v) in found {
        let have = early.entry(page).or_default();
        for (b, k, area) in v {
            match have.iter_mut().find(|(eb, ek, _)| (*eb, *ek) == (b, k)) {
                Some((_, _, seed)) if !covers(seed, &area) => {
                    let beside = seed.3 == area.3 && !seed.1 && !area.1;
                    *seed = (seed.0.union(&area.0), !beside, None, area.3);
                }
                Some(_) => {}
                None => have.push((b, k, area)),
            }
        }
    }
}

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
            let object = wordcraft_doc::Pos { story, path: Path(path.to_vec()), off };
            vec![Placed::Shape {
                rect,
                story,
                kind: *kind,
                fill: fill.clone(),
                stroke: *stroke,
                stroke_width: *stroke_width,
                effects: *effects,
                freeform: freeform.clone(),
                spin,
                extra: extra.clone(),
                object: Some(object),
                alt: shape_figure_alt(o).unwrap_or_default().to_string(),
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
                            fill: c.fill.clone(),
                            stroke: c.stroke,
                            stroke_width: c.stroke_width,
                            effects: Default::default(),
                            freeform: None,
                            spin,
                            extra: Default::default(),
                            object: Some(wordcraft_doc::Pos { story, path: Path(path.to_vec()), off }),
                            alt: String::new(),
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

/// The description a shape is tagged with as a figure in tagged PDF: its own, when it has one,
/// isn't marked decorative and shows no text. A shape without one is an artifact screen readers
/// skip, and a text box's text is read as text, so its description isn't used.
pub fn shape_figure_alt(o: &InlineObject) -> Option<&str> {
    match o {
        InlineObject::Shape { story: None, float, .. } if !float.alt.decorative && !float.alt.text.trim().is_empty() => Some(&float.alt.text),
        _ => None,
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
    // Hidden members aren't placed (nor their text boxes laid out).
    o.group_rects(rect.x, rect.y, rect.w, rect.h)
        .into_iter()
        .filter(|(_, c)| !c.frame().is_some_and(|(_, _, f)| f.hidden))
        .map(|([x, y, w, h], c)| (Rect::new(x, y, w, h), c))
        .collect()
}

/// Position the floating objects anchored in `p` for a paragraph whose text starts at `y0` in
/// the current column, add their wrap areas to the page (replacing any from an earlier attempt,
/// from `excl_mark` on) and lay the paragraph out around them, with its list `label`. `p` is
/// body block `block`. A float the page started with ([`EarlyFloats`]) keeps its place among the
/// page's wrap areas (its area as placed now). Without `place` (a later piece of the paragraph
/// broken again for another column) the floats stay where its first piece placed and drew them:
/// they aren't placed again in the new column, and their record for the page they are drawn on
/// ([`EarlyFloats`]) stands.
#[allow(clippy::too_many_arguments)]
fn anchor_floats(
    ctx: &mut Ctx,
    pb: &mut PageBuilder,
    p: &Paragraph,
    block: usize,
    y0: f32,
    before: f32,
    excl_mark: usize,
    label: &Option<(String, Level)>,
    place: bool,
) -> (HashMap<usize, Rect>, Arc<ParaLayout>, Vec<para::Exclusion>) {
    ctx.fields.page = pb.number;
    let (col_x, width) = (pb.col_x(), pb.col_w());
    let mut rects = HashMap::new();
    if !pb.web && place {
        // (A mark from a page before may be short of the areas this page started with.)
        pb.excl.truncate(excl_mark.max(pb.seeded));
        let page = pb.pages.len().saturating_sub(1);
        let early = pb.early.get(&page);
        // The paragraph's top, and whether text was placed before it on the page.
        let top = y0 - before;
        let text_before = top > pb.top + 0.5 || pb.col > 0;
        let frame = PageFrame { sect: pb.sect, origin: (0.0, 0.0), number: pb.number, mirror: ctx.mirror };
        // An object left out of the layout (hidden, or deleted in the final text) is not placed, so
        // it takes no room either.
        let left_out = left_out_objects(ctx.doc, p, None, ctx.opts.show_hidden, ctx.hide_deleted);
        for (oi, o) in p.objects.iter().enumerate() {
            let Some((w, h, float)) = floating(o) else { continue };
            if left_out(oi) {
                continue;
            }
            // Positions relative to the paragraph start above its space before (Word for Windows).
            let mut r = float_rect(Some(frame), (col_x, width), y0 - before, w, h, float);
            let outline = ctx.wrap_outline(o);
            let outline = outline.as_deref().map(|v| v.as_slice());
            let mut area = wrap_area(r, float, outline);
            if pb.settle && text_before {
                let seed = early.and_then(|v| v.iter().find(|(b, k, _)| (*b, *k) == (block, oi))).map(|(_, _, a)| a);
                (r, area) = settle_early(r, area, seed, top, |r| wrap_area(r, float, outline));
            }
            rects.insert(oi, r);
            // Above the paragraph, beside text laid out before it: the page is laid out again
            // with it known from the start (#2048).
            let above = area.as_ref().filter(|a| text_before && a.0.y < top - 0.5).map(|a| (page, a.clone()));
            ctx.floats_above.insert((block, oi), above);
            if let Some(area) = area {
                match early.and_then(|v| v.iter().position(|(b, k, _)| (*b, *k) == (block, oi))).filter(|k| *k < pb.seeded) {
                    Some(k) => {
                        if let Some(slot) = pb.excl.get_mut(k) {
                            *slot = area;
                        }
                    }
                    None => pb.excl.push(area),
                }
            }
        }
    }
    let rel = rel_exclusions(&body_wraps(ctx, pb), col_x, y0);
    let pl = ctx.para_labelled(p, width, None, &rel, label.clone());
    (rects, pl, rel)
}

/// Footer page tables wrap only the intersecting body lines, which resume below them.
fn footer_table_wraps(ctx: &mut Ctx, pb: &PageBuilder) -> Vec<WrapArea> {
    if pb.web || ctx.opts.view == ViewMode::Read {
        return Vec::new();
    }
    let Some(page) = pb.pages.last() else { return Vec::new() };
    let first = pb.pages.iter().rev().nth(1).is_none_or(|prev| prev.section != page.section);
    let key = (page.section, page.number, first);
    if let Some(areas) = ctx.footer_wraps.get(&key) {
        return areas.clone();
    }
    ctx.footer_wraps.insert(key, Vec::new());
    let doc = ctx.doc;
    let sections = doc.sections();
    let Some((_, sect)) = sections.get(page.section) else { return Vec::new() };
    if sect.text_direction.is_turned() {
        return Vec::new();
    }
    let set = linked_story_ids(&sections, page.section, false);
    let id = if sect.title_page && first {
        set.first
    } else if doc.settings.even_odd_headers && page.number.is_multiple_of(2) {
        set.even.or(set.default)
    } else {
        set.default
    };
    let Some(id) = id else { return Vec::new() };
    let Some(part) = doc.parts.get(&id) else { return Vec::new() };
    let blocks = part.blocks.clone();
    let x = sect.margin_left + sect.gutter;
    let base = defining_width(&sections, page.section, |s| s.footers, Some(id));
    let counters = std::mem::take(&mut ctx.counters);
    let boxes = std::mem::take(&mut ctx.boxes);
    let chains = std::mem::take(&mut ctx.chains);
    let fields = ctx.fields.clone();
    let grid = std::mem::take(&mut ctx.grid);
    let track = std::mem::replace(&mut ctx.track_totals, false);
    ctx.fields.page = page.number;
    let areas = again(ctx, 0, |ctx| {
        with_pct_base(ctx, base, |ctx| {
            let story = StoryRef::Part(id);
            let h = layout_box(ctx, story, &blocks, &[], sect.text_width(), BoxContext::Other, 0, None).height;
            let y = sect.page_h - sect.footer - h;
            let frame = PageFrame { sect, origin: (x, y), number: page.number, mirror: ctx.mirror };
            let mut areas = layout_box(ctx, story, &blocks, &[], sect.text_width(), BoxContext::Other, 0, Some(frame)).page_table_wraps;
            for (rect, ..) in &mut areas {
                rect.x += x;
                rect.y += y;
            }
            areas
        })
    });
    ctx.counters = counters;
    ctx.boxes = boxes;
    ctx.chains = chains;
    ctx.fields = fields;
    ctx.grid = grid;
    ctx.track_totals = track;
    ctx.footer_wraps.insert(key, areas.clone());
    areas
}

fn body_wraps(ctx: &mut Ctx, pb: &PageBuilder) -> Vec<WrapArea> {
    let mut areas = footer_table_wraps(ctx, pb);
    areas.extend_from_slice(&pb.excl);
    areas
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
    /// Floats placed on the page from a header or footer cell (by object index, in page
    /// coordinates): their items go to [`Ctx::pin`].
    pinned: Option<&'a HashMap<usize, Rect>>,
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
    // Each object's items (behind the text, in front), with its stacking order: drawn lowest
    // first once all are placed, as Word stacks floating objects by `relativeHeight`, not by
    // their order in the paragraph. Inline objects and objects without one keep their order.
    let mut stack: Vec<(u32, Vec<Placed>, Vec<Placed>)> = Vec::new();
    // The highest order so far: an object without one (inserted here) stacks above the objects
    // before it, as the writer gives it the next order up on save.
    let mut z_top = 0u32;
    let Some(fl) = pl.lines.get(l0) else { return (Vec::new(), Vec::new()) };
    // Objects left out of the layout (hidden, or deleted in the final text) aren't placed.
    let left_out = left_out_objects(ctx.doc, p, at.table_chr, ctx.opts.show_hidden, ctx.hide_deleted);
    // A floating object belongs to its paragraph, not to its line: Word puts it on the page the
    // paragraph starts on (a cover page is a paragraph holding a page break and then the text
    // box with the title; the box stays on the cover). So the paragraph's first piece places
    // every floating object, whichever of its lines holds the anchor, and later pieces none;
    // inline objects go with their own line.
    let first = l0 == 0;
    for li in 0..pl.lines.len() {
        let in_piece = (l0..l1).contains(&li);
        if !in_piece && !first {
            continue;
        }
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
            if if floating { !first } else { !in_piece } {
                continue;
            }
            let pinned = at.pinned.and_then(|m| m.get(&oi)).copied().filter(|_| floating);
            let rect = if let Some(r) = pinned {
                r
            } else if floating {
                floats.get(&oi).copied().unwrap_or_else(|| float_rect(at.page, at.col, at.para_y, w, h, float))
            } else {
                let cx = x + line.cl_left(k).unwrap_or(0.0);
                display::inline_rect(Some(obj), cx, y + (line.baseline - fl.top), c.adv, c.obj_h)
            };
            let (mut behind, mut front) = (Vec::new(), Vec::new());
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
            // Placed on the page from a header or footer cell: drawn with the header, once.
            if pinned.is_some() {
                if let Some(pin) = ctx.pin.as_mut()
                    && pin.seen.insert((path.to_vec(), c.start))
                {
                    pin.behind.extend(behind);
                    pin.front.extend(front);
                }
                continue;
            }
            let z = if floating && float.z > 0 {
                z_top = z_top.max(float.z);
                float.z
            } else {
                z_top
            };
            stack.push((z, behind, front));
        }
    }
    // Stable: equal orders (none given) keep the paragraph's order.
    stack.sort_by_key(|(z, _, _)| *z);
    let (mut behind, mut front) = (Vec::new(), Vec::new());
    for (_, b, f) in stack {
        behind.extend(b);
        front.extend(f);
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
                && let Some(o) = p.objects.get(oi).filter(|o| !o.frame().is_some_and(|(_, _, f)| f.hidden))
            {
                for id in o.text_boxes() {
                    notes.extend(doc.notes_in_text_box(id).into_iter().map(|n| (ci, n)));
                }
            }
        }
    }
    notes
}

/// A closing mark before a continuous section takes no room after content. An otherwise empty
/// section starting a new page retains its paragraph line.
fn place_para(ctx: &mut Ctx, pb: &mut PageBuilder, p: &Paragraph, block: usize, body_top: f32) {
    if !pb.web && ctx.quiet_marks.binary_search(&block).is_ok() {
        let follows_section = block.checked_sub(1).and_then(|i| ctx.doc.body.get(i)).and_then(|b| b.as_para()).is_some_and(|p| p.section.is_some());
        let section = pb.sect_idx;
        let own_page = pb.page().is_some_and(|page| page.section == section);
        // Word keeps the line when a new page's section contains only this mark.
        if p.section.is_some()
            && p.objects.is_empty()
            && (ctx.opts.show_hidden || !ctx.doc.styles.resolve_char(p.props.style.as_deref(), &p.mark).hidden)
            && !ctx.has_label(p)
            && (block == 0 || follows_section)
            && pb.at_top()
            && own_page
        {
            place_para_in_flow(ctx, pb, p, block, body_top);
            return;
        }
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

/// How many times one body paragraph is laid out again for the page numbers of the later pages
/// it runs onto (#1352).
const MAX_PAGE_RELAYS: usize = 64;

/// Run `f`, a layout of a paragraph again, with the automatic equation numbers it counted from
/// (`eq_start`) the first time, and leave the count where it was: laying it out again doesn't
/// count its numbered equations twice.
fn again<T>(ctx: &mut Ctx, eq_start: u32, f: impl FnOnce(&mut Ctx) -> T) -> T {
    let now = std::mem::replace(&mut ctx.eq_count, eq_start);
    let out = f(ctx);
    ctx.eq_count = now;
    out
}

/// How many times one body paragraph's lines are broken again for the wrap areas of the later
/// columns and pages it runs onto (#1374).
const MAX_WRAP_RELAYS: usize = 64;

#[cfg(test)]
thread_local! {
    /// How many wrap relays (#1374) the layouts on this thread made: tests check none happen
    /// without something new to wrap around.
    static WRAP_RELAYS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The wrap areas for a paragraph laid out against `rel` (relative to its top) whose lines from the
/// one at `t` (relative to its top) on go into the column starting at `x0`, that line at `y` on
/// the page, which has the wrap areas `excl` (#1374). The areas the lines before were broken
/// around end at `t`; the column's start there (one reaching above `t` with an outline keeps its
/// whole outline, so it may change a line before, and the paragraph is then not broken again).
/// Areas well clear of the column (outside `-width..2 * width`, room for negative indents) can't
/// touch its lines and are left out.
fn continued_exclusions(rel: &[para::Exclusion], excl: &[WrapArea], x0: f32, y: f32, t: f32, width: f32) -> Vec<para::Exclusion> {
    let before = rel.iter().filter(|e| e.top < t).map(|e| para::Exclusion { bottom: e.bottom.min(t), ..e.clone() });
    let here = rel_exclusions(excl, x0, y - t)
        .into_iter()
        .filter(|e| e.bottom > t && e.right > -width && e.left < 2.0 * width)
        .map(|e| if e.top < t && e.contour.is_none() { para::Exclusion { top: t, ..e } } else { e });
    before.chain(here).collect()
}

/// Run `f` with `map` as the field context's page map (`FieldCtx::page_from`), then take it back.
fn with_page_map<T>(ctx: &mut Ctx, map: &mut Vec<(usize, u32)>, f: impl FnOnce(&mut Ctx) -> T) -> T {
    ctx.fields.page_from = std::mem::take(map);
    let out = f(ctx);
    *map = std::mem::take(&mut ctx.fields.page_from);
    out
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
    // The pages this paragraph's fields are on after its first (`FieldCtx::page_from`), kept here
    // and lent to the field context only while this paragraph itself is laid out, so footnotes,
    // text boxes and headers laid out in between never see it (#1352).
    let mut page_map: Vec<(usize, u32)> = Vec::new();
    // Where its automatic equation numbers start: each layout of it again counts from here.
    let eq_start = ctx.eq_count;
    let (mut float_rects, mut pl, mut rel) = anchor_floats(ctx, pb, p, block, guess, space_before, excl_mark, &label, true);
    // The page its fields were laid out for (and the width), and how many times it was laid out again for a later
    // page (bounded: a paragraph over hundreds of pages keeps the last number it got).
    let (mut laid_page, mut laid_width) = (pb.number, pb.col_w());
    let mut page_relays = 0usize;
    // How many times its lines were broken again for the wrap areas of a later column (#1374).
    let mut wrap_relays = 0usize;
    // The anchor paragraph's top (above its space before) the floats were placed from.
    let mut anchor_y = pb.y;
    let spot = |pb: &PageBuilder| (pb.pages.len(), pb.col);
    let placed_at = spot(pb);
    let broke_before = pl.rp.page_break_before && !pb.at_top() && !pb.web;
    if broke_before {
        pb.new_page(block, body_top);
    }
    // Same-style contextual spacing removes this paragraph's before-space, or subtracts the
    // previous paragraph's after-space from the merged gap (ISO/IEC 29500-1 §17.3.1.9).
    let mut before = pl.rp.space_before;
    let prev_para = pb.prev.clone();
    // At the top of a page, measured in Word for Windows: no space before after a paragraph that
    // ended with a page break, nor (from Word 2013's layout, compatibility mode 15) for a page
    // break before; a section break or, in older modes, a page break before keeps it.
    let after_break_mark = std::mem::take(&mut pb.after_break_mark);
    if pb.at_top() && (after_break_mark || (broke_before && ctx.doc.settings.compat_mode >= 15)) {
        before = 0.0;
    } else if pb.at_top()
        && !ctx.doc.settings.sum_paragraph_spacing
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
        // Contextual spacing suppresses each paragraph's own side of the gap.
        if style == pl.rp.style {
            if ctxl {
                pb.y -= after;
                before = (before - after).max(0.0);
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
        if before > 0.0 && after > 0.0 && !ctx.doc.settings.sum_paragraph_spacing {
            before = (before - after).max(0.0);
        }
    }
    // Automatic spacing: none at the top of the document.
    if pl.rp.before_auto && block == 0 {
        before = 0.0;
    }
    pb.y += before;
    let mut applied_before = before;
    // Bordered paragraphs: room for their top and bottom borders, those of a group joined in one box.
    let above = joined(ctx.doc, &ctx.doc.body, block, block.checked_sub(1), None);
    let below = joined(ctx.doc, &ctx.doc.body, block, Some(block + 1), None);
    let own = Edges::own(&pl, 0, pl.lines.len());
    let (top_room, bottom_room) = (if above { 0.0 } else { own.top }, if below { 0.0 } else { own.bottom });
    // Keep lines together: if it doesn't fit but would on an empty column, move it. Keep with
    // next alone moves a paragraph only when it would fit here by itself (Word for Windows): one
    // that doesn't breaks as usual, its last lines going over with the next paragraph.
    let height = pl.height + top_room + bottom_room;
    if pl.rp.keep_lines || (pl.rp.keep_next && pb.y + height <= pb.bottom + 0.01) {
        let need = height + if pl.rp.keep_next { next_first_line(ctx, block, width, 0, pl.rp.space_after, &mut ctx.counters.clone()) } else { 0.0 };
        // (Against the whole column: balanced columns grow to hold it.)
        // With the 0.01 pt of slack the other fit checks take.
        if pb.y + need > pb.bottom + 0.01 && !pb.at_top() && need <= pb.uncapped_bottom() - pb.top {
            pb.advance(block, body_top);
            pb.y += pl.rp.space_before.min(0.0);
            applied_before = pl.rp.space_before.min(0.0);
        }
    }
    // A different top only matters when there is something to wrap around.
    if spot(pb) != placed_at || ((pb.y - guess).abs() > 0.01 && !pb.excl.is_empty()) {
        let y0 = pb.y;
        (float_rects, pl, rel) = again(ctx, eq_start, |ctx| anchor_floats(ctx, pb, p, block, y0, applied_before, excl_mark, &label, true));
        (laid_page, laid_width) = (pb.number, pb.col_w());
        anchor_y = y0 - applied_before;
    }
    let mut n = pl.lines.len();
    // Worked out only for a last line on a page with notes.
    let mut after_on_page: Option<f32> = None;
    let mut l0 = 0;
    while l0 < n {
        // The lines going into a new column (or page) were broken around the wrap areas where the
        // paragraph started: break them again around this column's (the lines before stay as they
        // were, and so do the paragraph's own floating objects), when they differ (#1374).
        if l0 > 0
            && !pb.web
            && wrap_relays < MAX_WRAP_RELAYS
            && let Some((resume, t)) = pl.lines.get(l0).map(|l| (l.start, l.top))
        {
            let here = continued_exclusions(&rel, &body_wraps(ctx, pb), pb.col_x(), pb.y, t, laid_width);
            if here != rel {
                wrap_relays += 1;
                ctx.fields.page = laid_page;
                #[cfg(test)]
                WRAP_RELAYS.with(|c| c.set(c.get() + 1));
                let relaid =
                    with_page_map(ctx, &mut page_map, |ctx| again(ctx, eq_start, |ctx| ctx.para_labelled(p, laid_width, None, &here, label.clone())));
                // Only if the rest still starts at the same line, at the same height (an outline
                // reaching above it could move the lines before down, and the column's areas with them).
                if let Some(k) = relaid.lines.iter().position(|l| l.start == resume)
                    && relaid.lines.get(k).is_some_and(|l| (l.top - t).abs() < 0.01)
                {
                    (pl, rel) = (relaid, here);
                    n = pl.lines.len();
                    l0 = k;
                }
            }
        }
        // Page fields in the lines going onto a new page show that page's number: lay the
        // paragraph out again with the fields from there on on this page (the lines before stay as
        // they were, so the line the rest starts with is still there), breaking the rest again for
        // the new results' widths (#1352). Only a paragraph with page fields pays for this.
        if l0 > 0
            && pl.has_page_fields
            && !pb.web
            && page_relays < MAX_PAGE_RELAYS
            && let Some(resume) = pl.lines.get(l0).map(|l| l.start)
            && page_map.last().map_or(laid_page, |e| e.1) != pb.number
            && has_page_number_fields(p)
        {
            page_relays += 1;
            page_map.push((resume, pb.number));
            ctx.fields.page = laid_page;
            let relaid =
                with_page_map(ctx, &mut page_map, |ctx| again(ctx, eq_start, |ctx| ctx.para_labelled(p, laid_width, None, &rel, label.clone())));
            match relaid.lines.iter().position(|l| l.start == resume) {
                Some(k) => {
                    pl = relaid;
                    n = pl.lines.len();
                    l0 = k;
                }
                None => {
                    page_map.pop();
                }
            }
        }
        // What is laid out for these lines from here (their footnotes) is on this page.
        ctx.fields.page = pb.number;
        // How many lines fit?
        let Some(first) = pl.lines.get(l0) else { break };
        // A paragraph's first line may start below its top (pushed down by a floating object).
        let lead = if l0 == 0 { first.top.max(0.0) + top_room } else { 0.0 };
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
                            layout_box(ctx, StoryRef::Part(*id), &blocks, &[], pb.sect.text_width(), BoxContext::Other, 0, None);
                        line_notes.push(NoteBox { id: *id, items, h, cont: false });
                    }
                }
            }
            let candidate_notes_h: f32 = pb.notes.iter().chain(&new_notes).chain(&line_notes).map(|note| note.h).sum();
            let candidate_note_h =
                if pb.notes.is_empty() && new_notes.is_empty() && line_notes.is_empty() { 0.0 } else { NOTE_SEP + candidate_notes_h };
            let limit = pb.orig_bottom - pb.reserved_note_h.max(candidate_note_h);
            let bottom = pb.y + lead + (l.top + l.height - first.top);
            // On a page with footnotes the paragraph's last line keeps its space after clear of
            // the separator (Word): if the space doesn't fit, the line goes to the next page (and,
            // with widow control, one more with it). Without notes the space after may run past
            // the bottom margin as usual.
            let has_notes = !pb.notes.is_empty() || !new_notes.is_empty() || !line_notes.is_empty();
            let tail = if has_notes && l1 + 1 == n {
                *after_on_page.get_or_insert_with(|| space_after_kept(ctx, block, &pl.rp, label.is_some()))
            } else {
                0.0
            };
            let over = bottom + tail > limit + 0.01;
            // Notes too long for the rest of the page continue on the next one: the line still
            // fits if the notes before its last one, and that one's first line, fit below it.
            // The notes then take the rest of the page.
            let split = bottom > limit + 0.01
                && line_notes.split_last().is_some_and(|(last, others)| {
                    let before: f32 = pb.notes.iter().chain(&new_notes).chain(others).map(|n| n.h).sum();
                    let sep = pb.seps.sep_h(pb.notes.first().is_some_and(|n| n.cont));
                    pb.body_floor().max(bottom) + sep + before + last.first_piece() + pb.seps.notice_h() <= pb.orig_bottom + 0.01
                });
            if over && l1 > l0 && !split {
                break;
            }
            // A line holding nothing but a page or column break stays at the bottom of the page
            // even when it doesn't fit: it only ends the page, and moving it would leave a blank
            // page behind it (Word does the same).
            let only_break = matches!(l.end, LineEnd::PageBreak | LineEnd::ColumnBreak)
                && p.text
                    .get(l.start..l.stop)
                    .is_some_and(|t| t.chars().all(|c| matches!(c, wordcraft_doc::para::PAGE_BREAK | wordcraft_doc::para::COLUMN_BREAK)));
            // An empty paragraph closing a section before a new page: measured in Word, it stays
            // on a full page instead of starting one of its own.
            let closing_mark = pb.next_starts_page && p.section.is_some() && p.text.is_empty();
            if over && l1 == l0 && !pb.at_top() && !split && !only_break && !closing_mark {
                break;
            }
            new_notes.extend(line_notes);
            l1 += 1;
            if matches!(l.end, LineEnd::PageBreak | LineEnd::ColumnBreak) && !pb.web {
                forced_break = Some(l.end);
                if l.end == LineEnd::PageBreak && l1 + 1 == n && pl.lines.get(l1).is_some_and(|m| only_markers(p, m.start..m.stop)) {
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
            } else if n - l1 == 1 && (l1 - l0 >= 3 || (l0 > 0 && l1 - l0 >= 2)) {
                l1 -= 1; // widow: take one more line along
            } else if n - l1 == 1 && l0 == 0 && l1 - l0 == 2 && !pb.at_top() {
                // Neither split of a three-line paragraph is allowed (two and a widow, or an
                // orphan and two): it moves whole, as in Word.
                l1 = l0;
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
                    (float_rects, pl, rel) = again(ctx, eq_start, |ctx| anchor_floats(ctx, pb, p, block, y0, 0.0, excl_mark, &label, true));
                    (laid_page, laid_width) = (pb.number, pb.col_w());
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
        let x = pb.col_x() + mirror_shift(&pl.rp, pb.number, pb.web);
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
        let mut edges = Edges { top: if l0 == 0 { top_room } else { 0.0 }, bottom: if l1 >= n { bottom_room } else { 0.0 }, up: 0.0 };
        if l0 == 0
            && above
            && let Some((page, col, end)) = pb.para_end
            && page == pb.pages.len()
            && col == pb.col
        {
            edges.up = (y - end).max(0.0);
        }
        let end = push_para(&mut items, StoryRef::Body, &[block as u32], &pl, l0, l1, x, y, width, edges);
        pb.para_end = Some((pb.pages.len(), pb.col, end));
        // Pictures, shapes and text boxes in these lines (web view: floats placed by this piece).
        let at = ObjFrame {
            page: Some(PageFrame { sect: pb.sect, origin: (0.0, 0.0), number: pb.number, mirror: ctx.mirror }),
            col: (x, width),
            para_y: if pb.web { y } else { anchor_y },
            table_chr: None,
            pinned: None,
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
            // The section boundary already started this page; a leading page-break mark on
            // the same section paragraph must not start another one.
            Some(LineEnd::PageBreak) if starts_with_break && p.section.is_some() && !pb.web => {}
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
                    // The floats were placed and drawn with the first piece: they stay there.
                    let (rects, relaid, relaid_rel) = with_page_map(ctx, &mut page_map, |ctx| {
                        again(ctx, eq_start, |ctx| anchor_floats(ctx, pb, p, block, y0, 0.0, excl_mark, &label, false))
                    });
                    if let Some(k) = relaid.lines.iter().position(|l| l.start == resume) {
                        (float_rects, pl, rel) = (rects, relaid, relaid_rel);
                        (laid_page, laid_width) = (pb.number, pb.col_w());
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
    // A mark that stayed with its page break keeps its space after on that page too, and its
    // bottom border, drawn there: the next page starts with the next paragraph (Word,
    // compatibility modes 12 to 15).
    let (bottom_room, space_after) = if pb.after_break_mark { (0.0, 0.0) } else { (bottom_room, pl.rp.space_after) };
    pb.y += bottom_room + space_after;
    pb.prev = Some((pl.rp.style.clone(), pl.rp.contextual_spacing, space_after, pl.rp.after_auto, label.is_some()));
}

/// How far a paragraph with mirrored indents (`w:mirrorIndents`) moves right on an even page.
/// Whether `range` of `p`'s text holds nothing but zero-width markers (bookmarks, field and
/// control boundaries…), or nothing at all: a line of them takes no room after a page break.
fn only_markers(p: &Paragraph, range: std::ops::Range<usize>) -> bool {
    let Some(text) = p.text.get(range.clone()) else { return false };
    text.char_indices().all(|(i, c)| c == wordcraft_doc::para::OBJ && p.object_at(range.start + i).is_some_and(InlineObject::is_marker))
}

/// The outline of the body for an Outline view pass.
struct OutlinePass {
    view: outline::OutlineView,
    levels: Vec<u8>,
    hidden: Vec<bool>,
}

impl OutlinePass {
    fn new(doc: &Document, view: outline::OutlineView) -> Self {
        let levels = outline::levels(doc);
        let hidden = outline::hidden(&levels, &view);
        OutlinePass { view, levels, hidden }
    }
}

/// A body paragraph in Outline view (one long sheet: no page or column breaks, no floats
/// wrapping): indented by its level behind its symbol; hidden when collapsed or deeper than Show
/// Level; body text cut to its first line with Show First Line Only.
fn place_outline_para(ctx: &mut Ctx, pb: &mut PageBuilder, p: &Paragraph, block: usize, o: &OutlinePass) {
    // List numbers count the hidden paragraphs too.
    let label = ctx.next_label(p);
    if o.hidden.get(block).copied().unwrap_or(false) {
        return;
    }
    let level = o.levels.get(block).copied().unwrap_or(outline::BODY);
    let indent = outline::indent(&o.levels, block);
    let x = pb.col_x() + indent + outline::SYMBOL_W;
    let width = (pb.col_w() - indent - outline::SYMBOL_W).max(72.0);
    let q = outline::outline_para(ctx.doc, p, o.view.plain);
    ctx.fields.page = pb.number;
    let pl = ctx.para_labelled(&q, width, None, &[], label);
    pb.y += pl.rp.space_before.max(0.0);
    let n = pl.lines.len();
    let l1 = if o.view.first_line_only && level >= outline::BODY { n.min(1) } else { n };
    // Its borders take their room above and below its lines.
    let edges = Edges::own(&pl, 0, l1);
    let y = pb.y + pl.lines.first().map(|f| f.top.max(0.0)).unwrap_or(0.0) + edges.top;
    let bottom_room = edges.bottom;
    let mut items = Vec::new();
    push_para(&mut items, StoryRef::Body, &[block as u32], &pl, 0, l1, x, y, width, edges);
    let at = ObjFrame {
        page: Some(PageFrame { sect: pb.sect, origin: (0.0, 0.0), number: pb.number, mirror: ctx.mirror }),
        col: (x, width),
        para_y: y,
        table_chr: None,
        pinned: None,
    };
    let (behind, front) = place_objects(ctx, StoryRef::Body, &[block as u32], &q, &pl, (0, l1), (x, y), &at, &HashMap::new(), 0);
    items.extend(front);
    let children = level < outline::BODY && outline::has_children(&o.levels, block);
    // Collapsed, or with something under it hidden by Show Level: the wavy line says so.
    let collapsed = children && o.hidden.get(block + 1).copied().unwrap_or(false);
    items.extend(outline::symbol_items(&pl, x, y, level, children, collapsed));
    if let Some(pg) = pb.page() {
        for (i, it) in behind.into_iter().enumerate() {
            pg.items.insert(i.min(pg.items.len()), it);
        }
        pg.items.extend(items);
    }
    if let (Some(f), Some(l)) = (pl.lines.first(), l1.checked_sub(1).and_then(|k| pl.lines.get(k))) {
        pb.y = y + (l.top + l.height - f.top);
    }
    pb.y += bottom_room + pl.rp.space_after.max(0.0);
}

/// How far a paragraph with mirrored indents (`w:mirrorIndents`) moves right on page `number`:
/// its left and right indents are the inside and outside ones, which swap sides on even pages.
fn mirror_shift(rp: &wordcraft_doc::resolve::ResolvedPara, number: u32, web: bool) -> f32 {
    if !rp.mirror_indents || web || !number.is_multiple_of(2) {
        return 0.0;
    }
    let (left, right) = rp.visual_indents();
    right - left
}

/// The space after body paragraph `block` (resolved as `rp`) that is really left below it: none
/// when contextual spacing or automatic spacing between list items drops it before the next
/// paragraph. Same tests as the spacing between paragraphs in `place_para_in_flow`: contextual
/// spacing removes this paragraph's space after only when this one asks for it, and the next
/// counts as a list item when it gets a label.
fn space_after_kept(ctx: &Ctx, block: usize, rp: &wordcraft_doc::resolve::ResolvedPara, listed: bool) -> f32 {
    let after = rp.space_after.max(0.0);
    if after <= 0.0 {
        return 0.0;
    }
    if let Some(Block::Para(next)) = ctx.doc.body.get(block + 1).map(|b| &**b) {
        let nrp = ctx.doc.styles.resolve_para(&next.props);
        if nrp.style == rp.style && rp.contextual_spacing {
            return 0.0;
        }
        if listed && rp.after_auto && ctx.has_label(next) {
            return 0.0;
        }
    }
    after
}

/// Height of the first line of the block after `block` (for keep-with-next).
/// The room the paragraphs after `block` need on its page for keep with next: the next one's
/// first line, or all of it and what it keeps with in turn when it is kept with the next too
/// (a chain moves together in Word).
fn next_first_line(ctx: &mut Ctx, block: usize, width: f32, depth: usize, preceding_after: f32, counters: &mut Counters) -> f32 {
    // The room the paragraphs after `block` need on the page for keep with next: enough of the
    // next one to start there — its first line, two of its lines with widow/orphan control (it
    // doesn't leave a single line at the bottom of a page; it moves over whole instead), or all
    // of it under keep lines together — or all of it and what it keeps with in turn when it is
    // kept with the next too (a chain moves together in Word). Word carries a kept heading over
    // when the paragraph after it cannot start on the page (#966).
    match ctx.doc.body.get(block + 1).map(|b| &**b) {
        Some(Block::Para(p)) => {
            // Keep list indents and label metrics without consuming the document counters.
            let rp = ctx.doc.styles.resolve_para(&p.props);
            let label =
                p.props.numbering.or(rp.numbering).filter(|n| n.num != 0).and_then(|n| counters.next_label(&ctx.doc.numbering, n.num, n.level));
            let env = para::ParaEnv {
                doc: ctx.doc,
                width,
                label,
                fields: &ctx.fields,
                show_hidden: ctx.opts.show_hidden,
                field_codes: ctx.opts.field_codes,
                hide_deleted: ctx.opts.hides_deleted(StoryRef::Body),
                table: None,
                proofing: None,
                typing_word: None,
                numeral: ctx.opts.numeral,
                exclusions: &[],
                eq_number: ctx.eq_count,
                read_line_spacing: if ctx.opts.view == ViewMode::Read { ctx.opts.read_line_spacing } else { None },
                grid: ctx.grid,
                fit_objects: false,
                fitted: &[],
            };
            let pl = para::layout_para(p, &env);
            // Adjacent paragraph spacing uses the larger positive before/after value. This
            // gap participates in the kept-together fit check, including the first line.
            let before = pl.rp.space_before;
            let gap = if before > 0.0 && preceding_after > 0.0 { before.max(preceding_after) } else { before + preceding_after };
            // Its border room, unless it joins the paragraph before (or after) in one box.
            let own = Edges::own(&pl, 0, pl.lines.len());
            let top = if joined(ctx.doc, &ctx.doc.body, block + 1, Some(block), None) { 0.0 } else { own.top };
            let bottom = if joined(ctx.doc, &ctx.doc.body, block + 1, Some(block + 2), None) { 0.0 } else { own.bottom };
            if pl.rp.keep_next && depth < 8 {
                gap.max(0.0) + top + pl.height + bottom + next_first_line(ctx, block + 1, width, depth + 1, pl.rp.space_after, counters)
            } else {
                // The room this one needs to start on the page with the kept paragraph before
                // it (#966).
                let to = |k: usize| match (pl.lines.first(), pl.lines.get(k)) {
                    (Some(first), Some(last)) => last.top + last.height - first.top,
                    _ => 0.0,
                };
                gap.max(0.0)
                    + top
                    + if pl.rp.keep_lines {
                        pl.height
                    } else if pl.rp.widow_control && pl.lines.len() >= 2 {
                        to(1)
                    } else {
                        to(0)
                    }
            }
        }
        Some(Block::Table(_)) => 24.0,
        None => 0.0,
    }
}

fn row_keeps_next(row: &table::RowLayout) -> bool {
    let Some(items) = row.cells.first().and_then(|cell| row.items.get(cell.items.clone())) else { return false };
    let Some((depth, story)) = items
        .iter()
        .filter_map(|item| match item {
            Placed::Cell { table, story, .. } => Some((table.0.len() + 3, *story)),
            _ => None,
        })
        .min_by_key(|(depth, _)| *depth)
    else {
        return false;
    };
    // Word takes the row's keep flag from the first paragraph of its first cell.
    items
        .iter()
        .find_map(|item| match item {
            Placed::Lines { path, story: own, para, .. } if path.0.len() == depth && path.0.last() == Some(&0) && *own == story => {
                Some(para.rp.keep_next)
            }
            _ => None,
        })
        .unwrap_or(false)
}

fn row_can_start(row: &wordcraft_doc::Row, laid: &table::RowLayout, room: f32, page_height: f32) -> bool {
    if laid.height <= room + 0.01 {
        return true;
    }
    if row.props.header || row.props.cant_split || (row.props.height_rule == HeightRule::Exact && row.props.height.is_some_and(|h| h > 0.0)) {
        return false;
    }
    if row.props.height_rule == HeightRule::AtLeast && row.props.height.is_some_and(|h| h > room + 0.01 && h <= page_height + 0.01) {
        return false;
    }
    table::split_row(laid, room).is_some()
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
            place_floating_table(ctx, pb, &tl, &f, legacy, block, body_top);
            return;
        }
        // Too tall for a page: Word lets it run across pages like a table in the text, from its
        // own left edge.
        tl.x = float_table_x(pb, tl.width, &f, legacy, ctx.mirror) - pb.col_x();
    }
    // A table in the text flow starts below the floating objects in its way.
    below_floats(ctx, pb, pb.col_x() + tl.x, pb.col_x() + tl.x + tl.width);
    let header_rows: Vec<usize> = (0..t.rows.len()).take_while(|r| t.rows.get(*r).is_some_and(|row| row.props.header)).collect();
    // Header rows are re-placed on every page a table spills onto, so a table with many header
    // rows that each spill onto their own page is O(header rows × pages) placed items — a few KB
    // of document can otherwise fill memory. Once this many header rows have been repeated across
    // the whole table, later pages carry none; pagination still finishes. Far above any real
    // table (e.g. 3 header rows over 30 000 pages).
    const MAX_HEADER_REPEATS: usize = 200_000;
    let mut header_repeats = 0usize;
    // Resolve kept chains once; nested paragraphs cannot keep their enclosing row.
    let mut kept: Vec<(usize, f32)> = (0..tl.rows.len()).map(|i| (i, 0.0)).collect();
    for i in (0..tl.rows.len().saturating_sub(1)).rev() {
        let Some(row) = tl.rows.get(i) else { continue };
        if i < header_rows.len() || !row_keeps_next(row) {
            continue;
        }
        let Some((end, height)) = kept.get(i + 1).copied() else { continue };
        if let Some(group) = kept.get_mut(i) {
            *group = (end, row.height + height);
        }
    }
    let repeated_height: f32 = header_rows.iter().filter_map(|i| tl.rows.get(*i)).map(|r| r.height).sum();
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
    // Measured in Word: an at-least row's minimum height is reserved, not split. Where less room
    // than that is left on a page that already holds something, the row starts the next page
    // whole; a row taller than a page then splits from the top of that page.
    let short_of_min = |ri: usize, room: f32| {
        t.rows.get(ri).is_some_and(|r| r.props.height_rule == HeightRule::AtLeast && r.props.height.is_some_and(|h| h > room + 0.01))
    };
    // Like Word, repeated header rows don't stay alone at the bottom of a page: when the first row
    // after them can't start under them there, the table starts on the next page.
    let first = header_rows.len();
    if !header_rows.is_empty()
        && !pb.at_top()
        && let Some(row) = tl.rows.get(first)
    {
        let room = pb.bottom - pb.y - tl.rows.iter().take(first).map(|r| r.height).sum::<f32>();
        let splittable = !t.rows.get(first).is_some_and(|r| r.props.cant_split);
        if row.height + row.edge > room + 0.01 && (short_of_min(first, room) || !(splittable && table::split_row(row, room).is_some())) {
            pb.advance(block, body_top);
        }
    }
    // For the move-on rule, the table's own header rows at the top of a page leave it empty.
    let table_at_top = pb.at_top();
    for (ri, row) in tl.rows.iter().enumerate() {
        // Word never splits a row of an exact height: it moves on whole.
        let splittable = !header_rows.contains(&ri)
            && !t
                .rows
                .get(ri)
                .is_some_and(|r| r.props.cant_split || (r.props.height_rule == HeightRule::Exact && r.props.height.is_some_and(|h| h > 0.0)));
        let mut rest: Option<table::RowLayout> = None;
        // On a new page holding only the repeated header rows.
        let mut fresh = false;
        // Nothing but this table's header rows above it on a page it started at the top of.
        let under_top_headers = ri == first && table_at_top;
        // Each pass places the part of the row that fits, then breaks the page.
        for _ in 0..1000 {
            let cur = rest.as_ref().unwrap_or(row);
            // A fitting kept row follows the next row when that row cannot start below it.
            // Move only the prefix's start; an oversized follower cannot separate its kept rows.
            let defer_kept = pb.y + cur.height <= pb.bottom + 0.01
                && !pb.web
                && !pb.at_top()
                && !fresh
                && rest.is_none()
                && !ri.checked_sub(1).and_then(|prev| kept.get(prev)).is_some_and(|&(end, _)| end >= ri)
                && kept.get(ri).is_some_and(|&(end, height)| {
                    if end == ri {
                        return false;
                    }
                    let (Some(next), Some(laid)) = (t.rows.get(end), tl.rows.get(end)) else { return false };
                    let page_height = pb.bottom - body_top - repeated_height;
                    height <= page_height + 0.01 && !row_can_start(next, laid, pb.bottom - pb.y - height, page_height)
                });
            // A row another row follows needs room for the border below it too (measured in Word).
            if pb.y + cur.height + cur.edge <= pb.bottom + 0.01 && !defer_kept {
                break;
            }
            // An at-least row with less room than its minimum moves on whole, unless nothing
            // is above it on this page (moving on would gain nothing).
            let room = pb.bottom - pb.y;
            let moves_on = rest.is_none() && !pb.at_top() && !fresh && !under_top_headers && short_of_min(ri, room);
            let split = if splittable && !moves_on && !defer_kept { table::split_row(cur, room) } else { None };
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
            // Repeat header rows, up to this table's cap.
            if !header_rows.contains(&ri) && header_repeats < MAX_HEADER_REPEATS {
                for hr in &header_rows {
                    if header_repeats >= MAX_HEADER_REPEATS {
                        break;
                    }
                    if let Some(h) = tl.rows.get(*hr) {
                        place(pb, h);
                        header_repeats += 1;
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
        let BoxLayout { items, height: h, .. } = layout_box(ctx, StoryRef::Part(id), &blocks, &[], pb.sect.text_width(), BoxContext::Other, 0, None);
        pb.notes.push(NoteBox { id, items, h, cont: false });
    }
    pb.bottom -= pb.notes_h() - before;
}

/// Header tables can displace a non-overlapping body table. Page-positioned drawings keep
/// their own wrapping rules, so only table areas are retained from the header layout.
fn header_table_areas(ctx: &mut Ctx, pb: &PageBuilder) -> Vec<Rect> {
    if pb.web || ctx.opts.view == ViewMode::Read {
        return Vec::new();
    }
    let Some(page) = pb.pages.last() else { return Vec::new() };
    let first = pb.pages.iter().rev().nth(1).is_none_or(|prev| prev.section != page.section);
    let key = (page.section, page.number, first);
    if let Some(areas) = ctx.header_tables.get(&key) {
        return areas.clone();
    }
    let sections = ctx.doc.sections();
    let Some((_, sect)) = sections.get(page.section) else { return Vec::new() };
    if sect.text_direction.is_turned() {
        return Vec::new();
    }
    let set = linked_story_ids(&sections, page.section, true);
    let id = if sect.title_page && first {
        set.first
    } else if ctx.doc.settings.even_odd_headers && page.number.is_multiple_of(2) {
        set.even.or(set.default)
    } else {
        set.default
    };
    let Some(id) = id else { return Vec::new() };
    let Some(part) = ctx.doc.parts.get(&id) else { return Vec::new() };
    let blocks = part.blocks.clone();
    let x = sect.margin_left + sect.gutter;
    let frame = PageFrame { sect, origin: (x, sect.header), number: page.number, mirror: ctx.mirror };
    let base = defining_width(&sections, page.section, |s| s.headers, Some(id));
    let counters = std::mem::take(&mut ctx.counters);
    let boxes = std::mem::take(&mut ctx.boxes);
    let chains = std::mem::take(&mut ctx.chains);
    let fields = ctx.fields.clone();
    let grid = std::mem::take(&mut ctx.grid);
    let track = std::mem::replace(&mut ctx.track_totals, false);
    ctx.fields.page = page.number;
    let mut areas = again(ctx, 0, |ctx| {
        with_pct_base(ctx, base, |ctx| layout_box(ctx, StoryRef::Part(id), &blocks, &[], sect.text_width(), BoxContext::Other, 0, Some(frame)))
            .table_areas
    });
    ctx.counters = counters;
    ctx.boxes = boxes;
    ctx.chains = chains;
    ctx.fields = fields;
    ctx.grid = grid;
    ctx.track_totals = track;
    for r in &mut areas {
        r.x += x;
        r.y += sect.header;
    }
    ctx.header_tables.insert(key, areas.clone());
    areas
}

/// Place floating table `tl` (`w:tblpPr`): at its own position, which takes no room in the text
/// flow; the text after it wraps around it.
fn place_floating_table(ctx: &mut Ctx, pb: &mut PageBuilder, tl: &table::TableLayout, f: &TableFloat, legacy: f32, block: usize, body_top: f32) {
    let h: f32 = tl.rows.iter().map(|r| r.height).sum::<f32>().clamp(0.0, 100_000.0);
    let w = tl.width.clamp(1.0, 100_000.0);
    let [dl, dt, dr, db] = f.dist_from_text();
    for attempt in 0..2 {
        let s = pb.sect;
        let h_area = float_table_h_area(pb, f, ctx.mirror);
        // The text a table anchored to it stands in: where the next paragraph starts.
        let v_area = match f.v_rel {
            Anchor::Page => (0.0, s.page_h),
            Anchor::Margin => (s.margin_top, s.text_height()),
            _ => (pb.y, 0.0),
        };
        let x = float_table_x(pb, w, f, legacy, ctx.mirror);
        let mut obstacles = if f.overlap { Vec::new() } else { header_table_areas(ctx, pb) };
        obstacles.extend_from_slice(&pb.float_tables);
        let r = clear_of_float_tables(Rect::new(x, align_in(v_area, f.y, h, f.v_align), w, h), f, legacy, h_area.0, &obstacles);
        // Like Word, a table that doesn't fit below the text before it moves to the next page with
        // its anchor: one placed relative to the paragraph when it would run off the page, one
        // placed on the page or margin when it would also cover the text already on the page.
        let moves = if f.v_rel == Anchor::Paragraph {
            r.bottom() > pb.bottom + 0.01
        } else {
            let covers_text = pb.page().is_some_and(|page| page.items.iter().any(|item| placed_intersects(item, &r)));
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
            pg.float_tables.push(Path::top(block));
        }
        let area = r.inset(-dl, -dt, -dr, -db);
        pb.excl.push((area, false, None, WrapText::BothSides));
        pb.float_tables.push(area);
        return;
    }
}

fn placed_intersects(item: &Placed, rect: &Rect) -> bool {
    match item {
        Placed::Lines { para, l0, l1, x, y, .. } => para.lines.get(*l0..*l1).is_some_and(|lines| {
            lines.iter().enumerate().any(|(offset, line)| {
                if para.label.is_some() && *l0 + offset == 0 {
                    return Rect::new(x + line.left, y + line.top, line.right - line.left, line.height).intersects(rect);
                }
                para.clusters.get(line.c0..line.c1).is_some_and(|clusters| {
                    clusters.iter().enumerate().any(|(index, cluster)| {
                        if !matches!(
                            cluster.kind,
                            crate::para::ClKind::Text | crate::para::ClKind::Space | crate::para::ClKind::Tab | crate::para::ClKind::Object(_)
                        ) {
                            return false;
                        }
                        let ci = line.c0 + index;
                        let Some(left) = line.cl_left(ci) else { return false };
                        let Some(right) = line.cl_right(ci) else { return false };
                        Rect::new(x + left, y + line.top, right - left, line.height).intersects(rect)
                    })
                })
            })
        }),
        Placed::Fill { rect: bounds, .. }
        | Placed::Image { rect: bounds, .. }
        | Placed::Shape { rect: bounds, .. }
        | Placed::Graphic { rect: bounds, .. }
        | Placed::Cell { rect: bounds, .. }
        | Placed::Object { rect: bounds, .. }
        | Placed::Clip { rect: bounds, .. } => bounds.intersects(rect),
        Placed::Rule { x0, y0, x1, y1, .. } => Rect::new(x0.min(*x1), y0.min(*y1), (x1 - x0).abs(), (y1 - y0).abs()).intersects(rect),
    }
}

/// The page `pb` is filling, for placing floating objects on it (`mirror`: [`Ctx::mirror`]).
fn page_frame<'a>(pb: &PageBuilder<'a>, mirror: bool) -> PageFrame<'a> {
    PageFrame { sect: pb.sect, origin: (0.0, 0.0), number: pb.number, mirror }
}

/// The area a floating table's `x` is measured in: its start and width on the page, as laid out
/// before an even page is mirrored (see [`float_rect`]).
fn float_table_h_area(pb: &PageBuilder, f: &TableFloat, mirror: bool) -> (f32, f32) {
    let s = pb.sect;
    let (left, _, shift) = page_frame(pb, mirror).sides();
    match f.h_rel {
        Anchor::Page => (-shift, s.page_w),
        Anchor::Margin => (left - shift, s.text_width()),
        _ => (pb.col_x(), pb.col_w()),
    }
}

/// A floating table's left edge on the page (`legacy`: the first cell's left margin before Word
/// 2013 layout, when an offset places the cell's text).
fn float_table_x(pb: &PageBuilder, w: f32, f: &TableFloat, legacy: f32, mirror: bool) -> f32 {
    let align = PageFrame::h_align(Some(&page_frame(pb, mirror)), f.h_align);
    let x = align_in(float_table_h_area(pb, f, mirror), f.x, w, align);
    if f.h_align.is_none() { x - legacy } else { x }
}

/// Move `pb.y` below the floating objects in the way of something spanning `x0..x1` that
/// starts there (a table in the text flow doesn't wrap around them).
fn below_floats(ctx: &mut Ctx, pb: &mut PageBuilder, x0: f32, x1: f32) {
    pb.y = below(&body_wraps(ctx, pb), pb.y, x0, x1);
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

/// Whether a header, a footer or a text box shows a `PAGEREF` field, which they resolve from the
/// paginated body (see [`bookmark_pages`]).
fn headers_footers_refer_to_pages(doc: &Document) -> bool {
    use wordcraft_doc::PartKind;
    doc.parts.iter().filter(|(_, part)| matches!(part.kind, PartKind::Header | PartKind::Footer | PartKind::TextBox)).any(|(id, _)| {
        let story = StoryRef::Part(*id);
        doc.para_paths(story)
            .iter()
            .filter_map(|path| doc.para(story, path))
            .any(|p| p.objects.iter().any(|o| matches!(o, InlineObject::Field { instr, .. } if fields::field_name(instr) == "PAGEREF")))
    })
}

/// Where each bookmark in the body starts: the number and number format of the page it is placed
/// on (with two of the same name, the first in the document).
fn bookmark_pages(doc: &Document, pages: &[Page], sections: &[(usize, &SectionProps)]) -> fields::BookmarkPages {
    // Each paragraph's bookmarks: name, offset, and its line in the paragraph layout last looked
    // up (the pieces of a paragraph usually share one).
    let mut wanted: HashMap<Path, Vec<(String, usize, Option<(usize, usize)>)>> = HashMap::new();
    let mut seen = HashSet::new();
    for path in doc.para_paths(StoryRef::Body) {
        let Some(p) = doc.para(StoryRef::Body, &path) else { continue };
        if !p.objects.iter().any(|o| matches!(o, InlineObject::BookmarkStart { .. })) {
            continue;
        }
        for (off, o) in p.object_offsets().into_iter().zip(&p.objects) {
            if let InlineObject::BookmarkStart { name } = o
                && seen.insert(name.as_str())
            {
                wanted.entry(path.clone()).or_default().push((name.clone(), off, None));
            }
        }
    }
    let mut out = fields::BookmarkPages::new();
    for page in pages {
        if wanted.is_empty() {
            break;
        }
        let format = sections.get(page.section).map(|(_, s)| s.page_num_format).unwrap_or_default();
        for it in &page.items {
            let Placed::Lines { story: StoryRef::Body, path, para, l0, l1, .. } = it else { continue };
            let Some(marks) = wanted.get_mut(path) else { continue };
            let layout = Arc::as_ptr(para) as usize;
            marks.retain_mut(|(name, off, line)| {
                let li = match *line {
                    Some((k, li)) if k == layout => li,
                    _ => {
                        let li = para.line_of(*off);
                        *line = Some((layout, li));
                        li
                    }
                };
                if !(*l0..*l1).contains(&li) {
                    return true;
                }
                out.insert(std::mem::take(name), (page.number, format));
                false
            });
            if marks.is_empty() {
                wanted.remove(path);
            }
        }
    }
    out
}

/// A key for caches standing for `marks` (0 for none).
fn marks_key(marks: &fields::BookmarkPages) -> u64 {
    if marks.is_empty() {
        return 0;
    }
    let mut v: Vec<_> = marks.iter().collect();
    v.sort_by(|a, b| a.0.cmp(b.0));
    hash_of(&v)
}

fn headers_footers(ctx: &mut Ctx, pages: &mut [Page], sections: &[(usize, &SectionProps)], totals: &Totals) {
    let total = pages.len() as u32;
    let even_odd = ctx.doc.settings.even_odd_headers;
    let styles = styleref::headers_use_styleref(ctx.doc).then(|| ctx.cache.style_index(ctx.doc));
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
        page.hf_slot = if sect.title_page && first_of_section.contains(&i) {
            HfSlot::First
        } else if even_odd && page.number % 2 == 0 {
            HfSlot::Even
        } else {
            HfSlot::Default
        };
        ctx.fields.page = page.number;
        ctx.fields.page_format = sect.page_num_format;
        ctx.fields.pages = total;
        ctx.fields.section_pages = totals.section(page.section);
        ctx.fields.section = page.section as u32 + 1;
        // STYLEREF: the body paragraphs on this page.
        ctx.fields.styleref = styles.as_ref().map(|ix| {
            let paths = page.items.iter().filter_map(|it| match it {
                Placed::Lines { story: StoryRef::Body, path, .. } => Some(path),
                _ => None,
            });
            (ix.clone(), ix.page_at(paths, page.first_block))
        });
        let w = sect.text_width();
        let x = sect.margin_left + sect.gutter;
        if let Some(id) = hid
            && let Some(part) = ctx.doc.parts.get(&id)
        {
            let blocks = part.blocks.clone();
            let frame = PageFrame { sect, origin: (x, sect.header), number: page.number, mirror: ctx.mirror };
            let base = defining_width(sections, page.section, |s| s.headers, hid);
            let (mut items, behind, front) = with_pin(ctx, id, sect, page.number, |ctx| {
                with_pct_base(ctx, base, |c| layout_box(c, StoryRef::Part(id), &blocks, &[], w, BoxContext::Other, 0, Some(frame)).items)
            });
            for it in &mut items {
                it.translate(x, sect.header);
            }
            page.header = behind.into_iter().chain(items).chain(front).collect();
            page.header_story = Some(id);
        }
        if let Some(id) = fid
            && let Some(part) = ctx.doc.parts.get(&id)
        {
            let blocks = part.blocks.clone();
            let base = defining_width(sections, page.section, |s| s.footers, fid);
            // The footer's height fixes where it sits, which page-relative floats need.
            let (h, ..) = with_pin(ctx, id, sect, page.number, |ctx| {
                with_pct_base(ctx, base, |c| layout_box(c, StoryRef::Part(id), &blocks, &[], w, BoxContext::Other, 0, None).height)
            });
            let frame = PageFrame { sect, origin: (x, sect.page_h - sect.footer - h), number: page.number, mirror: ctx.mirror };
            let (mut items, behind, front) = with_pin(ctx, id, sect, page.number, |ctx| {
                with_pct_base(ctx, base, |c| layout_box(c, StoryRef::Part(id), &blocks, &[], w, BoxContext::Other, 0, Some(frame)).items)
            });
            for it in &mut items {
                it.translate(x, sect.page_h - sect.footer - h);
            }
            page.footer = behind.into_iter().chain(items).chain(front).collect();
            page.footer_story = Some(id);
        }
    }
    ctx.fields.styleref = None;
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
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        T0.get_or_init(std::time::Instant::now).elapsed().as_secs_f64() * 1000.0
    }
    // `Instant::now()` panics on wasm32-unknown-unknown, so ask the browser's clock.
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let ms = js_sys::Date::now();
        if ms.is_finite() { ms } else { 0.0 }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_autospace;
#[cfg(test)]
mod tests_bidi;
#[cfg(test)]
mod tests_complex;
#[cfg(test)]
mod tests_grid;
#[cfg(test)]
mod tests_ligature;
#[cfg(test)]
mod tests_numeral;
#[cfg(test)]
mod tests_pages;
#[cfg(test)]
mod tests_styleref;
#[cfg(test)]
mod tests_textbox;
#[cfg(test)]
mod tests_vertical;
